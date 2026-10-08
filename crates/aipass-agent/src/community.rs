//! Account-scoped native Rust subscription adapters.
//! Only the Agent can commit credential rotations to the encrypted vault.
use crate::session::{map_vault_error, with_vault, AgentState, ServiceError, ServiceResult};
use crate::subscriptions::{Adapter, Operation};
use aipass_agent_protocol::{
    AgentErrorCode, AgentRequest, CommunityLoginInput, CommunityLoginStatus, SensitiveString,
};
use aipass_crypto::SecretString;
use aipass_provider_registry::{
    AuthScheme, CredentialKind, InterfaceType, ProviderEndpoint, ProviderKind,
    SubscriptionSnapshot, SubscriptionWindow,
};
use aipass_proxy::{
    ResolvedTarget, SubscriptionBackend, SubscriptionCodec, SubscriptionFuture,
    SubscriptionResponse, UpstreamKind, UpstreamProxyConfig,
};
use aipass_vault::{ProviderEntryInput, Vault};
use base64::{engine::general_purpose::STANDARD, Engine};
use bytes::Bytes;
use futures_util::{stream, Stream};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, Weak,
    },
    task::{Context, Poll},
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

const KEY: &str = "community_account_v1";
const MAX_FRAME: u64 = 8 * 1024 * 1024;
const ACCOUNT_BUSY: &str = "community account is busy; retry after its current request";
pub(crate) const PROVIDERS: &[&str] = &[
    "codex",
    "copilot",
    "gemini-cli",
    "zcode",
    "qoder",
    "qoder-cn",
    "devin",
    "zed",
    "factory",
    "grok",
    "commandcode-plan",
    "mimo-app",
    "cursor",
    "kiro",
    "workbuddy",
    "workbuddy-ai",
];

struct Run {
    process: Weak<Operation>,
    target: Option<ResolvedTarget>,
}
struct Login {
    status: CommunityLoginStatus,
    process: Weak<Operation>,
    created: Instant,
}
struct Inner {
    vault_dir: PathBuf,
    runs: Mutex<Vec<Run>>,
    accounts: Mutex<HashMap<Uuid, Weak<Mutex<()>>>>,
    logins: Mutex<HashMap<Uuid, Login>>,
    codec: SubscriptionCodec,
    owners: Mutex<HashMap<Uuid, Uuid>>,
    recovery: Mutex<AuthRecovery>,
}

