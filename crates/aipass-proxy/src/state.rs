use super::*;

#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("proxy is already running")]
    AlreadyRunning,
    #[error("proxy is not running")]
    NotRunning,
    #[error("invalid proxy configuration: {0}")]
    InvalidConfig(String),
    #[error("proxy IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("upstream request failed: {0}")]
    Upstream(String),
    #[error("conversion failed: {0}")]
    Conversion(#[from] ConversionError),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("proxy state lock poisoned")]
    Poisoned,
}

/// Resolved by the trusted agent, never inferred from an editable URL or a
/// token prefix. Subscription backends have contracts beyond their wire API.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UpstreamKind {
    #[default]
    Standard,
    CodexSubscription,
    GeminiNative,
    Copilot,
    CopilotCli,
    ClaudeSubscription,
    CommunitySubscription,
}

impl UpstreamKind {
    pub(crate) fn is_copilot(self) -> bool {
        matches!(self, Self::Copilot | Self::CopilotCli)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedTarget {
    pub upstream_proxy: Option<UpstreamProxyConfig>,
    pub model_override: Option<String>,
    pub profile: ProviderProfile,
    pub upstream_kind: UpstreamKind,
    /// Verified account-wide or model-scoped quota windows, supplied only by the agent.
    pub quota: Vec<QuotaWindow>,
    /// Provider-owned concurrency limit; missing/zero is unlimited.
    pub max_concurrent_requests: Option<u32>,
    /// Provider-owned capability, resolved from the vault on every refresh.
    pub supports_websockets: bool,
    pub config: ProxyTargetConfig,
    pub api_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuotaWindow {
    pub models: Option<Vec<String>>,
    pub not_models: Vec<String>,
    pub used_basis_points: u16,
    pub observed_at: u64,
    pub resets_at: Option<u64>,
}

impl QuotaWindow {
    pub fn current_usage(&self, now: u64) -> Option<u16> {
        (now >= self.observed_at
            && now - self.observed_at <= 300
            && self.resets_at.is_none_or(|reset| reset > now))
        .then_some(self.used_basis_points)
    }
}

impl ResolvedTarget {
    pub(crate) fn quota_usage(&self, now: u64, model: Option<&str>) -> Option<u16> {
        self.quota
            .iter()
            .filter(|w| {
                let Some(model) = self.model_override.as_deref().or(model) else {
                    return w.models.is_none() && w.not_models.is_empty();
                };
                w.models
                    .as_ref()
                    .is_none_or(|models| models.iter().any(|id| id.eq_ignore_ascii_case(model)))
                    && !w.not_models.iter().any(|id| id.eq_ignore_ascii_case(model))
            })
            .filter_map(|w| w.current_usage(now))
            .max()
    }
}

impl Drop for ResolvedTarget {
    fn drop(&mut self) {
        self.api_key.zeroize();
        if let Some(proxy) = self.upstream_proxy.as_mut() {
            proxy.custom_url.zeroize();
        }
        for (_, value) in &mut self.config.headers {
            value.zeroize();
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedRoute {
    pub config: ProxyRouteConfig,
    pub local_token: String,
    pub targets: Vec<ResolvedTarget>,
}

impl Drop for ResolvedRoute {
    fn drop(&mut self) {
        self.local_token.zeroize();
        self.config.token.zeroize();
        for target in &mut self.config.targets {
            for (_, value) in &mut target.headers {
                value.zeroize();
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeConfig {
    pub enabled: bool,
    pub bind_addr: String,
    pub routes: Vec<ResolvedRoute>,
    pub pricing: Vec<ModelPricing>,
    pub upstream_proxy: UpstreamProxyConfig,
}

impl RuntimeConfig {
    pub fn from_routes(bind_addr: impl Into<String>, routes: Vec<ResolvedRoute>) -> Self {
        Self {
            enabled: true,
            bind_addr: bind_addr.into(),
            routes,
            pricing: Vec::new(),
            upstream_proxy: UpstreamProxyConfig::default(),
        }
    }
}

// A vault sync may change another provider or only metadata. Watch route
// generations so unrelated HTTP/SSE/WS sessions keep their existing transport.
#[derive(Clone)]
pub(crate) struct ConfigWatch {
    pub(crate) receiver: tokio::sync::watch::Receiver<Arc<HashMap<Uuid, u64>>>,
    pub(crate) baseline: Arc<HashMap<Uuid, u64>>,
    pub(crate) route: Uuid,
}
impl ConfigWatch {
    pub(crate) fn subscribe(state: &RuntimeState) -> Self {
        let receiver = state.config_changed.subscribe();
        let baseline = receiver.borrow().clone();
        Self {
            receiver,
            baseline,
            route: Uuid::nil(),
        }
    }
    pub(crate) fn scope(&mut self, route: Uuid) {
        self.route = route;
    }
    pub(crate) fn has_changed(&self) -> Result<bool, tokio::sync::watch::error::RecvError> {
        self.receiver.has_changed()?;
        Ok(self
            .receiver
            .borrow()
            .get(&self.route)
            .copied()
            .unwrap_or(0)
            != self.baseline.get(&self.route).copied().unwrap_or(0))
    }
    pub(crate) async fn changed(&mut self) -> Result<(), tokio::sync::watch::error::RecvError> {
        loop {
            if self.has_changed()? {
                return Ok(());
            }
            self.receiver.changed().await?;
        }
    }
}

pub(crate) type UpstreamClientCache = HashMap<(u64, UpstreamProxyConfig, bool), reqwest::Client>;

#[derive(Clone)]
pub(crate) struct RuntimeState {
    pub(crate) subscription_backend: Arc<RwLock<Option<Arc<dyn SubscriptionBackend>>>>,
    pub(crate) copilot_sessions: Arc<tokio::sync::Mutex<copilot::Sessions>>,
    pub(crate) gemini_signatures: Arc<Mutex<gemini::SignatureLedger>>,
    pub(crate) config: Arc<RwLock<RuntimeConfig>>,
    pub(crate) stats: Arc<Mutex<RuntimeStats>>,
    pub(crate) usage: Arc<UsageStore>,
    pub(crate) health: Arc<Mutex<HashMap<Uuid, TargetHealth>>>,
    pub(crate) image_capabilities: Arc<Mutex<images::Ledger>>,
    pub(crate) ws_health:
        Arc<Mutex<HashMap<websocket::capability::Key, websocket::capability::Health>>>,
    pub(crate) ws_sessions: Arc<Mutex<websocket::capability::HttpSessions>>,
    pub(crate) rr_counters: Arc<Mutex<HashMap<Uuid, AtomicU64>>>,
    pub(crate) session_affinity: Arc<Mutex<HashMap<(Uuid, String), SessionAffinity>>>,
    pub(crate) clients: Arc<Mutex<UpstreamClientCache>>,
    pub(crate) config_changed: tokio::sync::watch::Sender<Arc<HashMap<Uuid, u64>>>,
    pub(crate) in_flight_requests: Arc<AtomicU64>,
    pub(crate) target_activity: Arc<Mutex<HashMap<Uuid, u64>>>,
    pub(crate) provider_activity: Arc<Mutex<HashMap<Uuid, u64>>>,
}

#[derive(Default)]
pub(crate) struct RuntimeStats {
    pub(crate) requests: u64,
    pub(crate) failures: u64,
    pub(crate) last_error: Option<String>,
    pub(crate) first_token_samples: VecDeque<Option<u64>>,
    pub(crate) recent_request_times: VecDeque<Instant>,
    pub(crate) recent_failure_times: VecDeque<Instant>,
    pub(crate) recent_token_totals: VecDeque<(Instant, u64)>,
}

pub(crate) struct InFlightGuard {
    pub(crate) counter: Arc<AtomicU64>,
}

impl InFlightGuard {
    pub(crate) fn new(counter: Arc<AtomicU64>) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        Self { counter }
    }
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Request-local activity only; never stores credentials or payloads.
pub(crate) struct TargetActivityGuard {
    pub(crate) counts: Arc<Mutex<HashMap<Uuid, u64>>>,
    pub(crate) target_id: Uuid,
}

impl TargetActivityGuard {
    pub(crate) fn new(state: &RuntimeState, target_id: Uuid) -> Self {
        let counts = state.target_activity.clone();
        if let Ok(mut counts) = counts.lock() {
            *counts.entry(target_id).or_default() += 1;
        }
        Self { counts, target_id }
    }
}

impl Drop for TargetActivityGuard {
    fn drop(&mut self) {
        if let Ok(mut counts) = self.counts.lock() {
            if let Some(count) = counts.get_mut(&self.target_id) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    counts.remove(&self.target_id);
                }
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct TargetHealth {
    pub(crate) last_status: Option<u16>,
    pub(crate) consecutive_failures: u8,
    pub(crate) consecutive_successes: u8,
    pub(crate) open_until: Option<Instant>,
    pub(crate) last_failure_at: Option<Instant>,
    pub(crate) recovering: bool,
    pub(crate) reopen_count: u8,
    pub(crate) probe_id: Option<Uuid>,
}

/// Require a short run of successful requests after a target failure so an
/// intermittent provider does not immediately flap back to healthy.
pub(crate) const RECOVERY_SUCCESS_THRESHOLD: u8 = 2;

#[derive(Clone, Debug)]
pub(crate) struct SessionAffinity {
    pub(crate) target_id: Uuid,
    pub(crate) last_used: Instant,
}

pub(crate) fn set_error(state: &RuntimeState, error: String) {
    // Raw errors can embed upstream URLs, userinfo, headers or response bodies.
    state
        .usage
        .log_diagnostic("error", "event=proxy.runtime.failed".into());
    if let Ok(mut stats) = state.stats.lock() {
        stats.last_error = Some(error);
    }
}

pub(crate) fn record_request(state: &RuntimeState, success: bool, first_token_ms: Option<u64>) {
    if let Ok(mut stats) = state.stats.lock() {
        stats.requests = stats.requests.saturating_add(1);
        if !success {
            stats.failures = stats.failures.saturating_add(1);
        } else {
            stats.last_error = None;
        }
        stats.first_token_samples.push_back(first_token_ms);
        while stats.first_token_samples.len() > 100 {
            stats.first_token_samples.pop_front();
        }
        stats.recent_request_times.push_back(Instant::now());
        if !success {
            stats.recent_failure_times.push_back(Instant::now());
        }
        let cutoff = Instant::now() - Duration::from_secs(60);
        while stats
            .recent_request_times
            .front()
            .is_some_and(|time| *time < cutoff)
        {
            stats.recent_request_times.pop_front();
        }
        while stats
            .recent_failure_times
            .front()
            .is_some_and(|time| *time < cutoff)
        {
            stats.recent_failure_times.pop_front();
        }
    }
}

pub(crate) fn record_recent_tokens(state: &RuntimeState, tokens: u64) {
    if let Ok(mut stats) = state.stats.lock() {
        let now = Instant::now();
        stats.recent_token_totals.push_back((now, tokens));
        let cutoff = now - Duration::from_secs(60);
        while stats
            .recent_token_totals
            .front()
            .is_some_and(|(time, _)| *time < cutoff)
        {
            stats.recent_token_totals.pop_front();
        }
    }
}

#[cfg(test)]
mod quota_tests {
    use super::*;
    #[test]
    fn stale_unknown_and_reset_quota_never_remain_exhausted() {
        let mut quota = QuotaWindow {
            models: None,
            not_models: Vec::new(),
            used_basis_points: 10_000,
            observed_at: 1000,
            resets_at: Some(1200),
        };
        assert_eq!(quota.current_usage(1100), Some(10_000));
        assert_eq!(quota.current_usage(1200), None);
        assert_eq!(quota.current_usage(999), None);
        quota.resets_at = None;
        assert_eq!(quota.current_usage(1301), None);
    }
}
