//! Genuine Claude Code process runtime. Caller tools cross authenticated Agent
//! IPC with an additional per-run capability; the child has no built-in tools.
use aipass_agent_protocol::{AgentRequest, ClaudeBridgeRequest, SensitiveString};
use aipass_proxy::{
    ResolvedTarget, SubscriptionBackend, SubscriptionFuture, SubscriptionResponse,
    SubscriptionStream,
};
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    pin::Pin,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex, Weak},
    task::{Context, Poll},
    time::{Duration, Instant},
};
use uuid::Uuid;
use zeroize::Zeroize;
const MAX_LINE: u64 = 4 * 1024 * 1024;
const MAX_RUNS: usize = 16;
const PARK_TTL: Duration = Duration::from_secs(300);
const IDLE_TTL: Duration = Duration::from_secs(3600);

type SegmentSender = tokio::sync::mpsc::Sender<(Bytes, bool)>;
struct ToolCall {
    id: String,
    name: String,
    arguments: Value,
    claimed: bool,
    result: Option<Value>,
}
struct Run {
    entry_id: Uuid,
    owner: Uuid,
    fingerprint: [u8; 32],
    context: [u8; 32],
    session: Option<String>,
    history: [u8; 32],
    history_messages: Vec<Value>,
    child: Child,
    stdin: ChildStdin,
    _directory: tempfile::TempDir,
    tools: Vec<Value>,
    sender: Option<SegmentSender>,
    calls: Vec<ToolCall>,
    content: Vec<Value>,
    message: Value,
    last_used: Instant,
    parked: bool,
    terminal: bool,
}
impl Drop for Run {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(Some(_))) {
            return;
        }
        #[cfg(unix)]
        {
            if let Some(pid) = rustix::process::Pid::from_raw(self.child.id() as i32) {
                let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
struct Inner {
    runs: Mutex<HashMap<String, Run>>,
    vault: PathBuf,
    #[cfg(test)]
    binary: Mutex<Option<PathBuf>>,
}
#[derive(Clone)]
pub(crate) struct ClaudeBridge {
    inner: Arc<Inner>,
}
impl ClaudeBridge {
    pub(crate) fn new(vault: &Path) -> Self {
        let inner = Arc::new(Inner {
            runs: Mutex::new(HashMap::new()),
            vault: vault.to_owned(),
            #[cfg(test)]
            binary: Mutex::new(None),
        });
        let weak = Arc::downgrade(&inner);
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(30));
            let Some(inner) = weak.upgrade() else {
                break;
            };
            if let Ok(mut runs) = inner.runs.lock() {
                runs.retain(|_, r| {
                    r.sender.is_some()
                        || r.last_used.elapsed() < if r.parked { PARK_TTL } else { IDLE_TTL }
                });
            };
        });
        Self { inner }
    }
    pub(crate) fn mcp(
        &self,
        capability: &str,
        request: ClaudeBridgeRequest,
    ) -> Result<Value, String> {
        let mut runs = self
            .inner
            .runs
            .lock()
            .map_err(|_| "Claude bridge unavailable")?;
        let run = runs
            .get_mut(capability)
            .ok_or("Claude bridge capability expired")?;
        if run.last_used.elapsed() > if run.parked { PARK_TTL } else { IDLE_TTL } {
            return Err("Claude session expired".into());
        }
        match request {
            ClaudeBridgeRequest::ListTools => Ok(json!({"tools":run.tools})),
            ClaudeBridgeRequest::Call { name, arguments } => {
                let Some(call) = run
                    .calls
                    .iter_mut()
                    .find(|c| !c.claimed && c.name == name && c.arguments == arguments)
                else {
                    return Ok(json!({"pending":true}));
                };
                call.claimed = true;
                Ok(json!({"callId":call.id}))
            }
            ClaudeBridgeRequest::Poll { call_id } => {
                let call = run
                    .calls
                    .iter_mut()
                    .find(|c| c.id == call_id && c.claimed)
                    .ok_or("unknown Claude tool call")?;
                Ok(match &call.result {
                    Some(result) => json!({"result":result}),
                    None => json!({"pending":true}),
                })
            }
        }
    }
    fn open(
        &self,
        target: &ResolvedTarget,
        payload: &Value,
        session: Option<String>,
    ) -> Result<SubscriptionStream, String> {
        if payload.to_string().len() > MAX_LINE as usize {
            return Err("Claude input exceeds bridge limit".into());
        }
        if payload.get("temperature").is_some()
            || payload.get("top_p").is_some()
            || payload
                .get("stop_sequences")
                .is_some_and(|v| v.as_array().is_none_or(|a| !a.is_empty()))
        {
            return Err(
                "Claude Code subscription does not expose sampling or stop-sequence controls"
                    .into(),
            );
        }
        if payload
            .get("tool_choice")
            .is_some_and(|v| v["type"] != "auto")
        {
            return Err("Claude Code subscription requires automatic tool choice".into());
        }
        let messages = payload["messages"]
            .as_array()
            .ok_or("Claude messages are required")?;
        if messages.is_empty() {
            return Err("Claude messages are empty".into());
        }
        let context = hash(
            &json!({"model":payload["model"],"system":payload["system"],"tools":payload["tools"],"thinking":payload["thinking"],"output_config":payload["output_config"],"max_tokens":payload["max_tokens"]}),
        );
        let fingerprint: [u8; 32] = Sha256::digest(target.api_key.as_bytes()).into();
        let read_grant = || {
            let client = crate::AgentClient::for_vault(self.inner.vault.clone())
                .map_err(|_| "Agent connection unavailable")?;
            client
                .request::<Option<SensitiveString>>(&AgentRequest::ClaudeNativeRead {
                    entry_id: target.config.provider_entry_id,
                    access_token: SensitiveString::new(&target.api_key),
                })
                .map_err(|_| "Claude account no longer available")
        };
        #[cfg(test)]
        let grant = if self.inner.binary.lock().unwrap().is_some() {
            None
        } else {
            read_grant()?
        };
        #[cfg(not(test))]
        let grant = read_grant()?;
        let cli_home = if let Some(reference) = grant.as_ref() {
            let value: Value = serde_json::from_str(reference.expose())
                .map_err(|_| "invalid Claude CLI reference")?;
            crate::subscriptions::cli_accounts::check_device(&value)?;
            let path = PathBuf::from(
                value["nativeHome"]
                    .as_str()
                    .ok_or("Reconnect Claude with its official CLI")?,
            );
            let account = crate::claude_cli::local_account(&path)?;
            if account.identity != value["accountId"] {
                return Err("Claude CLI account changed; reconnect it explicitly".into());
            }
            Some(path)
        } else {
            None
        };
        let (sender, receiver) = tokio::sync::mpsc::channel(8);
        let mut runs = self
            .inner
            .runs
            .lock()
            .map_err(|_| "Claude bridge unavailable")?;
        let results = messages
            .last()
            .and_then(|m| m["content"].as_array())
            .map(|blocks| {
                blocks
                    .iter()
                    .filter(|b| b["type"] == "tool_result")
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let prefix = history_hash(&messages[..messages.len() - 1]);
        let key = runs
            .iter()
            .find(|(_, r)| {
                r.owner == target.config.id
                    && r.fingerprint == fingerprint
                    && r.context == context
                    && r.sender.is_none()
                    && if !results.is_empty() {
                        r.parked
                            && results.iter().all(|b| {
                                r.calls.iter().any(|c| {
                                    c.result.is_none()
                                        && Some(c.id.as_str()) == b["tool_use_id"].as_str()
                                })
                            })
                    } else {
                        !r.parked && r.history == prefix
                    }
            })
            .map(|(key, _)| key.clone());
        if let Some(key) = key {
            let run = runs.get_mut(&key).unwrap();
            let prepared = validate_results(&run.calls, &results)?;
            if !results.is_empty() {
                for (id, result) in prepared {
                    let call = run.calls.iter_mut().find(|c| c.id == id).unwrap();
                    call.result = Some(result);
                }
            } else {
                write_user(&mut run.stdin, messages.last().unwrap()["content"].clone())?;
                run.calls.clear();
            }
            run.sender = Some(sender);
            run.last_used = Instant::now();
            run.terminal = false;
            run.parked = false;
            run.history = history_hash(messages);
            run.history_messages = messages.clone();
            return Ok(Box::pin(SegmentStream {
                receiver,
                inner: Arc::downgrade(&self.inner),
                key,
                complete: false,
            }));
        }
        if !results.is_empty() {
            return Err(
                "Claude tool session expired or account changed; restart the conversation".into(),
            );
        }
        if runs.values().any(|r| {
            r.owner == target.config.id
                && session.is_some()
                && r.session == session
                && r.sender.is_some()
        }) {
            return Err("Claude conversation already has an active turn".into());
        }
        while runs.len() >= MAX_RUNS {
            let oldest = runs
                .iter()
                .filter(|(_, r)| r.sender.is_none())
                .min_by_key(|(_, r)| r.last_used)
                .map(|(k, _)| k.clone())
                .ok_or("Claude process limit reached")?;
            runs.remove(&oldest);
        }
        let key = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let directory =
            tempfile::tempdir().map_err(|_| "could not create isolated Claude workspace")?;
        let tools = payload["tools"].as_array().map(|tools|tools.iter().map(|tool|json!({"name":tool["name"],"description":tool["description"],"inputSchema":tool["input_schema"]})).collect::<Vec<_>>()).unwrap_or_default();
        if tools.iter().any(|t| {
            t["name"].as_str().is_none_or(|n| {
                n.is_empty()
                    || !n
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            })
        }) {
            return Err("Claude bridge tool names must contain only letters, numbers, underscores or hyphens".into());
        }
        let helper = std::env::current_exe().map_err(|_| "agent helper executable unavailable")?;
        let mcp = json!({"mcpServers":{"aipass":{"command":helper,"args":["--claude-mcp-helper","--vault",self.inner.vault],"env":{"AIPASS_CLAUDE_CAPABILITY":key}}}});
        #[cfg(test)]
        let binary = self
            .inner
            .binary
            .lock()
            .unwrap()
            .clone()
            .or_else(claude_binary)
            .ok_or("mock Claude unavailable")?;
        #[cfg(not(test))]
        let binary = claude_binary()
            .ok_or("Install Claude Code to use a Claude subscription through the local proxy")?;
        let mut command = Command::new(binary);
        command
            .args([
                "-p",
                "--output-format",
                "stream-json",
                "--input-format",
                "stream-json",
                "--include-partial-messages",
                "--verbose",
                "--tools",
                "",
                "--strict-mcp-config",
                "--setting-sources",
                "",
                "--no-session-persistence",
            ])
            .arg("--model")
            .arg(
                payload["model"]
                    .as_str()
                    .ok_or("Claude model is required")?,
            )
            .arg("--mcp-config")
            .arg(mcp.to_string())
            .arg("--allowedTools")
            .arg("mcp__aipass__*")
            .current_dir(directory.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(system) = payload.get("system").filter(|v| !v.is_null()) {
            let system = system
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| system.to_string());
            if system.len() > 64 * 1024 {
                return Err("Claude system prompt exceeds process argument limit".into());
            }
            command.arg("--append-system-prompt").arg(system);
        }
        if let Some(effort) = payload
            .pointer("/output_config/effort")
            .and_then(Value::as_str)
        {
            command.arg("--effort").arg(effort);
        }
        crate::claude_cli::configure(&mut command, directory.path());
        if let Some(environment) = target
            .upstream_proxy
            .as_ref()
            .and_then(aipass_proxy::cli_proxy_environment)
        {
            for key in [
                "HTTP_PROXY",
                "HTTPS_PROXY",
                "ALL_PROXY",
                "NO_PROXY",
                "http_proxy",
                "https_proxy",
                "all_proxy",
                "no_proxy",
            ] {
                command.env_remove(key);
            }
            command.envs(environment);
        }
        command.env_remove("CLAUDE_CODE_SIMPLE");
        command.env_remove("CLAUDE_CONFIG_DIR");
        command.env("CLAUDE_CONFIG_DIR", directory.path());
        // The genuine CLI reads and rotates its own grant in place. The temp
        // workspace contains only tool capabilities and is never a grant holder.
        if let Some(home) = cli_home {
            crate::claude_cli::account_environment(&mut command, &home);
        } else {
            #[cfg(test)]
            command.env("CLAUDE_CODE_OAUTH_TOKEN", &target.api_key);
            #[cfg(not(test))]
            return Err("Reconnect Claude through its official CLI".into());
        }

        if let Some(limit) = payload["max_tokens"].as_u64() {
            command.env("CLAUDE_CODE_MAX_OUTPUT_TOKENS", limit.to_string());
        }
        if payload["thinking"]["type"] == "disabled" {
            command.env("MAX_THINKING_TOKENS", "0");
        } else if let Some(budget) = payload["thinking"]["budget_tokens"].as_u64() {
            command.env("MAX_THINKING_TOKENS", budget.to_string());
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn().map_err(|_| "could not start Claude Code")?;
        let stdout = child.stdout.take().ok_or("Claude output unavailable")?;
        let stdin = child.stdin.take().ok_or("Claude input unavailable")?;
        let mut run = Run {
            entry_id: target.config.provider_entry_id,
            owner: target.config.id,
            fingerprint,
            context,
            session,
            history: history_hash(messages),
            history_messages: messages.clone(),
            child,
            stdin,
            _directory: directory,
            tools,
            sender: Some(sender),
            calls: Vec::new(),
            content: Vec::new(),
            message: Value::Null,
            last_used: Instant::now(),
            parked: false,
            terminal: false,
        };
        let prompt = if messages.len() == 1 && messages[0]["role"] == "user" {
            messages[0]["content"].clone()
        } else {
            json!([{ "type":"text", "text":format!("Continue this conversation, preserving its tool results and user instructions:\n{}",json!(messages)) }])
        };
        write_user(&mut run.stdin, prompt)?;
        runs.insert(key.clone(), run);
        drop(runs);
        let weak = Arc::downgrade(&self.inner);
        let reader_key = key.clone();
        std::thread::spawn(move || read_events(stdout, weak, reader_key));
        Ok(Box::pin(SegmentStream {
            receiver,
            inner: Arc::downgrade(&self.inner),
            key,
            complete: false,
        }))
    }
}
impl SubscriptionBackend for ClaudeBridge {
    fn history_owner(&self, payload: &Value) -> Result<Option<Uuid>, String> {
        let ids = aipass_proxy::subscription_history_call_ids(payload);
        let runs = self
            .inner
            .runs
            .lock()
            .map_err(|_| "Claude bridge unavailable")?;
        let mut owner = None;
        for id in ids
            .into_iter()
            .filter(|id| id.starts_with("toolu_aipass_claude_"))
        {
            let target = runs
                .values()
                .find(|r| r.calls.iter().any(|c| c.id == id))
                .map(|r| r.owner)
                .ok_or("Claude tool session expired; restart the conversation")?;
            if owner.is_some_and(|owner| owner != target) {
                return Err("Claude history mixes accounts".into());
            }
            owner = Some(target);
        }
        Ok(owner)
    }
    fn retain_targets(&self, targets: &[&ResolvedTarget]) -> Vec<Uuid> {
        if let Ok(mut runs) = self.inner.runs.lock() {
            runs.retain(|_, run| {
                let Some(target) = targets.iter().find(|t| {
                    t.config.enabled
                        && t.config.id == run.owner
                        && t.config.provider_entry_id == run.entry_id
                }) else {
                    return false;
                };
                let fingerprint: [u8; 32] = Sha256::digest(target.api_key.as_bytes()).into();
                run.fingerprint == fingerprint
            });
        }
        Vec::new()
    }
    fn revoke(&self) {
        if let Ok(mut runs) = self.inner.runs.lock() {
            runs.clear();
        }
    }
    fn request(
        &self,
        target: ResolvedTarget,
        payload: Value,
        session: Option<String>,
    ) -> SubscriptionFuture {
        let bridge = self.clone();
        Box::pin(async move {
            let streaming = payload["stream"] == true;
            let mut stream =
                tokio::task::spawn_blocking(move || bridge.open(&target, &payload, session))
                    .await
                    .map_err(|_| "Claude process task failed")??;
            let mut headers = hyper::HeaderMap::new();
            if streaming {
                headers.insert(
                    hyper::header::CONTENT_TYPE,
                    hyper::header::HeaderValue::from_static("text/event-stream"),
                );
            } else {
                let mut collector = Collector::default();
                let mut total = 0;
                while let Some(chunk) = stream.next().await {
                    let chunk = chunk.map_err(|e| e.to_string())?;
                    total += chunk.len();
                    if total > 64 * 1024 * 1024 {
                        return Err("Claude response exceeds bridge limit".into());
                    }
                    collector.push(&chunk)?;
                }
                if !collector.complete {
                    return Err("Claude process ended before completion".into());
                }
                stream = Box::pin(futures_util::stream::once(async move {
                    Ok(Bytes::from(collector.message.to_string()))
                }));
                headers.insert(
                    hyper::header::CONTENT_TYPE,
                    hyper::header::HeaderValue::from_static("application/json"),
                );
            }
            Ok(SubscriptionResponse {
                status: hyper::StatusCode::OK,
                headers,
                body: stream,
            })
        })
    }
}
fn history_hash(messages: &[Value]) -> [u8; 32] {
    let mut messages = messages.to_vec();
    for message in &mut messages {
        if message["role"] == "assistant" {
            if let Some(parts) = message["content"].as_array_mut() {
                parts.retain(|p| {
                    !matches!(p["type"].as_str(), Some("thinking" | "redacted_thinking"))
                        && !(p["type"] == "text"
                            && p["text"].as_str().is_some_and(|s| {
                                s.starts_with("<reasoning>") && s.ends_with("</reasoning>")
                            }))
                });
            }
        }
    }
    hash(&json!(messages))
}
fn hash(v: &Value) -> [u8; 32] {
    Sha256::digest(v.to_string().as_bytes()).into()
}
fn write_user(stdin: &mut ChildStdin, content: Value) -> Result<(), String> {
    writeln!(
        stdin,
        "{}",
        json!({"type":"user","message":{"role":"user","content":content}})
    )
    .and_then(|_| stdin.flush())
    .map_err(|_| "Claude process input closed".into())
}
fn claude_binary() -> Option<PathBuf> {
    crate::claude_cli::binary()
}
fn validate_results(calls: &[ToolCall], results: &[Value]) -> Result<Vec<(String, Value)>, String> {
    if results.is_empty() {
        return Ok(Vec::new());
    }
    let pending = calls
        .iter()
        .filter(|c| c.result.is_none())
        .map(|c| c.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let ids = results
        .iter()
        .filter_map(|v| v["tool_use_id"].as_str())
        .collect::<std::collections::HashSet<_>>();
    if ids != pending || ids.len() != results.len() {
        return Err("return exactly one result for each pending Claude tool call".into());
    }
    results
        .iter()
        .map(|v| {
            Ok((
                v["tool_use_id"].as_str().unwrap().to_owned(),
                tool_result(v)?,
            ))
        })
        .collect()
}
fn tool_result(value: &Value) -> Result<Value, String> {
    let content=match &value["content"] {
        Value::String(text)=>vec![json!({"type":"text","text":text})],
        Value::Array(parts)=>parts.iter().map(|p|match p["type"].as_str() {
            Some("text")=>Ok(p.clone()),
            Some("image") if p["source"]["type"]=="base64"=>Ok(json!({"type":"image","data":p["source"]["data"],"mimeType":p["source"]["media_type"]})),
            _=>Err("unsupported Claude tool result part".to_owned())
        }).collect::<Result<Vec<_>,_>>()?,
        Value::Null=>vec![json!({"type":"text","text":""})],
        _=>return Err("invalid Claude tool result".into())
    };
    Ok(json!({"content":content,"isError":value["is_error"]==true}))
}

mod mcp;
mod native;
mod stream;
pub use mcp::run_mcp_helper;
#[cfg(target_os = "macos")]
pub(crate) use native::native_service;
pub(crate) use native::{validate_account, write_private};
use stream::{read_events, Collector, SegmentStream};

#[cfg(all(test, unix))]
mod tests;
