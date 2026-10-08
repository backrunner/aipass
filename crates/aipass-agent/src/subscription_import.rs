//! Session-bound batch imports. Progress contains metadata only; grants never leave Rust.
use crate::session::{map_vault_error, AgentState, ServiceError, ServiceResult, SessionState};
use aipass_agent_protocol::*;
use aipass_provider_registry::CredentialKind;
use futures_util::{stream, StreamExt};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, Weak,
};
use std::time::{Duration, Instant};
use uuid::Uuid;

mod persist;
pub(crate) mod reader;
mod sources;
#[cfg(test)]
mod tests;

#[derive(Default)]
pub(crate) struct Imports {
    current: Mutex<Option<Arc<Job>>>,
    completed: Mutex<HashMap<Uuid, Arc<Job>>>,
    cleanup_started: AtomicBool,
}
struct Job {
    session: Uuid,
    cancelled: AtomicBool,
    progress: Mutex<SubscriptionImportTask>,
    finished: Mutex<Option<Instant>>,
}
impl Drop for Imports {
    fn drop(&mut self) {
        self.clear();
    }
}
fn validation(message: impl Into<String>) -> ServiceError {
    ServiceError::new(AgentErrorCode::ValidationFailed, message)
}
fn session_id(state: &Arc<AgentState>) -> ServiceResult<Uuid> {
    let session = state
        .session
        .lock()
        .map_err(|_| validation("session unavailable"))?;
    match &*session {
        SessionState::Unlocked(info) => {
            crate::control_panel::check_session(info)?;
            Ok(info.id)
        }
        SessionState::Locked => Err(ServiceError::new(AgentErrorCode::Locked, "vault is locked")),
    }
}
impl Imports {
    pub(crate) fn clear(&self) {
        if let Ok(mut current) = self.current.lock() {
            if let Some(job) = current.take() {
                job.cancelled.store(true, Ordering::Release);
            }
        }
        if let Ok(mut completed) = self.completed.lock() {
            completed.clear();
        }
    }
    fn job(&self, state: &Arc<AgentState>, ticket: Uuid) -> ServiceResult<Arc<Job>> {
        let session = session_id(state)?;
        let current = self
            .current
            .lock()
            .map_err(|_| validation("import unavailable"))?;
        let completed = self
            .completed
            .lock()
            .map_err(|_| validation("import unavailable"))?;
        let job = current
            .as_ref()
            .filter(|job| {
                job.session == session && job.progress.lock().is_ok_and(|p| p.ticket == ticket)
            })
            .or_else(|| completed.get(&ticket).filter(|job| job.session == session))
            .ok_or_else(|| ServiceError::new(AgentErrorCode::NotFound, "import task expired"))?;
        if job
            .finished
            .lock()
            .map_err(|_| validation("import unavailable"))?
            .is_some_and(|t| t.elapsed() >= Duration::from_secs(300))
        {
            return Err(ServiceError::new(
                AgentErrorCode::NotFound,
                "import task expired",
            ));
        }
        Ok(job.clone())
    }
    pub(crate) fn poll(
        &self,
        state: &Arc<AgentState>,
        ticket: Uuid,
    ) -> ServiceResult<SubscriptionImportTask> {
        let job = self.job(state, ticket)?;
        let result = job
            .progress
            .lock()
            .map_err(|_| validation("import unavailable"))?
            .clone();
        Ok(result)
    }
    pub(crate) fn cancel(
        &self,
        state: &Arc<AgentState>,
        ticket: Uuid,
    ) -> ServiceResult<SubscriptionImportTask> {
        let job = self.job(state, ticket)?;
        // Serialize cancellation with the commit boundary.
        let progress = job
            .progress
            .lock()
            .map_err(|_| validation("import unavailable"))?;
        if matches!(progress.phase.as_str(), "discovering" | "importing") {
            job.cancelled.store(true, Ordering::Release);
        }
        Ok(progress.clone())
    }
    pub(crate) fn start(
        &self,
        state: &Arc<AgentState>,
        input: SubscriptionImportInput,
    ) -> ServiceResult<SubscriptionImportTask> {
        if crate::control_panel::remote_request() {
            return Err(ServiceError::new(
                AgentErrorCode::PermissionDenied,
                "local account discovery requires local IPC",
            ));
        }
        if input.sources.len() > 256 || input.provider_ids.len() > 12 {
            return Err(validation("too many import inputs"));
        }
        let session = session_id(state)?;
        let retry_sources = if let Some(retry) = &input.retry {
            let old = self.poll(state, retry.ticket)?;
            if matches!(old.phase.as_str(), "discovering" | "importing") {
                return Err(validation("import is still running"));
            }
            let requested: HashSet<_> = retry.source_ids.iter().copied().collect();
            let items = old
                .results
                .into_iter()
                .filter(|r| {
                    requested.contains(&r.source_id)
                        && matches!(
                            r.status,
                            SubscriptionImportStatus::Failed
                                | SubscriptionImportStatus::NeedsLogin
                                | SubscriptionImportStatus::Cancelled
                        )
                })
                .map(|r| r.source)
                .collect::<Vec<_>>();
            if items.len() != requested.len() {
                return Err(validation(
                    "retry must contain only failed or cancelled sources",
                ));
            }
            Some(items)
        } else {
            None
        };
        let mut current = self
            .current
            .lock()
            .map_err(|_| validation("import unavailable"))?;
        if current
            .as_ref()
            .is_some_and(|j| j.session == session && j.finished.lock().is_ok_and(|t| t.is_none()))
        {
            return Err(ServiceError::new(
                AgentErrorCode::Conflict,
                "an account import is already running",
            ));
        }
        if let Some(old) = current.take() {
            if old.session == session && old.finished.lock().is_ok_and(|t| t.is_some()) {
                if let Ok(mut completed) = self.completed.lock() {
                    let ticket = old
                        .progress
                        .lock()
                        .map_err(|_| validation("import unavailable"))?
                        .ticket;
                    completed.insert(ticket, old);
                }
            } else {
                old.cancelled.store(true, Ordering::Release);
            }
        }
        let task = SubscriptionImportTask {
            ticket: Uuid::new_v4(),
            phase: "discovering".into(),
            total: 0,
            completed: 0,
            results: Vec::new(),
        };
        let job = Arc::new(Job {
            session,
            cancelled: AtomicBool::new(false),
            progress: Mutex::new(task.clone()),
            finished: Mutex::new(None),
        });
        let snapshot = crate::session::with_vault(state, true, sources::snapshot)?;
        let outbound = crate::session::with_vault(state, false, |vault| {
            state
                .proxy
                .lock()
                .map_err(|_| validation("proxy unavailable"))?
                .load_config(vault)
                .map(|c| c.upstream_proxy)
        })?;
        if session_id(state)? != session {
            return Err(ServiceError::new(AgentErrorCode::Locked, "session changed"));
        }
        let weak = Arc::downgrade(state);
        let run_job = job.clone();
        std::thread::Builder::new()
            .name("subscription-import".into())
            .spawn(move || {
                run(weak, run_job, input, retry_sources, snapshot, outbound);
            })
            .map_err(|_| validation("cannot start import worker"))?;
        *current = Some(job);
        if !self.cleanup_started.swap(true, Ordering::AcqRel) {
            let state = Arc::downgrade(state);
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_secs(1));
                let Some(state) = state.upgrade() else { break };
                state.subscription_imports.prune();
            });
        }
        Ok(task)
    }
    fn prune(&self) {
        let expired = |job: &Arc<Job>| {
            job.finished
                .lock()
                .is_ok_and(|t| t.is_some_and(|t| t.elapsed() >= Duration::from_secs(300)))
        };
        if let Ok(mut current) = self.current.lock() {
            if current.as_ref().is_some_and(expired) {
                current.take();
            }
            if let Ok(mut completed) = self.completed.lock() {
                completed.retain(|_, job| !expired(job));
            }
        }
    }
}

