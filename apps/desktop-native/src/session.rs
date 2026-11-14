//! Renderer-independent desktop session state. All durable operations remain
//! authenticated Agent requests. A single worker orders auth and lock requests.
use crate::catalog::{Action as ProviderAction, Catalog};
use aipass_agent::{AgentClient, AgentCommandError};
use aipass_agent_protocol::{
    AgentErrorCode, AgentRequest, LockReason, SensitiveString, SessionStatus, SessionUnlockMode,
    VaultCreateResponse,
};
use aipass_vault::EntrySummary;
use std::{
    path::PathBuf,
    sync::mpsc::{Receiver, Sender, TryRecvError},
};
use uuid::Uuid;

mod activity;
mod worker;

pub enum Command {
    Refresh,
    Import(crate::onboarding::Import),
    CheckCloud,
    Touch,
    Trash(crate::trash::Request),
    Pricing(Box<crate::pricing::Request>),
    Provider(ProviderAction),
    Credential(Box<crate::credentials::Request>),
    ProviderTools(Box<crate::provider_tools::Request>),
    Integration(Box<crate::integration::Request>),
    Agents(Box<crate::agent_management::Request>),
    Subscription(Box<crate::subscriptions::Request>),
    Unlock(SensitiveString),
    Create {
        password: SensitiveString,
        local_only: bool,
    },
    Recover {
        key: SensitiveString,
        password: SensitiveString,
    },
    Lock,
    Reset,
}

pub enum Reply {
    Touched(SessionStatus),
    Trash(crate::trash::Completion),
    Pricing(Box<crate::pricing::Completion>),
    Credential(crate::credentials::Completion),
    ProviderTools(Box<crate::provider_tools::Completion>),
    Integration(Box<crate::integration::Completion>),
    Agents(Box<crate::agent_management::Completion>),
    Subscription(Box<crate::subscriptions::Completion>),
    ProviderApplied {
        selected: Option<Uuid>,
        saved: bool,
    },
    Catalog {
        status: SessionStatus,
        catalog: Box<Catalog>,
        sync_settings: Option<Result<aipass_agent_protocol::SyncSettings, AgentCommandError>>,
        selected: Option<Uuid>,
        saved: bool,
    },
    Snapshot {
        status: SessionStatus,
        entries: Vec<EntrySummary>,
    },
    Created(VaultCreateResponse),
    Locked(SessionStatus),
    Reset(SessionStatus),
}

pub trait Transport: Send + 'static {
    fn execute(&mut self, command: Command) -> Result<Reply, AgentCommandError>;
}

/// Does not invent an IPC endpoint or install a second credential authority.
pub struct AgentTransport(
    AgentClient,
    std::sync::Arc<crate::runtime::Gate>,
    Option<std::sync::Arc<crate::runtime::agent::AgentServices>>,
);

impl AgentTransport {
    pub fn for_vault(path: PathBuf) -> anyhow::Result<Self> {
        Ok(Self(
            AgentClient::for_vault(path)?,
            Default::default(),
            None,
        ))
    }

    pub fn with_runtime(client: AgentClient, gate: std::sync::Arc<crate::runtime::Gate>) -> Self {
        Self(client, gate, None)
    }

    pub fn managed(
        services: std::sync::Arc<crate::runtime::agent::AgentServices>,
        gate: std::sync::Arc<crate::runtime::Gate>,
    ) -> Self {
        Self(services.client.clone(), gate, Some(services))
    }

    fn snapshot(&self) -> Result<Reply, AgentCommandError> {
        let status: SessionStatus = self.0.request(&AgentRequest::SessionStatus)?;
        let catalog = if status.exists && !status.locked && !status.initial_sync_pending {
            Catalog {
                active: self
                    .0
                    .request(&AgentRequest::EntriesList { archived: false })?,
                archived: self
                    .0
                    .request(&AgentRequest::EntriesList { archived: true })?,
                trash: self.0.request(&AgentRequest::EntriesTrash)?,
            }
        } else {
            Catalog::default()
        };
        Ok(Reply::Catalog {
            sync_settings: if !status.exists {
                Some(self.0.request(&AgentRequest::SyncSettingsGet))
            } else {
                None
            },
            status,
            catalog: Box::new(catalog),
            selected: None,
            saved: false,
        })
    }
}

