use crate::ipc;
use crate::launcher;
use crate::paths::{canonical_vault_dir, default_vault_dir, namespace_for_vault_dir};
#[cfg(target_os = "windows")]
use crate::windows_service;
use aipass_agent_protocol::{
    read_frame, write_frame, AgentErrorCode, AgentRequest, AgentResponse,
    AuthenticatedAgentRequest, AGENT_PROTOCOL_VERSION,
};
use anyhow::Result;
use serde::de::DeserializeOwned;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

const AGENT_READY_TIMEOUT: Duration = Duration::from_secs(15);
const AGENT_READY_POLL_INTERVAL: Duration = Duration::from_millis(100);

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
        let payload = AuthenticatedAgentRequest {
            protocol_version: AGENT_PROTOCOL_VERSION,
            auth_token,
            request: request.clone(),
        };
        write_frame(&mut stream, &payload).map_err(AgentCommandError::internal)?;
        read_frame(&mut stream).map_err(AgentCommandError::internal)
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
        let initial_connection_error = match self.request_raw(&AgentRequest::SessionStatus) {
            Ok(_) => return Ok(()),
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
                    match crate::autostart::install_autostart(&agent_binary, &self.config.vault_dir)
                    {
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
        let deadline = Instant::now() + AGENT_READY_TIMEOUT;
        let last_connection_error = loop {
            match self.request_raw(&AgentRequest::SessionStatus) {
                Ok(_) => return Ok(()),
                Err(err) => {
                    let message = err.to_string();
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
            &binary_candidates,
            &initial_connection_error,
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
}