fn run(
    state: Weak<AgentState>,
    job: Arc<Job>,
    input: SubscriptionImportInput,
    retry: Option<Vec<SubscriptionImportSource>>,
    snapshot: sources::Snapshot,
    outbound: aipass_proxy::UpstreamProxyConfig,
) {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(_) => {
            job.cancelled.store(true, Ordering::Release);
            finish(&job);
            return;
        }
    };
    let known_retry = retry.clone();
    let sources = {
        let bound = snapshot.sources;
        runtime.block_on(async {
            let discover = tokio::task::spawn_blocking(move || match retry {
                Some(sources) => sources::retry_sources(sources),
                None => sources::discover(&input, &bound),
            });
            let discover = tokio::time::timeout(Duration::from_secs(60), discover);
            tokio::pin!(discover);
            loop {
                if job.cancelled.load(Ordering::Acquire) { break known_retry.map(Ok).unwrap_or_else(||Err(validation("discovery cancelled"))); }
                tokio::select! {
                    result = &mut discover => break result.map_err(|_|validation("discovery timed out"))
                        .and_then(|r|r.map_err(|_|validation("discovery failed"))).and_then(|r|r),
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {},
                }
            }
        })
    };
    let sources = match sources {
        Ok(sources) => sources,
        Err(_) => {
            if let Ok(mut p) = job.progress.lock() {
                if !job.cancelled.load(Ordering::Acquire) {
                    p.results.push(failure(
                        SubscriptionImportSource {
                            provider: "import".into(),
                            root: Default::default(),
                            selector: String::new(),
                        },
                        SubscriptionImportStatus::Failed,
                        "discovery_failed",
                        "choose_directory",
                    ));
                    p.total = 1;
                    p.completed = 1;
                }
            }
            runtime.shutdown_timeout(Duration::from_millis(100));
            finish(&job);
            return;
        }
    };
    if let Ok(mut p) = job.progress.lock() {
        p.total = sources.len();
        p.phase = "importing".into();
    }
    runtime.block_on(async {
        // Buffered preserves source priority even when lower-priority reads finish first.
        let mut work = stream::iter(sources.into_iter().map(|source| {
            let job = job.clone(); let outbound = outbound.clone();
            async move {
                let result = {
                let read = tokio::time::timeout(Duration::from_secs(60), reader::read(&source, &outbound));
                tokio::pin!(read);
                loop {
                    if job.cancelled.load(Ordering::Acquire) { break Err((SubscriptionImportStatus::Cancelled, "cancelled", "retry")); }
                    tokio::select! {
                        result = &mut read => break result.unwrap_or(Err((SubscriptionImportStatus::Failed, "timeout", "retry"))),
                        _ = tokio::time::sleep(Duration::from_millis(100)) => {},
                    }
                }
                };
                (source, result)
            }
        })).buffered(4);
        let mut revisions = snapshot.revisions;
        let mut seen = HashMap::new();
        while let Some((source, read)) = work.next().await {
            let mut progress = match job.progress.lock() { Ok(p) => p, Err(_) => break };
            let result = match read {
                Err((status, code, action)) => failure(source, status, code, action),
                Ok(account) => {
                    if job.cancelled.load(Ordering::Acquire) { failure(source, SubscriptionImportStatus::Cancelled, "cancelled", "retry") }
                    else if let Some(state) = state.upgrade() {
                        persist::commit(&state, job.session, source, account, &mut revisions, &mut seen)
                    } else { job.cancelled.store(true, Ordering::Release); failure(source, SubscriptionImportStatus::Cancelled, "cancelled", "retry") }
                }
            };
            progress.results.push(result); progress.completed += 1;
        }
    });
    // Bound blocking CLI reads can finish after cancellation; they have no commit authority.
    runtime.shutdown_timeout(Duration::from_millis(100));
    finish(&job);
}
fn finish(job: &Job) {
    if let Ok(mut progress) = job.progress.lock() {
        progress.phase = if job.cancelled.load(Ordering::Acquire) {
            "cancelled"
        } else {
            "complete"
        }
        .into();
        if let Ok(mut finished) = job.finished.lock() {
            *finished = Some(Instant::now());
        }
    }
}
fn failure(
    source: SubscriptionImportSource,
    status: SubscriptionImportStatus,
    code: &str,
    action: &str,
) -> SubscriptionImportResult {
    SubscriptionImportResult {
        source_id: Uuid::new_v4(),
        source,
        account_identity: None,
        status,
        entry_id: None,
        error_code: Some(code.into()),
        action: Some(action.into()),
    }
}