impl Transport for AgentTransport {
    fn execute(&mut self, command: Command) -> Result<Reply, AgentCommandError> {
        let _guard = self.1.enter().map_err(|error| AgentCommandError {
            code: Some(AgentErrorCode::ServiceUnavailable),
            message: error.to_string(),
        })?;
        if let Some(services) = &self.2 {
            services.ensure_ready().map_err(|error| AgentCommandError {
                code: Some(AgentErrorCode::ServiceUnavailable),
                message: error.to_string(),
            })?;
        }
        match command {
            Command::Pricing(request) => Ok(Reply::Pricing(Box::new(request.execute(&self.0)))),
            Command::Trash(request) => Ok(Reply::Trash(request.execute(&self.0))),
            Command::Refresh => self.snapshot(),
            Command::Import(request) => {
                self.0.request::<serde_json::Value>(&request.request())?;
                self.snapshot()
            }
            Command::CheckCloud => {
                crate::onboarding::check_cloud(&self.0)?;
                self.snapshot()
            }
            Command::Touch => self
                .0
                .request(&AgentRequest::SessionTouch)
                .map(Reply::Touched),
            Command::Subscription(request) => {
                Ok(Reply::Subscription(Box::new(request.execute(&self.0))))
            }
            Command::ProviderTools(request) => {
                Ok(Reply::ProviderTools(Box::new(request.execute(&self.0))))
            }
            Command::Credential(request) => Ok(Reply::Credential(request.execute(&self.0))),
            Command::Agents(request) => Ok(Reply::Agents(Box::new(request.execute(&self.0)))),
            Command::Integration(request) => {
                Ok(Reply::Integration(Box::new(request.execute(&self.0))))
            }
            Command::Provider(action) => {
                let saved = action.saves_form();
                let mut selected = action.selected();
                let request = action.into_request(|id| {
                    let result: aipass_agent_protocol::ProviderHeaderValues =
                        self.0.request(&AgentRequest::SecretRevealHeaders { id })?;
                    Ok(result
                        .headers
                        .into_iter()
                        .map(|(name, value)| (name, value.expose().to_owned()))
                        .collect())
                })?;
                if matches!(request, AgentRequest::ProviderAdd { .. }) {
                    selected = Some(self.0.request::<Uuid>(&request)?);
                } else {
                    self.0.request::<serde_json::Value>(&request)?;
                }
                Ok(Reply::ProviderApplied { selected, saved })
            }
            Command::Unlock(password) => {
                let _: SessionStatus = self.0.request(&AgentRequest::SessionUnlock {
                    mode: SessionUnlockMode::Password { password },
                })?;
                self.snapshot()
            }
            Command::Create {
                password,
                local_only,
            } => self
                .0
                .request(&AgentRequest::VaultCreate {
                    password,
                    local_only,
                })
                .map(Reply::Created),
            Command::Recover { key, password } => self
                .0
                .request(&AgentRequest::VaultRecover {
                    recovery_key: key,
                    new_password: password,
                })
                .map(Reply::Created),
            Command::Lock => self
                .0
                .request(&AgentRequest::SessionLock {
                    reason: LockReason::Manual,
                })
                .map(Reply::Locked),
            Command::Reset => self.0.request(&AgentRequest::VaultReset).map(Reply::Reset),
        }
    }
}

struct Job {
    generation: u64,
    command: Command,
}
struct Completion {
    generation: u64,
    activity: bool,
    result: Result<Reply, AgentCommandError>,
}

/// Root view retains this controller across authentication/workspace changes.
/// UI polling uses try_recv and never waits for IPC on the platform thread.
pub struct Session {
    activity: activity::Activity,
    sender: Option<Sender<Job>>,
    receiver: Receiver<Completion>,
    generation: u64,
    pub status: Option<SessionStatus>,
    pub sync_settings: Option<aipass_agent_protocol::SyncSettings>,
    pub catalog: Catalog,
    pub selected: Option<Uuid>,
    pub provider_saved: bool,
    pub provider_applied: bool,
    pub pricing_completion: Option<crate::pricing::Completion>,
    pub trash_completion: Option<crate::trash::Completion>,
    pub tools_completion: Option<crate::provider_tools::Completion>,
    pub credential_completion: Option<crate::credentials::Completion>,
    pub agents_completion: Option<crate::agent_management::Completion>,
    pub integration_completion: Option<crate::integration::Completion>,
    pub subscription_completion: Option<crate::subscriptions::Completion>,
    pub recovery: Option<SensitiveString>,
    pub error: Option<AgentCommandError>,
    pub pending: bool,
    pub foreground_pending: bool,
    pub lock_pending: bool,
}

