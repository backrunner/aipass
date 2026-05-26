use crate::ipc;
use crate::paths::{canonical_vault_dir, default_vault_dir, namespace_for_vault_dir};
#[cfg(target_os = "windows")]
use crate::windows_service;
use aipass_agent_protocol::{
    read_frame, write_frame, AgentErrorCode, AgentRequest, AgentResponse, AuthenticatedAgentRequest,
};
use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

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
        if self.request_raw(&AgentRequest::SessionStatus).is_ok() {
            return Ok(());
        }
        #[cfg(target_os = "windows")]
        {
            windows_service::start_service(&self.config.vault_dir)
                .context("failed to start registered Windows agent service")?;
        }
        #[cfg(not(target_os = "windows"))]
        {
            let binary = agent_binary_path()?;
            Command::new(binary)
                .arg("--vault")
                .arg(&self.config.vault_dir)
                .spawn()
                .context("failed to launch agent")?;
        }
        for _ in 0..40 {
            if self.request_raw(&AgentRequest::SessionStatus).is_ok() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err(anyhow::anyhow!("agent did not become ready"))
    }

    pub fn shutdown(&self) -> Result<()> {
        self.request::<serde_json::Value>(&AgentRequest::AgentShutdown)
            .map(|_| ())
            .map_err(anyhow::Error::from)
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

fn agent_binary_path() -> Result<PathBuf> {
    let current = std::env::current_exe().context("cannot determine current executable")?;
    let sibling = crate::windows_service::binary_for_service(&current);
    if sibling.exists() {
        return Ok(sibling);
    }
    Ok(sibling)
}
