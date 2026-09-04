use crate::ipc;
use crate::launcher;
use crate::paths::{canonical_vault_dir, default_vault_dir, namespace_for_vault_dir};
#[cfg(target_os = "windows")]
use crate::windows_service;
use aipass_agent_protocol::{
    read_frame, write_frame, AgentErrorCode, AgentRequest, AgentResponse,
    AuthenticatedAgentRequest, SessionStatus, AGENT_PROTOCOL_VERSION,
};
use anyhow::Result;
use serde::de::DeserializeOwned;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

const AGENT_READY_TIMEOUT: Duration = Duration::from_secs(15);
/// The first sync after launch can legitimately outlast the boot timeout;
/// readiness waits for it, but only within this extra bounded grace window.
const INITIAL_SYNC_READY_GRACE: Duration = Duration::from_secs(120);
const AGENT_READY_POLL_INTERVAL: Duration = Duration::from_millis(100);
#[cfg(target_os = "macos")]
const AUTOSTART_RECOVERY_GRACE: Duration = Duration::from_secs(3);
/// A send should never take as long as the work behind a response.
const REQUEST_SEND_TIMEOUT: Duration = Duration::from_secs(30);

// Desktop setup warms the resident agent while the first frontend request may
// also call ensure_running. Serialize those paths so they cannot both trigger
// LaunchAgent repair and restart the supervisor underneath each other.
static AGENT_START_LOCK: Mutex<()> = Mutex::new(());

fn apply_request_timeouts(
    stream: &interprocess::local_socket::Stream,
    response_timeout: Duration,
) -> std::result::Result<(), AgentCommandError> {
    use interprocess::local_socket::traits::Stream as _;
    stream
        .set_send_timeout(Some(REQUEST_SEND_TIMEOUT))
        .and_then(|()| stream.set_recv_timeout(Some(response_timeout)))
        .map_err(|err| AgentCommandError {
            code: Some(AgentErrorCode::Internal),
            message: format!("failed to set agent request timeout: {err}"),
        })
}

/// A timed-out agent is reported as unavailable rather than an internal fault,
/// so callers retry or fall back to their "agent not reachable" path.
fn timeout_aware_error(err: anyhow::Error) -> AgentCommandError {
    let timed_out = err
        .downcast_ref::<std::io::Error>()
        .is_some_and(|err| matches!(err.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock));
    if timed_out {
        return AgentCommandError {
            code: Some(AgentErrorCode::ServiceUnavailable),
            message: format!("agent did not respond in time: {err}"),
        };
    }
    AgentCommandError::internal(err)
}

#[derive(Clone, Debug)]
pub struct AgentClientConfig {
    pub vault_dir: PathBuf,
    pub namespace: String,
}

impl AgentClientConfig {
    pub fn for_vault(vault_dir: PathBuf) -> Result<Self> {
        let vault_dir = canonical_vault_dir(vault_dir)?;
        let namespace = namespace_for_vault_dir(&vault_dir)?;
        Ok(Self {
            vault_dir,
            namespace,
        })
    }

    pub fn default_vault() -> Result<Self> {
        Self::for_vault(default_vault_dir()?)
    }
}

#[derive(Clone, Debug)]
pub struct AgentClient {
    pub config: AgentClientConfig,
}

#[derive(Debug)]
pub struct AgentCommandError {
    pub code: Option<AgentErrorCode>,
    pub message: String,
}

impl std::fmt::Display for AgentCommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for AgentCommandError {}

impl AgentClient {
    pub fn new(config: AgentClientConfig) -> Self {
        Self { config }
    }

    pub fn for_vault(vault_dir: PathBuf) -> Result<Self> {
        Ok(Self::new(AgentClientConfig::for_vault(vault_dir)?))
    }

    pub fn default_vault() -> Result<Self> {
        Ok(Self::new(AgentClientConfig::default_vault()?))
    }

