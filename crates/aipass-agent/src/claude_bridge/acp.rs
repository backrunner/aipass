//! Rust-owned ACP v1 session adapter for the official Claude Code control protocol.
//! The proxy is an ACP client over an in-process JSON-RPC transport. No provider
//! engine or JavaScript adapter is downloaded, and no print-mode fallback exists.
use super::*;
use std::sync::mpsc::{self, Receiver, SyncSender};

mod content;
mod events;
mod lifecycle;
mod permissions;

pub(super) fn validate_prompt(prompt: &Value) -> Result<(), String> {
    if prompt.to_string().len() > MAX_LINE as usize - 1024 {
        return Err("Claude handoff exceeds input limit".into());
    }
    content::to_acp(prompt).map(|_| ())
}

pub(super) struct Connection {
    pub(super) child: Child,
    pub(super) client: Client,
    pub(super) output: Receiver<Value>,
}

#[derive(Clone)]
pub(super) struct Client {
    state: Arc<Mutex<State>>,
    output: SyncSender<Value>,
}
struct State {
    stdin: ChildStdin,
    session: String,
    cwd: PathBuf,
    initialized: bool,
    ready: bool,
    opening: Option<Value>,
    prompt: Option<Value>,
    stop_reason: String,
    cancelling: bool,
    streamed_message: Option<String>,
    permissions: HashMap<String, Value>,
    mode_changes: HashMap<String, (String, Value)>,
    main_message: bool,
    deferred_updates: Vec<Value>,
    child_calls: std::collections::HashSet<String>,
    lifecycle: lifecycle::Lifecycle,
}

impl Connection {
    pub(super) fn spawn(command: &mut Command, session: Uuid) -> Result<Self, String> {
        let cwd = command
            .get_current_dir()
            .ok_or("ACP requires cwd")?
            .to_owned();
        let mut child = command.spawn().map_err(|_| "could not start Claude Code")?;
        let stdout = child.stdout.take().ok_or("Claude output unavailable")?;
        let stdin = child.stdin.take().ok_or("Claude input unavailable")?;
        let (output, receiver) = mpsc::sync_channel(8);
        let client = Client {
            state: Arc::new(Mutex::new(State {
                stdin,
                session: session.to_string(),
                cwd: cwd.clone(),
                initialized: false,
                ready: false,
                opening: None,
                prompt: None,
                stop_reason: "end_turn".into(),
                cancelling: false,
                streamed_message: None,
                permissions: HashMap::new(),
                mode_changes: HashMap::new(),
                main_message: false,
                deferred_updates: Vec::new(),
                child_calls: Default::default(),
                lifecycle: Default::default(),
            })),
            output,
        };
        let reader_client = client.clone();
        std::thread::spawn(move || events::read(stdout, reader_client));
        Ok(Self {
            child,
            client,
            output: receiver,
        })
    }
}
pub(super) fn initialize(
    client: &Client,
    output: &Receiver<Value>,
    cwd: &Path,
) -> Result<(), String> {
    client.send(json!({"jsonrpc":"2.0","id":"initialize","method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{}}}))?;
    expect(output, "initialize")?;
    client.send(json!({"jsonrpc":"2.0","id":"new","method":"session/new","params":{"cwd":cwd,"mcpServers":[]}}))?;
    expect(output, "new")
}
fn expect(output: &Receiver<Value>, id: &str) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let response = output
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| "Claude ACP initialization failed or timed out")?;
        if response["id"] == id {
            return if response.get("result").is_some() {
                Ok(())
            } else {
                Err("Claude ACP initialization rejected".into())
            };
        }
        if response["method"] == "_aipass/closed" {
            return Err("Claude Code closed during ACP initialization".into());
        }
    }
}