impl Session {
    pub fn new(transport: impl Transport) -> std::io::Result<Self> {
        let (sender, receiver) = worker::start(transport)?;
        Ok(Self {
            activity: Default::default(),
            sender: Some(sender),
            receiver,
            generation: 0,
            status: None,
            sync_settings: None,
            catalog: Catalog::default(),
            selected: None,
            provider_saved: false,
            provider_applied: false,
            pricing_completion: None,
            trash_completion: None,
            tools_completion: None,
            credential_completion: None,
            agents_completion: None,
            integration_completion: None,
            subscription_completion: None,
            recovery: None,
            error: None,
            pending: false,
            foreground_pending: false,
            lock_pending: false,
        })
    }

    pub fn is_unlocked(&self) -> bool {
        !self.lock_pending
            && self
                .status
                .as_ref()
                .is_some_and(|s| s.exists && !s.locked && !s.initial_sync_pending)
    }

    pub fn record_activity(&mut self) {
        if self.is_unlocked() {
            self.activity.record(std::time::Instant::now());
        }
    }

    pub fn submit_activity(&mut self) -> bool {
        if !self.is_unlocked() || self.pending || !self.activity.take(std::time::Instant::now()) {
            return false;
        }
        self.submit(Command::Touch)
    }

    pub fn submit(&mut self, command: Command) -> bool {
        if matches!(
            command,
            Command::Touch
                | Command::Pricing(_)
                | Command::Trash(_)
                | Command::Provider(_)
                | Command::ProviderTools(_)
                | Command::Credential(_)
                | Command::Integration(_)
                | Command::Agents(_)
                | Command::Subscription(_)
        ) && !self.is_unlocked()
        {
            return false;
        }
        // Lock must queue behind in-flight authentication, so its final state
        // cannot be undone by a slower password derivation completing later.
        let locking = matches!(command, Command::Lock);
        if (self.pending || self.lock_pending) && !locking {
            return false;
        }
        if locking {
            self.activity = Default::default();
            aipass_desktop_platform::clipboard::invalidate();
            if self.lock_pending && self.pending {
                return false;
            }
            self.lock_pending = true;
            self.catalog = Catalog::default();
            self.selected = None;
            self.recovery = None;
            self.pricing_completion = None;
            self.trash_completion = None;
            self.tools_completion = None;
            self.credential_completion = None;
            self.integration_completion = None;
            self.agents_completion = None;
            self.subscription_completion = None;
            if let Some(status) = &mut self.status {
                status.locked = true;
            }
        }
        self.generation += 1;
        self.pending = true;
        self.foreground_pending = !matches!(&command, Command::Refresh | Command::Touch)
            && !matches!(&command, Command::Pricing(request) if matches!(request.operation, crate::pricing::Operation::Load));
        self.error = None;
        if self.sender.as_ref().is_none_or(|sender| {
            sender
                .send(Job {
                    generation: self.generation,
                    command,
                })
                .is_err()
        }) {
            self.disconnected();
            return false;
        }
        true
    }