    pub fn request_raw(
        &self,
        request: &AgentRequest,
    ) -> std::result::Result<AgentResponse, AgentCommandError> {
        let mut stream = ipc::connect(&self.config.vault_dir).map_err(|err| AgentCommandError {
            code: Some(AgentErrorCode::ServiceUnavailable),
            message: err.to_string(),
        })?;
        let auth_token =
            ipc::read_auth_token(&self.config.vault_dir).map_err(|err| AgentCommandError {
                code: Some(AgentErrorCode::ServiceUnavailable),
                message: err.to_string(),
            })?;
        // Without a deadline a wedged agent hangs the caller forever — the tray
        // polls on a timer and the desktop issues these from UI commands, so a
        // stuck read is never recovered from.
        apply_request_timeouts(&stream, request.response_timeout())?;
        let payload = AuthenticatedAgentRequest {
            protocol_version: AGENT_PROTOCOL_VERSION,
            auth_token,
            request: request.clone(),
        };
        write_frame(&mut stream, &payload).map_err(timeout_aware_error)?;
        read_frame(&mut stream).map_err(timeout_aware_error)
    }

    pub fn request<T: DeserializeOwned>(
        &self,
        request: &AgentRequest,
    ) -> std::result::Result<T, AgentCommandError> {
        let response = self.request_raw(request)?;
        decode_response(response)
    }

    pub fn ensure_running(&self) -> Result<()> {
        self.ensure_running_with_mode(AgentStartupMode::Autostart)
    }

    pub fn ensure_running_for_app(&self) -> Result<()> {
        self.ensure_running_with_mode(AgentStartupMode::Direct {
            suppress_desktop_tray: false,
        })
    }

    pub fn ensure_running_for_desktop_companion(&self) -> Result<()> {
        self.ensure_running_with_mode(AgentStartupMode::Direct {
            suppress_desktop_tray: true,
        })
    }

    fn ensure_running_with_mode(&self, mode: AgentStartupMode) -> Result<()> {
        let _startup_guard = AGENT_START_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let initial_connection_error =
            match self.request::<SessionStatus>(&AgentRequest::SessionStatus) {
                Ok(status) if !status.initial_sync_pending => return Ok(()),
                Ok(_) => {
                    // The agent is already up but still running its first
                    // sync: wait for readiness without launching a second
                    // instance underneath it.
                    return self.wait_for_ready(None, &[], "agent initial sync pending", false);
                }
                Err(err) => err.to_string(),
            };
        #[cfg(target_os = "windows")]
        let (launched_binary, binary_candidates) = if mode.install_autostart() {
            let candidates = launcher::agent_binary_candidates();
            if let Err(err) = windows_service::start_service(&self.config.vault_dir) {
                anyhow::bail!(launcher::windows_service_start_failure_message(
                    &self.config.vault_dir,
                    &self.config.namespace,
                    &initial_connection_error,
                    &err.to_string(),
                ));
            }
            (None, candidates)
        } else {
            let launch = launcher::launch_agent(
                &self.config.vault_dir,
                &self.config.namespace,
                &initial_connection_error,
                mode.launch_options(),
            )?;
            (Some(launch.binary), launch.candidates)
        };
        #[cfg(not(target_os = "windows"))]
        let (launched_binary, binary_candidates) = match launcher::agent_binary_path() {
            Ok(agent_binary) => {
                let candidates = launcher::agent_binary_candidates();
                if mode.install_autostart() {
                    #[cfg(target_os = "macos")]
                    let install_result =
                        crate::autostart::ensure_autostart(&agent_binary, &self.config.vault_dir);
                    #[cfg(not(target_os = "macos"))]
                    let install_result =
                        crate::autostart::install_autostart(&agent_binary, &self.config.vault_dir);
                    match install_result {
                        Ok(_) => (Some(agent_binary), candidates),
                        Err(_) => {
                            let launch = launcher::launch_agent(
                                &self.config.vault_dir,
                                &self.config.namespace,
                                &initial_connection_error,
                                mode.launch_options(),
                            )?;
                            (Some(launch.binary), launch.candidates)
                        }
                    }
                } else {
                    let launch = launcher::launch_agent(
                        &self.config.vault_dir,
                        &self.config.namespace,
                        &initial_connection_error,
                        mode.launch_options(),
                    )?;
                    (Some(launch.binary), launch.candidates)
                }
            }
            Err(_) => {
                let launch = launcher::launch_agent(
                    &self.config.vault_dir,
                    &self.config.namespace,
                    &initial_connection_error,
                    mode.launch_options(),
                )?;
                (Some(launch.binary), launch.candidates)
            }
        };
        self.wait_for_ready(
            launched_binary,
            &binary_candidates,
            &initial_connection_error,
            mode.install_autostart(),
        )
    }

