use crate::*;

pub(crate) fn vault_dir(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    default_vault_dir()
}

pub(crate) struct CliAgent {
    client: AgentClient,
    password: Option<String>,
    interactive: bool,
}

impl CliAgent {
    pub(crate) fn from_parts(vault: Option<PathBuf>, password: Option<String>) -> Result<Self> {
        let config = AgentClientConfig::for_vault(vault_dir(vault)?)?;
        Ok(Self {
            client: AgentClient::new(config),
            password,
            interactive: std::io::stdin().is_terminal(),
        })
    }

    pub(crate) fn ensure_running(&self) -> Result<()> {
        self.client.ensure_running()
    }

    pub(crate) fn request<T: serde::de::DeserializeOwned>(
        &self,
        request: AgentRequest,
    ) -> Result<T> {
        self.ensure_running()?;
        match self.client.request::<T>(&request) {
            Ok(value) => Ok(value),
            Err(err) if err.is_locked() => {
                self.unlock_for_request()?;
                self.client
                    .request::<T>(&request)
                    .map_err(agent_error_to_anyhow)
            }
            Err(err) => Err(agent_error_to_anyhow(err)),
        }
    }

    pub(crate) fn request_no_unlock<T: serde::de::DeserializeOwned>(
        &self,
        request: AgentRequest,
    ) -> Result<T> {
        self.ensure_running()?;
        self.client
            .request::<T>(&request)
            .map_err(agent_error_to_anyhow)
    }

    pub(crate) fn unlock_for_request(&self) -> Result<SessionStatus> {
        let mut password = if let Some(password) = self.password.clone() {
            password
        } else if self.interactive {
            prompt_password("AIPass master password: ").context("failed to read master password")?
        } else {
            anyhow::bail!("vault is locked");
        };
        let response = self
            .client
            .request::<SessionStatus>(&AgentRequest::SessionUnlock {
                mode: aipass_agent_protocol::SessionUnlockMode::Password {
                    password: password.as_str().into(),
                },
            })
            .map_err(agent_error_to_anyhow);
        password.clear();
        response
    }
}

pub(crate) fn agent_error_to_anyhow(err: AgentCommandError) -> anyhow::Error {
    let message = match err.code {
        Some(code) => format!(
            "{}: {}",
            aipass_agent_protocol::error_code_name(&code),
            err.message
        ),
        None => err.message,
    };
    anyhow::anyhow!(message)
}

/// Resolve a CLI entry selector without ever reading credential material.
/// UUIDs remain the unambiguous fast path; names must match exactly once.
pub(crate) fn resolve_entry_id(agent: &CliAgent, selector: &str) -> Result<Uuid> {
    if let Ok(id) = Uuid::parse_str(selector) {
        return Ok(id);
    }

    let wanted = selector.trim();
    if wanted.is_empty() {
        anyhow::bail!("credential name cannot be empty");
    }
    let mut entries =
        agent.request::<Vec<aipass_vault::EntrySummary>>(AgentRequest::EntriesList {
            archived: false,
        })?;
    entries.extend(agent.request::<Vec<aipass_vault::EntrySummary>>(
        AgentRequest::EntriesList { archived: true },
    )?);
    let matches = entries
        .into_iter()
        .filter(|entry| entry.title.trim().eq_ignore_ascii_case(wanted))
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [entry] => Ok(entry.id),
        [] => anyhow::bail!("no credential named '{wanted}'"),
        _ => anyhow::bail!("credential name '{wanted}' is ambiguous; use its UUID"),
    }
}
