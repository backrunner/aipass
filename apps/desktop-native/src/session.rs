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
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread,
};
use uuid::Uuid;

pub enum Command {
    Refresh,
    Provider(ProviderAction),
    Credential(Box<crate::credentials::Request>),
    Integration(Box<crate::integration::Request>),
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
    Credential(crate::credentials::Completion),
    Integration(Box<crate::integration::Completion>),
    ProviderApplied {
        selected: Option<Uuid>,
        saved: bool,
    },
    Catalog {
        status: SessionStatus,
        catalog: Box<Catalog>,
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
pub struct AgentTransport(AgentClient);

impl AgentTransport {
    pub fn for_vault(path: PathBuf) -> anyhow::Result<Self> {
        Ok(Self(AgentClient::for_vault(path)?))
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
            status,
            catalog: Box::new(catalog),
            selected: None,
            saved: false,
        })
    }
}

impl Transport for AgentTransport {
    fn execute(&mut self, command: Command) -> Result<Reply, AgentCommandError> {
        match command {
            Command::Refresh => self.snapshot(),
            Command::Credential(request) => Ok(Reply::Credential(request.execute(&self.0))),
            Command::Integration(request) => {
                Ok(Reply::Integration(Box::new(request.execute(&self.0))))
            }
            Command::Provider(action) => {
                let saved = action.saves_form();
                let mut selected = action.selected();
                let request = action.into_request();
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
    result: Result<Reply, AgentCommandError>,
}

/// Root view retains this controller across authentication/workspace changes.
/// UI polling uses try_recv and never waits for IPC on the platform thread.
pub struct Session {
    sender: Option<Sender<Job>>,
    receiver: Receiver<Completion>,
    generation: u64,
    pub status: Option<SessionStatus>,
    pub catalog: Catalog,
    pub selected: Option<Uuid>,
    pub provider_saved: bool,
    pub provider_applied: bool,
    pub credential_completion: Option<crate::credentials::Completion>,
    pub integration_completion: Option<crate::integration::Completion>,
    pub recovery: Option<SensitiveString>,
    pub error: Option<AgentCommandError>,
    pub pending: bool,
    pub foreground_pending: bool,
    pub lock_pending: bool,
}

impl Session {
    pub fn new(mut transport: impl Transport) -> std::io::Result<Self> {
        let (sender, jobs) = mpsc::channel::<Job>();
        let (completed, receiver) = mpsc::channel();
        thread::Builder::new()
            .name("desktop-agent-client".into())
            .spawn(move || {
                while let Ok(job) = jobs.recv() {
                    let result = transport.execute(job.command);
                    // A view can disappear while authentication is in flight. Still
                    // execute its already-queued lock before dropping the transport.
                    let _ = completed.send(Completion {
                        generation: job.generation,
                        result,
                    });
                }
            })?;
        Ok(Self {
            sender: Some(sender),
            receiver,
            generation: 0,
            status: None,
            catalog: Catalog::default(),
            selected: None,
            provider_saved: false,
            provider_applied: false,
            credential_completion: None,
            integration_completion: None,
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

    pub fn submit(&mut self, command: Command) -> bool {
        if matches!(
            command,
            Command::Provider(_) | Command::Credential(_) | Command::Integration(_)
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
            if self.lock_pending && self.pending {
                return false;
            }
            self.lock_pending = true;
            self.catalog = Catalog::default();
            self.selected = None;
            self.recovery = None;
            self.credential_completion = None;
            self.integration_completion = None;
            if let Some(status) = &mut self.status {
                status.locked = true;
            }
        }
        self.generation += 1;
        self.pending = true;
        self.foreground_pending = !matches!(command, Command::Refresh);
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
        self.provider_saved = false;
        self.provider_applied = false;
        self.credential_completion = None;
        self.integration_completion = None;
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
                        Ok(Reply::Integration(completion)) => {
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
                            self.error = Some(error);
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
        changed
    }

    fn disconnected(&mut self) {
        self.sender = None;
        self.pending = false;
        self.foreground_pending = false;
        self.catalog = Catalog::default();
        self.selected = None;
        self.recovery = None;
        self.credential_completion = None;
        self.integration_completion = None;
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