    fn wait_for_ready(
        &self,
        launched_binary: Option<PathBuf>,
        binary_candidates: &[PathBuf],
        initial_connection_error: &str,
        repair_autostart: bool,
    ) -> Result<()> {
        #[cfg(not(target_os = "macos"))]
        let _ = repair_autostart;
        let started = Instant::now();
        let mut deadline = ready_deadline(started, false);
        #[cfg(target_os = "macos")]
        let mut force_repair_at =
            repair_autostart.then(|| Instant::now() + AUTOSTART_RECOVERY_GRACE);
        let last_connection_error = loop {
            match self.request::<SessionStatus>(&AgentRequest::SessionStatus) {
                Ok(status) if status.initial_sync_pending => {
                    // A pending first sync is not readiness, but it does earn
                    // a bounded extension over the plain boot timeout.
                    deadline = ready_deadline(started, true);
                    if Instant::now() >= deadline {
                        break "agent initial sync did not finish in time".to_string();
                    }
                }
                Ok(_) => return Ok(()),
                Err(err) => {
                    let message = err.to_string();
                    #[cfg(target_os = "macos")]
                    if force_repair_at.is_some_and(|repair_at| Instant::now() >= repair_at) {
                        force_repair_at = None;
                        if let Some(agent_binary) = launched_binary.as_deref() {
                            let _ = crate::autostart::install_autostart(
                                agent_binary,
                                &self.config.vault_dir,
                            );
                        }
                    }
                    if Instant::now() >= deadline {
                        break message;
                    }
                }
            }
            thread::sleep(AGENT_READY_POLL_INTERVAL);
        };
        Err(anyhow::anyhow!(launcher::agent_ready_timeout_message(
            &self.config.vault_dir,
            &self.config.namespace,
            launched_binary.as_deref(),
            binary_candidates,
            initial_connection_error,
            Some(&last_connection_error),
        )))
    }

    pub fn shutdown(&self) -> Result<()> {
        self.request::<serde_json::Value>(&AgentRequest::AgentShutdown)
            .map(|_| ())
            .map_err(anyhow::Error::from)
    }
}

#[derive(Clone, Copy, Debug)]
enum AgentStartupMode {
    Autostart,
    Direct { suppress_desktop_tray: bool },
}

impl AgentStartupMode {
    fn install_autostart(self) -> bool {
        matches!(self, Self::Autostart)
    }

    fn launch_options(self) -> launcher::AgentLaunchOptions {
        launcher::AgentLaunchOptions {
            suppress_desktop_tray: matches!(
                self,
                Self::Autostart
                    | Self::Direct {
                        suppress_desktop_tray: true
                    }
            ),
        }
    }
}

/// How long to keep waiting for agent readiness based on the latest status
/// poll: a pending first sync extends the base boot deadline by a bounded
/// grace window; once the sync settles (or never started) the base timeout
/// applies again.
fn ready_deadline(started: Instant, initial_sync_pending: bool) -> Instant {
    let base = started + AGENT_READY_TIMEOUT;
    if initial_sync_pending {
        base + INITIAL_SYNC_READY_GRACE
    } else {
        base
    }
}

impl AgentCommandError {
    fn internal(err: impl Into<anyhow::Error>) -> Self {
        Self {
            code: Some(AgentErrorCode::Internal),
            message: err.into().to_string(),
        }
    }