impl Client {
    pub(super) fn pending_work(&self) -> bool {
        self.state.lock().map_or(true, |s| s.lifecycle.pending())
    }
    pub(super) fn background_active(&self) -> bool {
        self.state.lock().map_or(true, |s| s.lifecycle.active())
    }
    pub(super) fn prompt(&self, native: Value) -> Result<(), String> {
        validate_prompt(&native)?;
        let session = self
            .state
            .lock()
            .map_err(|_| "ACP session unavailable")?
            .session
            .clone();
        let prompt = content::to_acp(&native)?;
        self.send(json!({"jsonrpc":"2.0","id":Uuid::new_v4().to_string(),"method":"session/prompt","params":{"sessionId":session,"prompt":prompt}}))
    }
    pub(super) fn cancel(&self) {
        let Ok(state) = self.state.lock() else { return };
        let session = state.session.clone();
        drop(state);
        let _ = self.send(
            json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":session}}),
        );
    }
    pub(super) fn respond(&self, id: &str, result: Value) -> Result<(), String> {
        self.send(json!({"jsonrpc":"2.0","id":id,"result":result}))
    }
    pub(super) fn validate_reply(&self, id: &str, result: &Value) -> Result<(), String> {
        let state = self.state.lock().map_err(|_| "ACP session unavailable")?;
        permissions::response(&state, &json!({"id":id,"result":result})).map(|_| ())
    }
    fn emit(&self, value: Value) -> Result<(), String> {
        self.output
            .send(value)
            .map_err(|_| "ACP client disconnected".into())
    }
    fn send(&self, message: Value) -> Result<(), String> {
        if message["jsonrpc"] != "2.0" {
            return Err("ACP requires JSON-RPC 2.0".into());
        }
        let mut state = self.state.lock().map_err(|_| "ACP session unavailable")?;
        if message.get("method").is_none() {
            return permissions::respond(&mut state, &message);
        }
        let id = message["id"].clone();
        let params = &message["params"];
        match message["method"].as_str() {
            Some("initialize") => {
                if state.initialized || params["protocolVersion"] != 1 {
                    return Err("unsupported ACP initialization".into());
                }
                state.initialized = true;
                drop(state);
                self.emit(json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":1,"agentCapabilities":{"promptCapabilities":{"image":true,"embeddedContext":true},"loadSession":false},"agentInfo":{"name":"aipass-claude","version":env!("CARGO_PKG_VERSION")},"authMethods":[]}}))
            }
            Some("session/new") => {
                if !state.initialized
                    || state.opening.is_some()
                    || state.ready
                    || params["cwd"].as_str().map(Path::new) != Some(state.cwd.as_path())
                    || !params["mcpServers"].as_array().is_some_and(Vec::is_empty)
                {
                    return Err("invalid ACP session setup".into());
                }
                state.opening = Some(id);
                write(
                    &mut state.stdin,
                    &json!({"type":"control_request","request_id":"aipass-initialize","request":{"subtype":"initialize","hooks":{},"forwardSubagentText":true}}),
                )
            }
            Some("session/prompt" | "session/cancel") => {
                if !state.ready || params["sessionId"] != state.session {
                    return Err("unknown ACP session".into());
                }
                if message["method"] == "session/cancel" {
                    state.cancelling = true;
                    state.permissions.clear();
                    state.mode_changes.clear();
                    state.lifecycle.queued_user = None;
                    return write(
                        &mut state.stdin,
                        &json!({"type":"control_request","request_id":"aipass-interrupt","request":{"subtype":"interrupt"}}),
                    );
                }
                if state.prompt.is_some() {
                    return Err("ACP session already has an active prompt".into());
                }
                let content = content::to_native(&params["prompt"])?;
                let user = json!({"type":"user","session_id":state.session,"message":{"role":"user","content":content},"parent_tool_use_id":null});
                let State {
                    lifecycle, stdin, ..
                } = &mut *state;
                lifecycle.send(stdin, user)?;
                state.prompt = Some(id);
                state.cancelling = false;
                state.stop_reason = "end_turn".into();
                Ok(())
            }
            _ => Err("unsupported ACP method".into()),
        }
    }
}

fn write(stdin: &mut ChildStdin, message: &Value) -> Result<(), String> {
    let mut encoded = message.to_string();
    if encoded.len() > MAX_LINE as usize {
        return Err("Claude control message exceeds limit".into());
    }
    let result = writeln!(stdin, "{encoded}").and_then(|_| stdin.flush());
    encoded.zeroize();
    result.map_err(|_| "Claude control input closed".into())
}
pub(super) fn stop(child: &mut Child) {
    #[cfg(windows)]
    crate::windows_process::terminate_tree(child.id());
    #[cfg(unix)]
    if let Some(pid) = rustix::process::Pid::from_raw(child.id() as i32) {
        let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(all(test, unix))]
mod runtime_tests;
#[cfg(all(test, unix))]
mod tests;
