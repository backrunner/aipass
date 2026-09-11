use super::*;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RetryPolicy {
    pub max_attempts: u8,
    pub failure_threshold: u8,
    pub circuit_open_seconds: u64,
    pub connect_timeout_ms: u64,
    pub first_byte_timeout_ms: u64,
    pub stream_idle_timeout_ms: u64,
    /// Retry failed upstream rounds without returning an error to the local caller.
    #[serde(default)]
    pub silent_retry: bool,
    /// Number of additional target-selection rounds after the initial round.
    #[serde(default = "default_max_silent_retries")]
    pub max_silent_retries: u8,
    /// Hold the client request and poll upstreams with exponential backoff
    /// instead of returning 502 when every target fails.
    #[serde(default)]
    pub hold_on_failure: bool,
    #[serde(default = "default_hold_initial_delay_ms")]
    pub hold_initial_delay_ms: u64,
    #[serde(default = "default_hold_max_delay_ms")]
    pub hold_max_delay_ms: u64,
    /// Total hold budget in milliseconds; 0 means no time limit.
    #[serde(default = "default_hold_max_duration_ms")]
    pub hold_max_duration_ms: u64,
}

pub(crate) fn default_max_silent_retries() -> u8 {
    3
}

pub(crate) fn default_hold_initial_delay_ms() -> u64 {
    500
}

pub(crate) fn default_hold_max_delay_ms() -> u64 {
    10_000
}

pub(crate) fn default_hold_max_duration_ms() -> u64 {
    300_000
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            failure_threshold: 3,
            circuit_open_seconds: 30,
            connect_timeout_ms: 10_000,
            first_byte_timeout_ms: 30_000,
            stream_idle_timeout_ms: 120_000,
            silent_retry: false,
            max_silent_retries: default_max_silent_retries(),
            hold_on_failure: false,
            hold_initial_delay_ms: default_hold_initial_delay_ms(),
            hold_max_delay_ms: default_hold_max_delay_ms(),
            hold_max_duration_ms: default_hold_max_duration_ms(),
        }
    }
}

pub(crate) fn default_weight() -> u32 {
    1
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProxyTargetConfig {
    pub id: Uuid,
    pub provider_entry_id: Uuid,
    pub secret_id: String,
    pub label: String,
    pub base_url: String,
    pub auth_scheme: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    pub group: Option<String>,
    pub priority: u16,
    #[serde(default = "default_weight")]
    pub weight: u32,
    pub enabled: bool,
    /// Native wire protocol of this target's upstream. `None` means "use the
    /// route-level `upstream_protocol`", which keeps configs written before
    /// per-target protocols existed working unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<ProxyProtocol>,
    /// Prefer WebSocket connections when available for this target.
    #[serde(default)]
    pub prefer_ws: bool,
}

impl ProxyTargetConfig {
    pub fn effective_protocol(&self, route_fallback: ProxyProtocol) -> ProxyProtocol {
        self.protocol.unwrap_or(route_fallback)
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RouteStrategy {
    #[default]
    Fallback,
    RoundRobin,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProxyRouteConfig {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub token: String,
    pub inbound_protocol: ProxyProtocol,
    pub upstream_protocol: ProxyProtocol,
    pub conversion_enabled: bool,
    #[serde(default)]
    pub strategy: RouteStrategy,
    pub targets: Vec<ProxyTargetConfig>,
    pub retry: RetryPolicy,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamProxyMode {
    /// Follow the OS system proxy settings (and process env vars).
    #[default]
    System,
    /// Bypass all proxies.
    Direct,
    /// Use proxy env vars, including ones captured from the user's login
    /// shell (e.g. ~/.zshrc) which GUI-launched apps do not inherit.
    Environment,
    /// Use an explicit proxy URL (http/https/socks5, optional user:pass@).
    Custom,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamProxyConfig {
    #[serde(default)]
    pub mode: UpstreamProxyMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProxyConfig {
    pub enabled: bool,
    pub bind_addr: String,
    pub routes: Vec<ProxyRouteConfig>,
    #[serde(default)]
    pub pricing: Vec<ModelPricing>,
    #[serde(default)]
    pub upstream_proxy: UpstreamProxyConfig,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind_addr: "127.0.0.1:8787".into(),
            routes: Vec::new(),
            pricing: Vec::new(),
            upstream_proxy: UpstreamProxyConfig::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelPricing {
    pub model: String,
    pub input_micros_per_million: u64,
    pub output_micros_per_million: u64,
    pub cache_read_micros_per_million: u64,
    pub cache_creation_micros_per_million: u64,
}