    pub fn is_locked(&self) -> bool {
        matches!(self.code, Some(AgentErrorCode::Locked))
    }
}
fn decode_response<T: DeserializeOwned>(
    response: AgentResponse,
) -> std::result::Result<T, AgentCommandError> {
    if !response.ok {
        return Err(AgentCommandError {
            code: response.code,
            message: response
                .message
                .unwrap_or_else(|| "agent request failed".to_string()),
        });
    }
    serde_json::from_value(response.data).map_err(AgentCommandError::internal)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A socket that accepts a connection and then never answers is exactly
    /// the wedged-agent case: without a receive timeout the caller blocks
    /// forever, which used to freeze the tray and every desktop command.
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn a_silent_agent_times_out_instead_of_hanging() {
        use interprocess::local_socket::{prelude::*, GenericFilePath, ListenerOptions};

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("silent.sock");
        let name = path
            .clone()
            .to_fs_name::<GenericFilePath>()
            .expect("socket name");
        let listener = ListenerOptions::new()
            .name(name)
            .create_sync()
            .expect("listener");
        // Accept and hold the connection open without ever replying.
        let accepted = thread::spawn(move || {
            let _held = listener.accept();
            thread::sleep(Duration::from_secs(30));
        });

        let connect_name = path.to_fs_name::<GenericFilePath>().expect("connect name");
        let stream = interprocess::local_socket::Stream::connect(connect_name).expect("connect");
        apply_request_timeouts(&stream, Duration::from_millis(250)).expect("set timeouts");

        let started = Instant::now();
        let result = read_frame::<AgentResponse>(&stream);
        let elapsed = started.elapsed();

        assert!(result.is_err(), "expected the silent agent to time out");
        assert!(elapsed < Duration::from_secs(5), "elapsed {elapsed:?}");
        // The caller sees an unavailable agent, not an internal fault, so it
        // falls back to its "agent not reachable" handling.
        let err = timeout_aware_error(result.unwrap_err());
        assert_eq!(err.code, Some(AgentErrorCode::ServiceUnavailable));

        drop(stream);
        drop(accepted);
    }

    #[test]
    fn resident_agent_startup_suppresses_tray_launch() {
        let companion = AgentStartupMode::Direct {
            suppress_desktop_tray: true,
        }
        .launch_options();
        let app = AgentStartupMode::Direct {
            suppress_desktop_tray: false,
        }
        .launch_options();
        let autostart = AgentStartupMode::Autostart.launch_options();

        assert!(companion.suppress_desktop_tray);
        assert!(!app.suppress_desktop_tray);
        assert!(autostart.suppress_desktop_tray);
    }

    #[test]
    fn failed_status_response_is_not_treated_as_ready() {
        let response = AgentResponse::error(
            AgentErrorCode::ValidationFailed,
            "unsupported agent protocol version",
        );

        let error = decode_response::<SessionStatus>(response).unwrap_err();

        assert_eq!(error.code, Some(AgentErrorCode::ValidationFailed));
    }

    #[test]
    fn initial_sync_pending_extends_the_ready_deadline_within_a_cap() {
        let started = Instant::now();

        assert_eq!(
            ready_deadline(started, false),
            started + AGENT_READY_TIMEOUT
        );
        assert_eq!(
            ready_deadline(started, true),
            started + AGENT_READY_TIMEOUT + INITIAL_SYNC_READY_GRACE
        );
        // The grace window stays bounded: pending sync can never push
        // readiness beyond 15s + 120s from the wait start.
        assert!(INITIAL_SYNC_READY_GRACE <= Duration::from_secs(120));
    }

    #[test]
    fn legacy_status_without_initial_sync_field_decodes_as_ready() {
        let status: SessionStatus = serde_json::from_value(serde_json::json!({
            "exists": true,
            "locked": false,
            "policy": {
                "idleLockMinutes": 60,
                "lockOnSleep": true,
                "lockOnScreenLock": true
            }
        }))
        .expect("decode legacy status");

        assert!(!status.initial_sync_pending);
    }
}
