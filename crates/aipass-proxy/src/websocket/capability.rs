//! Capability evidence is distinct from connection health and session fallback.
use super::*;
use sha2::{Digest, Sha256};

pub(crate) type Key = [u8; 32];

/// Opaque configuration identity; never retain credentials in capability events.
pub fn websocket_config_key(target: &ResolvedTarget, outbound: &UpstreamProxyConfig) -> Key {
    let mut hash = Sha256::new();
    for bytes in [
        target.config.provider_entry_id.as_bytes().as_slice(),
        target.config.base_url.as_bytes(),
        target.config.auth_scheme.as_bytes(),
        target.api_key.as_bytes(),
    ] {
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    let metadata = zeroize::Zeroizing::new(
        serde_json::to_vec(&(&target.config.headers, target.config.protocol, outbound))
            .expect("serializable transport configuration"),
    );
    hash.update(metadata.as_slice());
    hash.finalize().into()
}

pub(crate) fn key(state: &RuntimeState, target: &ResolvedTarget) -> Key {
    websocket_config_key(
        target,
        &state.config.read().expect("proxy config").upstream_proxy,
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebsocketCapabilityEvent {
    pub id: Uuid,
    pub provider_entry_id: Uuid,
    pub config_key: Key,
    pub status: u16,
    pub detected_at: i64,
}

#[derive(Clone)]
pub(crate) struct Evidence {
    pub event: WebsocketCapabilityEvent,
    created: Instant,
}

#[derive(Clone, Copy)]
pub struct Observation {
    pub key: Key,
    epoch: Uuid,
}

pub(crate) struct Health {
    epoch: Uuid,
    last_success: Option<Instant>,
    candidate: Option<Evidence>,
    confirmed: bool,
}

impl Default for Health {
    fn default() -> Self {
        Self {
            epoch: Uuid::new_v4(),
            last_success: None,
            candidate: None,
            confirmed: false,
        }
    }
}

pub(crate) fn initial_health(config: &RuntimeConfig) -> HashMap<Key, Health> {
    config
        .routes
        .iter()
        .flat_map(|route| &route.targets)
        .map(|target| {
            (
                websocket_config_key(target, &config.upstream_proxy),
                Health::default(),
            )
        })
        .collect()
}

pub(crate) fn observe(state: &RuntimeState, key: Key) -> Option<Observation> {
    state.ws_health.lock().ok()?.get(&key).map(|h| Observation {
        key,
        epoch: h.epoch,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HandshakeError {
    UnsupportedCandidate(StatusCode),
    Transport(StatusCode),
    Rejected(StatusCode),
}
impl HandshakeError {
    pub fn from_status(status: StatusCode) -> Self {
        match status {
            StatusCode::NOT_FOUND
            | StatusCode::METHOD_NOT_ALLOWED
            | StatusCode::NOT_IMPLEMENTED => Self::UnsupportedCandidate(status),
            StatusCode::UPGRADE_REQUIRED => Self::Transport(status),
            s if s.is_server_error() => Self::Transport(s),
            s => Self::Rejected(s),
        }
    }
    pub fn status(self) -> StatusCode {
        match self {
            Self::UnsupportedCandidate(s) | Self::Transport(s) | Self::Rejected(s) => s,
        }
    }
    pub fn permits_fallback(self) -> bool {
        !matches!(self, Self::Rejected(_))
    }
}

pub(crate) struct ConnectContext<'a> {
    pub client: &'a reqwest::Client,
    pub headers: &'a HeaderMap,
    pub query: Option<&'a str>,
    pub target: &'a ResolvedTarget,
    pub diagnostic: &'a WsDiagnostic<'a>,
    pub timeout: Duration,
    pub hold_deadline: Option<tokio::time::Instant>,
    pub attempts: &'a mut u8,
    pub model: Option<&'a str>,
}

/// Only handshake attempts are retried. This function never submits a generation.
pub(crate) async fn connect_twice(
    ctx: ConnectContext<'_>,
) -> Result<(reqwest::Upgraded, HeaderMap), (HandshakeError, bool)> {
    let mut rejected = 0;
    let mut last = HandshakeError::Transport(StatusCode::GATEWAY_TIMEOUT);
    for index in 0..2 {
        if ctx
            .hold_deadline
            .is_some_and(|d| tokio::time::Instant::now() >= d)
        {
            break;
        }
        if index > 0 {
            *ctx.attempts = ctx.attempts.saturating_add(1);
        }
        let started = Instant::now();
        let started_at = now_unix();
        let result = tokio::time::timeout_at(
            bounded_deadline(ctx.timeout, ctx.hold_deadline),
            connect_upstream(
                ctx.client,
                ctx.headers,
                ctx.query,
                ctx.target,
                Some(ctx.diagnostic),
            ),
        )
        .await
        .unwrap_or_else(|_| {
            ctx.diagnostic
                .log("handshake_timeout", Some(StatusCode::GATEWAY_TIMEOUT), None);
            Err(HandshakeError::Transport(StatusCode::GATEWAY_TIMEOUT))
        });
        match result {
            Ok(connected) => return Ok(connected),
            Err(error) => {
                persist_attempt(
                    ctx.diagnostic.store,
                    (ctx.diagnostic.request_id, ctx.diagnostic.route_id),
                    ctx.target,
                    ctx.model,
                    started_at,
                    started,
                    AttemptOutcome::failure(Some(error.status()), None),
                );
                last = error;
                rejected = if matches!(error, HandshakeError::UnsupportedCandidate(_)) {
                    rejected + 1
                } else {
                    0
                };
                if !error.permits_fallback() {
                    break;
                }
            }
        }
    }
    Err((last, rejected == 2))
}

pub(crate) fn candidate(
    state: &RuntimeState,
    target: &ResolvedTarget,
    observation: Option<Observation>,
    started: Instant,
    status: StatusCode,
) -> Option<Evidence> {
    let mut health = state.ws_health.lock().ok()?;
    let observation = observation?;
    let h = health.get_mut(&observation.key)?;
    if h.epoch != observation.epoch || started.elapsed() >= SESSION_AFFINITY_TTL {
        return None;
    }
    if h.last_success.is_some_and(|success| success >= started) {
        return None;
    }
    let evidence = Evidence {
        event: WebsocketCapabilityEvent {
            id: Uuid::new_v4(),
            provider_entry_id: target.config.provider_entry_id,
            config_key: observation.key,
            status: status.as_u16(),
            detected_at: now_unix(),
        },
        created: Instant::now(),
    };
    if h.candidate
        .as_ref()
        .is_none_or(|e| !h.confirmed && e.created.elapsed() >= SESSION_AFFINITY_TTL)
    {
        h.candidate = Some(evidence.clone());
    }
    h.candidate.clone()
}

pub(crate) fn valid_completion(event: &serde_json::Value) -> bool {
    matches!(
        event["type"].as_str(),
        Some("response.completed" | "response.incomplete")
    ) && event
        .pointer("/response/id")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|id| !id.trim().is_empty())
        && event
            .pointer("/response/output")
            .is_some_and(serde_json::Value::is_array)
}

pub(crate) fn success(state: &RuntimeState, observation: Option<Observation>) {
    let Some(observation) = observation else {
        return;
    };
    if let Ok(mut health) = state.ws_health.lock() {
        let Some(h) = health.get_mut(&observation.key) else {
            return;
        };
        if h.epoch != observation.epoch {
            return;
        }
        h.last_success = Some(Instant::now());
        h.candidate = None;
        h.confirmed = false;
    }
}

pub(crate) fn http_success(state: &RuntimeState, evidence: Option<&Evidence>) {
    let Some(evidence) = evidence.filter(|e| e.created.elapsed() < SESSION_AFFINITY_TTL) else {
        return;
    };
    if let Ok(mut health) = state.ws_health.lock() {
        if let Some(h) = health.get_mut(&evidence.event.config_key) {
            if h.candidate
                .as_ref()
                .is_some_and(|e| e.event.id == evidence.event.id)
                && !h.confirmed
            {
                h.confirmed = true;
                state.usage.log_diagnostic(
                    "warn",
                    format!(
                        "event=proxy.websocket.unsupported provider_entry_id={} status={}",
                        evidence.event.provider_entry_id, evidence.event.status
                    ),
                );
            }
        }
    }
}

pub(crate) fn allowed(state: &RuntimeState, key: Key) -> bool {
    if state.config.read().is_ok_and(|config| {
        config
            .routes
            .iter()
            .flat_map(|route| &route.targets)
            .any(|target| {
                !target.supports_websockets
                    && websocket_config_key(target, &config.upstream_proxy) == key
            })
    }) {
        return false;
    }
    state
        .ws_health
        .lock()
        .map(|h| !h.get(&key).is_some_and(|h| h.confirmed))
        .unwrap_or(true)
}

impl ProxyHandle {
    pub fn begin_websocket_probe(&self, key: Key) -> Option<Observation> {
        observe(&self.state, key)
    }
    pub fn confirm_websocket_probe(&self, observation: Option<Observation>) {
        success(&self.state, observation);
    }
    /// Drain confirmed observations only after all transport tasks have ended.
    pub fn stop_with_websocket_capability_events(&mut self) -> Vec<WebsocketCapabilityEvent> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        self.websocket_capability_events()
    }

    /// Carry unsaved evidence across a local listener restart. A successful new
    /// WS session wins over an older detached event.
    pub fn restore_websocket_capability_event(&self, event: &WebsocketCapabilityEvent) {
        if let Ok(mut health) = self.state.ws_health.lock() {
            if let Some(h) = health.get_mut(&event.config_key) {
                if h.last_success.is_none() && !h.confirmed {
                    h.candidate = Some(Evidence {
                        event: event.clone(),
                        created: Instant::now(),
                    });
                    h.confirmed = true;
                }
            }
        }
    }

    pub fn websocket_capability_events(&self) -> Vec<WebsocketCapabilityEvent> {
        self.state
            .ws_health
            .lock()
            .map(|health| {
                health
                    .values()
                    .filter(|h| h.confirmed)
                    .filter_map(|h| h.candidate.as_ref().map(|e| e.event.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }
    /// Serialize persistence with concurrent WS completion invalidating evidence.
    pub fn with_websocket_capability_event<T>(
        &self,
        event: &WebsocketCapabilityEvent,
        apply: impl FnOnce() -> T,
    ) -> Option<T> {
        let health = self.state.ws_health.lock().ok()?;
        let current = health.get(&event.config_key)?;
        if !current.confirmed
            || !current
                .candidate
                .as_ref()
                .is_some_and(|e| e.event == *event)
        {
            return None;
        }
        Some(apply())
    }
    pub fn acknowledge_websocket_capability_event(&self, event: &WebsocketCapabilityEvent) {
        if let Ok(mut health) = self.state.ws_health.lock() {
            if let Some(h) = health.get_mut(&event.config_key) {
                if h.candidate.as_ref().is_some_and(|e| e.event.id == event.id) {
                    h.candidate = None;
                }
            }
        }
    }
}

/// HTTP sessions use the same bounded identity and expiry as routing affinity.
pub(crate) type HttpSessions = HashMap<(Uuid, String, Key), HttpSession>;

pub(crate) struct HttpSession {
    used: Instant,
    evidence: Option<Evidence>,
}

pub(crate) fn http_fallback(
    state: &RuntimeState,
    route: Uuid,
    session: Option<&str>,
    key: Key,
    set: Option<Option<Evidence>>,
) -> Option<Option<Evidence>> {
    let session = session?;
    let mut sessions = state.ws_sessions.lock().ok()?;
    sessions.retain(|_, entry| entry.used.elapsed() < SESSION_AFFINITY_TTL);
    let identity = (route, session.to_owned(), key);
    if let Some(evidence) = set {
        if sessions.len() >= MAX_SESSION_AFFINITY_ENTRIES {
            if let Some(oldest) = sessions
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| key.clone())
            {
                sessions.remove(&oldest);
            }
        }
        sessions.insert(
            identity,
            HttpSession {
                used: Instant::now(),
                evidence: evidence.clone(),
            },
        );
        Some(evidence)
    } else {
        let entry = sessions.get_mut(&identity)?;
        entry.used = Instant::now();
        Some(entry.evidence.clone())
    }
}
