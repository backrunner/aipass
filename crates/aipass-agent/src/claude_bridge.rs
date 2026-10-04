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
    token: zeroize::Zeroizing<String>,
    native: bool,
    native_grant_hash: Option<[u8; 32]>,
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
        #[cfg(unix)]
        {
            if let Some(pid) = rustix::process::Pid::from_raw(self.child.id() as i32) {
                let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        #[cfg(target_os = "macos")]
        if self.native {
            if let Ok(mut cleanup) = Command::new("security")
                .args([
                    "delete-generic-password",
                    "-s",
                    &native_service(self._directory.path()),
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                let deadline = Instant::now() + Duration::from_secs(3);
                loop {
                    match cleanup.try_wait() {
                        Ok(Some(_)) => break,
                        _ if Instant::now() >= deadline => {
                            let _ = cleanup.kill();
                            let _ = cleanup.wait();
                            break;
                        }
                        _ => std::thread::sleep(Duration::from_millis(10)),
                    }
                }
            }
        }
        let path = self._directory.path().join(".credentials.json");
        if let Ok(metadata) = std::fs::metadata(&path) {
            if metadata.len() <= 1024 * 1024 {
                let _ = std::fs::write(&path, vec![0; metadata.len() as usize]);
            }
        }
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
            let keys = inner
                .runs
                .lock()
                .map(|runs| runs.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            let pending: std::collections::HashSet<_> = keys
                .into_iter()
                .filter(|key| !persist_native(&inner, key))
                .collect();
            if let Ok(mut runs) = inner.runs.lock() {
                runs.retain(|key, r| {
                    pending.contains(key)
                        || r.sender.is_some()
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
                .filter(|(_, r)| r.sender.is_none() && !pending_native_grant(r))
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
        for name in [
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_BASE_URL",
            "CLAUDECODE",
            "CLAUDE_CODE_ENTRYPOINT",
            "CLAUDE_CODE_SSE_PORT",
            "CLAUDE_CODE_OAUTH_TOKEN",
            "CLAUDE_CODE_USE_BEDROCK",
            "CLAUDE_CODE_USE_VERTEX",
            "CLAUDE_CODE_USE_FOUNDRY",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_MODEL",
            "MAX_THINKING_TOKENS",
            "CLAUDE_CODE_MAX_OUTPUT_TOKENS",
        ] {
            command.env_remove(name);
        }
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
        let native = grant.is_some();
        let mut native_grant_hash = None;
        if let Some(grant) = grant {
            native_grant_hash = Some(hash(&validate_native(grant.expose())?));
            write_private(
                &directory.path().join(".credentials.json"),
                grant.expose().as_bytes(),
            )?;
            write_private(
                &directory.path().join(".claude.json"),
                b"{\"hasCompletedOnboarding\":true}",
            )?;
        } else {
            command.env("CLAUDE_CODE_OAUTH_TOKEN", &target.api_key);
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
            token: zeroize::Zeroizing::new(target.api_key.clone()),
            native,
            native_grant_hash,
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
        let mut retained = Vec::new();
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
                if run.fingerprint == fingerprint {
                    return true;
                }
                // Only the exact grant rotated by this process can preserve it.
                if run.native
                    && read_native(run._directory.path()).is_some_and(|v| {
                        v["claudeAiOauth"]["accessToken"].as_str() == Some(&target.api_key)
                    })
                {
                    run.fingerprint = fingerprint;
                    *run.token = target.api_key.clone();
                    retained.push(run.owner);
                    true
                } else {
                    false
                }
            });
        }
        retained
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
pub(crate) fn validate_account(
    vault: &aipass_vault::Vault,
    id: Uuid,
    token: &str,
) -> crate::session::ServiceResult<()> {
    use crate::session::{map_vault_error, ServiceError};
    let entry = vault.get_provider_summary(id).map_err(map_vault_error)?;
    let secret =
        aipass_provider_registry::primary_secret_ref(&entry.secret_refs).ok_or_else(|| {
            ServiceError::new(
                aipass_agent_protocol::AgentErrorCode::NotFound,
                "Claude credential missing",
            )
        })?;
    let current = vault
        .runtime_provider_credentials(id, &secret.id)
        .map_err(map_vault_error)?;
    if entry.provider_kind != aipass_provider_registry::ProviderKind::Official
        || entry.provider_id.as_deref() != Some("anthropic")
        || entry.credential_kind != aipass_provider_registry::CredentialKind::OAuth
        || vault.fingerprint_secret(current.secret.expose()) != vault.fingerprint_secret(token)
    {
        return Err(ServiceError::new(
            aipass_agent_protocol::AgentErrorCode::Conflict,
            "Claude account changed",
        ));
    }
    Ok(())
}
pub(crate) fn validate_native(raw: &str) -> Result<Value, String> {
    if raw.len() > 1024 * 1024 {
        return Err("Claude native credentials exceed limit".into());
    }
    let value: Value =
        serde_json::from_str(raw).map_err(|_| "invalid Claude native credentials")?;
    let oauth = &value["claudeAiOauth"];
    if ["accessToken", "refreshToken"]
        .iter()
        .any(|field| oauth[*field].as_str().is_none_or(|s| s.is_empty()))
    {
        return Err("incomplete Claude native credentials".into());
    }
    Ok(value)
}

/// Compare the complete native grant, including refresh-only rotations. An
/// identical candidate is a durable replay after a lost ACK or mirror failure.
pub(crate) fn persist_native_account(
    vault: &aipass_vault::Vault,
    id: Uuid,
    previous_token: &str,
    previous_grant_hash: [u8; 32],
    raw: &str,
) -> crate::session::ServiceResult<()> {
    use crate::session::{map_vault_error, ServiceError};
    use aipass_agent_protocol::AgentErrorCode;
    let candidate =
        validate_native(raw).map_err(|e| ServiceError::new(AgentErrorCode::ValidationFailed, e))?;
    let current = vault
        .provider_runtime_extension(id, "claude_native")
        .map_err(map_vault_error)?
        .ok_or_else(|| ServiceError::new(AgentErrorCode::Conflict, "Claude grant removed"))?;
    let current = validate_native(current.expose())
        .map_err(|_| ServiceError::new(AgentErrorCode::Conflict, "Claude grant changed"))?;
    let current_hash = hash(&current);
    if current_hash != hash(&candidate) {
        if current_hash != previous_grant_hash {
            return Err(ServiceError::new(
                AgentErrorCode::Conflict,
                "Claude grant changed",
            ));
        }
        validate_account(vault, id, previous_token)?;
        vault
            .set_provider_runtime_extension(
                id,
                "claude_native",
                Some(&aipass_crypto::SecretString::new(raw)),
            )
            .map_err(map_vault_error)?;
    } else {
        // Check entry ownership even on an idempotent replay; the mirror may
        // contain either side of this exact, already committed rotation.
        validate_account(vault, id, previous_token).or_else(|_| {
            validate_account(
                vault,
                id,
                candidate["claudeAiOauth"]["accessToken"].as_str().unwrap(),
            )
        })?;
    }
    crate::official_accounts::refresh_account_secret(
        vault,
        id,
        candidate["claudeAiOauth"]["accessToken"].as_str().unwrap(),
    )
    .map_err(ServiceError::internal)
}
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|_| "could not prepare native Claude configuration".into())
}
#[cfg(target_os = "macos")]
fn native_service(path: &Path) -> String {
    format!(
        "Claude Code-credentials-{}",
        &format!("{:x}", Sha256::digest(path.to_string_lossy().as_bytes()))[..8]
    )
}
fn read_native(path: &Path) -> Option<Value> {
    use std::io::Read;
    let mut bytes = zeroize::Zeroizing::new(Vec::new());
    let file = std::fs::File::open(path.join(".credentials.json"))
        .ok()
        .and_then(|file| file.take(1024 * 1024 + 1).read_to_end(&mut bytes).ok())
        .and_then(|_| std::str::from_utf8(&bytes).ok())
        .and_then(|s| validate_native(s).ok());
    #[cfg(target_os = "macos")]
    {
        let keychain = crate::official_accounts::read_keychain(&native_service(path)).ok();
        match (file, keychain) {
            (Some(file), Some(key)) => Some(
                if key["claudeAiOauth"]["expiresAt"].as_i64()
                    >= file["claudeAiOauth"]["expiresAt"].as_i64()
                {
                    key
                } else {
                    file
                },
            ),
            (file, key) => file.or(key),
        }
    }
    #[cfg(not(target_os = "macos"))]
    file
}
fn persist_native(inner: &Inner, key: &str) -> bool {
    let snapshot = inner.runs.lock().ok().and_then(|runs| {
        runs.get(key).filter(|r| r.native).map(|r| {
            (
                r.entry_id,
                r.token.clone(),
                r.native_grant_hash,
                r._directory.path().to_owned(),
            )
        })
    });
    let Some((entry_id, previous, Some(previous_grant_hash), path)) = snapshot else {
        return true;
    };
    let Some(value) = read_native(&path) else {
        return true;
    };
    let next_hash = hash(&value);
    if next_hash == previous_grant_hash {
        return true;
    }
    match crate::AgentClient::for_vault(inner.vault.clone()) {
        Ok(client) => match client.request::<Value>(&AgentRequest::ClaudeNativeWrite {
            entry_id,
            previous_token: SensitiveString::new(previous.as_str()),
            previous_grant_hash,
            credentials: SensitiveString::new(value.to_string()),
        }) {
            Ok(_) => {
                if let Ok(mut runs) = inner.runs.lock() {
                    if let Some(run) = runs.get_mut(key) {
                        if run.native_grant_hash == Some(previous_grant_hash) {
                            run.native_grant_hash = Some(next_hash);
                        }
                    }
                }
                true
            }
            Err(error) => !retry_native_persistence(error.code),
        },
        Err(_) => false,
    }
}

fn pending_native_grant(run: &Run) -> bool {
    run.native
        && read_native(run._directory.path())
            .is_some_and(|v| Some(hash(&v)) != run.native_grant_hash)
}

fn retry_native_persistence(code: Option<aipass_agent_protocol::AgentErrorCode>) -> bool {
    use aipass_agent_protocol::AgentErrorCode;
    !matches!(
        code,
        Some(
            AgentErrorCode::Locked
                | AgentErrorCode::Conflict
                | AgentErrorCode::NotFound
                | AgentErrorCode::ValidationFailed
        )
    )
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
    let candidates = std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .map(|p| p.join("claude"))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    candidates
        .into_iter()
        .chain([
            PathBuf::from("/opt/homebrew/bin/claude"),
            PathBuf::from("/usr/local/bin/claude"),
        ])
        .chain(std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/bin/claude")))
        .find(|p| p.is_file())
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
struct SegmentStream {
    receiver: tokio::sync::mpsc::Receiver<(Bytes, bool)>,
    inner: Weak<Inner>,
    key: String,
    complete: bool,
}
impl Stream for SegmentStream {
    type Item = Result<Bytes, Box<dyn std::error::Error + Send + Sync>>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.complete {
            return Poll::Ready(None);
        }
        match self.receiver.poll_recv(cx) {
            Poll::Ready(Some((bytes, complete))) => {
                self.complete = complete;
                Poll::Ready(Some(Ok(bytes)))
            }
            Poll::Ready(None) => {
                self.complete = true;
                Poll::Ready(Some(Err(std::io::Error::other(
                    "Claude process ended before completion",
                )
                .into())))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
impl Drop for SegmentStream {
    fn drop(&mut self) {
        if !self.complete {
            if let Some(inner) = self.inner.upgrade() {
                if let Ok(mut runs) = inner.runs.lock() {
                    runs.remove(&self.key);
                }
            }
        }
    }
}
#[derive(Default)]
struct Collector {
    message: Value,
    complete: bool,
}
impl Collector {
    fn event(&mut self, event: &Value) -> Result<(), String> {
        match event["type"].as_str() {
            Some("message_start") => {
                self.message = event["message"].clone();
                self.message["content"] = json!([]);
            }
            Some("content_block_start") => {
                let i = event["index"]
                    .as_u64()
                    .ok_or("invalid Claude block index")? as usize;
                if i > 4096 {
                    return Err("too many Claude blocks".into());
                }
                let content = self.message["content"]
                    .as_array_mut()
                    .ok_or("Claude block before message start")?;
                while content.len() <= i {
                    content.push(Value::Null);
                }
                content[i] = event["content_block"].clone();
            }
            Some("content_block_delta") => {
                let i = event["index"]
                    .as_u64()
                    .ok_or("invalid Claude block index")? as usize;
                let block = self.message["content"]
                    .as_array_mut()
                    .and_then(|v| v.get_mut(i))
                    .ok_or("unknown Claude block")?;
                let delta = &event["delta"];
                let (field, source) = match delta["type"].as_str() {
                    Some("text_delta") => ("text", "text"),
                    Some("thinking_delta") => ("thinking", "thinking"),
                    Some("signature_delta") => ("signature", "signature"),
                    Some("input_json_delta") => ("_arguments", "partial_json"),
                    _ => return Ok(()),
                };
                let mut value = block[field].as_str().unwrap_or("").to_owned();
                value.push_str(delta[source].as_str().unwrap_or(""));
                block[field] = json!(value);
            }
            Some("content_block_stop") => {
                let i = event["index"].as_u64().unwrap_or(0) as usize;
                if let Some(block) = self.message["content"]
                    .as_array_mut()
                    .and_then(|v| v.get_mut(i))
                {
                    if let Some(args) = block.get("_arguments").and_then(Value::as_str) {
                        block["input"] = serde_json::from_str(args)
                            .map_err(|_| "invalid Claude tool arguments")?;
                        block.as_object_mut().unwrap().remove("_arguments");
                    }
                }
            }
            Some("message_delta") => {
                self.message["stop_reason"] = event["delta"]["stop_reason"].clone();
                if let Some(usage) = event["usage"].as_object() {
                    for (k, v) in usage {
                        self.message["usage"][k] = v.clone();
                    }
                }
            }
            Some("message_stop") => self.complete = true,
            Some("error") => return Err("Claude reported an upstream error".into()),
            _ => {}
        }
        Ok(())
    }
    fn push(&mut self, bytes: &[u8]) -> Result<(), String> {
        let text = std::str::from_utf8(bytes).map_err(|_| "invalid Claude stream encoding")?;
        for line in text.lines().filter_map(|l| l.strip_prefix("data: ")) {
            self.event(&serde_json::from_str::<Value>(line).map_err(|_| "invalid Claude event")?)?;
        }
        Ok(())
    }
}
fn read_events(stdout: std::process::ChildStdout, inner: Weak<Inner>, key: String) {
    use std::io::Read;
    let mut reader = BufReader::new(stdout);
    let mut collector = Collector::default();
    loop {
        let mut line = Vec::new();
        let Ok(n) = reader
            .by_ref()
            .take(MAX_LINE + 1)
            .read_until(b'\n', &mut line)
        else {
            break;
        };
        if n == 0 || n as u64 > MAX_LINE {
            break;
        }
        let parsed = serde_json::from_slice::<Value>(&line);
        line.zeroize();
        let Ok(value) = parsed else {
            break;
        };
        if value["type"] == "result" && value["is_error"] == true {
            break;
        }
        if value["type"] != "stream_event" {
            continue;
        }
        let mut event = value["event"].clone();
        if let Some(name) = event
            .pointer("/content_block/name")
            .and_then(Value::as_str)
            .and_then(|n| n.strip_prefix("mcp__aipass__"))
        {
            event["content_block"]["name"] = json!(name);
        }
        if event["type"] == "content_block_start" && event["content_block"]["type"] == "tool_use" {
            event["content_block"]["id"] =
                json!(format!("toolu_aipass_claude_{}", Uuid::new_v4().simple()));
        }
        if event["type"] == "message_start" {
            collector = Collector::default();
        }
        if collector.event(&event).is_err() {
            break;
        }
        let Some(inner) = inner.upgrade() else {
            break;
        };
        let sender = {
            let Ok(mut runs) = inner.runs.lock() else {
                break;
            };
            let Some(run) = runs.get_mut(&key) else {
                break;
            };
            if event["type"] == "content_block_stop" {
                let index = event["index"].as_u64().unwrap_or(0) as usize;
                if let Some(block) = collector.message["content"]
                    .as_array()
                    .and_then(|c| c.get(index))
                    .filter(|b| b["type"] == "tool_use")
                {
                    run.calls.push(ToolCall {
                        id: block["id"].as_str().unwrap_or("").into(),
                        name: block["name"].as_str().unwrap_or("").into(),
                        arguments: block["input"].clone(),
                        claimed: false,
                        result: None,
                    });
                }
            }
            run.message = collector.message.clone();
            run.content = collector.message["content"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let sender = run.sender.clone();
            if collector.complete {
                run.terminal = true;
                run.parked = run.message["stop_reason"] == "tool_use";
                run.last_used = Instant::now();
                run.history_messages
                    .push(json!({"role":"assistant","content":run.content}));
                run.history = history_hash(&run.history_messages);
                run.sender = None;
            }
            sender
        };
        if collector.complete {
            persist_native(&inner, &key);
        }
        if let Some(sender) = sender {
            let kind = event["type"].as_str().unwrap_or("message");
            if sender
                .blocking_send((
                    Bytes::from(format!("event: {kind}\ndata: {event}\n\n")),
                    collector.complete,
                ))
                .is_err()
            {
                break;
            }
        }
    }
    if let Some(inner) = inner.upgrade() {
        if let Ok(mut runs) = inner.runs.lock() {
            runs.remove(&key);
        }
    }
}

/// Stdio MCP helper launched only by the Agent's Claude child. The normal IPC
/// auth frame and the unguessable run capability are both required.
pub fn run_mcp_helper(vault: PathBuf) -> anyhow::Result<()> {
    let mut capability = std::env::var("AIPASS_CLAUDE_CAPABILITY")?;
    let client = crate::AgentClient::for_vault(vault)?;
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let mut stdout = std::io::stdout();
    loop {
        use std::io::Read;
        let mut line = String::new();
        let n = reader.by_ref().take(MAX_LINE + 1).read_line(&mut line)?;
        if n == 0 {
            break;
        }
        if n as u64 > MAX_LINE {
            anyhow::bail!("MCP request exceeds limit");
        }
        let request: Value = serde_json::from_str(&line)?;
        line.zeroize();
        let Some(id) = request.get("id") else {
            continue;
        };
        let call = |request| {
            client.request::<Value>(&AgentRequest::ClaudeBridgeMcp {
                capability: SensitiveString::new(&capability),
                request,
            })
        };
        let result: Result<Value, String> = match request["method"].as_str() {
            Some("initialize") => Ok(
                json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"aipass","version":"1"}}),
            ),
            Some("ping") => Ok(json!({})),
            Some("tools/list") => call(ClaudeBridgeRequest::ListTools).map_err(|e| e.to_string()),
            Some("tools/call") => (|| {
                let deadline = Instant::now() + Duration::from_secs(10);
                let start = loop {
                    let value = call(ClaudeBridgeRequest::Call {
                        name: request["params"]["name"]
                            .as_str()
                            .ok_or("missing tool name")?
                            .into(),
                        arguments: request["params"]["arguments"].clone(),
                    })
                    .map_err(|e| e.to_string())?;
                    if value.get("callId").is_some() {
                        break value;
                    }
                    if Instant::now() >= deadline {
                        return Err("tool call does not match the model request".into());
                    }
                    std::thread::sleep(Duration::from_millis(25));
                };
                let id = start["callId"].as_str().ok_or("missing call ID")?;
                let deadline = Instant::now() + PARK_TTL;
                loop {
                    let value = call(ClaudeBridgeRequest::Poll { call_id: id.into() })
                        .map_err(|e| e.to_string())?;
                    if let Some(result) = value.get("result") {
                        return Ok(result.clone());
                    }
                    if Instant::now() >= deadline {
                        return Err("tool result timed out".into());
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            })(),
            _ => Err("unsupported MCP method".into()),
        };
        let response = match result {
            Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
            Err(message) => {
                json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":message}})
            }
        };
        writeln!(stdout, "{response}")?;
        stdout.flush()?;
    }
    capability.zeroize();
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn refresh_only_rotations_are_durable_and_stale_native_writers_cannot_overwrite_them() {
        let temp = tempfile::tempdir().unwrap();
        let vault = aipass_vault::Vault::create(
            temp.path(),
            &aipass_crypto::SecretString::new("test password"),
        )
        .unwrap()
        .vault;
        let original = json!({"claudeAiOauth":{"accessToken":"same-access","refreshToken":"old-refresh","expiresAt":123456}});
        let id = crate::official_accounts::persist_login_account(
            &vault,
            "anthropic",
            Some("alice".into()),
            None,
            "same-access".into(),
            None,
        )
        .unwrap();
        vault
            .set_provider_runtime_extension(
                id,
                "claude_native",
                Some(&aipass_crypto::SecretString::new(original.to_string())),
            )
            .unwrap();
        let mut rotated = original.clone();
        rotated["claudeAiOauth"]["refreshToken"] = json!("new-refresh");
        persist_native_account(
            &vault,
            id,
            "same-access",
            hash(&original),
            &rotated.to_string(),
        )
        .unwrap();
        let saved = vault
            .provider_runtime_extension(id, "claude_native")
            .unwrap()
            .unwrap();
        assert_eq!(validate_native(saved.expose()).unwrap(), rotated);
        // Lost ACK retries are idempotent, even though the previous grant changed.
        let revision = vault.sync_revision().unwrap();
        persist_native_account(
            &vault,
            id,
            "same-access",
            hash(&original),
            &rotated.to_string(),
        )
        .unwrap();
        assert_eq!(vault.sync_revision().unwrap(), revision);
        let mut stale = original.clone();
        stale["claudeAiOauth"]["refreshToken"] = json!("stale-refresh");
        let error = persist_native_account(
            &vault,
            id,
            "same-access",
            hash(&original),
            &stale.to_string(),
        )
        .unwrap_err();
        assert_eq!(error.code, aipass_agent_protocol::AgentErrorCode::Conflict);
        let saved = vault
            .provider_runtime_extension(id, "claude_native")
            .unwrap()
            .unwrap();
        assert_eq!(validate_native(saved.expose()).unwrap(), rotated);
    }

    #[test]
    fn native_refresh_only_rotation_remains_pending_without_access_change() {
        let (_temp, bridge, target) = fixture("while IFS= read -r line; do :; done");
        let stream = bridge
            .open(
                &target,
                &json!({"model":"claude-sonnet","messages":[{"role":"user","content":"test"}]}),
                None,
            )
            .unwrap();
        {
            let mut runs = bridge.inner.runs.lock().unwrap();
            let run = runs.values_mut().next().unwrap();
            let original = json!({"claudeAiOauth":{"accessToken":"synthetic-token","refreshToken":"old-refresh","expiresAt":123456}});
            run.native = true;
            run.native_grant_hash = Some(hash(&original));
            let path = run._directory.path().join(".credentials.json");
            write_private(&path, original.to_string().as_bytes()).unwrap();
            assert!(!pending_native_grant(run));
            let mut rotated = original;
            rotated["claudeAiOauth"]["refreshToken"] = json!("new-refresh");
            std::fs::write(&path, rotated.to_string()).unwrap();
            assert!(pending_native_grant(run));
            run.native_grant_hash = Some(hash(&rotated));
            assert!(!pending_native_grant(run));
        }
        drop(stream);
        bridge.revoke();
    }

    #[test]
    fn rotated_native_grants_retry_io_but_never_restore_revoked_accounts() {
        use aipass_agent_protocol::AgentErrorCode;
        assert!(retry_native_persistence(Some(AgentErrorCode::Internal)));
        assert!(retry_native_persistence(Some(
            AgentErrorCode::ServiceUnavailable
        )));
        for code in [
            AgentErrorCode::Locked,
            AgentErrorCode::Conflict,
            AgentErrorCode::NotFound,
            AgentErrorCode::ValidationFailed,
        ] {
            assert!(!retry_native_persistence(Some(code)));
        }
    }
    use std::os::unix::fs::PermissionsExt;
    fn fixture(script: &str) -> (tempfile::TempDir, ClaudeBridge, ResolvedTarget) {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("claude");
        std::fs::write(&path, format!("#!/bin/sh\n{script}")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let bridge = ClaudeBridge::new(temp.path());
        *bridge.inner.binary.lock().unwrap() = Some(path);
        let config=serde_json::from_value(json!({"id":Uuid::new_v4(),"providerEntryId":Uuid::new_v4(),"secretId":"test","label":"test","baseUrl":"https://api.anthropic.com","authScheme":"bearer","enabled":true,"priority":0,"weight":1,"headers":[]})).unwrap();
        let target = ResolvedTarget {
            upstream_proxy: None,
            model_override: None,
            profile: Default::default(),
            upstream_kind: aipass_proxy::UpstreamKind::ClaudeSubscription,
            quota: Vec::new(),
            max_concurrent_requests: None,
            supports_websockets: false,
            config,
            api_key: "synthetic-token".into(),
        };
        (temp, bridge, target)
    }
    async fn collect(mut stream: SubscriptionStream) -> Value {
        let mut collector = Collector::default();
        while let Some(chunk) = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap()
        {
            collector.push(&chunk.unwrap()).unwrap();
        }
        assert!(collector.complete);
        collector.message
    }
    #[tokio::test]
    async fn parallel_tools_resume_original_process_and_reject_partial_or_replayed_results() {
        let script = r#"read -r input
cat <<'EVENTS'
{"type":"stream_event","event":{"type":"message_start","message":{"id":"msg_1","role":"assistant","usage":{"input_tokens":10}}}}
{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"native_a","name":"mcp__aipass__lookup","input":{"n":1}}}}
{"type":"stream_event","event":{"type":"content_block_stop","index":0}}
{"type":"stream_event","event":{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"native_b","name":"mcp__aipass__lookup","input":{"n":2}}}}
{"type":"stream_event","event":{"type":"content_block_stop","index":1}}
{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":20}}}
{"type":"stream_event","event":{"type":"message_stop"}}
EVENTS
while [ ! -f .resume ]; do sleep 0.01; done
cat <<'EVENTS'
{"type":"stream_event","event":{"type":"message_start","message":{"id":"msg_2","role":"assistant","usage":{"input_tokens":30}}}}
{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":"done"}}}
{"type":"stream_event","event":{"type":"content_block_stop","index":0}}
{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}}}
{"type":"stream_event","event":{"type":"message_stop"}}
EVENTS
while read -r input; do :; done
"#;
        let (_temp, bridge, target) = fixture(script);
        let mut payload = json!({"model":"claude-test","stream":true,"messages":[{"role":"user","content":"go"}],"tools":[{"name":"lookup","input_schema":{"type":"object"}}]});
        let first = collect(bridge.open(&target, &payload, None).unwrap()).await;
        let ids = first["content"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert!(ids.iter().all(|id| id.starts_with("toolu_aipass_claude_")));
        let (capability, pid, path) = {
            let runs = bridge.inner.runs.lock().unwrap();
            let (key, run) = runs.iter().next().unwrap();
            (
                key.clone(),
                run.child.id(),
                run._directory.path().to_owned(),
            )
        };
        for (index, id) in ids.iter().enumerate() {
            assert_eq!(
                bridge
                    .mcp(
                        &capability,
                        ClaudeBridgeRequest::Call {
                            name: "lookup".into(),
                            arguments: json!({"n":index+1})
                        }
                    )
                    .unwrap()["callId"],
                *id
            );
        }
        payload["messages"]
            .as_array_mut()
            .unwrap()
            .push(json!({"role":"assistant","content":first["content"]}));
        payload["messages"].as_array_mut().unwrap().push(json!({"role":"user","content":[{"type":"tool_result","tool_use_id":ids[0],"content":"one"}]}));
        assert!(bridge.open(&target, &payload, None).is_err());
        assert!(bridge.inner.runs.lock().unwrap()[&capability]
            .sender
            .is_none());
        payload["messages"][2]["content"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"tool_result","tool_use_id":ids[1],"content":"two"}));
        let stream = bridge.open(&target, &payload, None).unwrap();
        let one = bridge
            .mcp(
                &capability,
                ClaudeBridgeRequest::Poll {
                    call_id: ids[0].clone(),
                },
            )
            .unwrap();
        assert_eq!(
            one,
            bridge
                .mcp(
                    &capability,
                    ClaudeBridgeRequest::Poll {
                        call_id: ids[0].clone()
                    }
                )
                .unwrap()
        );
        assert_eq!(one["result"]["content"][0]["text"], "one");
        std::fs::write(path.join(".resume"), b"").unwrap();
        let second = collect(stream).await;
        assert_eq!(second["content"][0]["text"], "done");
        assert_eq!(
            bridge.inner.runs.lock().unwrap()[&capability].child.id(),
            pid
        );
        assert!(bridge.open(&target, &payload, None).is_err());
        bridge.revoke();
        assert!(bridge.inner.runs.lock().unwrap().is_empty());
        assert!(!path.exists());
    }
    #[tokio::test]
    async fn cancellation_kills_process_and_capability() {
        let (_temp, bridge, target) = fixture("read -r input\nwhile read -r input; do :; done\n");
        let stream = bridge
            .open(
                &target,
                &json!({"model":"test","messages":[{"role":"user","content":"wait"}]}),
                None,
            )
            .unwrap();
        let capability = bridge
            .inner
            .runs
            .lock()
            .unwrap()
            .keys()
            .next()
            .unwrap()
            .clone();
        drop(stream);
        assert!(bridge.inner.runs.lock().unwrap().is_empty());
        assert!(bridge
            .mcp(&capability, ClaudeBridgeRequest::ListTools)
            .is_err());
    }
    #[test]
    fn history_only_ignores_nonportable_assistant_thinking() {
        let a = json!([{"role":"assistant","content":[{"type":"thinking","thinking":"x","signature":"provider"},{"type":"text","text":"answer"}]}]);
        let b = json!([{"role":"assistant","content":[{"type":"text","text":"<reasoning>x</reasoning>"},{"type":"text","text":"answer"}]}]);
        assert_eq!(
            history_hash(a.as_array().unwrap()),
            history_hash(b.as_array().unwrap())
        );
        let c = json!([{"role":"assistant","content":[{"type":"text","text":"changed answer"}]}]);
        assert_ne!(
            history_hash(a.as_array().unwrap()),
            history_hash(c.as_array().unwrap())
        );
    }
}