pub(crate) fn refresh_compat(
    state: &Arc<AgentState>,
    provider_ids: Vec<String>,
) -> ServiceResult<Vec<OfficialAccountRefreshResult>> {
    // Legacy refresh ignores unsupported filters and only covers official CLIs.
    let filtered = provider_ids
        .iter()
        .filter_map(|p| sources::canonical_provider(p))
        .filter(|p| sources::native_cli(p))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if !provider_ids.is_empty() && filtered.is_empty() {
        return Ok(Vec::new());
    }
    let task = state.subscription_imports.start(
        state,
        SubscriptionImportInput {
            provider_ids: if provider_ids.is_empty() {
                vec![
                    "anthropic".into(),
                    "codex".into(),
                    "grok".into(),
                    "copilot".into(),
                    "gemini-cli".into(),
                ]
            } else {
                filtered
            },
            ..Default::default()
        },
    )?;
    loop {
        let task = state.subscription_imports.poll(state, task.ticket)?;
        if matches!(task.phase.as_str(), "complete" | "cancelled") {
            return Ok(task
                .results
                .into_iter()
                .map(|r| OfficialAccountRefreshResult {
                    provider_id: r.source.provider,
                    account_identity: r.account_identity,
                    credential_kind: CredentialKind::OAuth,
                    snapshot: None,
                    status: match r.status {
                        SubscriptionImportStatus::Imported
                        | SubscriptionImportStatus::Existing
                        | SubscriptionImportStatus::Updated => "imported",
                        _ => "unavailable",
                    }
                    .into(),
                    error: r.error_code,
                })
                .collect());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