#[derive(Default)]
struct AuthRecovery {
    epoch: u64,
    pending: HashMap<Uuid, (Account, Account)>,
}
impl AuthRecovery {
    fn candidate(&mut self, id: Uuid, current: &Account) -> Option<Account> {
        let (before, next) = self.pending.get(&id)?;
        if current.generation == before.generation
            && current.revision == before.revision
            && current.auth.expose() == before.auth.expose()
        {
            return Some(next.clone());
        }
        // A committed write with a lost ACK, or a newer edit/re-login, wins.
        self.pending.remove(&id);
        None
    }
}
#[derive(Clone)]
pub(crate) struct CommunityBridge {
    inner: Arc<Inner>,
}
impl CommunityBridge {
    pub(crate) fn new(path: &Path) -> Self {
        Self {
            inner: Arc::new(Inner {
                vault_dir: path.to_owned(),
                runs: Mutex::new(Vec::new()),
                accounts: Mutex::new(HashMap::new()),
                logins: Mutex::new(HashMap::new()),
                codec: SubscriptionCodec::default(),
                owners: Mutex::new(HashMap::new()),
                recovery: Mutex::new(AuthRecovery::default()),
            }),
        }
    }
    fn register(&self, p: &Arc<Operation>, target: Option<ResolvedTarget>) -> Result<(), String> {
        let mut runs = self
            .inner
            .runs
            .lock()
            .map_err(|_| "community workers unavailable")?;
        runs.retain(|r| r.process.strong_count() > 0);
        if runs.len() >= 64 {
            return Err("too many community operations".into());
        }
        runs.push(Run {
            process: Arc::downgrade(p),
            target,
        });
        Ok(())
    }
    fn account_lock(&self, id: Uuid) -> Result<Arc<Mutex<()>>, String> {
        let mut map = self
            .inner
            .accounts
            .lock()
            .map_err(|_| "account coordinator unavailable")?;
        map.retain(|_, v| v.strong_count() > 0);
        if let Some(v) = map.get(&id).and_then(Weak::upgrade) {
            return Ok(v);
        }
        let v = Arc::new(Mutex::new(()));
        map.insert(id, Arc::downgrade(&v));
        Ok(v)
    }
    fn load(&self, id: Uuid, marker: &str) -> Result<Account, String> {
        let mut account = self.load_raw(id, marker)?;
        let (epoch, recovered) = {
            let mut recovery = self
                .inner
                .recovery
                .lock()
                .map_err(|_| "account recovery unavailable")?;
            (recovery.epoch, recovery.candidate(id, &account))
        };
        if let Some(next) = recovered {
            let value: Value = serde_json::from_str(next.auth.expose())
                .map_err(|_| "invalid recovered account")?;
            self.update(
                id,
                marker,
                &mut account,
                &json!({"type":"auth","value":value}),
                epoch,
            )?;
        }
        Ok(account)
    }
    fn load_raw(&self, id: Uuid, marker: &str) -> Result<Account, String> {
        let raw: SensitiveString = crate::AgentClient::for_vault(self.inner.vault_dir.clone())
            .map_err(|_| "Agent unavailable")?
            .request(&AgentRequest::CommunityAccountRead {
                entry_id: id,
                marker: SensitiveString::new(marker),
            })
            .map_err(|_| "community account is locked or changed")?;
        decode(raw.expose()).map_err(|e| e.message)
    }
    fn update(
        &self,
        id: Uuid,
        marker: &str,
        account: &mut Account,
        frame: &Value,
        epoch: u64,
    ) -> Result<(), String> {
        let mut next = account.clone();
        match frame["type"].as_str() {
            Some("auth") => next.auth = SensitiveString::new(frame["value"].to_string()),
            Some("models") => next.models = frame["value"].clone(),
            _ => return Err("invalid account update".into()),
        }
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or("account revision overflow")?;
        {
            let mut recovery = self
                .inner
                .recovery
                .lock()
                .map_err(|_| "account recovery unavailable")?;
            if recovery.epoch != epoch {
                return Err("account operation revoked".into());
            }
            if frame["type"] == "auth" {
                let before: Value =
                    serde_json::from_str(account.auth.expose()).map_err(|_| "invalid account")?;
                ensure_owner(&before, &frame["value"])?;
                recovery.pending.insert(id, (account.clone(), next.clone()));
            }
        }
        let committed = crate::AgentClient::for_vault(self.inner.vault_dir.clone())
            .map_err(|_| "Agent unavailable")?
            .request::<()>(&AgentRequest::CommunityAccountWrite {
                entry_id: id,
                marker: SensitiveString::new(marker),
                revision: account.revision,
                bundle: SensitiveString::new(
                    serde_json::to_string(&next).map_err(|_| "invalid account")?,
                ),
            });
        if committed.is_err() {
            // Auditing/transport may fail after the encrypted record committed.
            // Read back the exact revision before acknowledging that outcome.
            let saved = self.load_raw(id, marker)?;
            if saved.generation != next.generation
                || saved.revision != next.revision
                || saved.auth.expose() != next.auth.expose()
                || saved.models != next.models
            {
                return Err("account update refused; account is locked or changed".into());
            }
        }
        self.inner
            .recovery
            .lock()
            .map_err(|_| "account recovery unavailable")?
            .pending
            .remove(&id);
        *account = next;
        Ok(())
    }
    pub(crate) fn catalog(&self) -> Result<Value, String> {
        let mut worker = Adapter::start(None)?;
        self.register(&worker.process, None)?;
        worker.process.send(&json!({"op":"catalog"}))?;
        let frame = worker.next()?;
        if frame["type"] != "result" {
            return Err("invalid provider catalog".into());
        }
        Ok(frame["value"].clone())
    }
    pub(crate) fn login_start(
        &self,
        state: &Arc<AgentState>,
        input: CommunityLoginInput,
        proxy: UpstreamProxyConfig,
    ) -> Result<CommunityLoginStatus, String> {
        if !PROVIDERS.contains(&input.provider.as_str()) {
            return Err("unknown community provider".into());
        }
        let mut logins = self
            .inner
            .logins
            .lock()
            .map_err(|_| "sign-in coordinator unavailable")?;
        logins.retain(|_, l| l.created.elapsed() < Duration::from_secs(1200));
        if logins
            .values()
            .filter(|l| l.status.status == "pending")
            .count()
            >= 4
        {
            return Err("too many pending sign-ins".into());
        }
        let worker = Adapter::start(Some(&proxy))?;
        self.register(&worker.process, None)?;
        let ticket = Uuid::new_v4();
        let status = CommunityLoginStatus {
            ticket,
            status: "pending".into(),
            ..Default::default()
        };
        logins.insert(
            ticket,
            Login {
                status: status.clone(),
                process: Arc::downgrade(&worker.process),
                created: Instant::now(),
            },
        );
        drop(logins);
        let bridge = self.clone();
        let state = Arc::downgrade(state);
        std::thread::spawn(move || {
            let result = bridge.login_run(&state, ticket, worker, input);
            if let Ok(mut logins) = bridge.inner.logins.lock() {
                if let Some(login) = logins.get_mut(&ticket) {
                    if login.status.status == "pending" {
                        match result {
                            Ok(id) => {
                                login.status.status = "complete".into();
                                login.status.entry_id = Some(id);
                            }
                            Err(e) => {
                                login.status.status = "failed".into();
                                login.status.error = Some(e);
                            }
                        }
                    }
                }
            }
        });
        Ok(status)
    }
    fn login_run(
        &self,
        state: &Weak<AgentState>,
        ticket: Uuid,
        mut worker: Adapter,
        input: CommunityLoginInput,
    ) -> Result<Uuid, String> {
        worker.process.send(&json!({"op":"login","provider":input.provider,"method":input.method,"inputs":input.inputs,"key":input.api_key}))?;
        let mut auth = None;
        let mut models = json!({});
        loop {
            let frame = worker.next()?;
            match frame["type"].as_str() {
                Some("challenge") => {
                    let mut logins = self
                        .inner
                        .logins
                        .lock()
                        .map_err(|_| "sign-in coordinator unavailable")?;
                    let login = logins.get_mut(&ticket).ok_or("sign-in expired")?;
                    login.status.url = frame["value"]["url"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned);
                    login.status.instructions =
                        frame["value"]["instructions"].as_str().map(str::to_owned);
                    login.status.user_code = frame["value"]["userCode"].as_str().map(str::to_owned);
                    login.status.method = frame["value"]["method"].as_str().map(str::to_owned);
                }
                Some("auth") => {
                    auth = Some(SensitiveString::new(frame["value"].to_string()));
                    worker.ack(&frame, true)?;
                }
                Some("models") => {
                    models = frame["value"].clone();
                    worker.ack(&frame, true)?;
                }
                Some("result") => {
                    let account = Account {
                        provider: input.provider,
                        generation: Uuid::new_v4(),
                        auth: auth.ok_or("sign-in returned no credentials")?,
                        models,
                        revision: 0,
                        native_method: frame["value"]["nativeMethod"].as_u64().map(|v| v as usize),
                        identity: frame["value"]["identity"].as_str().unwrap_or("").to_owned(),
                    };
                    let state = state.upgrade().ok_or("Agent stopped")?;
                    // Holding the ticket lock makes cancellation linearizable with persistence.
                    let mut logins = self
                        .inner
                        .logins
                        .lock()
                        .map_err(|_| "sign-in coordinator unavailable")?;
                    if logins
                        .get(&ticket)
                        .is_none_or(|l| l.status.status != "pending")
                        || worker.process.canceled.load(Ordering::Acquire)
                    {
                        return Err("sign-in cancelled".into());
                    }
                    let id = with_vault(&state, false, |vault| {
                        let id = if matches!(
                            account.provider.as_str(),
                            "codex" | "grok" | "copilot" | "gemini-cli"
                        ) {
                            let auth = serde_json::from_str(account.auth.expose())
                                .map_err(ServiceError::internal)?;
                            register_cli_with_models(
                                vault,
                                &account.provider,
                                auth,
                                Some(account.models),
                            )?
                        } else {
                            persist(vault, account)?
                        };
                        if state
                            .proxy
                            .lock()
                            .map_err(|_| invalid("proxy unavailable"))?
                            .refresh_provider_credentials(vault, id)
                            .is_err()
                        {
                            crate::logging::write_component_log(
                                crate::logging::AGENT_LOG,
                                "WARN",
                                "event=subscription.login.proxy_refresh_failed",
                            );
                        }
                        Ok(id)
                    })
                    .map_err(|e| e.message)?;
                    if let Some(login) = logins.get_mut(&ticket) {
                        login.status.status = "complete".into();
                        login.status.entry_id = Some(id);
                    }
                    drop(logins);
                    state.sync_revision.fetch_add(1, Ordering::Relaxed);
                    return Ok(id);
                }
                _ => return Err("unexpected sign-in response".into()),
            }
        }
    }
    pub(crate) fn login_poll(&self, ticket: Uuid) -> Result<CommunityLoginStatus, String> {
        self.inner
            .logins
            .lock()
            .map_err(|_| "sign-in coordinator unavailable")?
            .get(&ticket)
            .map(|l| l.status.clone())
            .ok_or_else(|| "sign-in expired".into())
    }
    pub(crate) fn login_code(&self, ticket: Uuid, code: &str) -> Result<(), String> {
        let logins = self
            .inner
            .logins
            .lock()
            .map_err(|_| "sign-in coordinator unavailable")?;
        let login = logins.get(&ticket).ok_or("sign-in expired")?;
        if login.status.status != "pending" || login.status.method.as_deref() != Some("code") {
            return Err("sign-in does not need a code".into());
        }
        login
            .process
            .upgrade()
            .ok_or("sign-in expired")?
            .send(&json!({"type":"code","code":code}))
    }
    pub(crate) fn login_cancel(&self, ticket: Uuid) -> Result<(), String> {
        let mut logins = self
            .inner
            .logins
            .lock()
            .map_err(|_| "sign-in coordinator unavailable")?;
        if let Some(login) = logins.get_mut(&ticket) {
            if login.status.status == "pending" {
                login.status.status = "cancelled".into();
                if let Some(p) = login.process.upgrade() {
                    p.kill();
                }
            }
        }
        Ok(())
    }
    pub(crate) fn refresh(
        &self,
        id: Uuid,
        marker: &str,
        proxy: &UpstreamProxyConfig,
    ) -> Result<Value, String> {
        let lock = self.account_lock(id)?;
        let _guard = lock.try_lock().map_err(|_| ACCOUNT_BUSY)?;
        let mut account = self.load(id, marker)?;
        let epoch = self
            .inner
            .recovery
            .lock()
            .map_err(|_| "account recovery unavailable")?
            .epoch;
        let mut worker = Adapter::start(Some(proxy))?;
        self.register(&worker.process, None)?;
        worker.process.send(&operation("refresh", &account))?;
        loop {
            let frame = worker.next()?;
            match frame["type"].as_str() {
                Some("auth" | "models") => {
                    let result = self.update(id, marker, &mut account, &frame, epoch);
                    worker.ack(&frame, result.is_ok())?;
                    result?;
                }
                Some("result") => return Ok(frame["value"].clone()),
                _ => return Err("invalid refresh response".into()),
            }
        }
    }
    fn generate(
        &self,
        target: ResolvedTarget,
        mut payload: Value,
        session: Option<String>,
        protocol: aipass_proxy::Protocol,
        output: GenerationOutput,
    ) {
        let GenerationOutput {
            head,
            body_tx,
            body_rx,
            cancel,
        } = output;
        let mut head = Some(head);
        let result = (|| -> Result<(), String> {
            let id = target.config.provider_entry_id;
            let lock = self.account_lock(id)?;
            let _guard = lock.try_lock().map_err(|_| ACCOUNT_BUSY)?;
            if cancel.canceled.load(Ordering::Acquire) {
                return Err("request cancelled".into());
            }
            let mut account = self.load(id, &target.api_key)?;
            let epoch = self
                .inner
                .recovery
                .lock()
                .map_err(|_| "account recovery unavailable")?
                .epoch;
            self.inner
                .owners
                .lock()
                .map_err(|_| "session coordinator unavailable")?
                .insert(target.config.id, account.generation);
            let mut worker = Adapter::start(target.upstream_proxy.as_ref())?;
            cancel.attach(&worker.process);
            self.register(&worker.process, Some(target.clone()))?;
            let mut initial = operation("request", &account);
            initial["model"] = payload["model"].clone();
            initial["session"] = json!(session);
            worker.process.send(&initial)?;
            let mut receiver = Some(body_rx);
            let mut wire = String::new();
            let mut bytes = 0usize;
            loop {
                let frame = worker.next()?;
                match frame["type"].as_str() {
                    Some("auth" | "models") => {
                        let result = self.update(id, &target.api_key, &mut account, &frame, epoch);
                        worker.ack(&frame, result.is_ok())?;
                        result?;
                    }
                    Some("prepared") => {
                        wire = frame["value"]["wire"]
                            .as_str()
                            .ok_or("missing provider protocol")?
                            .into();
                        payload["model"] = frame["value"]["model"].clone();
                        let body = self.inner.codec.prepare_protocol(
                            &wire,
                            protocol,
                            payload.take(),
                            target.config.id,
                        )?;
                        worker
                            .process
                            .send(&json!({"type":"request","body":body}))?;
                    }
                    Some("headers") => {
                        if wire.is_empty() {
                            return Err("provider replied before preparation".into());
                        }
                        let status = hyper::StatusCode::from_u16(
                            frame["value"]["status"]
                                .as_u64()
                                .and_then(|s| u16::try_from(s).ok())
                                .ok_or("invalid provider status")?,
                        )
                        .map_err(|_| "invalid provider status")?;
                        let mut headers = hyper::HeaderMap::new();
                        for pair in frame["value"]["headers"].as_array().into_iter().flatten() {
                            if let (Some(k), Some(v)) = (pair[0].as_str(), pair[1].as_str()) {
                                if let (Ok(k), Ok(v)) = (
                                    hyper::header::HeaderName::from_bytes(k.as_bytes()),
                                    hyper::header::HeaderValue::from_str(v),
                                ) {
                                    headers.append(k, v);
                                }
                            }
                        }
                        let response = SubscriptionResponse {
                            status,
                            headers,
                            body: Box::pin(Body {
                                receiver: receiver.take().ok_or("duplicate provider headers")?,
                                _cancel: cancel.clone(),
                            }),
                        };
                        head.take()
                            .ok_or("duplicate provider headers")?
                            .send(Ok((response, wire.clone())))
                            .map_err(|_| "request cancelled")?;
                    }
                    Some("data") => {
                        if head.is_some() {
                            return Err("provider sent data before headers".into());
                        }
                        let data = STANDARD
                            .decode(frame["value"].as_str().ok_or("invalid provider data")?)
                            .map_err(|_| "invalid provider data")?;
                        bytes += data.len();
                        if bytes > 128 * 1024 * 1024 {
                            return Err("provider response exceeds limit".into());
                        }
                        body_tx
                            .blocking_send(Ok(Bytes::from(data)))
                            .map_err(|_| "request cancelled")?;
                        worker.ack(&frame, true)?;
                    }
                    Some("end") => {
                        if head.is_some() {
                            return Err("provider completed without headers".into());
                        }
                        return Ok(());
                    }
                    _ => return Err("unexpected provider response".into()),
                }
            }
        })();
        if let Err(error) = result {
            if let Some(head) = head {
                let _ = head.send(Err(error));
            } else {
                let _ = body_tx.blocking_send(Err(std::io::Error::other(error).into()));
            }
        }
    }
}

mod account;
mod backend;
mod quota;
pub(crate) use account::{
    bind_cli, ensure_owner, identity, identity_value, is_account, legacy_scope_matches, read,
    register_cli, write,
};
use account::{decode, invalid, persist, register_cli_with_models, Account};
#[cfg(test)]
use backend::Cancel;
pub(crate) use backend::Dispatch;
use backend::{operation, Body, GenerationOutput};
use quota::now;
pub(crate) use quota::{quota, refresh, refresh_due};

#[cfg(test)]
mod tests;