    pub fn poll(&mut self) -> bool {
        let was_unlocked = self.is_unlocked();
        self.provider_saved = false;
        self.provider_applied = false;
        self.pricing_completion = None;
        self.trash_completion = None;
        self.tools_completion = None;
        self.credential_completion = None;
        self.integration_completion = None;
        self.agents_completion = None;
        self.subscription_completion = None;
        self.selected = None;
        let mut changed = false;
        loop {
            match self.receiver.try_recv() {
                Ok(completion) => {
                    if completion.generation != self.generation {
                        continue;
                    }
                    self.pending = false;
                    self.foreground_pending = false;
                    changed = true;
                    match completion.result {
                        Ok(Reply::Pricing(completion)) => {
                            if completion
                                .result
                                .as_ref()
                                .is_err_and(|e| e.code == Some(AgentErrorCode::Locked))
                            {
                                self.catalog = Catalog::default();
                                self.recovery = None;
                                if let Some(status) = &mut self.status {
                                    status.locked = true;
                                }
                            }
                            self.pricing_completion = Some(*completion);
                        }
                        Ok(Reply::Trash(completion)) => {
                            // Emptying trash can partially succeed before an I/O failure.
                            // Refresh even on failure so the list reflects durable state.
                            self.provider_applied = true;
                            if completion
                                .result
                                .as_ref()
                                .is_err_and(|error| error.code == Some(AgentErrorCode::Locked))
                            {
                                self.catalog = Catalog::default();
                                self.recovery = None;
                                if let Some(status) = &mut self.status {
                                    status.locked = true;
                                }
                            }
                            self.trash_completion = Some(completion);
                        }
                        Ok(Reply::Touched(status)) => {
                            if status.locked || !status.exists || status.initial_sync_pending {
                                self.catalog = Catalog::default();
                                self.recovery = None;
                            }
                            self.status = Some(status);
                        }
                        Ok(Reply::Subscription(completion)) => {
                            if completion
                                .result
                                .as_ref()
                                .is_err_and(|error| error.code == Some(AgentErrorCode::Locked))
                            {
                                self.catalog = Catalog::default();
                                self.recovery = None;
                                if let Some(status) = &mut self.status {
                                    status.locked = true;
                                }
                            }
                            if let Ok(crate::subscriptions::Outcome::Login(login)) =
                                &completion.result
                            {
                                if login.entry_id.is_some() {
                                    self.provider_applied = true;
                                }
                            }
                            if let (
                                crate::subscriptions::Kind::Refresh(id),
                                Ok(crate::subscriptions::Outcome::Refreshed(snapshot)),
                            ) = (&completion.kind, &completion.result)
                            {
                                for entry in self
                                    .catalog
                                    .active
                                    .iter_mut()
                                    .chain(self.catalog.archived.iter_mut())
                                    .chain(self.catalog.trash.iter_mut())
                                    .filter(|e| e.id == *id)
                                {
                                    entry.subscription = Some(snapshot.clone());
                                }
                            }
                            if matches!(
                                completion.result,
                                Ok(crate::subscriptions::Outcome::Import(_)
                                    | crate::subscriptions::Outcome::Refreshed(_))
                            ) {
                                self.provider_applied = true;
                            }
                            self.subscription_completion = Some(*completion);
                        }
                        Ok(Reply::Agents(completion)) => {
                            if matches!(&completion.result, Ok(crate::agent_management::Outcome::Applied(result)) if result.outcome == aipass_agent_protocol::ToolConfigOutcome::Applied)
                            {
                                self.provider_applied = true;
                            }
                            if completion
                                .result
                                .as_ref()
                                .is_err_and(|e| e.code == Some(AgentErrorCode::Locked))
                            {
                                self.catalog = Catalog::default();
                                self.recovery = None;
                                if let Some(status) = &mut self.status {
                                    status.locked = true;
                                }
                            }
                            self.agents_completion = Some(*completion);
                        }
                        Ok(Reply::Integration(completion)) => {
                            if matches!(&completion.result, Ok(crate::integration::Outcome::Login(status))
                                if status.result.as_ref().is_some_and(|r| r.outcome == aipass_agent_protocol::ToolConfigOutcome::Applied))
                            {
                                self.provider_applied = true;
                            }
                            if completion
                                .result
                                .as_ref()
                                .is_err_and(|error| error.code == Some(AgentErrorCode::Locked))
                            {
                                self.catalog = Catalog::default();
                                self.recovery = None;
                                if let Some(status) = &mut self.status {
                                    status.locked = true;
                                }
                            }
                            self.integration_completion = Some(*completion);
                        }
                        Ok(Reply::ProviderTools(completion)) => {
                            self.provider_applied = completion.mutated;
                            if let Ok(crate::provider_tools::Outcome::Reset(entry)) =
                                &completion.result
                            {
                                for current in self
                                    .catalog
                                    .active
                                    .iter_mut()
                                    .chain(self.catalog.archived.iter_mut())
                                {
                                    if current.id == entry.id {
                                        *current = *entry.clone();
                                    }
                                }
                            }
                            if completion
                                .result
                                .as_ref()
                                .is_err_and(|error| error.code == Some(AgentErrorCode::Locked))
                            {
                                self.catalog = Catalog::default();
                                self.recovery = None;
                                if let Some(status) = &mut self.status {
                                    status.locked = true;
                                }
                            }
                            self.tools_completion = Some(*completion);
                        }
                        Ok(Reply::Credential(completion)) => {
                            self.provider_applied = matches!(
                                completion.result,
                                Ok(crate::credentials::Outcome::Applied)
                            );
                            if completion
                                .result
                                .as_ref()
                                .is_err_and(|error| error.code == Some(AgentErrorCode::Locked))
                            {
                                self.catalog = Catalog::default();
                                self.recovery = None;
                                if let Some(status) = &mut self.status {
                                    status.locked = true;
                                }
                            }
                            self.credential_completion = Some(completion);
                        }
                        Ok(Reply::ProviderApplied { selected, saved }) => {
                            self.selected = selected;
                            self.provider_saved = saved;
                            self.provider_applied = true;
                        }
                        Ok(Reply::Catalog {
                            sync_settings,
                            status,
                            catalog,
                            selected,
                            saved,
                        }) => {
                            let ready =
                                status.exists && !status.locked && !status.initial_sync_pending;
                            if !ready {
                                self.recovery = None;
                            }
                            self.catalog = if ready { *catalog } else { Catalog::default() };
                            self.selected = if ready { selected } else { None };
                            self.provider_saved = saved && ready;
                            match sync_settings {
                                Some(Ok(settings)) => self.sync_settings = Some(settings),
                                Some(Err(error)) => self.error = Some(error),
                                None => {}
                            }
                            self.status = Some(status);
                        }
                        Ok(Reply::Snapshot { status, entries }) => {
                            if status.locked {
                                self.recovery = None;
                            }
                            self.catalog = Catalog {
                                active: if status.locked { Vec::new() } else { entries },
                                ..Default::default()
                            };
                            self.status = Some(status);
                        }
                        Ok(Reply::Created(created)) => {
                            self.status = Some(created.session);
                            self.recovery =
                                Some(SensitiveString::new(created.recovery_kit.recovery_key));
                        }
                        Ok(Reply::Locked(status)) => {
                            self.catalog = Catalog::default();
                            self.selected = None;
                            self.recovery = None;
                            if status.locked || !status.exists {
                                self.status = Some(status);
                                self.lock_pending = false;
                            } else {
                                self.error = Some(AgentCommandError {
                                    code: Some(AgentErrorCode::Internal),
                                    message: "Agent did not acknowledge the vault lock".into(),
                                });
                            }
                        }
                        Ok(Reply::Reset(status)) => {
                            self.catalog = Catalog::default();
                            self.selected = None;
                            self.recovery = None;
                            if !status.exists && status.locked {
                                self.status = Some(status);
                            } else {
                                self.error = Some(AgentCommandError {
                                    code: Some(AgentErrorCode::Internal),
                                    message: "Agent did not acknowledge the vault reset".into(),
                                });
                            }
                        }
                        Err(error) => {
                            if error.code == Some(AgentErrorCode::Locked) {
                                self.catalog = Catalog::default();
                                self.selected = None;
                                self.recovery = None;
                                if let Some(status) = &mut self.status {
                                    status.locked = true;
                                }
                            }
                            if !completion.activity || error.code == Some(AgentErrorCode::Locked) {
                                self.error = Some(error);
                            }
                        }
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.sender.is_some() {
                        self.disconnected();
                        changed = true;
                    }
                    break;
                }
            }
        }
        if was_unlocked && !self.is_unlocked() {
            self.activity = Default::default();
            aipass_desktop_platform::clipboard::invalidate();
        }
        changed
    }

    fn disconnected(&mut self) {
        self.activity = Default::default();
        aipass_desktop_platform::clipboard::invalidate();
        self.sender = None;
        self.pending = false;
        self.foreground_pending = false;
        self.catalog = Catalog::default();
        self.selected = None;
        self.recovery = None;
        self.pricing_completion = None;
        self.trash_completion = None;
        self.tools_completion = None;
        self.credential_completion = None;
        self.integration_completion = None;
        self.agents_completion = None;
        self.subscription_completion = None;
        if let Some(status) = &mut self.status {
            status.locked = true;
        }
        self.error = Some(AgentCommandError {
            code: Some(AgentErrorCode::ServiceUnavailable),
            message: "Desktop Agent worker disconnected".into(),
        });
    }
}

#[cfg(test)]
mod tests;
