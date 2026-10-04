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

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Account {
    provider: String,
    generation: Uuid,
    auth: SensitiveString,
    models: Value,
    revision: u64,
    native_method: Option<usize>,
    identity: String,
}
fn invalid(message: impl Into<String>) -> ServiceError {
    ServiceError::new(AgentErrorCode::ValidationFailed, message)
}
fn decode(raw: &str) -> ServiceResult<Account> {
    if raw.len() > MAX_FRAME as usize {
        return Err(invalid("community account exceeds limit"));
    }
    let account: Account =
        serde_json::from_str(raw).map_err(|_| invalid("invalid community account"))?;
    if !PROVIDERS.contains(&account.provider.as_str())
        || !account.models.is_object()
        || account.models.as_object().is_some_and(|m| m.len() > 4096)
    {
        return Err(invalid("invalid community provider or catalog"));
    }
    let auth: Value = serde_json::from_str(account.auth.expose())
        .map_err(|_| invalid("invalid community credentials"))?;
    if !matches!(auth["type"].as_str(), Some("api" | "oauth")) {
        return Err(invalid("invalid community credential type"));
    }
    Ok(account)
}
pub(crate) fn read(vault: &Vault, id: Uuid, marker: &str) -> ServiceResult<SensitiveString> {
    let entry = vault.get_provider_summary(id).map_err(map_vault_error)?;
    if entry.provider_kind != ProviderKind::Official
        || entry.credential_kind != CredentialKind::OAuth
        || !PROVIDERS.contains(&entry.provider_id.as_deref().unwrap_or(""))
        || entry.deleted_at.is_some()
        || entry.archived_at.is_some()
        || !marker.starts_with("aipass:community:")
        || vault.reveal_secret(id).map_err(map_vault_error)? != marker
    {
        return Err(invalid("community account changed or is unavailable"));
    }
    let raw = vault
        .provider_runtime_extension(id, KEY)
        .map_err(map_vault_error)?
        .ok_or_else(|| invalid("community account is missing; reconnect it"))?;
    let account = decode(raw.expose())?;
    if marker != format!("aipass:community:{}", account.generation)
        || Some(account.provider.as_str()) != entry.provider_id.as_deref()
    {
        return Err(invalid("community provider changed; reconnect it"));
    }
    Ok(SensitiveString::new(raw.expose()))
}
pub(crate) fn write(
    vault: &Vault,
    id: Uuid,
    marker: &str,
    revision: u64,
    raw: &str,
) -> ServiceResult<()> {
    let current = decode(read(vault, id, marker)?.expose())?;
    let next = decode(raw)?;
    if current.revision != revision
        || next.revision
            != revision
                .checked_add(1)
                .ok_or_else(|| invalid("account revision overflow"))?
        || next.provider != current.provider
        || next.generation != current.generation
        || next.identity != current.identity
        || next.native_method != current.native_method
    {
        return Err(invalid("stale community credential update refused"));
    }
    let before: Value =
        serde_json::from_str(current.auth.expose()).map_err(ServiceError::internal)?;
    let after: Value = serde_json::from_str(next.auth.expose()).map_err(ServiceError::internal)?;
    ensure_owner(&before, &after).map_err(invalid)?;
    vault
        .set_provider_runtime_extension(id, KEY, Some(&SecretString::new(raw)))
        .map_err(map_vault_error)
}
pub(super) fn identity(auth: &Value) -> Option<String> {
    [
        auth.get("accountId"),
        auth.pointer("/metadata/email"),
        auth.pointer("/metadata/userId"),
        auth.get("userId"),
        auth.get("uid"),
        auth.get("profileArn"),
    ]
    .into_iter()
    .flatten()
    .find_map(identity_value)
    .or_else(|| {
        let token = auth["access"].as_str().or(auth["key"].as_str())?;
        let data = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(token.split('.').nth(1)?)
            .ok()?;
        let value: Value = serde_json::from_slice(&data).ok()?;
        value["sub"]
            .as_str()
            .or(value["email"].as_str())
            .map(str::to_owned)
    })
}
fn identity_value(v: &Value) -> Option<String> {
    v.as_str()
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .or_else(|| v.as_u64().map(|n| n.to_string()))
}
pub(super) fn ensure_owner(before: &Value, after: &Value) -> Result<(), String> {
    let mut anchored = false;
    for path in [
        "/accountId",
        "/userId",
        "/uid",
        "/profileArn",
        "/metadata/email",
        "/metadata/userId",
    ] {
        if let Some(owner) = before.pointer(path).and_then(identity_value) {
            anchored = true;
            if after.pointer(path).and_then(identity_value).as_ref() != Some(&owner) {
                return Err("subscription account ownership changed; reconnect explicitly".into());
            }
        }
    }
    // Compare the same token claim on both sides, independently of cached
    // account metadata. Adding a profile/email is not a change of JWT subject;
    // keeping stale accountId metadata must not hide an actual subject change.
    for field in ["access", "key"] {
        let claims = |auth: &Value| {
            auth[field]
                .as_str()
                .map(crate::subscriptions::claims)
                .unwrap_or(Value::Null)
        };
        let a = claims(before);
        let b = claims(after);
        for key in ["sub", "email"] {
            if let Some(a) = a.get(key).and_then(identity_value) {
                let b = b.get(key).and_then(identity_value);
                if b.as_ref().is_some_and(|b| b != &a) || (b.is_none() && !anchored) {
                    return Err("subscription token ownership changed; reconnect explicitly".into());
                }
            }
        }
    }
    Ok(())
}
pub(crate) fn is_account(vault: &Vault, id: Uuid) -> bool {
    vault
        .provider_runtime_extension(id, KEY)
        .ok()
        .flatten()
        .is_some()
}

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
                    let id = with_vault(&state, false, |vault| persist(vault, account))
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
fn operation(op: &str, account: &Account) -> Value {
    json!({"op":op,"provider":account.provider,"auth":serde_json::from_str::<Value>(account.auth.expose()).unwrap_or(Value::Null),"models":account.models,"nativeMethod":account.native_method})
}
type BodyItem = Result<Bytes, Box<dyn std::error::Error + Send + Sync>>;
struct GenerationOutput {
    head: oneshot::Sender<Result<(SubscriptionResponse, String), String>>,
    body_tx: mpsc::Sender<BodyItem>,
    body_rx: mpsc::Receiver<BodyItem>,
    cancel: Arc<Cancel>,
}
#[derive(Default)]
struct Cancel {
    canceled: AtomicBool,
    process: Mutex<Weak<Operation>>,
}
impl Cancel {
    fn attach(&self, p: &Arc<Operation>) {
        if let Ok(mut current) = self.process.lock() {
            *current = Arc::downgrade(p);
            if self.canceled.load(Ordering::Acquire) {
                p.kill();
            }
        }
    }
    fn cancel(&self) {
        self.canceled.store(true, Ordering::Release);
        if let Ok(current) = self.process.lock() {
            if let Some(p) = current.upgrade() {
                p.kill();
            }
        }
    }
}
struct CancelOnDrop(Option<Arc<Cancel>>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(c) = &self.0 {
            c.cancel();
        }
    }
}
struct Body {
    receiver: mpsc::Receiver<Result<Bytes, Box<dyn std::error::Error + Send + Sync>>>,
    _cancel: Arc<Cancel>,
}
impl Stream for Body {
    type Item = Result<Bytes, Box<dyn std::error::Error + Send + Sync>>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(cx)
    }
}
impl Drop for Body {
    fn drop(&mut self) {
        self._cancel.cancel();
    }
}
impl SubscriptionBackend for CommunityBridge {
    fn request(
        &self,
        target: ResolvedTarget,
        payload: Value,
        session: Option<String>,
    ) -> SubscriptionFuture {
        self.request_protocol(
            target,
            payload,
            aipass_proxy::Protocol::OpenAiChatCompletions,
            session,
        )
    }
    fn request_protocol(
        &self,
        target: ResolvedTarget,
        payload: Value,
        protocol: aipass_proxy::Protocol,
        session: Option<String>,
    ) -> SubscriptionFuture {
        let bridge = self.clone();
        Box::pin(async move {
            let streaming = payload["stream"] == true;
            let owner = target.config.id;
            let (head_tx, head_rx) = oneshot::channel();
            let (body_tx, body_rx) = mpsc::channel(8);
            let cancel = Arc::new(Cancel::default());
            let mut guard = CancelOnDrop(Some(cancel.clone()));
            let worker = bridge.clone();
            std::thread::spawn(move || {
                worker.generate(
                    target,
                    payload,
                    session,
                    protocol,
                    GenerationOutput {
                        head: head_tx,
                        body_tx,
                        body_rx,
                        cancel,
                    },
                )
            });
            let (response, wire) = head_rx.await.map_err(|_| "community worker stopped")??;
            let result = bridge
                .inner
                .codec
                .normalize_protocol(response, &wire, owner, streaming, protocol)
                .await;
            guard.0 = None;
            result
        })
    }
    fn models(&self, target: ResolvedTarget) -> SubscriptionFuture {
        let bridge = self.clone();
        Box::pin(async move {
            let account = tokio::task::spawn_blocking(move || {
                bridge.load(target.config.provider_entry_id, &target.api_key)
            })
            .await
            .map_err(|_| "model discovery stopped")??;
            let data:Vec<Value>=account.models.as_object().ok_or("model catalog unavailable")?.iter().map(|(id,m)|json!({"id":id,"object":"model","owned_by":account.provider,"name":m["name"],"context_window":m["limit"]["context"],"max_output_tokens":m["limit"]["output"],"reasoning":m["reasoning"],"tool_call":m["tool_call"]})).collect();
            let body = Bytes::from(json!({"object":"list","data":data}).to_string());
            let mut headers = hyper::HeaderMap::new();
            headers.insert(
                hyper::header::CONTENT_TYPE,
                hyper::header::HeaderValue::from_static("application/json"),
            );
            Ok(SubscriptionResponse {
                status: hyper::StatusCode::OK,
                headers,
                body: Box::pin(stream::once(async move { Ok(body) })),
            })
        })
    }
    fn history_owner(&self, payload: &Value) -> Result<Option<Uuid>, String> {
        self.inner.codec.history_owner(payload)
    }
    fn revoke(&self) {
        if let Ok(mut recovery) = self.inner.recovery.lock() {
            recovery.epoch = recovery.epoch.wrapping_add(1);
            recovery.pending.clear();
        }
        if let Ok(mut runs) = self.inner.runs.lock() {
            for run in runs.drain(..) {
                if let Some(p) = run.process.upgrade() {
                    p.kill();
                }
            }
        }
        self.inner.codec.retain_targets(&HashSet::new());
        if let Ok(mut owners) = self.inner.owners.lock() {
            owners.clear();
        }
    }
    fn retain_targets(&self, targets: &[&ResolvedTarget]) -> Vec<Uuid> {
        let mut ids: HashSet<Uuid> = targets
            .iter()
            .filter(|t| t.upstream_kind == UpstreamKind::CommunitySubscription)
            .map(|t| t.config.id)
            .collect();
        if let Ok(mut owners) = self.inner.owners.lock() {
            owners.retain(|id, generation| {
                targets.iter().any(|t| {
                    t.config.id == *id && t.api_key == format!("aipass:community:{generation}")
                })
            });
            ids.retain(|id| owners.contains_key(id));
        }
        self.inner.codec.retain_targets(&ids);
        if let Ok(mut runs) = self.inner.runs.lock() {
            runs.retain(|run| {
                let Some(p) = run.process.upgrade() else {
                    return false;
                };
                if let Some(old) = &run.target {
                    if !targets.iter().any(|t| {
                        t.config.id == old.config.id
                            && t.api_key == old.api_key
                            && t.upstream_proxy == old.upstream_proxy
                            && t.upstream_kind == old.upstream_kind
                    }) {
                        p.kill();
                        return false;
                    }
                }
                true
            });
        }
        Vec::new()
    }
}
pub(crate) struct Dispatch {
    pub claude: Arc<crate::claude_bridge::ClaudeBridge>,
    pub community: Arc<CommunityBridge>,
}
impl SubscriptionBackend for Dispatch {
    fn request_protocol(
        &self,
        t: ResolvedTarget,
        p: Value,
        protocol: aipass_proxy::Protocol,
        s: Option<String>,
    ) -> SubscriptionFuture {
        if t.upstream_kind == UpstreamKind::CommunitySubscription {
            self.community.request_protocol(t, p, protocol, s)
        } else {
            self.claude.request(t, p, s)
        }
    }
    fn request(&self, t: ResolvedTarget, p: Value, s: Option<String>) -> SubscriptionFuture {
        if t.upstream_kind == UpstreamKind::CommunitySubscription {
            self.community.request(t, p, s)
        } else {
            self.claude.request(t, p, s)
        }
    }
    fn models(&self, t: ResolvedTarget) -> SubscriptionFuture {
        if t.upstream_kind == UpstreamKind::CommunitySubscription {
            self.community.models(t)
        } else {
            self.claude.models(t)
        }
    }
    fn revoke(&self) {
        self.claude.revoke();
        self.community.revoke();
    }
    fn retain_targets(&self, t: &[&ResolvedTarget]) -> Vec<Uuid> {
        let mut ids = self.claude.retain_targets(t);
        ids.extend(self.community.retain_targets(t));
        ids
    }
    fn history_owner(&self, p: &Value) -> Result<Option<Uuid>, String> {
        let a = self.claude.history_owner(p)?;
        let b = self.community.history_owner(p)?;
        if a.is_some() && b.is_some() && a != b {
            Err("conversation mixes subscription accounts".into())
        } else {
            Ok(a.or(b))
        }
    }
}
fn persist(vault: &Vault, account: Account) -> ServiceResult<Uuid> {
    let raw = SecretString::new(serde_json::to_string(&account).map_err(ServiceError::internal)?);
    decode(raw.expose())?;
    let marker = format!("aipass:community:{}", account.generation);
    let entry = vault
        .add_provider_with_runtime_extension(
            ProviderEntryInput {
                title: format!(
                    "{}{}",
                    account.provider,
                    if account.identity.is_empty() {
                        String::new()
                    } else {
                        format!(" · {}", account.identity)
                    }
                ),
                provider_kind: ProviderKind::Official,
                provider_id: Some(account.provider.clone()),
                credential_kind: CredentialKind::OAuth,
                account_identity: (!account.identity.is_empty()).then_some(account.identity),
                domains: Vec::new(),
                favicon_url: None,
                endpoints: vec![ProviderEndpoint::api("https://community.aipass.invalid/v1")],
                interface_type: InterfaceType::OpenAiCompatible,
                max_concurrent_requests: Some(1),
                supports_websockets: Some(false),
                auth_scheme: AuthScheme::Bearer,
                api_key: marker,
                secret_label: Some("Subscription".into()),
                default_model: account
                    .models
                    .as_object()
                    .and_then(|m| m.keys().next())
                    .cloned(),
                model_aliases: Vec::new(),
                headers: Vec::new(),
                quota: None,
                subscription: Some(SubscriptionSnapshot {
                    source: format!("community:{}", account.provider),
                    observed_at: now(),
                    ..Default::default()
                }),
                gateway: None,
                tags: vec!["subscription".into()],
                notes: None,
                secret_metadata: Default::default(),
            },
            KEY,
            &raw,
        )
        .map_err(map_vault_error)?;
    Ok(entry)
}
fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}
fn snapshot(provider: &str, value: &Value) -> SubscriptionSnapshot {
    SubscriptionSnapshot {
        plan: value["plan"].as_str().map(str::to_owned),
        credits_remaining: value["balance"].as_str().map(str::to_owned),
        source: format!("community:{provider}"),
        observed_at: now(),
        error: value["error"].as_str().map(str::to_owned),
        stale: value["error"].is_string(),
        windows: value["windows"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .map(|(i, w)| SubscriptionWindow {
                id: if w["aside"] != true
                    && w.get("models").is_none()
                    && w.get("notModels").is_none()
                {
                    format!("community_account_{i}")
                } else {
                    format!("community_scoped_{i}")
                },
                label: w["name"].as_str().unwrap_or("Usage").to_owned(),
                used_percent: w["used"].as_f64().filter(|v| v.is_finite() && *v >= 0.),
                resets_at: w["resetsAt"].as_str().map(str::to_owned),
                window_minutes: w["span"].as_u64().map(|seconds| seconds / 60),
                source: Some(format!("community:{provider}")),
            })
            .collect(),
        ..Default::default()
    }
}
pub(crate) fn refresh(state: &Arc<AgentState>, id: Uuid) -> ServiceResult<SubscriptionSnapshot> {
    let (marker, provider, proxy, bridge) = with_vault(state, false, |vault| {
        let marker = SensitiveString::new(vault.reveal_secret(id).map_err(map_vault_error)?);
        let account = decode(read(vault, id, marker.expose())?.expose())?;
        let prefs = crate::provider_runtime::load(vault, id)?;
        let mut proxy = state
            .proxy
            .lock()
            .map_err(|_| invalid("proxy unavailable"))?;
        let global = proxy.load_config(vault)?.upstream_proxy;
        let outbound = crate::provider_runtime::outbound(&prefs)
            .map_err(invalid)?
            .unwrap_or(global);
        Ok((marker, account.provider, outbound, proxy.community_bridge()))
    })?;
    let usage = match bridge.refresh(id, marker.expose(), &proxy) {
        Ok(usage) => usage,
        // An in-flight generation does not invalidate the previous quota reading.
        Err(error) if error == ACCOUNT_BUSY => return Err(invalid(error)),
        Err(error) => json!({"error":error}),
    };
    let snapshot = snapshot(&provider, &usage);
    with_vault(state, false, |vault| {
        read(vault, id, marker.expose())?;
        vault
            .set_provider_runtime_extension(
                id,
                "community_usage_v1",
                Some(&SecretString::new(
                    json!({"observedAt":now(),"usage":usage}).to_string(),
                )),
            )
            .map_err(map_vault_error)?;
        vault
            .update_provider_subscription(id, Some(snapshot.clone()))
            .map_err(map_vault_error)?;
        state
            .proxy
            .lock()
            .map_err(|_| invalid("proxy unavailable"))?
            .refresh_provider_credentials(vault, id)?;
        Ok(())
    })?;
    state.sync_revision.fetch_add(1, Ordering::Relaxed);
    Ok(snapshot)
}
pub(crate) fn refresh_due(state: &Arc<AgentState>) {
    let due = with_vault(state, false, |vault| {
        let mut due = Vec::new();
        for entry in vault.list_provider_summaries().map_err(map_vault_error)? {
            if !is_account(vault, entry.id) {
                continue;
            }
            let preferences = crate::provider_runtime::load(vault, entry.id)?;
            if !preferences.quota_tracking {
                continue;
            }
            let previous = entry.subscription.as_ref().and_then(|s| {
                time::OffsetDateTime::parse(
                    &s.observed_at,
                    &time::format_description::well_known::Rfc3339,
                )
                .ok()
            });
            if previous.is_none_or(|t| {
                (time::OffsetDateTime::now_utc() - t).whole_seconds()
                    >= preferences.quota_refresh_seconds as i64
            }) {
                due.push(entry.id);
            }
        }
        Ok(due)
    });
    if let Ok(due) = due {
        for id in due {
            let _ = refresh(state, id);
        }
    }
}

pub(crate) fn quota(vault: &Vault, id: Uuid) -> Vec<aipass_proxy::QuotaWindow> {
    let Some(raw) = vault
        .provider_runtime_extension(id, "community_usage_v1")
        .ok()
        .flatten()
    else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(raw.expose()) else {
        return Vec::new();
    };
    let timestamp = |v: &str| {
        time::OffsetDateTime::parse(v, &time::format_description::well_known::Rfc3339)
            .ok()
            .and_then(|t| u64::try_from(t.unix_timestamp()).ok())
    };
    let Some(observed_at) = value["observedAt"].as_str().and_then(timestamp) else {
        return Vec::new();
    };
    if value["usage"]["error"].is_string() {
        return Vec::new();
    }
    value["usage"]["windows"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|w| {
            if w["aside"] == true {
                return None;
            }
            let used = w["used"].as_f64().filter(|v| v.is_finite() && *v >= 0.)?;
            let strings = |field: &str| -> Option<Vec<String>> {
                w.get(field).map(|v| {
                    v.as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
            };
            Some(aipass_proxy::QuotaWindow {
                used_basis_points: (used.min(100.) * 100.).round() as u16,
                observed_at,
                resets_at: w["resetsAt"].as_str().and_then(timestamp),
                models: strings("models"),
                not_models: strings("notModels").unwrap_or_default(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn account() -> Account {
        Account{provider:"factory".into(),generation:Uuid::new_v4(),auth:SensitiveString::new(json!({"type":"oauth","access":"fake-private-community-access","refresh":"fake-private-community-refresh","accountId":"alice"}).to_string()),models:json!({"claude-sonnet-4-6":{"name":"Sonnet","api":{"npm":"@ai-sdk/anthropic","url":"https://api.factory.ai/api/llm/a/v1"}}}),revision:0,native_method:None,identity:"alice".into()}
    }
    #[test]
    fn account_rotation_is_encrypted_and_generation_bound() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path(), &SecretString::new("test password"))
            .unwrap()
            .vault;
        let id = persist(&vault, account()).unwrap();
        let marker = vault.reveal_secret(id).unwrap();
        let mut next = decode(read(&vault, id, &marker).unwrap().expose()).unwrap();
        assert!(
            !serde_json::to_string(&vault.get_provider_summary(id).unwrap())
                .unwrap()
                .contains("fake-private")
        );
        next.revision = 1;
        next.auth = SensitiveString::new(
            json!({"type":"oauth","access":"fake-private-community-new","accountId":"alice"})
                .to_string(),
        );
        let raw = serde_json::to_string(&next).unwrap();
        write(&vault, id, &marker, 0, &raw).unwrap();
        assert!(write(&vault, id, &marker, 0, &raw).is_err());
        next.revision = 2;
        next.auth = SensitiveString::new(
            json!({"type":"oauth","access":"other","accountId":"bob"}).to_string(),
        );
        assert!(write(
            &vault,
            id,
            &marker,
            1,
            &serde_json::to_string(&next).unwrap()
        )
        .is_err());
        assert!(read(&vault, id, "aipass:community:wrong").is_err());
        fn scan(path: &Path) {
            for entry in std::fs::read_dir(path).unwrap() {
                let p = entry.unwrap().path();
                if p.is_dir() {
                    scan(&p);
                } else {
                    let bytes = std::fs::read(p).unwrap();
                    assert!(!String::from_utf8_lossy(&bytes).contains("fake-private-community"));
                }
            }
        }
        scan(dir.path());
        vault.archive_provider(id).unwrap();
        assert!(read(&vault, id, &marker).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_auth_write_replays_the_saved_pair_without_overwriting_newer_credentials() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path(), &SecretString::new("test password"))
            .unwrap()
            .vault;
        let before = account();
        let id = persist(&vault, before.clone()).unwrap();
        let marker = vault.reveal_secret(id).unwrap();
        let mut next = before.clone();
        next.revision += 1;
        next.auth = SensitiveString::new(json!({"type":"oauth","accountId":"alice","access":"rotated-access","refresh":"rotated-refresh"}).to_string());
        let mut recovery = AuthRecovery::default();
        recovery.pending.insert(id, (before.clone(), next.clone()));
        let objects = dir.path().join("objects");
        let permissions = std::fs::metadata(&objects).unwrap().permissions();
        std::fs::set_permissions(&objects, std::fs::Permissions::from_mode(0o500)).unwrap();
        let failed = write(
            &vault,
            id,
            &marker,
            0,
            &serde_json::to_string(&next).unwrap(),
        );
        std::fs::set_permissions(&objects, permissions).unwrap();
        assert!(failed.is_err());
        let current = decode(read(&vault, id, &marker).unwrap().expose()).unwrap();
        let replay = recovery.candidate(id, &current).unwrap();
        write(
            &vault,
            id,
            &marker,
            current.revision,
            &serde_json::to_string(&replay).unwrap(),
        )
        .unwrap();
        let saved = decode(read(&vault, id, &marker).unwrap().expose()).unwrap();
        assert!(recovery.candidate(id, &saved).is_none());
        assert!(saved.auth.expose().contains("rotated-refresh"));
        recovery.pending.insert(id, (before, next));
        let mut new_login = saved;
        new_login.generation = Uuid::new_v4();
        assert!(recovery.candidate(id, &new_login).is_none());
        let bridge = CommunityBridge::new(dir.path());
        bridge
            .inner
            .recovery
            .lock()
            .unwrap()
            .pending
            .insert(id, (new_login.clone(), new_login));
        bridge.revoke();
        assert!(bridge.inner.recovery.lock().unwrap().pending.is_empty());
    }
    #[test]
    fn native_catalog_matches_rust_allowlist_and_has_no_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let bridge = CommunityBridge::new(dir.path());
        let value = bridge.catalog().unwrap();
        let ids: HashSet<&str> = value
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, PROVIDERS.iter().copied().collect());
        assert!(value
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["methods"].as_array().is_some_and(|m| !m.is_empty())));
    }
    #[test]
    fn scope_and_advisory_windows_never_exhaust_other_models() {
        let value = json!({"windows":[{"name":"All","used":40,"resetsAt":"2026-10-03T00:00:00Z"},{"name":"Core","used":100,"models":["core"]},{"name":"Total","used":100,"aside":true}]});
        let result = snapshot("factory", &value);
        assert_eq!(result.windows.len(), 3);
        assert!(result.windows[0].id.starts_with("community_account_"));
        assert!(result.windows[1].id.starts_with("community_scoped_"));
        assert!(result.windows[2].id.starts_with("community_scoped_"));
    }
    #[test]
    fn cancellation_stops_a_waiting_native_adapter() {
        let mut worker = Adapter::start(None).unwrap();
        let p = worker.process.clone();
        let cancel = Cancel::default();
        cancel.attach(&p);
        cancel.cancel();
        assert!(p.canceled.load(Ordering::Acquire));
        assert!(p.send(&json!({"op":"catalog"})).is_err());
        assert!(worker.next().is_err());
    }
}
