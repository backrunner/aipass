use aipass_proxy_conversion::{
    BuiltinConversionPlugin, ConversionPlugin, ProxyProtocol, StreamConverter, TokenUsage,
};
use bytes::Bytes;
use concurrency::{capacity_response, ProviderAtCapacity, ProviderPermit};
use futures_util::{stream, Stream, StreamExt};
use http::{header, HeaderMap, HeaderValue, Request, Response, StatusCode};
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::convert::Infallible;
use std::error::Error as StdError;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use uuid::Uuid;
use zeroize::Zeroize;

pub use aipass_proxy_conversion::{supports, ConversionError, ProxyProtocol as Protocol};

mod concurrency;
mod diagnostics;
mod images;
mod routing;
use routing::{complete_target_success, RecoveryPermit};
mod shell_env;
mod websocket;
pub use websocket::capability::{
    websocket_config_key, Observation as WebsocketObservation, WebsocketCapabilityEvent,
};
pub use websocket::{probe_websocket, WebsocketProbeResult};

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

fn default_max_silent_retries() -> u8 {
    3
}

fn default_hold_initial_delay_ms() -> u64 {
    500
}

fn default_hold_max_delay_ms() -> u64 {
    10_000
}

fn default_hold_max_duration_ms() -> u64 {
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

fn default_weight() -> u32 {
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct UsageAggregate {
    pub request_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub estimated_cost_micros: u64,
    #[serde(default)]
    pub attempt_count: u64,
    #[serde(default)]
    pub completed_attempts: u64,
    #[serde(default)]
    pub successful_attempts: u64,
    #[serde(default)]
    pub success_rate_bps: u16,
    #[serde(default)]
    pub average_first_token_ms: Option<u64>,
    pub providers: Vec<ProviderUsageAggregate>,
    #[serde(default)]
    pub models: Vec<ModelUsageAggregate>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UsageGranularity {
    Hour,
    #[default]
    Day,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UsageTimeseriesPoint {
    /// Local YYYY-MM-DD for daily buckets; UTC RFC 3339 bucket start for hours.
    pub date: String,
    pub request_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub estimated_cost_micros: u64,
    #[serde(default)]
    pub models: Vec<UsageTimeseriesModel>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UsageTimeseriesModel {
    pub model: Option<String>,
    pub request_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub estimated_cost_micros: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsageAggregate {
    pub provider_entry_id: Uuid,
    pub secret_id: String,
    pub request_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub estimated_cost_micros: u64,
    #[serde(default)]
    pub attempt_count: u64,
    #[serde(default)]
    pub completed_attempts: u64,
    #[serde(default)]
    pub successful_attempts: u64,
    #[serde(default)]
    pub success_rate_bps: u16,
    #[serde(default)]
    pub average_first_token_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsageAggregate {
    pub provider_entry_id: Uuid,
    pub secret_id: String,
    pub model: Option<String>,
    pub request_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub estimated_cost_micros: u64,
    #[serde(default)]
    pub attempt_count: u64,
    #[serde(default)]
    pub completed_attempts: u64,
    #[serde(default)]
    pub successful_attempts: u64,
    #[serde(default)]
    pub success_rate_bps: u16,
    #[serde(default)]
    pub average_first_token_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStatus {
    pub running: bool,
    pub enabled: bool,
    pub bind_addr: String,
    pub active_routes: usize,
    pub requests: u64,
    pub failures: u64,
    pub last_error: Option<String>,
    #[serde(default)]
    pub degraded: bool,
    /// Enabled targets with an unresolved failure in the last 60 seconds or an open circuit.
    #[serde(default)]
    pub degraded_target_ids: Vec<Uuid>,
    /// Requests completed in the last 60 seconds (for RPM display).
    #[serde(default)]
    pub recent_requests: u64,
    /// Tokens (input + output + cache) consumed in the last 60 seconds (for TPM display).
    #[serde(default)]
    pub recent_tokens: u64,
    #[serde(default)]
    pub success_rate_bps: u16,
    #[serde(default)]
    pub average_first_token_ms: Option<u64>,
    /// Active HTTP generations and individual WebSocket response.create requests.
    /// Idle WebSocket connections are not requests.
    #[serde(default)]
    pub in_flight_requests: u64,
    /// Enabled upstream targets available while the proxy is running; zero
    /// while stopped.
    #[serde(default)]
    pub available_channels: usize,
    /// Total enabled upstream targets on enabled routes.
    #[serde(default)]
    pub total_channels: usize,
    #[serde(default)]
    pub channels: Vec<ChannelStatus>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChannelStatus {
    pub route_id: Uuid,
    pub target_id: Uuid,
    pub provider_entry_id: Uuid,
    pub secret_id: String,
    pub in_flight_requests: u64,
    pub degraded: bool,
    pub available: bool,
    pub cooldown_remaining_ms: u64,
    pub websocket_cooling_down: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProxyLogEntry {
    pub timestamp: i64,
    pub level: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UsageRecord {
    pub id: Uuid,
    pub started_at: i64,
    pub duration_ms: u64,
    #[serde(default)]
    pub first_token_ms: Option<u64>,
    pub route_id: Uuid,
    pub provider_entry_id: Uuid,
    pub secret_id: String,
    pub model: Option<String>,
    pub inbound_protocol: ProxyProtocol,
    pub upstream_protocol: ProxyProtocol,
    pub status: u16,
    pub attempts: u8,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub estimated_cost_micros: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AttemptRecord {
    pub id: Uuid,
    #[serde(default)]
    pub request_id: Option<Uuid>,
    pub started_at: i64,
    pub duration_ms: u64,
    pub first_token_ms: Option<u64>,
    pub route_id: Uuid,
    pub target_id: Uuid,
    pub provider_entry_id: Uuid,
    pub secret_id: String,
    pub model: Option<String>,
    pub status: Option<u16>,
    /// `None` means the client disconnected before the stream outcome was known.
    pub success: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsageRow {
    pub started_at: i64,
    pub provider_entry_id: Uuid,
    pub secret_id: String,
    pub model: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub status: u16,
    pub first_token_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AttemptRow {
    started_at: i64,
    provider_entry_id: Uuid,
    secret_id: String,
    model: Option<String>,
    success: Option<bool>,
    first_token_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default)]
struct AttemptStats {
    attempt_count: u64,
    completed_attempts: u64,
    successful_attempts: u64,
}

#[derive(Default)]
struct RequestStats {
    request_count: u64,
    successful_requests: u64,
    recent_first_tokens: VecDeque<Option<u64>>,
}

impl RequestStats {
    fn observe(&mut self, row: &UsageRow) {
        self.request_count = self.request_count.saturating_add(1);
        if (200..300).contains(&row.status) {
            self.successful_requests = self.successful_requests.saturating_add(1);
        }
        self.recent_first_tokens.push_back(row.first_token_ms);
        while self.recent_first_tokens.len() > 100 {
            self.recent_first_tokens.pop_front();
        }
    }

    fn success_rate_bps(&self) -> u16 {
        if self.request_count == 0 {
            return 0;
        }
        let bps = self
            .successful_requests
            .saturating_mul(10_000)
            .saturating_add(self.request_count / 2)
            / self.request_count;
        u16::try_from(bps.min(10_000)).unwrap_or(10_000)
    }

    fn average_first_token_ms(&self) -> Option<u64> {
        let (total, count) = self
            .recent_first_tokens
            .iter()
            .flatten()
            .fold((0_u64, 0_u64), |(total, count), value| {
                (total.saturating_add(*value), count.saturating_add(1))
            });
        (count > 0).then(|| total / count)
    }
}

impl AttemptStats {
    fn observe(&mut self, row: &AttemptRow) {
        self.attempt_count = self.attempt_count.saturating_add(1);
        if let Some(success) = row.success {
            self.completed_attempts = self.completed_attempts.saturating_add(1);
            if success {
                self.successful_attempts = self.successful_attempts.saturating_add(1);
            }
        }
    }
}

pub struct UsageStore {
    path: PathBuf,
    connection: Mutex<Connection>,
}

impl UsageStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ProxyError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(ProxyError::Io)?;
        }
        let connection = Connection::open(&path).map_err(ProxyError::Sqlite)?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; CREATE TABLE IF NOT EXISTS proxy_usage (id TEXT PRIMARY KEY, started_at INTEGER NOT NULL, duration_ms INTEGER NOT NULL, first_token_ms INTEGER, route_id TEXT NOT NULL, provider_entry_id TEXT NOT NULL, secret_id TEXT NOT NULL, model TEXT, inbound_protocol TEXT NOT NULL, upstream_protocol TEXT NOT NULL, status INTEGER NOT NULL, attempts INTEGER NOT NULL, input_tokens INTEGER NOT NULL, output_tokens INTEGER NOT NULL, cache_read_tokens INTEGER NOT NULL, cache_creation_tokens INTEGER NOT NULL, estimated_cost_micros INTEGER NOT NULL); CREATE INDEX IF NOT EXISTS proxy_usage_started_at_idx ON proxy_usage(started_at); CREATE TABLE IF NOT EXISTS proxy_attempts (id TEXT PRIMARY KEY, request_id TEXT, started_at INTEGER NOT NULL, duration_ms INTEGER NOT NULL, first_token_ms INTEGER, route_id TEXT NOT NULL, target_id TEXT NOT NULL, provider_entry_id TEXT NOT NULL, secret_id TEXT NOT NULL, model TEXT, status INTEGER, success INTEGER); CREATE INDEX IF NOT EXISTS proxy_attempts_started_at_idx ON proxy_attempts(started_at)").map_err(ProxyError::Sqlite)?;
        let has_request_id = connection
            .prepare("PRAGMA table_info(proxy_attempts)")
            .map_err(ProxyError::Sqlite)?
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(ProxyError::Sqlite)?
            .filter_map(Result::ok)
            .any(|name| name == "request_id");
        if !has_request_id {
            connection
                .execute("ALTER TABLE proxy_attempts ADD COLUMN request_id TEXT", [])
                .map_err(ProxyError::Sqlite)?;
        }
        let has_first_token = connection
            .prepare("PRAGMA table_info(proxy_usage)")
            .map_err(ProxyError::Sqlite)?
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(ProxyError::Sqlite)?
            .filter_map(Result::ok)
            .any(|name| name == "first_token_ms");
        if !has_first_token {
            connection
                .execute(
                    "ALTER TABLE proxy_usage ADD COLUMN first_token_ms INTEGER",
                    [],
                )
                .map_err(ProxyError::Sqlite)?;
        }
        Self::init_diagnostics(&connection)?;
        Ok(Self {
            path,
            connection: Mutex::new(connection),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn record(&self, item: &UsageRecord) -> Result<(), ProxyError> {
        self.log_request(item);
        let conn = self.connection.lock().map_err(|_| ProxyError::Poisoned)?;
        conn.execute("INSERT OR REPLACE INTO proxy_usage (id, started_at, duration_ms, first_token_ms, route_id, provider_entry_id, secret_id, model, inbound_protocol, upstream_protocol, status, attempts, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens, estimated_cost_micros) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)", params![item.id.to_string(), item.started_at, item.duration_ms, item.first_token_ms, item.route_id.to_string(), item.provider_entry_id.to_string(), item.secret_id, item.model, serde_json::to_string(&item.inbound_protocol).unwrap_or_default(), serde_json::to_string(&item.upstream_protocol).unwrap_or_default(), item.status, item.attempts, item.input_tokens, item.output_tokens, item.cache_read_tokens, item.cache_creation_tokens, item.estimated_cost_micros]).map_err(ProxyError::Sqlite)?;
        Ok(())
    }

    pub fn record_attempt(&self, item: &AttemptRecord) -> Result<(), ProxyError> {
        self.log_attempt(item);
        let conn = self.connection.lock().map_err(|_| ProxyError::Poisoned)?;
        conn.execute(
            "INSERT OR REPLACE INTO proxy_attempts (id, request_id, started_at, duration_ms, first_token_ms, route_id, target_id, provider_entry_id, secret_id, model, status, success) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                item.id.to_string(),
                item.request_id.map(|id| id.to_string()),
                item.started_at,
                item.duration_ms,
                item.first_token_ms,
                item.route_id.to_string(),
                item.target_id.to_string(),
                item.provider_entry_id.to_string(),
                item.secret_id,
                item.model,
                item.status,
                item.success.map(|value| if value { 1_i64 } else { 0_i64 }),
            ],
        )
        .map_err(ProxyError::Sqlite)?;
        Ok(())
    }

    pub fn count(&self) -> Result<u64, ProxyError> {
        let conn = self.connection.lock().map_err(|_| ProxyError::Poisoned)?;
        conn.query_row("SELECT COUNT(*) FROM proxy_usage", [], |row| {
            row.get::<_, i64>(0)
        })
        .map(|value| value as u64)
        .map_err(ProxyError::Sqlite)
    }

    pub fn clear(&self) -> Result<(), ProxyError> {
        let conn = self.connection.lock().map_err(|_| ProxyError::Poisoned)?;
        conn.execute_batch(
            "DELETE FROM proxy_usage; DELETE FROM proxy_attempts; PRAGMA wal_checkpoint(TRUNCATE);",
        )
        .map_err(ProxyError::Sqlite)
    }

    pub fn iter_rows(&self) -> Result<Vec<UsageRow>, ProxyError> {
        Ok(self
            .rows_since(None)?
            .into_iter()
            .map(|(_, row)| row)
            .collect())
    }

    pub fn summary(&self, cost: impl Fn(&UsageRow) -> u64) -> Result<UsageAggregate, ProxyError> {
        self.summary_since(None, cost)
    }

    pub fn summary_since(
        &self,
        since: Option<i64>,
        cost: impl Fn(&UsageRow) -> u64,
    ) -> Result<UsageAggregate, ProxyError> {
        let mut aggregate = UsageAggregate::default();
        let mut request_stats = RequestStats::default();
        let mut providers: HashMap<(Uuid, String), (ProviderUsageAggregate, i64)> = HashMap::new();
        let mut models: HashMap<(Uuid, String, Option<String>), (ModelUsageAggregate, i64)> =
            HashMap::new();
        let mut provider_request_stats: HashMap<(Uuid, String), RequestStats> = HashMap::new();
        let mut model_request_stats: HashMap<(Uuid, String, Option<String>), RequestStats> =
            HashMap::new();
        self.visit_rows_since(since, |_, row| {
            let row_cost = cost(&row);
            request_stats.observe(&row);
            aggregate.request_count = aggregate.request_count.saturating_add(1);
            aggregate.input_tokens = aggregate.input_tokens.saturating_add(row.input_tokens);
            aggregate.output_tokens = aggregate.output_tokens.saturating_add(row.output_tokens);
            aggregate.cache_read_tokens = aggregate
                .cache_read_tokens
                .saturating_add(row.cache_read_tokens);
            aggregate.cache_creation_tokens = aggregate
                .cache_creation_tokens
                .saturating_add(row.cache_creation_tokens);
            aggregate.estimated_cost_micros =
                aggregate.estimated_cost_micros.saturating_add(row_cost);
            let key = (row.provider_entry_id, row.secret_id.clone());
            let (provider, last_started) = providers.entry(key.clone()).or_insert_with(|| {
                (
                    ProviderUsageAggregate {
                        provider_entry_id: row.provider_entry_id,
                        secret_id: row.secret_id.clone(),
                        request_count: 0,
                        input_tokens: 0,
                        output_tokens: 0,
                        cache_read_tokens: 0,
                        cache_creation_tokens: 0,
                        estimated_cost_micros: 0,
                        attempt_count: 0,
                        completed_attempts: 0,
                        successful_attempts: 0,
                        success_rate_bps: 0,
                        average_first_token_ms: None,
                    },
                    0,
                )
            });
            provider.request_count = provider.request_count.saturating_add(1);
            provider.input_tokens = provider.input_tokens.saturating_add(row.input_tokens);
            provider.output_tokens = provider.output_tokens.saturating_add(row.output_tokens);
            provider.cache_read_tokens = provider
                .cache_read_tokens
                .saturating_add(row.cache_read_tokens);
            provider.cache_creation_tokens = provider
                .cache_creation_tokens
                .saturating_add(row.cache_creation_tokens);
            provider.estimated_cost_micros =
                provider.estimated_cost_micros.saturating_add(row_cost);
            *last_started = (*last_started).max(row.started_at);
            provider_request_stats
                .entry(key.clone())
                .or_default()
                .observe(&row);

            let model_key = (
                row.provider_entry_id,
                row.secret_id.clone(),
                row.model.clone(),
            );
            let (model, last_started) = models.entry(model_key.clone()).or_insert_with(|| {
                (
                    ModelUsageAggregate {
                        provider_entry_id: model_key.0,
                        secret_id: model_key.1.clone(),
                        model: model_key.2.clone(),
                        request_count: 0,
                        input_tokens: 0,
                        output_tokens: 0,
                        cache_read_tokens: 0,
                        cache_creation_tokens: 0,
                        estimated_cost_micros: 0,
                        attempt_count: 0,
                        completed_attempts: 0,
                        successful_attempts: 0,
                        success_rate_bps: 0,
                        average_first_token_ms: None,
                    },
                    0,
                )
            });
            model.request_count = model.request_count.saturating_add(1);
            model.input_tokens = model.input_tokens.saturating_add(row.input_tokens);
            model.output_tokens = model.output_tokens.saturating_add(row.output_tokens);
            model.cache_read_tokens = model
                .cache_read_tokens
                .saturating_add(row.cache_read_tokens);
            model.cache_creation_tokens = model
                .cache_creation_tokens
                .saturating_add(row.cache_creation_tokens);
            model.estimated_cost_micros = model.estimated_cost_micros.saturating_add(row_cost);
            *last_started = (*last_started).max(row.started_at);
            model_request_stats
                .entry(model_key)
                .or_default()
                .observe(&row);
        })?;
        let mut attempt_stats = AttemptStats::default();
        let mut provider_attempts: HashMap<(Uuid, String), AttemptStats> = HashMap::new();
        let mut model_attempts: HashMap<(Uuid, String, Option<String>), AttemptStats> =
            HashMap::new();
        self.visit_attempt_rows(since, |row| {
            attempt_stats.observe(&row);
            let provider_key = (row.provider_entry_id, row.secret_id.clone());
            let (provider, last_started) =
                providers.entry(provider_key.clone()).or_insert_with(|| {
                    (
                        ProviderUsageAggregate {
                            provider_entry_id: row.provider_entry_id,
                            secret_id: row.secret_id.clone(),
                            request_count: 0,
                            input_tokens: 0,
                            output_tokens: 0,
                            cache_read_tokens: 0,
                            cache_creation_tokens: 0,
                            estimated_cost_micros: 0,
                            attempt_count: 0,
                            completed_attempts: 0,
                            successful_attempts: 0,
                            success_rate_bps: 0,
                            average_first_token_ms: None,
                        },
                        0,
                    )
                });
            *last_started = (*last_started).max(row.started_at);
            let stats = provider_attempts.entry(provider_key).or_default();
            stats.observe(&row);
            provider.attempt_count = stats.attempt_count;
            provider.completed_attempts = stats.completed_attempts;
            provider.successful_attempts = stats.successful_attempts;

            let model_key = (
                row.provider_entry_id,
                row.secret_id.clone(),
                row.model.clone(),
            );
            let (model, last_started) = models.entry(model_key.clone()).or_insert_with(|| {
                (
                    ModelUsageAggregate {
                        provider_entry_id: row.provider_entry_id,
                        secret_id: row.secret_id.clone(),
                        model: row.model.clone(),
                        request_count: 0,
                        input_tokens: 0,
                        output_tokens: 0,
                        cache_read_tokens: 0,
                        cache_creation_tokens: 0,
                        estimated_cost_micros: 0,
                        attempt_count: 0,
                        completed_attempts: 0,
                        successful_attempts: 0,
                        success_rate_bps: 0,
                        average_first_token_ms: None,
                    },
                    0,
                )
            });
            *last_started = (*last_started).max(row.started_at);
            let stats = model_attempts.entry(model_key).or_default();
            stats.observe(&row);
            model.attempt_count = stats.attempt_count;
            model.completed_attempts = stats.completed_attempts;
            model.successful_attempts = stats.successful_attempts;
        })?;
        aggregate.attempt_count = attempt_stats.attempt_count;
        aggregate.completed_attempts = attempt_stats.completed_attempts;
        aggregate.successful_attempts = attempt_stats.successful_attempts;
        aggregate.success_rate_bps = request_stats.success_rate_bps();
        aggregate.average_first_token_ms = request_stats.average_first_token_ms();
        let mut providers: Vec<(ProviderUsageAggregate, i64)> = providers.into_values().collect();
        for (provider, _) in &mut providers {
            let key = (provider.provider_entry_id, provider.secret_id.clone());
            if let Some(stats) = provider_request_stats.get(&key) {
                provider.success_rate_bps = stats.success_rate_bps();
                provider.average_first_token_ms = stats.average_first_token_ms();
            }
        }
        providers.sort_by_key(|provider| std::cmp::Reverse(provider.1));
        aggregate.providers = providers
            .into_iter()
            .map(|(provider, _)| provider)
            .collect();
        let mut models: Vec<(ModelUsageAggregate, i64)> = models.into_values().collect();
        for (model, _) in &mut models {
            let key = (
                model.provider_entry_id,
                model.secret_id.clone(),
                model.model.clone(),
            );
            if let Some(stats) = model_request_stats.get(&key) {
                model.success_rate_bps = stats.success_rate_bps();
                model.average_first_token_ms = stats.average_first_token_ms();
            }
        }
        models.sort_by_key(|model| std::cmp::Reverse(model.1));
        aggregate.models = models.into_iter().map(|(model, _)| model).collect();
        Ok(aggregate)
    }

    pub fn timeseries(
        &self,
        days: u32,
        timezone_offset_minutes: i32,
        granularity: UsageGranularity,
        cost: impl Fn(&UsageRow) -> u64,
    ) -> Result<Vec<UsageTimeseriesPoint>, ProxyError> {
        let timezone_offset_seconds = i64::from(timezone_offset_minutes.clamp(-1_440, 1_440)) * 60;
        let cutoff = usage_window_start(days, timezone_offset_minutes, granularity);
        let mut buckets: std::collections::BTreeMap<String, UsageTimeseriesPoint> =
            std::collections::BTreeMap::new();
        self.visit_rows_since_with_offset(
            Some(cutoff),
            timezone_offset_seconds,
            granularity,
            |date, row| {
                let point = buckets
                    .entry(date.clone())
                    .or_insert_with(|| UsageTimeseriesPoint {
                        date,
                        request_count: 0,
                        input_tokens: 0,
                        output_tokens: 0,
                        cache_read_tokens: 0,
                        cache_creation_tokens: 0,
                        estimated_cost_micros: 0,
                        models: Vec::new(),
                    });
                point.request_count = point.request_count.saturating_add(1);
                point.input_tokens = point.input_tokens.saturating_add(row.input_tokens);
                point.output_tokens = point.output_tokens.saturating_add(row.output_tokens);
                point.cache_read_tokens = point
                    .cache_read_tokens
                    .saturating_add(row.cache_read_tokens);
                point.cache_creation_tokens = point
                    .cache_creation_tokens
                    .saturating_add(row.cache_creation_tokens);
                let row_cost = cost(&row);
                point.estimated_cost_micros = point.estimated_cost_micros.saturating_add(row_cost);
                if let Some(model) = point
                    .models
                    .iter_mut()
                    .find(|model| model.model == row.model)
                {
                    model.request_count = model.request_count.saturating_add(1);
                    model.input_tokens = model.input_tokens.saturating_add(row.input_tokens);
                    model.output_tokens = model.output_tokens.saturating_add(row.output_tokens);
                    model.cache_read_tokens = model
                        .cache_read_tokens
                        .saturating_add(row.cache_read_tokens);
                    model.cache_creation_tokens = model
                        .cache_creation_tokens
                        .saturating_add(row.cache_creation_tokens);
                    model.estimated_cost_micros =
                        model.estimated_cost_micros.saturating_add(row_cost);
                } else {
                    point.models.push(UsageTimeseriesModel {
                        model: row.model,
                        request_count: 1,
                        input_tokens: row.input_tokens,
                        output_tokens: row.output_tokens,
                        cache_read_tokens: row.cache_read_tokens,
                        cache_creation_tokens: row.cache_creation_tokens,
                        estimated_cost_micros: row_cost,
                    });
                }
            },
        )?;
        for point in buckets.values_mut() {
            point.models.sort_by(|left, right| {
                right
                    .input_tokens
                    .saturating_add(right.output_tokens)
                    .saturating_add(right.cache_read_tokens)
                    .saturating_add(right.cache_creation_tokens)
                    .cmp(
                        &left
                            .input_tokens
                            .saturating_add(left.output_tokens)
                            .saturating_add(left.cache_read_tokens)
                            .saturating_add(left.cache_creation_tokens),
                    )
                    .then_with(|| left.model.cmp(&right.model))
            });
        }
        Ok(buckets.into_values().collect())
    }

    /// Requests and total tokens recorded since `since` (unix seconds).
    pub fn recent_totals(&self, since: i64) -> Result<(u64, u64), ProxyError> {
        let conn = self.connection.lock().map_err(|_| ProxyError::Poisoned)?;
        conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(input_tokens + output_tokens + cache_read_tokens + cache_creation_tokens), 0) FROM proxy_usage WHERE started_at >= ?1",
            params![since],
            |row| Ok((row.get::<_, i64>(0)?.max(0) as u64, row.get::<_, i64>(1)?.max(0) as u64)),
        )
        .map_err(ProxyError::Sqlite)
    }

    fn rows_since(&self, since: Option<i64>) -> Result<Vec<(String, UsageRow)>, ProxyError> {
        let mut rows = Vec::new();
        self.visit_rows_since(since, |date, row| rows.push((date, row)))?;
        Ok(rows)
    }

    fn visit_rows_since(
        &self,
        since: Option<i64>,
        visit: impl FnMut(String, UsageRow),
    ) -> Result<(), ProxyError> {
        self.visit_rows_since_with_offset(since, 0, UsageGranularity::Day, visit)
    }

    fn visit_rows_since_with_offset(
        &self,
        since: Option<i64>,
        timezone_offset_seconds: i64,
        granularity: UsageGranularity,
        mut visit: impl FnMut(String, UsageRow),
    ) -> Result<(), ProxyError> {
        let conn = self.connection.lock().map_err(|_| ProxyError::Poisoned)?;
        let date_modifier = format!("{timezone_offset_seconds:+} seconds");
        let bucket = match granularity {
            UsageGranularity::Day => format!("date(started_at, 'unixepoch', '{date_modifier}')"),
            UsageGranularity::Hour => format!(
                "strftime('%Y-%m-%dT%H:%M:%SZ', (started_at + {timezone_offset_seconds}) / 3600 * 3600 - {timezone_offset_seconds}, 'unixepoch')"
            ),
        };
        let columns = format!("{bucket}, started_at, provider_entry_id, secret_id, model, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens, status, first_token_ms");
        let sql = if since.is_some() {
            format!("SELECT {columns} FROM proxy_usage WHERE started_at >= ?1 ORDER BY started_at")
        } else {
            format!("SELECT {columns} FROM proxy_usage ORDER BY started_at")
        };
        let mut statement = conn.prepare(&sql).map_err(ProxyError::Sqlite)?;
        let rows = match since {
            Some(since) => statement.query_map(params![since], decode_usage_row),
            None => statement.query_map([], decode_usage_row),
        }
        .map_err(ProxyError::Sqlite)?;
        for row in rows {
            let (date, row) = row.map_err(ProxyError::Sqlite)?;
            visit(date, row);
        }
        Ok(())
    }

    fn visit_attempt_rows(
        &self,
        since: Option<i64>,
        mut visit: impl FnMut(AttemptRow),
    ) -> Result<(), ProxyError> {
        let conn = self.connection.lock().map_err(|_| ProxyError::Poisoned)?;
        let mut statement = conn
            .prepare(
                "SELECT started_at, provider_entry_id, secret_id, model, success, first_token_ms FROM proxy_attempts WHERE (?1 IS NULL OR started_at >= ?1) ORDER BY started_at",
            )
            .map_err(ProxyError::Sqlite)?;
        let rows = statement
            .query_map(params![since], decode_attempt_row)
            .map_err(ProxyError::Sqlite)?;
        for row in rows {
            visit(row.map_err(ProxyError::Sqlite)?);
        }
        Ok(())
    }
}

/// Shared lower bound for usage charts and provider/model summaries.
pub fn usage_window_start(
    days: u32,
    timezone_offset_minutes: i32,
    granularity: UsageGranularity,
) -> i64 {
    let days = i64::from(days.max(1));
    let timezone_offset_minutes = timezone_offset_minutes.clamp(-1_440, 1_440);
    let timezone_offset_seconds = i64::from(timezone_offset_minutes) * 60;
    match granularity {
        UsageGranularity::Day => {
            local_day_start(now_unix(), timezone_offset_seconds) - (days - 1) * 86_400
        }
        // Include the current local hour and the preceding hours.
        UsageGranularity::Hour => {
            let hour_start = (now_unix() + timezone_offset_seconds).div_euclid(3_600) * 3_600
                - timezone_offset_seconds;
            hour_start - (days * 24 - 1) * 3_600
        }
    }
}

fn decode_usage_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(String, UsageRow)> {
    Ok((
        row.get::<_, String>(0)?,
        UsageRow {
            started_at: row.get(1)?,
            provider_entry_id: Uuid::parse_str(&row.get::<_, String>(2)?)
                .unwrap_or_else(|_| Uuid::nil()),
            secret_id: row.get(3)?,
            model: row.get(4)?,
            input_tokens: row.get::<_, i64>(5)?.max(0) as u64,
            output_tokens: row.get::<_, i64>(6)?.max(0) as u64,
            cache_read_tokens: row.get::<_, i64>(7)?.max(0) as u64,
            cache_creation_tokens: row.get::<_, i64>(8)?.max(0) as u64,
            status: row.get::<_, i64>(9)?.clamp(0, u16::MAX as i64) as u16,
            first_token_ms: row
                .get::<_, Option<i64>>(10)?
                .map(|value| value.max(0) as u64),
        },
    ))
}

fn decode_attempt_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AttemptRow> {
    Ok(AttemptRow {
        started_at: row.get(0)?,
        provider_entry_id: Uuid::parse_str(&row.get::<_, String>(1)?)
            .unwrap_or_else(|_| Uuid::nil()),
        secret_id: row.get(2)?,
        model: row.get(3)?,
        success: row.get::<_, Option<i64>>(4)?.map(|value| value != 0),
        first_token_ms: row
            .get::<_, Option<i64>>(5)?
            .map(|value| value.max(0) as u64),
    })
}

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedTarget {
    /// Provider-owned concurrency limit; missing/zero is unlimited.
    pub max_concurrent_requests: Option<u32>,
    /// Provider-owned capability, resolved from the vault on every refresh.
    pub supports_websockets: bool,
    pub config: ProxyTargetConfig,
    pub api_key: String,
}

impl Drop for ResolvedTarget {
    fn drop(&mut self) {
        self.api_key.zeroize();
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
struct ConfigWatch {
    receiver: tokio::sync::watch::Receiver<Arc<HashMap<Uuid, u64>>>,
    baseline: Arc<HashMap<Uuid, u64>>,
    route: Uuid,
}
impl ConfigWatch {
    fn subscribe(state: &RuntimeState) -> Self {
        let receiver = state.config_changed.subscribe();
        let baseline = receiver.borrow().clone();
        Self {
            receiver,
            baseline,
            route: Uuid::nil(),
        }
    }
    fn scope(&mut self, route: Uuid) {
        self.route = route;
    }
    fn has_changed(&self) -> Result<bool, tokio::sync::watch::error::RecvError> {
        self.receiver.has_changed()?;
        Ok(self
            .receiver
            .borrow()
            .get(&self.route)
            .copied()
            .unwrap_or(0)
            != self.baseline.get(&self.route).copied().unwrap_or(0))
    }
    async fn changed(&mut self) -> Result<(), tokio::sync::watch::error::RecvError> {
        loop {
            if self.has_changed()? {
                return Ok(());
            }
            self.receiver.changed().await?;
        }
    }
}

type UpstreamClientCache = HashMap<(u64, UpstreamProxyConfig, bool), reqwest::Client>;

#[derive(Clone)]
struct RuntimeState {
    config: Arc<RwLock<RuntimeConfig>>,
    stats: Arc<Mutex<RuntimeStats>>,
    usage: Arc<UsageStore>,
    health: Arc<Mutex<HashMap<Uuid, TargetHealth>>>,
    image_capabilities: Arc<Mutex<images::Ledger>>,
    ws_health: Arc<Mutex<HashMap<websocket::capability::Key, websocket::capability::Health>>>,
    ws_sessions: Arc<Mutex<websocket::capability::HttpSessions>>,
    rr_counters: Arc<Mutex<HashMap<Uuid, AtomicU64>>>,
    session_affinity: Arc<Mutex<HashMap<(Uuid, String), SessionAffinity>>>,
    clients: Arc<Mutex<UpstreamClientCache>>,
    config_changed: tokio::sync::watch::Sender<Arc<HashMap<Uuid, u64>>>,
    in_flight_requests: Arc<AtomicU64>,
    target_activity: Arc<Mutex<HashMap<Uuid, u64>>>,
    provider_activity: Arc<Mutex<HashMap<Uuid, u64>>>,
}

#[derive(Default)]
struct RuntimeStats {
    requests: u64,
    failures: u64,
    last_error: Option<String>,
    first_token_samples: VecDeque<Option<u64>>,
    recent_request_times: VecDeque<Instant>,
    recent_failure_times: VecDeque<Instant>,
    recent_token_totals: VecDeque<(Instant, u64)>,
}

struct InFlightGuard {
    counter: Arc<AtomicU64>,
}

impl InFlightGuard {
    fn new(counter: Arc<AtomicU64>) -> Self {
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
struct TargetActivityGuard {
    counts: Arc<Mutex<HashMap<Uuid, u64>>>,
    target_id: Uuid,
}

impl TargetActivityGuard {
    fn new(state: &RuntimeState, target_id: Uuid) -> Self {
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
struct TargetHealth {
    consecutive_failures: u8,
    consecutive_successes: u8,
    open_until: Option<Instant>,
    last_failure_at: Option<Instant>,
    recovering: bool,
    reopen_count: u8,
    probe_id: Option<Uuid>,
}

/// Require a short run of successful requests after a target failure so an
/// intermittent provider does not immediately flap back to healthy.
const RECOVERY_SUCCESS_THRESHOLD: u8 = 2;

#[derive(Clone, Debug)]
struct SessionAffinity {
    target_id: Uuid,
    last_used: Instant,
}

pub struct ProxyHandle {
    state: RuntimeState,
    stop: Option<oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
    bind_addr: String,
}

impl ProxyHandle {
    pub fn start(config: RuntimeConfig, usage: Arc<UsageStore>) -> Result<Self, ProxyError> {
        for route in &config.routes {
            for target in route.targets.iter().filter(|target| target.config.enabled) {
                let target_protocol = target
                    .config
                    .effective_protocol(route.config.upstream_protocol);
                let inbound = route.config.inbound_protocol;
                if inbound != target_protocol && !route.config.conversion_enabled {
                    return Err(ProxyError::InvalidConfig(format!(
                        "route {} target {} speaks {target_protocol:?} but the route accepts {inbound:?}; enable protocol conversion for this route",
                        route.config.name, target.config.label
                    )));
                }
                if !aipass_proxy_conversion::supports(inbound, target_protocol) {
                    return Err(ProxyError::InvalidConfig(format!(
                        "route {} target {}: protocol conversion {inbound:?} -> {target_protocol:?} is not supported",
                        route.config.name, target.config.label
                    )));
                }
            }
        }
        let bind_addr = config.bind_addr.clone();
        let socket: SocketAddr = bind_addr
            .parse()
            .map_err(|_| ProxyError::InvalidConfig("bind address must be host:port".into()))?;
        let ws_health = websocket::capability::initial_health(&config);
        let state = RuntimeState {
            config: Arc::new(RwLock::new(config)),
            stats: Arc::new(Mutex::new(RuntimeStats::default())),
            usage,
            health: Arc::new(Mutex::new(HashMap::new())),
            image_capabilities: Arc::new(Mutex::new(images::Ledger::default())),
            ws_health: Arc::new(Mutex::new(ws_health)),
            ws_sessions: Arc::new(Mutex::new(HashMap::new())),
            rr_counters: Arc::new(Mutex::new(HashMap::new())),
            session_affinity: Arc::new(Mutex::new(HashMap::new())),
            clients: Arc::new(Mutex::new(HashMap::new())),
            config_changed: tokio::sync::watch::channel(Arc::new(HashMap::new())).0,
            in_flight_requests: Arc::new(AtomicU64::new(0)),
            target_activity: Arc::new(Mutex::new(HashMap::new())),
            provider_activity: Arc::new(Mutex::new(HashMap::new())),
        };
        let thread_state = state.clone();
        let (stop_tx, stop_rx) = oneshot::channel();
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let thread = std::thread::Builder::new().name("aipass-proxy".into()).spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
                Ok(runtime) => runtime,
                Err(err) => { let _ = ready_tx.send(Err(err.to_string())); return; }
            };
            runtime.block_on(async move {
                let listener = match TcpListener::bind(socket).await {
                    Ok(listener) => { let _ = ready_tx.send(Ok(())); listener }
                    Err(err) => { set_error(&thread_state, err.to_string()); let _ = ready_tx.send(Err(err.to_string())); return; }
                };
                let mut stop_rx = stop_rx;
                loop {
                    tokio::select! {
                        _ = &mut stop_rx => break,
                        result = listener.accept() => match result {
                            Ok((stream, _)) => {
                                let state = thread_state.clone();
                                tokio::spawn(async move {
                                    let service = service_fn(move |request| handle_request(request, state.clone()));
                                    let io = TokioIo::new(stream);
                                    let _ = hyper::server::conn::http1::Builder::new().serve_connection(io, service).with_upgrades().await;
                                });
                            }
                            Err(err) => { set_error(&thread_state, err.to_string()); break; }
                        }
                    }
                }
            });
        }).map_err(ProxyError::Io)?;
        match ready_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                let _ = thread.join();
                return Err(ProxyError::InvalidConfig(format!(
                    "failed to start proxy listener: {err}"
                )));
            }
            Err(err) => {
                let _ = stop_tx.send(());
                let _ = thread.join();
                return Err(ProxyError::InvalidConfig(format!(
                    "proxy listener startup timed out: {err}"
                )));
            }
        }
        state
            .usage
            .log_diagnostic("info", "event=proxy.started".into());
        Ok(Self {
            state,
            stop: Some(stop_tx),
            thread: Some(thread),
            bind_addr,
        })
    }

    pub fn status(&self) -> ProxyStatus {
        let config = self
            .state
            .config
            .read()
            .map(|config| {
                let now = Instant::now();
                let health = self.state.health.lock().ok();
                let activity = self.state.target_activity.lock().ok();
                let channels = config
                    .routes
                    .iter()
                    .filter(|route| route.config.enabled)
                    .flat_map(|route| {
                        route
                            .targets
                            .iter()
                            .filter(|target| target.config.enabled)
                            .map(|target| {
                                let health = health
                                    .as_ref()
                                    .and_then(|health| health.get(&target.config.id));
                                let cooldown = health
                                    .and_then(|health| health.open_until)
                                    .map(|until| until.saturating_duration_since(now))
                                    .unwrap_or_default();
                                ChannelStatus {
                                    route_id: route.config.id,
                                    target_id: target.config.id,
                                    provider_entry_id: target.config.provider_entry_id,
                                    secret_id: target.config.secret_id.clone(),
                                    in_flight_requests: activity
                                        .as_ref()
                                        .and_then(|activity| activity.get(&target.config.id))
                                        .copied()
                                        .unwrap_or_default(),
                                    degraded: health.is_some_and(TargetHealth::degraded),
                                    available: cooldown.is_zero(),
                                    cooldown_remaining_ms: cooldown.as_millis() as u64,
                                    websocket_cooling_down: false,
                                }
                            })
                    })
                    .collect::<Vec<_>>();
                (
                    config.enabled,
                    config
                        .routes
                        .iter()
                        .filter(|route| route.config.enabled)
                        .count(),
                    channels,
                )
            })
            .unwrap_or_default();
        let stats = self.state.stats.lock();
        let (
            requests,
            failures,
            last_error,
            average_first_token_ms,
            recent_requests,
            recent_failures,
            recent_tokens,
        ) = stats
            .as_ref()
            .map(|stats| {
                let cutoff = Instant::now() - Duration::from_secs(60);
                let mut recent_request_times = stats.recent_request_times.clone();
                while recent_request_times
                    .front()
                    .is_some_and(|time| *time < cutoff)
                {
                    recent_request_times.pop_front();
                }
                let mut recent_token_totals = stats.recent_token_totals.clone();
                while recent_token_totals
                    .front()
                    .is_some_and(|(time, _)| *time < cutoff)
                {
                    recent_token_totals.pop_front();
                }
                let mut recent_failure_times = stats.recent_failure_times.clone();
                while recent_failure_times
                    .front()
                    .is_some_and(|time| *time < cutoff)
                {
                    recent_failure_times.pop_front();
                }
                let (total, count) = stats
                    .first_token_samples
                    .iter()
                    .flatten()
                    .fold((0_u64, 0_u64), |(total, count), value| {
                        (total.saturating_add(*value), count.saturating_add(1))
                    });
                (
                    stats.requests,
                    stats.failures,
                    stats.last_error.clone(),
                    (count > 0).then(|| total / count),
                    recent_request_times.len() as u64,
                    recent_failure_times.len() as u64,
                    recent_token_totals.iter().map(|(_, tokens)| *tokens).sum(),
                )
            })
            .unwrap_or_default();
        let success_rate_bps = if requests == 0 {
            0
        } else {
            u16::try_from(
                requests
                    .saturating_sub(failures)
                    .saturating_mul(10_000)
                    .saturating_add(requests / 2)
                    .checked_div(requests)
                    .unwrap_or_default(),
            )
            .unwrap_or(10_000)
        };
        let running = self
            .thread
            .as_ref()
            .is_some_and(|thread| !thread.is_finished());
        let mut channels = config.2;
        if !running {
            for channel in &mut channels {
                channel.in_flight_requests = 0;
                channel.available = false;
                channel.degraded = false;
                channel.cooldown_remaining_ms = 0;
                channel.websocket_cooling_down = false;
            }
        }
        let degraded_target_ids = channels
            .iter()
            .filter(|channel| channel.degraded)
            .map(|channel| channel.target_id)
            .collect::<Vec<_>>();
        let total_channels = channels.len();
        let available_channels = channels.iter().filter(|channel| channel.available).count();
        let degraded = running
            && (!degraded_target_ids.is_empty()
                || (recent_requests > 0
                    && recent_failures.saturating_mul(100) >= recent_requests.saturating_mul(20)));
        ProxyStatus {
            running,
            enabled: config.0,
            bind_addr: self.bind_addr.clone(),
            active_routes: config.1,
            requests,
            failures,
            last_error,
            degraded,
            degraded_target_ids,
            recent_requests,
            recent_tokens,
            success_rate_bps,
            average_first_token_ms,
            in_flight_requests: self.state.in_flight_requests.load(Ordering::Relaxed),
            available_channels,
            total_channels,
            channels,
        }
    }

    pub fn update_config(&self, config: RuntimeConfig) -> Result<(), ProxyError> {
        let mut current = self
            .state
            .config
            .write()
            .map_err(|_| ProxyError::Poisoned)?;
        if *current == config {
            // Syncing unrelated vault records must not invalidate live HTTP,
            // SSE, WS sessions, pooled connections, or routing health.
            return Ok(());
        }
        let mut health = self.state.health.lock().map_err(|_| ProxyError::Poisoned)?;
        let mut ws_health = self
            .state
            .ws_health
            .lock()
            .map_err(|_| ProxyError::Poisoned)?;
        let mut rr_counters = self
            .state
            .rr_counters
            .lock()
            .map_err(|_| ProxyError::Poisoned)?;
        let mut session_affinity = self
            .state
            .session_affinity
            .lock()
            .map_err(|_| ProxyError::Poisoned)?;
        let preserved = routing::preserved_targets(&current, &config);
        health.retain(|id, _| preserved.iter().any(|(_, target)| target == id));
        session_affinity
            .retain(|(route, _), affinity| preserved.contains(&(*route, affinity.target_id)));
        // Transport sessions still reconnect and authenticate against the new
        // snapshot, while unrelated metadata edits retain generation stability.
        let valid_ws_keys: HashSet<_> = config
            .routes
            .iter()
            .flat_map(|route| &route.targets)
            .map(|target| websocket_config_key(target, &config.upstream_proxy))
            .collect();
        ws_health.retain(|key, _| valid_ws_keys.contains(key));
        for target in config
            .routes
            .iter()
            .flat_map(|route| &route.targets)
            .filter(|target| target.supports_websockets)
        {
            if current
                .routes
                .iter()
                .flat_map(|route| &route.targets)
                .any(|old| {
                    old.config.provider_entry_id == target.config.provider_entry_id
                        && !old.supports_websockets
                })
            {
                ws_health.remove(&websocket_config_key(target, &config.upstream_proxy));
            }
        }
        for key in &valid_ws_keys {
            ws_health.entry(*key).or_default();
        }
        if let Ok(mut sessions) = self.state.ws_sessions.lock() {
            sessions.retain(|(_, _, key), _| valid_ws_keys.contains(key));
        }
        rr_counters.clear();
        let mut generations = (**self.state.config_changed.borrow()).clone();
        *generations.entry(Uuid::nil()).or_default() += 1;
        for route in &current.routes {
            let revoked = current.enabled != config.enabled
                || current.bind_addr != config.bind_addr
                || route
                    .targets
                    .iter()
                    .any(|target| !preserved.contains(&(route.config.id, target.config.id)));
            if revoked {
                *generations.entry(route.config.id).or_default() += 1;
            }
        }
        *current = config;
        self.state
            .config_changed
            .send_replace(Arc::new(generations));
        self.state
            .usage
            .log_diagnostic("info", "event=proxy.config.reloaded".into());
        Ok(())
    }

    pub fn logs(&self) -> Result<Vec<ProxyLogEntry>, ProxyError> {
        self.state.usage.logs()
    }

    pub fn usage_count(&self) -> Result<u64, ProxyError> {
        self.state.usage.count()
    }
}

impl Drop for ProxyHandle {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        self.state
            .usage
            .log_diagnostic("info", "event=proxy.stopped".into());
    }
}

fn tokens_match(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.as_bytes()
        .iter()
        .zip(right.as_bytes())
        .fold(0_u8, |diff, (left, right)| diff | (left ^ right))
        == 0
}

fn upstream_client(
    state: &RuntimeState,
    connect_timeout_ms: u64,
) -> Result<reqwest::Client, String> {
    upstream_client_for_transport(state, connect_timeout_ms, false)
}

fn upstream_client_for_transport(
    state: &RuntimeState,
    connect_timeout_ms: u64,
    http1_only: bool,
) -> Result<reqwest::Client, String> {
    let connect_timeout_ms = connect_timeout_ms.max(1);
    let upstream_proxy = state
        .config
        .read()
        .map_err(|_| "proxy config lock poisoned".to_string())?
        .upstream_proxy
        .clone();
    let cache_key = (connect_timeout_ms, upstream_proxy.clone(), http1_only);
    let mut clients = state
        .clients
        .lock()
        .map_err(|_| "proxy HTTP client cache lock poisoned".to_string())?;
    if let Some(client) = clients.get(&cache_key) {
        return Ok(client.clone());
    }
    let builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_millis(connect_timeout_ms))
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never());
    let builder = if http1_only {
        builder.http1_only()
    } else {
        builder
    };
    let client = apply_upstream_proxy(builder, &upstream_proxy)?
        .build()
        .map_err(|err| err.to_string())?;
    clients.insert(cache_key, client.clone());
    Ok(client)
}

/// Resolve outbound proxy selection once for async forwarding and blocking probes.
pub fn upstream_proxy_rules(
    config: &UpstreamProxyConfig,
) -> Result<Option<Vec<reqwest::Proxy>>, String> {
    match config.mode {
        UpstreamProxyMode::System => Ok(None),
        UpstreamProxyMode::Direct => Ok(Some(Vec::new())),
        UpstreamProxyMode::Custom => {
            let url = config
                .custom_url
                .as_deref()
                .map(str::trim)
                .filter(|url| !url.is_empty())
                .ok_or_else(|| {
                    "upstream proxy mode is custom but no proxy URL is configured".to_string()
                })?;
            let proxy = reqwest::Proxy::all(url)
                .map_err(|err| format!("invalid upstream proxy URL: {err}"))?;
            Ok(Some(vec![proxy]))
        }
        UpstreamProxyMode::Environment => {
            let vars = shell_env::proxy_env();
            let no_proxy = shell_env::lookup(&vars, &["NO_PROXY", "no_proxy"])
                .map(reqwest::NoProxy::from_string);
            let mut proxies = Vec::new();
            type ProxyCtor = fn(&str) -> reqwest::Result<reqwest::Proxy>;
            let constructors: [(&[&str], ProxyCtor); 3] = [
                (&["HTTPS_PROXY", "https_proxy"][..], |url| {
                    reqwest::Proxy::https(url)
                }),
                (&["HTTP_PROXY", "http_proxy"][..], |url| {
                    reqwest::Proxy::http(url)
                }),
                (&["ALL_PROXY", "all_proxy"][..], |url| {
                    reqwest::Proxy::all(url)
                }),
            ];
            for (keys, ctor) in constructors {
                let Some(url) = shell_env::lookup(&vars, keys) else {
                    continue;
                };
                if let Ok(proxy) = ctor(url) {
                    proxies.push(match no_proxy.clone() {
                        Some(no_proxy) => proxy.no_proxy(no_proxy),
                        None => proxy,
                    });
                }
            }
            Ok(Some(proxies))
        }
    }
}

type BoxError = Box<dyn StdError + Send + Sync>;
type BoxBody = http_body_util::combinators::UnsyncBoxBody<Bytes, BoxError>;
type UpstreamBodyStream =
    Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static>>;

const MAX_REQUEST_BODY_BYTES: usize = 512 * 1024 * 1024;
const REQUEST_BODY_MEMORY_THRESHOLD: usize = 8 * 1024 * 1024;
const MAX_BUFFERED_RESPONSE_BYTES: usize = 64 * 1024 * 1024;
const MAX_PROXY_LOG_ENTRIES: usize = 1_000;
/// Session affinity is intentionally ephemeral. Provider prompt caches are
/// useful while a client session is active, but retaining arbitrary client
/// supplied keys indefinitely would make the proxy's memory usage unbounded.
const SESSION_AFFINITY_TTL: Duration = Duration::from_secs(30 * 60);
const MAX_SESSION_AFFINITY_ENTRIES: usize = 4_096;
const MAX_SESSION_AFFINITY_KEY_BYTES: usize = 512;

const SESSION_AFFINITY_HEADERS: [&str; 11] = [
    "x-aipass-session-id",
    "x-aipass-session",
    "x-session-id",
    "x-client-session-id",
    "x-codex-session-id",
    "x-openai-session-id",
    "x-anthropic-session-id",
    "x-claude-session-id",
    "session-id",
    "session_id",
    "prompt-cache-key",
];

const SESSION_AFFINITY_FIELDS: [&str; 10] = [
    "prompt_cache_key",
    "promptCacheKey",
    "session_id",
    "sessionId",
    "session",
    "sessionKey",
    "conversation_id",
    "conversationId",
    "previous_response_id",
    "previousResponseId",
];

enum ReplayableRequestBody {
    Memory(Bytes),
    File { file: std::fs::File, len: u64 },
}

#[derive(Default, Deserialize)]
struct RequestMetadata {
    #[serde(default)]
    stream: bool,
    model: Option<String>,
    /// A stable client supplied key lets providers reuse their prompt cache
    /// for a conversation. `prompt_cache_key` is the OpenAI API spelling;
    /// the other fields cover clients that expose the same value as a
    /// session or conversation identifier.
    #[serde(alias = "promptCacheKey")]
    prompt_cache_key: Option<String>,
    #[serde(alias = "sessionId")]
    session_id: Option<String>,
    #[serde(alias = "sessionKey")]
    session: Option<String>,
    #[serde(alias = "conversationId")]
    conversation_id: Option<String>,
    #[serde(alias = "previousResponseId")]
    previous_response_id: Option<String>,
    /// Responses clients may carry a stable conversation identifier as a
    /// string or as `{ "id": "..." }`.
    conversation: Option<serde_json::Value>,
}

impl ReplayableRequestBody {
    fn bytes(&self) -> Option<&Bytes> {
        match self {
            Self::Memory(bytes) => Some(bytes),
            Self::File { .. } => None,
        }
    }

    async fn json(&self) -> Option<serde_json::Value> {
        match self {
            Self::Memory(bytes) => serde_json::from_slice(bytes).ok(),
            Self::File { file, .. } => {
                let mut file = file.try_clone().ok()?;
                tokio::task::spawn_blocking(move || {
                    use std::io::{BufReader, Seek, SeekFrom};

                    file.seek(SeekFrom::Start(0)).ok()?;
                    serde_json::from_reader(BufReader::new(file)).ok()
                })
                .await
                .ok()
                .flatten()
            }
        }
    }

    fn len(&self) -> u64 {
        match self {
            Self::Memory(bytes) => bytes.len() as u64,
            Self::File { len, .. } => *len,
        }
    }

    async fn metadata(&self) -> Option<RequestMetadata> {
        self.parse_metadata().await
    }

    async fn parse_metadata<T: serde::de::DeserializeOwned + Send + 'static>(&self) -> Option<T> {
        match self {
            Self::Memory(bytes) => serde_json::from_slice(bytes).ok(),
            Self::File { file, .. } => {
                let mut file = file.try_clone().ok()?;
                tokio::task::spawn_blocking(move || {
                    use std::io::{BufReader, Seek, SeekFrom};

                    file.seek(SeekFrom::Start(0)).ok()?;
                    serde_json::from_reader(BufReader::new(file)).ok()
                })
                .await
                .ok()
                .flatten()
            }
        }
    }

    async fn request_body(&self) -> Result<reqwest::Body, std::io::Error> {
        match self {
            Self::Memory(bytes) => Ok(reqwest::Body::from(bytes.clone())),
            Self::File { file, .. } => {
                use std::io::{Seek, SeekFrom};

                let mut file = file.try_clone()?;
                file.seek(SeekFrom::Start(0))?;
                let file = tokio::fs::File::from_std(file);
                Ok(reqwest::Body::from(file))
            }
        }
    }
}

enum RequestBodyReadError {
    TooLarge,
    Io(std::io::Error),
    Transport(String),
}

impl std::fmt::Debug for RequestBodyReadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge => formatter.write_str("request body too large"),
            Self::Io(err) => formatter.debug_tuple("Io").field(err).finish(),
            Self::Transport(err) => formatter.debug_tuple("Transport").field(err).finish(),
        }
    }
}

async fn read_replayable_request_body(
    body: Incoming,
) -> Result<ReplayableRequestBody, RequestBodyReadError> {
    read_replayable_request_chunks(
        body.into_data_stream(),
        MAX_REQUEST_BODY_BYTES,
        REQUEST_BODY_MEMORY_THRESHOLD,
    )
    .await
}

async fn read_replayable_request_chunks<S, E>(
    stream: S,
    max_bytes: usize,
    memory_threshold: usize,
) -> Result<ReplayableRequestBody, RequestBodyReadError>
where
    S: Stream<Item = Result<Bytes, E>> + Send,
    E: std::fmt::Display,
{
    let mut stream = Box::pin(stream);
    let mut memory = Vec::new();
    let mut file: Option<tokio::fs::File> = None;
    let mut total = 0_usize;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|err| RequestBodyReadError::Transport(err.to_string()))?;
        total = total
            .checked_add(chunk.len())
            .ok_or(RequestBodyReadError::TooLarge)?;
        if total > max_bytes {
            return Err(RequestBodyReadError::TooLarge);
        }

        if let Some(file) = file.as_mut() {
            file.write_all(&chunk)
                .await
                .map_err(RequestBodyReadError::Io)?;
            continue;
        }
        if total <= memory_threshold {
            memory.extend_from_slice(&chunk);
            continue;
        }

        let file_handle = tempfile::tempfile().map_err(RequestBodyReadError::Io)?;
        let mut async_file = tokio::fs::File::from_std(file_handle);
        async_file
            .write_all(&memory)
            .await
            .map_err(RequestBodyReadError::Io)?;
        async_file
            .write_all(&chunk)
            .await
            .map_err(RequestBodyReadError::Io)?;
        memory.clear();
        file = Some(async_file);
    }

    match file {
        Some(mut file) => {
            file.flush().await.map_err(RequestBodyReadError::Io)?;
            let file = file.into_std().await;
            Ok(ReplayableRequestBody::File {
                file,
                len: total as u64,
            })
        }
        None => Ok(ReplayableRequestBody::Memory(Bytes::from(memory))),
    }
}

fn apply_upstream_proxy(
    mut builder: reqwest::ClientBuilder,
    config: &UpstreamProxyConfig,
) -> Result<reqwest::ClientBuilder, String> {
    if let Some(proxies) = upstream_proxy_rules(config)? {
        builder = builder.no_proxy();
        for proxy in proxies {
            builder = builder.proxy(proxy);
        }
    }
    Ok(builder)
}

fn select_route(
    state: &RuntimeState,
    bearer_token: Option<&str>,
    api_key_token: Option<&str>,
    inbound: Option<ProxyProtocol>,
) -> Option<(ResolvedRoute, Vec<ModelPricing>)> {
    state.config.read().ok().and_then(|config| {
        config.enabled.then(|| {
            config
                .routes
                .iter()
                .find(|route| {
                    route.config.enabled
                        && inbound.is_none_or(|protocol| {
                            route_accepts_inbound_protocol(&route.config, protocol)
                        })
                        && (bearer_token
                            .is_some_and(|token| tokens_match(&route.local_token, token))
                            || api_key_token
                                .is_some_and(|token| tokens_match(&route.local_token, token)))
                })
                .cloned()
                .map(|route| (route, config.pricing.clone()))
        })?
    })
}

/// A local token is scoped to one route's inbound wire protocol. Keeping this
/// check explicit prevents a token configured for Codex/Responses from being
/// accepted on another OpenAI-compatible endpoint such as Chat Completions.
fn route_accepts_inbound_protocol(route: &ProxyRouteConfig, protocol: ProxyProtocol) -> bool {
    route.inbound_protocol == protocol
}

fn silent_retry_rounds(policy: &RetryPolicy) -> u8 {
    if policy.silent_retry {
        policy.max_silent_retries.saturating_add(1).max(1)
    } else {
        1
    }
}

fn hold_backoff_delay(policy: &RetryPolicy, hold_round: u32) -> Duration {
    let mut delay_ms = policy.hold_initial_delay_ms.max(1);
    for _ in 0..hold_round {
        if delay_ms >= policy.hold_max_delay_ms {
            break;
        }
        delay_ms = delay_ms.saturating_mul(2);
    }
    Duration::from_millis(delay_ms.min(policy.hold_max_delay_ms.max(1)))
}

// The hold budget applies until the response is committed. A live stream
// uses its idle timeout after commit and must never be replayed at this deadline.
fn hold_deadline(policy: &RetryPolicy, started: Instant) -> Option<tokio::time::Instant> {
    (policy.hold_on_failure && policy.hold_max_duration_ms > 0)
        .then(|| started.checked_add(Duration::from_millis(policy.hold_max_duration_ms)))
        .flatten()
        .map(tokio::time::Instant::from_std)
}

fn bounded_deadline(
    timeout: Duration,
    hold_deadline: Option<tokio::time::Instant>,
) -> tokio::time::Instant {
    let deadline = tokio::time::Instant::now() + timeout;
    hold_deadline.map_or(deadline, |hold| deadline.min(hold))
}

fn normalize_session_affinity_key(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.len() <= MAX_SESSION_AFFINITY_KEY_BYTES).then(|| value.to_owned())
}

/// Resolve an affinity key without forwarding a proxy-only header as a
/// provider credential. The body key is used as a fallback so clients can
/// opt into affinity through the standard `prompt_cache_key` request field.
fn session_affinity_key(headers: &HeaderMap, metadata: Option<&RequestMetadata>) -> Option<String> {
    for name in SESSION_AFFINITY_HEADERS {
        if let Some(value) = headers.get(name).and_then(|value| value.to_str().ok()) {
            if let Some(value) = normalize_session_affinity_key(value) {
                return Some(value);
            }
        }
    }
    metadata
        .and_then(|metadata| {
            metadata
                .prompt_cache_key
                .as_deref()
                .or(metadata.session_id.as_deref())
                .or(metadata.session.as_deref())
                .or(metadata.conversation_id.as_deref())
                .or(metadata.previous_response_id.as_deref())
        })
        .and_then(normalize_session_affinity_key)
        .or_else(|| {
            metadata
                .and_then(|metadata| metadata.conversation.as_ref())
                .and_then(|conversation| {
                    conversation
                        .as_str()
                        .or_else(|| conversation.get("id").and_then(serde_json::Value::as_str))
                })
                .and_then(normalize_session_affinity_key)
        })
}

fn session_affinity_key_from_value(value: &serde_json::Value) -> Option<String> {
    let object = value.as_object()?;
    SESSION_AFFINITY_FIELDS
        .iter()
        .find_map(|field| object.get(*field).and_then(serde_json::Value::as_str))
        .and_then(normalize_session_affinity_key)
        .or_else(|| {
            object
                .get("conversation")
                .and_then(|conversation| {
                    conversation
                        .as_str()
                        .or_else(|| conversation.get("id").and_then(serde_json::Value::as_str))
                })
                .and_then(normalize_session_affinity_key)
        })
}

fn affinity_target(
    state: &RuntimeState,
    route_id: Uuid,
    session_key: Option<&str>,
    targets: &[ResolvedTarget],
) -> Option<Uuid> {
    let session_key = session_key.and_then(normalize_session_affinity_key)?;
    let key = (route_id, session_key);
    let now = Instant::now();
    let target_id = {
        let mut affinities = state.session_affinity.lock().ok()?;
        affinities.retain(|_, affinity| {
            now.saturating_duration_since(affinity.last_used) < SESSION_AFFINITY_TTL
        });
        affinities.get(&key)?.target_id
    };
    if !targets
        .iter()
        .any(|target| target.config.enabled && target.config.id == target_id)
        || circuit_open(state, target_id)
    {
        if let Ok(mut affinities) = state.session_affinity.lock() {
            affinities.remove(&key);
        }
        return None;
    }
    if let Ok(mut affinities) = state.session_affinity.lock() {
        if let Some(affinity) = affinities.get_mut(&key) {
            affinity.last_used = now;
        }
    }
    Some(target_id)
}

fn remember_affinity_target(
    state: &RuntimeState,
    route_id: Uuid,
    session_key: Option<&str>,
    target_id: Uuid,
) {
    let Some(session_key) = session_key.and_then(normalize_session_affinity_key) else {
        return;
    };
    let Ok(mut affinities) = state.session_affinity.lock() else {
        return;
    };
    let now = Instant::now();
    affinities.retain(|_, affinity| {
        now.saturating_duration_since(affinity.last_used) < SESSION_AFFINITY_TTL
    });
    // An older request finishing on a previous provider must not steal a
    // session that has already completed fallback successfully.
    let affinity = affinities
        .entry((route_id, session_key))
        .or_insert(SessionAffinity {
            target_id,
            last_used: now,
        });
    if affinity.target_id == target_id {
        affinity.last_used = now;
    }
    while affinities.len() > MAX_SESSION_AFFINITY_ENTRIES {
        let Some(oldest_key) = affinities
            .iter()
            .min_by_key(|(_, affinity)| affinity.last_used)
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        affinities.remove(&oldest_key);
    }
}

fn clear_affinity_for_target(state: &RuntimeState, target_id: Uuid) {
    if let Ok(mut affinities) = state.session_affinity.lock() {
        affinities.retain(|_, affinity| affinity.target_id != target_id);
    }
}

fn clear_rejected_session(
    state: &RuntimeState,
    route_id: Uuid,
    session_key: Option<&str>,
    target_id: Uuid,
) {
    if let Some(key) = session_key {
        if let Ok(mut affinities) = state.session_affinity.lock() {
            affinities.retain(|(route, session), affinity| {
                *route != route_id || session != key || affinity.target_id != target_id
            });
        }
    }
}

#[cfg(test)]
fn select_route_targets(state: &RuntimeState, route: &ResolvedRoute) -> Vec<ResolvedTarget> {
    select_route_targets_with_affinity(state, route, None)
}

fn select_route_targets_with_affinity(
    state: &RuntimeState,
    route: &ResolvedRoute,
    session_key: Option<&str>,
) -> Vec<ResolvedTarget> {
    let mut targets = ordered_route_targets(state, route, session_key);
    targets.truncate(usize::from(route.config.retry.max_attempts.max(1)));
    targets
}

fn ordered_route_targets(
    state: &RuntimeState,
    route: &ResolvedRoute,
    session_key: Option<&str>,
) -> Vec<ResolvedTarget> {
    let mut targets = route.targets.clone();
    targets.retain(|target| target.config.enabled);
    targets.sort_by_key(|target| target.config.priority);
    targets.retain(|target| !circuit_open(state, target.config.id));
    // Weight only eligible peers in the best stability tier, so an unavailable
    // or degraded high-weight provider cannot donate traffic to another peer.
    let ranks = state
        .health
        .lock()
        .map(|health| {
            let now = Instant::now();
            targets.retain(|target| {
                !health
                    .get(&target.config.id)
                    .is_some_and(|h| h.probe_id.is_some())
            });
            targets
                .iter()
                .map(|target| {
                    (
                        target.config.id,
                        health
                            .get(&target.config.id)
                            .map_or(0, |h| h.stability_rank(now)),
                    )
                })
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();
    targets.sort_by_key(|target| ranks.get(&target.config.id).copied().unwrap_or_default());
    if route.config.strategy == RouteStrategy::RoundRobin && !targets.is_empty() {
        let best_rank = ranks.get(&targets[0].config.id);
        let peer_count = targets
            .iter()
            .take_while(|target| ranks.get(&target.config.id) == best_rank)
            .count();
        let start = round_robin_start(
            state,
            route.config.id,
            &targets[..peer_count]
                .iter()
                .map(|target| target.config.weight)
                .collect::<Vec<_>>(),
        );
        targets[..peer_count].rotate_left(start);
    }
    if let Some(target_id) = affinity_target(state, route.config.id, session_key, &route.targets) {
        if let Some(index) = targets
            .iter()
            .position(|target| target.config.id == target_id)
        {
            let preferred = targets.remove(index);
            targets.insert(0, preferred);
        }
    }
    targets
}

async fn handle_models_request(
    request: Request<Incoming>,
    state: RuntimeState,
) -> Response<BoxBody> {
    if request.method() != http::Method::GET {
        return error_response(
            StatusCode::METHOD_NOT_ALLOWED,
            "model discovery requires GET",
        );
    }
    let request_id = request
        .extensions()
        .get::<Uuid>()
        .copied()
        .unwrap_or_else(Uuid::new_v4);
    let incoming_headers = request.headers().clone();
    let request_query = request.uri().query().map(str::to_owned);
    let session_key = session_affinity_key(&incoming_headers, None);
    let (bearer_token, api_key_token) = local_proxy_tokens(&incoming_headers);
    if bearer_token.is_none() && api_key_token.is_none() {
        return error_response(StatusCode::UNAUTHORIZED, "missing local proxy token");
    }
    let Some((mut route, _)) = select_route(&state, bearer_token, api_key_token, None) else {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "invalid local proxy token or route",
        );
    };
    route.local_token.zeroize();

    let mut last_error = None;
    let mut saw_not_found = false;
    let mut saw_other_failure = false;
    let mut capacity_skipped = false;
    for _round in 0..silent_retry_rounds(&route.config.retry) {
        let targets = ordered_route_targets(&state, &route, session_key.as_deref());
        let mut attempts = 0;
        for target in targets {
            if attempts >= route.config.retry.max_attempts.max(1) {
                break;
            }
            let Some(_recovery) = RecoveryPermit::acquire(&state, target.config.id) else {
                continue;
            };
            let Some(_permit) = ProviderPermit::acquire(&state, &target) else {
                capacity_skipped = true;
                continue;
            };
            attempts += 1;
            let client = match upstream_client(&state, route.config.retry.connect_timeout_ms) {
                Ok(client) => client,
                Err(err) => {
                    last_error = Some(err);
                    saw_other_failure = true;
                    continue;
                }
            };
            let upstream_path = if target.config.auth_scheme == "azure_api_key" {
                "/models"
            } else {
                "/v1/models"
            };
            let url = match upstream_url_with_query(
                &target.config.base_url,
                upstream_path,
                request_query.as_deref(),
            ) {
                Ok(url) => url,
                Err(err) => {
                    last_error = Some(err.to_string());
                    saw_other_failure = true;
                    mark_failure(&state, target.config.id, &route.config.retry);
                    continue;
                }
            };
            let headers = match build_upstream_headers(
                &incoming_headers,
                &target,
                target
                    .config
                    .effective_protocol(route.config.upstream_protocol),
            ) {
                Ok(headers) => headers,
                Err(err) => {
                    last_error = Some(err);
                    saw_other_failure = true;
                    mark_failure(&state, target.config.id, &route.config.retry);
                    continue;
                }
            };
            let response = match client.get(url).headers(headers).send().await {
                Ok(response) => response,
                Err(err) => {
                    last_error = Some(err.to_string());
                    saw_other_failure = true;
                    mark_failure(&state, target.config.id, &route.config.retry);
                    continue;
                }
            };
            let status = response.status();
            if !status.is_success() {
                let detail = diagnostics::upstream::read_error(
                    response,
                    &target,
                    &local_token_redactions(&incoming_headers),
                )
                .await;
                state.usage.log_upstream_error(
                    request_id,
                    route.config.id,
                    &target.config,
                    status,
                    "http_models",
                    &detail,
                );
                last_error = Some(format!("upstream returned {status}"));
                if status == StatusCode::NOT_FOUND {
                    saw_not_found = true;
                } else {
                    saw_other_failure = true;
                }
                if status_affects_circuit(status) {
                    mark_failure(&state, target.config.id, &route.config.retry);
                }
                continue;
            }
            let response_headers = response.headers().clone();
            let mut source: UpstreamBodyStream = Box::pin(response.bytes_stream());
            let payload = match collect_upstream_body(None, &mut source).await {
                Ok(payload) => payload,
                Err(err) => {
                    last_error = Some(err);
                    saw_other_failure = true;
                    mark_failure(&state, target.config.id, &route.config.retry);
                    continue;
                }
            };
            if payload.is_empty() || is_upstream_error_payload(&payload) {
                last_error = Some("upstream returned an empty or error model list".into());
                saw_other_failure = true;
                mark_failure(&state, target.config.id, &route.config.retry);
                continue;
            }
            let payload = enrich_models_payload(payload, route.config.inbound_protocol);
            let body = BodyExt::boxed_unsync(
                Full::new(payload).map_err(|never| -> BoxError { match never {} }),
            );
            let mut builder = Response::builder().status(status);
            let response_hop_headers = connection_header_names(&response_headers);
            for (name, value) in response_headers.iter() {
                if !is_hop_header(name)
                    && !response_hop_headers.contains(name)
                    && name != header::CONTENT_LENGTH
                    && name != header::CONTENT_ENCODING
                {
                    builder = builder.header(name, value);
                }
            }
            return builder.body(body).unwrap_or_else(|_| {
                error_response(
                    StatusCode::BAD_GATEWAY,
                    "failed to build model list response",
                )
            });
        }
    }

    if capacity_skipped && !saw_not_found && !saw_other_failure {
        return capacity_response();
    }
    // Model discovery is optional and several upstreams (notably Anthropic)
    // legitimately do not expose a /v1/models endpoint. Keep that client
    // response visible without treating it as a proxy health failure.
    if saw_not_found && !saw_other_failure {
        return error_response(
            StatusCode::NOT_FOUND,
            "upstream model discovery endpoint not found",
        );
    }

    set_error(
        &state,
        last_error.unwrap_or_else(|| "all model discovery targets failed".into()),
    );
    error_response(
        StatusCode::BAD_GATEWAY,
        "all model discovery targets failed",
    )
}

fn enrich_models_payload(payload: Bytes, protocol: ProxyProtocol) -> Bytes {
    let Ok(mut root) = serde_json::from_slice::<serde_json::Value>(&payload) else {
        return payload;
    };
    let Some(models) = root
        .get_mut("data")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return payload;
    };
    let api_type = match protocol {
        ProxyProtocol::OpenAiResponses => "responses",
        ProxyProtocol::OpenAiChatCompletions => "chat_completions",
        ProxyProtocol::AnthropicMessages => "anthropic_messages",
    };
    for model in models {
        let Some(model) = model.as_object_mut() else {
            continue;
        };
        model
            .entry("api_types")
            .or_insert_with(|| serde_json::json!([api_type]));
        model.entry("capabilities").or_insert_with(|| {
            serde_json::json!({
                "output_modalities": ["text"],
                "supports_tool_use": true
            })
        });
    }
    serde_json::to_vec(&root).map_or(payload, Bytes::from)
}

async fn handle_local_health_request(
    request: Request<Incoming>,
    state: &RuntimeState,
) -> Response<BoxBody> {
    if !matches!(request.method(), &http::Method::GET | &http::Method::HEAD) {
        return error_response(StatusCode::METHOD_NOT_ALLOWED, "health check requires GET");
    }
    let (enabled, active_routes) = state
        .config
        .read()
        .map(|config| {
            (
                config.enabled,
                config
                    .routes
                    .iter()
                    .filter(|route| route.config.enabled)
                    .count(),
            )
        })
        .unwrap_or((false, 0));
    let (requests, failures) = state
        .stats
        .lock()
        .map(|stats| (stats.requests, stats.failures))
        .unwrap_or((0, 0));
    let body = serde_json::json!({
        "status": "ok",
        "service": "aipass-proxy",
        "enabled": enabled,
        "activeRoutes": active_routes,
        "requests": requests,
        "failures": failures,
    });
    let body = if request.method() == http::Method::HEAD {
        Bytes::new()
    } else {
        Bytes::from(body.to_string())
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(BodyExt::boxed_unsync(
            Full::new(body).map_err(|never| -> BoxError { match never {} }),
        ))
        .unwrap_or_else(|_| {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "health response failed")
        })
}

async fn handle_request(
    mut request: Request<Incoming>,
    state: RuntimeState,
) -> Result<Response<BoxBody>, Infallible> {
    let request_id = Uuid::new_v4();
    request.extensions_mut().insert(request_id);
    let health = request.uri().path().trim_end_matches('/') == "/health";
    let started = Instant::now();
    if !health {
        state.usage.log_diagnostic(
            "info",
            format!("event=proxy.http.received request_id={request_id}"),
        );
    }
    let response = handle_request_inner(request, state.clone()).await?;
    if !health || !response.status().is_success() {
        state.usage.log_diagnostic(
            if response.status().is_client_error() || response.status().is_server_error() {
                "warn"
            } else {
                "info"
            },
            format!(
                "event=proxy.http.response_headers request_id={request_id} status={} elapsed_ms={}",
                response.status().as_u16(),
                started.elapsed().as_millis()
            ),
        );
    }
    Ok(response)
}

fn attach_in_flight_guard<G: Send + 'static>(
    response: Response<BoxBody>,
    guard: Option<G>,
) -> Response<BoxBody> {
    let Some(guard) = guard else {
        return response;
    };
    let (parts, body) = response.into_parts();
    let body = body
        .map_frame(move |frame| {
            // Keep the request counted until the body is fully consumed or
            // dropped, which includes long-lived HTTP streaming responses.
            let _keep_alive = &guard;
            frame
        })
        .boxed_unsync();
    Response::from_parts(parts, body)
}

async fn handle_request_inner(
    request: Request<Incoming>,
    state: RuntimeState,
) -> Result<Response<BoxBody>, Infallible> {
    let mut config_changed = ConfigWatch::subscribe(&state);
    let request_id = *request
        .extensions()
        .get::<Uuid>()
        .expect("assigned by HTTP entry point");
    if websocket::is_upgrade_request(&request) {
        return Ok(websocket::handle_request(request, state).await);
    }
    let started = Instant::now();
    let started_at = now_unix();
    let path = request.uri().path().to_string();
    let method = request.method().clone();
    let request_query = request.uri().query().map(str::to_owned);
    if path.trim_end_matches('/') == "/health" {
        return Ok(handle_local_health_request(request, &state).await);
    }
    if path.trim_end_matches('/') == "/v1/models" {
        let mut changed = config_changed;
        return Ok(tokio::select! {
            biased;
            _ = changed.changed() => error_response(StatusCode::SERVICE_UNAVAILABLE, "proxy configuration changed; reconnect"),
            response = handle_models_request(request, state) => response,
        });
    }
    let image_operation = images::Operation::from_path(&path);
    let inbound = ProxyProtocol::from_path(&path);
    if inbound.is_none() && image_operation.is_none() {
        return Ok(error_response(
            StatusCode::NOT_FOUND,
            "unsupported proxy path",
        ));
    }
    if image_operation.is_some() && method != http::Method::POST {
        return Ok(error_response(
            StatusCode::METHOD_NOT_ALLOWED,
            "Images API requires POST",
        ));
    }
    let incoming_headers = request.headers().clone();
    let (bearer_token, api_key_token) = local_proxy_tokens(&incoming_headers);
    if bearer_token.is_none() && api_key_token.is_none() {
        return Ok(error_response(
            StatusCode::UNAUTHORIZED,
            "missing local proxy token",
        ));
    }
    let selected = select_route(&state, bearer_token, api_key_token, inbound)
        .filter(|(route, _)| image_operation.is_none() || images::accepts_route(route));
    let Some((mut route, pricing)) = selected else {
        return Ok(error_response(
            StatusCode::UNAUTHORIZED,
            "invalid local proxy token or route",
        ));
    };
    config_changed.scope(route.config.id);
    route.local_token.zeroize();
    let body = match read_replayable_request_body(request.into_body()).await {
        Ok(body) => body,
        Err(RequestBodyReadError::TooLarge) => {
            return Ok(error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                "proxy request body too large",
            ))
        }
        Err(RequestBodyReadError::Io(err)) => {
            return Ok(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("failed to buffer proxy request body: {err}"),
            ))
        }
        Err(RequestBodyReadError::Transport(err)) => {
            return Ok(error_response(StatusCode::BAD_REQUEST, &err))
        }
    };
    forward_request(
        ForwardRequest {
            image_operation,
            websocket: false,
            upstream_pool: None,
            affinity_fallback: None,
            config_changed,
            request_id,
            method,
            request_query,
            incoming_headers,
            body,
            started,
            started_at,
        },
        state,
        route,
        pricing,
    )
    .await
}

struct ForwardRequest {
    image_operation: Option<images::Operation>,
    websocket: bool,
    upstream_pool: Option<Arc<websocket::pool::Pool>>,
    affinity_fallback: Option<String>,
    config_changed: ConfigWatch,
    request_id: Uuid,
    method: http::Method,
    request_query: Option<String>,
    incoming_headers: HeaderMap,
    body: ReplayableRequestBody,
    started: Instant,
    started_at: i64,
}

#[derive(Clone, Copy)]
struct UpstreamIdentity {
    target_id: Uuid,
}

// Shared HTTP/SSE execution, including conversion, failover and usage tracking.
// WS adaptation calls this directly after its authenticated upgrade.
async fn forward_request(
    request: ForwardRequest,
    state: RuntimeState,
    route: ResolvedRoute,
    pricing: Vec<ModelPricing>,
) -> Result<Response<BoxBody>, Infallible> {
    let guard = InFlightGuard::new(state.in_flight_requests.clone());
    let mut changed = request.config_changed.clone();
    if changed.has_changed().unwrap_or(true) {
        return Ok(error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "proxy configuration changed; reconnect",
        ));
    }
    let response = tokio::select! {
        biased;
        _ = changed.changed() => error_response(StatusCode::SERVICE_UNAVAILABLE, "proxy configuration changed; reconnect"),
        response = forward_request_inner(request, state, route, pricing) => response?,
    };
    Ok(attach_in_flight_guard(response, Some(guard)))
}

async fn forward_request_inner(
    request: ForwardRequest,
    state: RuntimeState,
    route: ResolvedRoute,
    pricing: Vec<ModelPricing>,
) -> Result<Response<BoxBody>, Infallible> {
    if let Some(operation) = request.image_operation {
        return Ok(images::forward(request, state, route, operation).await);
    }
    let ForwardRequest {
        image_operation: _,
        websocket,
        upstream_pool,
        affinity_fallback,
        config_changed,
        request_id,
        method,
        request_query,
        incoming_headers,
        body,
        started,
        started_at,
    } = request;
    state.usage.log_diagnostic(
        "info",
        format!(
            "event=proxy.request.started request_id={request_id} route_id={}",
            route.config.id
        ),
    );
    let request_json = if route.config.conversion_enabled
        && route.targets.iter().any(|target| {
            target.config.enabled
                && route.config.inbound_protocol
                    != target
                        .config
                        .effective_protocol(route.config.upstream_protocol)
        }) {
        body.json().await
    } else {
        None
    };
    let request_metadata = body.metadata().await.unwrap_or_default();
    let tool_summary = body
        .parse_metadata::<diagnostics::protocol::RequestSummary>()
        .await;
    diagnostics::protocol::RequestSummary::log(
        tool_summary.as_ref(),
        &state.usage,
        request_id,
        if websocket {
            "ws_bridge_prepared"
        } else {
            "http_inbound"
        },
    );
    let session_key =
        session_affinity_key(&incoming_headers, Some(&request_metadata)).or(affinity_fallback);
    let streaming_request = request_metadata.stream;
    let model = request_metadata.model;
    let mut last_error = None;
    let mut ws_rejection = None;
    // Keep an identity for a request that is rejected after every target is
    // filtered by its circuit breaker. Such failures still belong in the
    // request-level usage history and must not disappear from the denominator.
    let mut failure_target = route
        .targets
        .iter()
        .find(|target| target.config.enabled)
        .map(|target| {
            (
                target.config.provider_entry_id,
                target.config.secret_id.clone(),
            )
        });
    let mut target_attempts = 0u8;
    let mut generation_submitted = false;
    let mut capacity_skipped = false;
    let mut hold_round = 0u32;
    let hold_deadline = hold_deadline(&route.config.retry, started);
    'hold: loop {
        if hold_deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline) {
            break;
        }
        for _round in 0..silent_retry_rounds(&route.config.retry) {
            let targets = ordered_route_targets(&state, &route, session_key.as_deref());
            let mut round_attempts = 0u8;
            for target in targets {
                if round_attempts >= route.config.retry.max_attempts.max(1) {
                    break;
                }
                if generation_submitted {
                    break 'hold;
                }
                if hold_deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline) {
                    break 'hold;
                }
                let Some(recovery) = RecoveryPermit::acquire(&state, target.config.id) else {
                    continue;
                };
                let Some(provider_permit) = ProviderPermit::acquire(&state, &target) else {
                    capacity_skipped = true;
                    continue;
                };
                round_attempts += 1;
                let target_activity = (
                    TargetActivityGuard::new(&state, target.config.id),
                    recovery,
                    provider_permit,
                );
                target_attempts = target_attempts.saturating_add(1);
                failure_target = Some((
                    target.config.provider_entry_id,
                    target.config.secret_id.clone(),
                ));
                let attempt_started_at = now_unix();
                let attempt_started = Instant::now();
                let mut attempts = target_attempts;
                let mut ws_evidence = None;
                if method == http::Method::POST
                    && route.config.inbound_protocol == ProxyProtocol::OpenAiResponses
                    && target.supports_websockets
                    && target
                        .config
                        .effective_protocol(route.config.upstream_protocol)
                        == ProxyProtocol::OpenAiResponses
                {
                    match websocket::upstream::forward(websocket::upstream::RequestContext {
                        state: &state,
                        route: &route,
                        target: &target,
                        pricing: &pricing,
                        incoming_headers: &incoming_headers,
                        session_key: session_key.as_deref(),
                        query: request_query.as_deref(),
                        body: &body,
                        request_id,
                        attempts: &mut attempts,
                        hold_deadline,
                        pool: upstream_pool.clone(),
                        config_changed: config_changed.clone(),
                        streaming: streaming_request,
                    })
                    .await
                    {
                        websocket::upstream::ForwardOutcome::Response(response) => {
                            return Ok(attach_in_flight_guard(response, Some(target_activity)))
                        }
                        websocket::upstream::ForwardOutcome::HttpFallback(evidence) => {
                            ws_evidence = evidence
                        }
                        websocket::upstream::ForwardOutcome::Rejected(status) => {
                            last_error = Some(format!("WebSocket request rejected ({status})"));
                            ws_rejection = Some(status);
                            target_attempts = attempts;
                            if status_affects_circuit(status) {
                                mark_failure(&state, target.config.id, &route.config.retry);
                            }
                            continue;
                        }
                    }
                    if hold_deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline)
                    {
                        break 'hold;
                    }
                    target_attempts = attempts;
                }
                let client = match upstream_client(&state, route.config.retry.connect_timeout_ms) {
                    Ok(client) => client,
                    Err(err) => {
                        last_error = Some(err);
                        mark_failure(&state, target.config.id, &route.config.retry);
                        persist_attempt(
                            &state.usage,
                            (request_id, route.config.id),
                            &target,
                            model.as_deref(),
                            attempt_started_at,
                            attempt_started,
                            AttemptOutcome::failure(None, None),
                        );
                        continue;
                    }
                };
                let target_protocol = target
                    .config
                    .effective_protocol(route.config.upstream_protocol);
                let conversion = route.config.conversion_enabled
                    && route.config.inbound_protocol != target_protocol;
                state.usage.log_diagnostic("info", format!(
                    "event=proxy.request.forwarding request_id={request_id} target_id={} provider_id={} attempt={attempts} transport=http inbound={:?} upstream={target_protocol:?} converted={conversion}",
                    target.config.id, target.config.provider_entry_id, route.config.inbound_protocol,
                ));
                let mut rewritten_payload = None;
                if conversion {
                    let Some(json_payload) = request_json.clone() else {
                        return Ok(error_response(
                            StatusCode::BAD_REQUEST,
                            "protocol conversion requires a JSON request",
                        ));
                    };
                    rewritten_payload = Some(
                        match BuiltinConversionPlugin
                            .convert_request(
                                route.config.inbound_protocol,
                                target_protocol,
                                json_payload,
                            )
                            .and_then(|value| {
                                let summary =
                                    diagnostics::protocol::RequestSummary::deserialize(&value).ok();
                                diagnostics::protocol::RequestSummary::log(
                                    summary.as_ref(),
                                    &state.usage,
                                    request_id,
                                    "converted_upstream",
                                );
                                serde_json::to_vec(&value).map_err(|err| {
                                    aipass_proxy_conversion::ConversionError::InvalidPayload {
                                        protocol: route.config.inbound_protocol,
                                        message: err.to_string(),
                                    }
                                })
                            }) {
                            Ok(payload) => Bytes::from(payload),
                            Err(err) => {
                                return Ok(error_response(
                                    StatusCode::BAD_REQUEST,
                                    &err.to_string(),
                                ))
                            }
                        },
                    );
                }
                if let Some(payload) = rewritten_payload.take().or_else(|| body.bytes().cloned()) {
                    let updated =
                        request_stream_usage(target_protocol, streaming_request, payload.clone());
                    if updated != payload {
                        rewritten_payload = Some(updated);
                    } else if conversion {
                        rewritten_payload = Some(payload);
                    }
                }
                let upstream_path = if target.config.auth_scheme == "azure_api_key" {
                    target_protocol
                        .path()
                        .strip_prefix("/v1")
                        .unwrap_or(target_protocol.path())
                } else {
                    target_protocol.path()
                };
                let url = match upstream_url_with_query(
                    &target.config.base_url,
                    upstream_path,
                    request_query.as_deref(),
                ) {
                    Ok(url) => url,
                    Err(err) => {
                        last_error = Some(err.to_string());
                        mark_failure(&state, target.config.id, &route.config.retry);
                        persist_attempt(
                            &state.usage,
                            (request_id, route.config.id),
                            &target,
                            model.as_deref(),
                            attempt_started_at,
                            attempt_started,
                            AttemptOutcome::failure(None, None),
                        );
                        continue;
                    }
                };
                let upstream_headers =
                    match build_upstream_headers(&incoming_headers, &target, target_protocol) {
                        Ok(headers) => headers,
                        Err(err) => {
                            last_error = Some(err);
                            mark_failure(&state, target.config.id, &route.config.retry);
                            persist_attempt(
                                &state.usage,
                                (request_id, route.config.id),
                                &target,
                                model.as_deref(),
                                attempt_started_at,
                                attempt_started,
                                AttemptOutcome::failure(None, None),
                            );
                            continue;
                        }
                    };
                let payload_len = rewritten_payload
                    .as_ref()
                    .map_or_else(|| body.len(), |payload| payload.len() as u64);
                let payload = match rewritten_payload {
                    Some(payload) => reqwest::Body::from(payload),
                    None => match body.request_body().await {
                        Ok(payload) => payload,
                        Err(err) => {
                            last_error = Some(err.to_string());
                            mark_failure(&state, target.config.id, &route.config.retry);
                            persist_attempt(
                                &state.usage,
                                (request_id, route.config.id),
                                &target,
                                model.as_deref(),
                                attempt_started_at,
                                attempt_started,
                                AttemptOutcome::failure(None, None),
                            );
                            continue;
                        }
                    },
                };
                let mut upstream_headers = upstream_headers;
                match HeaderValue::from_str(&payload_len.to_string()) {
                    Ok(value) => {
                        upstream_headers.insert(header::CONTENT_LENGTH, value);
                    }
                    Err(err) => {
                        last_error = Some(err.to_string());
                        mark_failure(&state, target.config.id, &route.config.retry);
                        persist_attempt(
                            &state.usage,
                            (request_id, route.config.id),
                            &target,
                            model.as_deref(),
                            attempt_started_at,
                            attempt_started,
                            AttemptOutcome::failure(None, None),
                        );
                        continue;
                    }
                }
                let upstream = client
                    .request(method.clone(), url)
                    .headers(upstream_headers)
                    .body(payload);
                generation_submitted = true;
                let response = match upstream.send().await {
                    Ok(response) => response,
                    Err(err) => {
                        // A connect failure proves no generation was submitted. A
                        // lost response/partial write does not: never replay it.
                        generation_submitted = !err.is_connect();
                        last_error = Some(err.to_string());
                        mark_failure(&state, target.config.id, &route.config.retry);
                        persist_attempt(
                            &state.usage,
                            (request_id, route.config.id),
                            &target,
                            model.as_deref(),
                            attempt_started_at,
                            attempt_started,
                            AttemptOutcome::failure(None, None),
                        );
                        continue;
                    }
                };
                let status = response.status();
                let retryable_status = is_retryable_status(status);
                if retryable_status {
                    generation_submitted = false; // Explicit rejection before generation.
                    let detail = diagnostics::upstream::read_error(
                        response,
                        &target,
                        &local_token_redactions(&incoming_headers),
                    )
                    .await;
                    state.usage.log_upstream_error(
                        request_id,
                        route.config.id,
                        &target.config,
                        status,
                        "http",
                        &detail,
                    );
                    last_error = Some(format!("upstream returned {status}: {detail}"));
                    if status_affects_circuit(status) {
                        mark_failure(&state, target.config.id, &route.config.retry);
                    } else {
                        clear_rejected_session(
                            &state,
                            route.config.id,
                            session_key.as_deref(),
                            target.config.id,
                        );
                    }
                    persist_attempt(
                        &state.usage,
                        (request_id, route.config.id),
                        &target,
                        model.as_deref(),
                        attempt_started_at,
                        attempt_started,
                        AttemptOutcome::failure(Some(status), None),
                    );
                    continue;
                }
                let response_headers = response.headers().clone();
                let content_type = response_headers
                    .get(header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                let streaming_response = streaming_request && is_event_stream(&content_type);
                // A silent retry must not commit a response before the upstream
                // stream has completed. Only an explicit upstream error can permit
                // a retry; an incomplete stream must never replay generation.
                let buffer_streaming =
                    streaming_response && route.config.retry.silent_retry && !websocket;
                let mut upstream_stream: UpstreamBodyStream = Box::pin(response.bytes_stream());
                if is_event_stream(&content_type) {
                    let diagnostic_state = state.clone();
                    let route_id = route.config.id;
                    let target_id = target.config.id;
                    let mut errors = diagnostics::upstream::ErrorEvents::default();
                    upstream_stream = Box::pin(upstream_stream.map(move |chunk| {
                        if let Ok(bytes) = &chunk {
                            if let Some(error) = errors.observe(bytes) {
                                diagnostics::upstream::log_wire_error(
                                    &diagnostic_state,
                                    request_id,
                                    route_id,
                                    target_id,
                                    status,
                                    "sse",
                                    &error,
                                );
                            }
                        }
                        chunk
                    }));
                }
                let first_chunk = match upstream_stream.next().await {
                    Some(Ok(chunk)) => Some(chunk),
                    Some(Err(err)) => {
                        last_error = Some(err.to_string());
                        mark_failure(&state, target.config.id, &route.config.retry);
                        persist_attempt(
                            &state.usage,
                            (request_id, route.config.id),
                            &target,
                            model.as_deref(),
                            attempt_started_at,
                            attempt_started,
                            AttemptOutcome::failure(Some(status), None),
                        );
                        continue;
                    }
                    None => None,
                };
                let (first_chunk, first_token_observed) =
                    if streaming_response && !buffer_streaming && !websocket {
                        // Once this event is returned to the client, replaying on another target is unsafe.
                        match prefetch_sse_event(target_protocol, first_chunk, &mut upstream_stream)
                            .await
                        {
                            Ok(Some(prefetched)) => {
                                (Some(prefetched.bytes), prefetched.first_token_observed)
                            }
                            Ok(None) => {
                                last_error =
                                    Some("upstream stream ended before the first event".into());
                                mark_failure(&state, target.config.id, &route.config.retry);
                                persist_attempt(
                                    &state.usage,
                                    (request_id, route.config.id),
                                    &target,
                                    model.as_deref(),
                                    attempt_started_at,
                                    attempt_started,
                                    AttemptOutcome::failure(Some(status), None),
                                );
                                continue;
                            }
                            Err(err) => {
                                generation_submitted = !err.confirmed_failure;
                                last_error = Some(err.message);
                                mark_failure(&state, target.config.id, &route.config.retry);
                                persist_attempt(
                                    &state.usage,
                                    (request_id, route.config.id),
                                    &target,
                                    model.as_deref(),
                                    attempt_started_at,
                                    attempt_started,
                                    AttemptOutcome::failure(Some(status), None),
                                );
                                continue;
                            }
                        }
                    } else {
                        (first_chunk, false)
                    };
                let first_token_ms =
                    first_token_observed.then(|| attempt_started.elapsed().as_millis() as u64);
                let upstream_protocol = target_protocol;
                let inbound_protocol = route.config.inbound_protocol;
                let model = model.clone();
                let model_pricing = model.as_deref().and_then(|model| {
                    pricing
                        .iter()
                        .filter(|item| item.model == model || model.starts_with(&item.model))
                        .max_by_key(|item| item.model.len())
                        .cloned()
                });
                let record = UsageRecord {
                    id: request_id,
                    started_at,
                    duration_ms: started.elapsed().as_millis() as u64,
                    first_token_ms,
                    route_id: route.config.id,
                    provider_entry_id: target.config.provider_entry_id,
                    secret_id: target.config.secret_id.clone(),
                    model: model.clone(),
                    inbound_protocol,
                    upstream_protocol,
                    status: status.as_u16(),
                    attempts,
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_read_tokens: 0,
                    cache_creation_tokens: 0,
                    estimated_cost_micros: 0,
                };
                let mut converted_payload = None;
                let mut response_id = None;
                let body_stream: UpstreamBodyStream = if streaming_response && !buffer_streaming {
                    if let Some(first_chunk) = first_chunk {
                        Box::pin(
                            stream::once(async move { Ok(first_chunk) }).chain(upstream_stream),
                        )
                    } else {
                        Box::pin(stream::empty())
                    }
                } else {
                    let buffered =
                        match collect_upstream_body(first_chunk, &mut upstream_stream).await {
                            Ok(buffered) => buffered,
                            Err(err) => {
                                last_error = Some(err);
                                mark_failure(&state, target.config.id, &route.config.retry);
                                persist_attempt(
                                    &state.usage,
                                    (request_id, route.config.id),
                                    &target,
                                    model.as_deref(),
                                    attempt_started_at,
                                    attempt_started,
                                    AttemptOutcome::failure(Some(status), first_token_ms),
                                );
                                continue;
                            }
                        };
                    if buffer_streaming
                        && (stream_reports_error(&buffered)
                            || !stream_reports_completion(target_protocol, &buffered))
                    {
                        generation_submitted = !stream_reports_error(&buffered);
                        last_error =
                            Some("upstream stream ended before protocol completion".into());
                        mark_failure(&state, target.config.id, &route.config.retry);
                        persist_attempt(
                            &state.usage,
                            (request_id, route.config.id),
                            &target,
                            model.as_deref(),
                            attempt_started_at,
                            attempt_started,
                            AttemptOutcome::failure(Some(status), first_token_ms),
                        );
                        continue;
                    }
                    if status.is_success() && is_upstream_error_payload(&buffered) {
                        generation_submitted = false;
                        let detail = diagnostics::upstream::error_detail(
                            &buffered,
                            &target,
                            &local_token_redactions(&incoming_headers),
                        );
                        state.usage.log_upstream_error(
                            request_id,
                            route.config.id,
                            &target.config,
                            status,
                            "http",
                            &detail,
                        );
                        last_error = Some("upstream returned an error payload".into());
                        mark_failure(&state, target.config.id, &route.config.retry);
                        persist_attempt(
                            &state.usage,
                            (request_id, route.config.id),
                            &target,
                            model.as_deref(),
                            attempt_started_at,
                            attempt_started,
                            AttemptOutcome::failure(Some(status), first_token_ms),
                        );
                        continue;
                    }
                    if status.is_success() && buffered.is_empty() {
                        generation_submitted = false;
                        last_error = Some("upstream returned an empty response".into());
                        mark_failure(&state, target.config.id, &route.config.retry);
                        persist_attempt(
                            &state.usage,
                            (request_id, route.config.id),
                            &target,
                            model.as_deref(),
                            attempt_started_at,
                            attempt_started,
                            AttemptOutcome::failure(Some(status), first_token_ms),
                        );
                        continue;
                    }
                    if !streaming_response && inbound_protocol == ProxyProtocol::OpenAiResponses {
                        #[derive(Deserialize)]
                        struct ResponseId {
                            id: Option<String>,
                        }
                        response_id = serde_json::from_slice::<ResponseId>(&buffered)
                            .ok()
                            .and_then(|response| response.id)
                            .and_then(|id| normalize_session_affinity_key(&id));
                    }
                    if conversion && !streaming_response {
                        converted_payload = serde_json::from_slice::<serde_json::Value>(&buffered)
                            .ok()
                            .and_then(|value| {
                                BuiltinConversionPlugin
                                    .convert_response(upstream_protocol, inbound_protocol, value)
                                    .ok()
                            })
                            .and_then(|value| serde_json::to_vec(&value).ok())
                            .map(Bytes::from);
                        if converted_payload.is_none() {
                            generation_submitted = false;
                            last_error =
                                Some("protocol conversion failed for upstream response".into());
                            mark_failure(&state, target.config.id, &route.config.retry);
                            persist_attempt(
                                &state.usage,
                                (request_id, route.config.id),
                                &target,
                                model.as_deref(),
                                attempt_started_at,
                                attempt_started,
                                AttemptOutcome::failure(Some(status), first_token_ms),
                            );
                            continue;
                        }
                    }
                    Box::pin(stream::once(async move {
                        Ok::<Bytes, reqwest::Error>(buffered)
                    }))
                };
                let streaming_attempt = streaming_response.then(|| {
                    (
                        AttemptRecord {
                            id: Uuid::new_v4(),
                            request_id: Some(record.id),
                            started_at: attempt_started_at,
                            duration_ms: 0,
                            first_token_ms,
                            route_id: route.config.id,
                            target_id: target.config.id,
                            provider_entry_id: target.config.provider_entry_id,
                            secret_id: target.config.secret_id.clone(),
                            model: model.clone(),
                            status: Some(status.as_u16()),
                            success: None,
                        },
                        attempt_started,
                    )
                });
                let body_stream = track_usage_stream(
                    body_stream,
                    UsageTrackingContext {
                        protocol: upstream_protocol,
                        ws_evidence,
                        store: state.usage.clone(),
                        record,
                        pricing: model_pricing,
                        streaming: streaming_response,
                        attempt_started,
                        started,
                        failure_state: state.clone(),
                        config_changed: config_changed.clone(),
                        route_id: route.config.id,
                        target_id: target.config.id,
                        session_key: session_key.clone(),
                        retry_policy: route.config.retry.clone(),
                        attempt: streaming_attempt,
                    },
                );
                let output_stream: Pin<Box<dyn Stream<Item = Result<Bytes, BoxError>> + Send>> =
                    if conversion && streaming_response {
                        convert_sse_stream(body_stream, upstream_protocol, inbound_protocol)
                    } else if let Some(converted) = converted_payload {
                        // Track usage from the original provider payload only after
                        // conversion has succeeded, then emit the converted body.
                        Box::pin(body_stream.map(move |result| result.map(|_| converted.clone())))
                    } else {
                        Box::pin(body_stream)
                    };
                if !streaming_response {
                    persist_attempt(
                        &state.usage,
                        (request_id, route.config.id),
                        &target,
                        model.as_deref(),
                        attempt_started_at,
                        attempt_started,
                        AttemptOutcome::success(status, first_token_ms),
                    );
                    complete_target_success(
                        &state,
                        route.config.id,
                        session_key.as_deref(),
                        target.config.id,
                        attempt_started,
                        response_id.as_deref(),
                    );
                }
                let frame_stream = output_stream.map(|result| result.map(Frame::data));
                let stream_body = BodyExt::boxed_unsync(StreamBody::new(frame_stream));
                let mut builder = Response::builder().status(status);
                let response_hop_headers = connection_header_names(&response_headers);
                for (name, value) in response_headers.iter() {
                    if !(is_hop_header(name)
                        || response_hop_headers.contains(name)
                        || name == header::CONTENT_LENGTH
                        || conversion && name == header::CONTENT_ENCODING)
                    {
                        builder = builder.header(name, value);
                    }
                }
                let mut response = builder.body(stream_body).unwrap_or_else(|_| {
                    error_response(StatusCode::BAD_GATEWAY, "failed to build proxy response")
                });
                response.extensions_mut().insert(UpstreamIdentity {
                    target_id: target.config.id,
                });
                return Ok(attach_in_flight_guard(response, Some(target_activity)));
            }
        }
        if generation_submitted {
            break;
        }
        let retry = &route.config.retry;
        if !retry.hold_on_failure || !route.targets.iter().any(|target| target.config.enabled) {
            break;
        }
        let elapsed = started.elapsed();
        let mut delay = hold_backoff_delay(retry, hold_round);
        if retry.hold_max_duration_ms > 0 {
            let budget = Duration::from_millis(retry.hold_max_duration_ms);
            if elapsed >= budget {
                break;
            }
            delay = delay.min(budget - elapsed);
        }
        tokio::time::sleep(delay).await;
        hold_round = hold_round.saturating_add(1);
    }

    let at_capacity = capacity_skipped && target_attempts == 0;
    let final_status = if at_capacity {
        StatusCode::TOO_MANY_REQUESTS
    } else {
        ws_rejection.unwrap_or(StatusCode::BAD_GATEWAY)
    };
    let message = if at_capacity {
        "all available providers are at their concurrency limit"
    } else {
        "all upstream targets failed"
    };
    let diagnostic = last_error.unwrap_or_else(|| message.into());
    record_request(&state, false, None);
    set_error(&state, diagnostic);
    if let Some((provider_entry_id, secret_id)) = failure_target {
        let _ = state.usage.record(&UsageRecord {
            id: request_id,
            started_at,
            duration_ms: started.elapsed().as_millis() as u64,
            first_token_ms: None,
            route_id: route.config.id,
            provider_entry_id,
            secret_id,
            model,
            inbound_protocol: route.config.inbound_protocol,
            upstream_protocol: route.config.upstream_protocol,
            status: final_status.as_u16(),
            attempts: target_attempts,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            estimated_cost_micros: 0,
        });
    }
    Ok(if at_capacity {
        capacity_response()
    } else {
        error_response(final_status, message)
    })
}

struct AttemptOutcome {
    first_token_ms: Option<u64>,
    status: Option<u16>,
    success: bool,
}

impl AttemptOutcome {
    fn failure(status: Option<StatusCode>, first_token_ms: Option<u64>) -> Self {
        Self {
            first_token_ms,
            status: status.map(|status| status.as_u16()),
            success: false,
        }
    }

    fn success(status: StatusCode, first_token_ms: Option<u64>) -> Self {
        Self {
            first_token_ms,
            status: Some(status.as_u16()),
            success: true,
        }
    }
}

fn persist_attempt(
    store: &UsageStore,
    ids: (Uuid, Uuid),
    target: &ResolvedTarget,
    model: Option<&str>,
    started_at: i64,
    started: Instant,
    outcome: AttemptOutcome,
) {
    let (request_id, route_id) = ids;
    let _ = store.record_attempt(&AttemptRecord {
        id: Uuid::new_v4(),
        request_id: Some(request_id),
        started_at,
        duration_ms: started.elapsed().as_millis() as u64,
        first_token_ms: outcome.first_token_ms,
        route_id,
        target_id: target.config.id,
        provider_entry_id: target.config.provider_entry_id,
        secret_id: target.config.secret_id.clone(),
        model: model.map(str::to_owned),
        status: outcome.status,
        success: Some(outcome.success),
    });
}

fn request_stream_usage(protocol: ProxyProtocol, streaming: bool, payload: Bytes) -> Bytes {
    if !streaming || protocol != ProxyProtocol::OpenAiChatCompletions {
        return payload;
    }
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&payload) else {
        return payload;
    };
    let Some(object) = value.as_object_mut() else {
        return payload;
    };
    match object.get_mut("stream_options") {
        Some(serde_json::Value::Object(options)) => {
            options.insert("include_usage".into(), serde_json::Value::Bool(true));
        }
        Some(serde_json::Value::Null) | None => {
            object.insert(
                "stream_options".into(),
                serde_json::json!({ "include_usage": true }),
            );
        }
        Some(_) => return payload,
    }
    serde_json::to_vec(&value).map_or(payload, Bytes::from)
}

fn local_token_redactions(headers: &HeaderMap) -> [&str; 2] {
    let (bearer, api_key) = local_proxy_tokens(headers);
    [bearer.unwrap_or_default(), api_key.unwrap_or_default()]
}

fn local_proxy_tokens(headers: &HeaderMap) -> (Option<&str>, Option<&str>) {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            let (scheme, token) = value.split_once(' ')?;
            scheme
                .eq_ignore_ascii_case("bearer")
                .then_some(token.trim())
        })
        .filter(|token| !token.is_empty());
    let api_key = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|token| !token.is_empty());
    (bearer, api_key)
}

struct PrefetchedSse {
    bytes: Bytes,
    first_token_observed: bool,
}

struct PrefetchError {
    message: String,
    confirmed_failure: bool,
}

impl From<String> for PrefetchError {
    fn from(message: String) -> Self {
        Self {
            message,
            confirmed_failure: false,
        }
    }
}
impl From<&str> for PrefetchError {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}

async fn prefetch_sse_event(
    protocol: ProxyProtocol,
    first_chunk: Option<Bytes>,
    source: &mut UpstreamBodyStream,
) -> Result<Option<PrefetchedSse>, PrefetchError> {
    let Some(first_chunk) = first_chunk else {
        return Ok(None);
    };
    let mut buffered = first_chunk.to_vec();
    let mut inspected = 0;
    loop {
        if buffered.len() > MAX_BUFFERED_RESPONSE_BYTES {
            return Err("upstream first event exceeds proxy buffer limit".into());
        }
        while let Some(relative_end) = sse_event_boundary_end(&buffered[inspected..]) {
            let event_end = inspected + relative_end;
            let event = &buffered[inspected..event_end];
            inspected = event_end;
            if sse_event_reports_error(event) {
                return Err(PrefetchError {
                    message: "upstream returned an error event".into(),
                    confirmed_failure: true,
                });
            }
            if sse_event_is_heartbeat(event) {
                continue;
            }
            if sse_event_reports_output(protocol, event) {
                return Ok(Some(PrefetchedSse {
                    bytes: Bytes::from(buffered),
                    first_token_observed: true,
                }));
            }
            if sse_event_reports_completion(protocol, event) {
                return Ok(Some(PrefetchedSse {
                    bytes: Bytes::from(buffered),
                    first_token_observed: false,
                }));
            }
        }
        match source.next().await {
            Some(Ok(chunk)) => buffered.extend_from_slice(&chunk),
            Some(Err(err)) => return Err(err.to_string().into()),
            None => return Err("upstream stream ended before the first complete event".into()),
        }
    }
}

fn sse_event_boundary_end(bytes: &[u8]) -> Option<usize> {
    let line_feed = bytes
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|index| index + 2);
    let carriage_return = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4);
    match (line_feed, carriage_return) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(end), None) | (None, Some(end)) => Some(end),
        (None, None) => None,
    }
}

fn sse_event_reports_error(event: &[u8]) -> bool {
    let event_is_error = sse_event_name(event)
        .is_some_and(|name| name.eq_ignore_ascii_case(b"error") || name.ends_with(b".failed"));
    if event_is_error {
        return true;
    }
    sse_event_data(event)
        .as_deref()
        .is_some_and(is_upstream_error_payload)
}

fn sse_event_reports_output(protocol: ProxyProtocol, event: &[u8]) -> bool {
    let Some(data) = sse_event_data(event) else {
        return false;
    };
    let data = trim_ascii(&data);
    if data.is_empty() || data == b"[DONE]" {
        return false;
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(data) else {
        return true;
    };
    match protocol {
        ProxyProtocol::OpenAiResponses => {
            let Some(kind) = value.get("type").and_then(serde_json::Value::as_str) else {
                return true;
            };
            if matches!(
                kind,
                "response.created"
                    | "response.in_progress"
                    | "response.output_item.added"
                    | "response.content_part.added"
            ) {
                return false;
            }
            if kind.ends_with(".delta") {
                return value.get("delta").is_some_and(json_value_has_output);
            }
            value
                .get("text")
                .or_else(|| value.get("content"))
                .is_some_and(json_value_has_output)
        }
        ProxyProtocol::OpenAiChatCompletions => {
            let Some(choices) = value.get("choices").and_then(serde_json::Value::as_array) else {
                return value.get("usage").is_none();
            };
            choices.iter().any(|choice| {
                let Some(delta) = choice.get("delta").or_else(|| choice.get("message")) else {
                    return false;
                };
                delta.get("content").is_some_and(json_value_has_output)
                    || delta.get("tool_calls").is_some_and(json_value_has_output)
                    || delta
                        .get("function_call")
                        .is_some_and(json_value_has_output)
            })
        }
        ProxyProtocol::AnthropicMessages => {
            let Some(kind) = value.get("type").and_then(serde_json::Value::as_str) else {
                return true;
            };
            match kind {
                "message_start" | "content_block_start" | "message_delta" | "message_stop" => false,
                "content_block_delta" => value.get("delta").is_some_and(json_value_has_output),
                _ => true,
            }
        }
    }
}

fn json_value_has_output(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(_) | serde_json::Value::Number(_) => true,
        serde_json::Value::String(value) => !value.is_empty(),
        serde_json::Value::Array(value) => !value.is_empty(),
        serde_json::Value::Object(value) => !value.is_empty(),
    }
}

fn sse_event_is_heartbeat(event: &[u8]) -> bool {
    if sse_event_name(event).is_some_and(|name| {
        name.eq_ignore_ascii_case(b"ping") || name.eq_ignore_ascii_case(b"heartbeat")
    }) {
        return true;
    }
    sse_event_data(event)
        .and_then(|data| serde_json::from_slice::<serde_json::Value>(&data).ok())
        .is_some_and(|value| {
            value
                .get("type")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|kind| {
                    kind.eq_ignore_ascii_case("ping") || kind.eq_ignore_ascii_case("heartbeat")
                })
        })
}

fn sse_event_name(event: &[u8]) -> Option<&[u8]> {
    event.split(|byte| *byte == b'\n').find_map(|line| {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        line.strip_prefix(b"event:").map(trim_ascii)
    })
}

fn sse_event_data(event: &[u8]) -> Option<Vec<u8>> {
    let mut found = false;
    let mut data = Vec::new();
    for line in event.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let value = if line == b"data" {
            &[][..]
        } else if let Some(value) = line.strip_prefix(b"data:") {
            value.strip_prefix(b" ").unwrap_or(value)
        } else {
            continue;
        };
        if found {
            data.push(b'\n');
        }
        found = true;
        data.extend_from_slice(value);
    }
    found.then_some(data)
}

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn is_upstream_error_payload(bytes: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .is_some_and(|value| {
            value.get("error").is_some_and(|error| !error.is_null())
                || value
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|kind| kind == "error" || kind.ends_with(".failed"))
                || value.get("status").and_then(serde_json::Value::as_str) == Some("failed")
                || value
                    .pointer("/response/error")
                    .is_some_and(|error| !error.is_null())
                || value
                    .pointer("/response/status")
                    .and_then(serde_json::Value::as_str)
                    == Some("failed")
        })
}

fn stream_reports_error(bytes: &[u8]) -> bool {
    let mut offset = 0;
    while let Some(relative_end) = sse_event_boundary_end(&bytes[offset..]) {
        let end = offset + relative_end;
        if sse_event_reports_error(&bytes[offset..end]) {
            return true;
        }
        offset = end;
    }
    offset < bytes.len() && sse_event_reports_error(&bytes[offset..])
}

fn stream_reports_completion(protocol: ProxyProtocol, bytes: &[u8]) -> bool {
    let mut offset = 0;
    while let Some(relative_end) = sse_event_boundary_end(&bytes[offset..]) {
        let end = offset + relative_end;
        if sse_event_reports_completion(protocol, &bytes[offset..end]) {
            return true;
        }
        offset = end;
    }
    offset < bytes.len() && sse_event_reports_completion(protocol, &bytes[offset..])
}

fn sse_event_reports_completion(protocol: ProxyProtocol, event: &[u8]) -> bool {
    let Some(data) = sse_event_data(event) else {
        return false;
    };
    if data == b"[DONE]" && protocol != ProxyProtocol::AnthropicMessages {
        return true;
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&data) else {
        return false;
    };
    match protocol {
        ProxyProtocol::OpenAiResponses => {
            matches!(
                value.get("type").and_then(serde_json::Value::as_str),
                Some("response.completed" | "response.incomplete")
            ) || matches!(
                value
                    .pointer("/response/status")
                    .and_then(serde_json::Value::as_str),
                Some("completed" | "incomplete")
            )
        }
        ProxyProtocol::OpenAiChatCompletions => value
            .get("choices")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|choices| {
                !choices.is_empty()
                    && choices.iter().all(|choice| {
                        choice
                            .get("finish_reason")
                            .is_some_and(|reason| !reason.is_null())
                    })
            }),
        ProxyProtocol::AnthropicMessages => {
            value.get("type").and_then(serde_json::Value::as_str) == Some("message_stop")
        }
    }
}

fn sse_event_is_terminal(protocol: ProxyProtocol, event: &[u8]) -> bool {
    let Some(data) = sse_event_data(event) else {
        return false;
    };
    if protocol == ProxyProtocol::OpenAiChatCompletions {
        return trim_ascii(&data) == b"[DONE]";
    }
    sse_event_reports_completion(protocol, event)
}

fn is_event_stream(content_type: &str) -> bool {
    content_type
        .split(';')
        .next()
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("text/event-stream"))
}

async fn collect_upstream_body(
    first_chunk: Option<Bytes>,
    source: &mut UpstreamBodyStream,
) -> Result<Bytes, String> {
    let mut buffered = first_chunk.map_or_else(Vec::new, |chunk| chunk.to_vec());
    loop {
        if buffered.len() > MAX_BUFFERED_RESPONSE_BYTES {
            return Err("upstream response exceeds proxy buffer limit".into());
        }
        match source.next().await {
            Some(Ok(chunk)) => buffered.extend_from_slice(&chunk),
            Some(Err(err)) => return Err(err.to_string()),
            None => return Ok(Bytes::from(buffered)),
        }
    }
}

fn convert_sse_stream<S>(
    source: S,
    from: ProxyProtocol,
    to: ProxyProtocol,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, BoxError>> + Send>>
where
    S: Stream<Item = Result<Bytes, BoxError>> + Send + 'static,
{
    let converter = match StreamConverter::new(from, to) {
        Ok(converter) => converter,
        Err(err) => return Box::pin(stream::once(async move { Err(Box::new(err) as BoxError) })),
    };
    let source = Box::pin(source);
    Box::pin(stream::unfold(
        (
            source,
            Vec::<u8>::new(),
            VecDeque::<Bytes>::new(),
            converter,
            false,
        ),
        move |(mut source, mut buffer, mut pending, mut converter, mut done)| async move {
            loop {
                if let Some(value) = pending.pop_front() {
                    return Some((Ok(value), (source, buffer, pending, converter, done)));
                }
                if done {
                    return None;
                }
                if let Some(end) = sse_event_boundary_end(&buffer) {
                    let event = String::from_utf8_lossy(&buffer[..end]).to_string();
                    buffer.drain(..end);
                    match converter.push_event(&event) {
                        Ok(events) => pending.extend(events.into_iter().map(Bytes::from)),
                        Err(err) => {
                            done = true;
                            return Some((
                                Err(Box::new(err) as BoxError),
                                (source, buffer, pending, converter, done),
                            ));
                        }
                    }
                    continue;
                }
                match source.next().await {
                    Some(Ok(chunk)) => buffer.extend_from_slice(&chunk),
                    Some(Err(err)) => {
                        done = true;
                        return Some((Err(err), (source, buffer, pending, converter, done)));
                    }
                    None => {
                        done = true;
                        if !buffer.is_empty() {
                            let event = String::from_utf8_lossy(&buffer).to_string();
                            buffer.clear();
                            match converter.push_event(&event) {
                                Ok(events) => pending.extend(events.into_iter().map(Bytes::from)),
                                Err(err) => {
                                    return Some((
                                        Err(Box::new(err) as BoxError),
                                        (source, buffer, pending, converter, done),
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        },
    ))
}

struct UsageTrackingContext {
    ws_evidence: Option<websocket::capability::Evidence>,
    protocol: ProxyProtocol,
    store: Arc<UsageStore>,
    record: UsageRecord,
    pricing: Option<ModelPricing>,
    streaming: bool,
    attempt_started: Instant,
    started: Instant,
    failure_state: RuntimeState,
    config_changed: ConfigWatch,
    route_id: Uuid,
    target_id: Uuid,
    session_key: Option<String>,
    retry_policy: RetryPolicy,
    attempt: Option<(AttemptRecord, Instant)>,
}

fn track_usage_stream<S>(
    source: S,
    context: UsageTrackingContext,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, BoxError>> + Send>>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    let UsageTrackingContext {
        ws_evidence,
        protocol,
        store,
        mut record,
        pricing,
        streaming,
        attempt_started,
        started,
        failure_state,
        mut config_changed,
        route_id,
        target_id,
        session_key,
        retry_policy,
        attempt,
    } = context;
    let mut source = Box::pin(source);
    let (sender, receiver) = tokio::sync::mpsc::channel(8);
    tokio::spawn(async move {
        let mut tail = Vec::new();
        let mut usage_event_buffer = Vec::new();
        let mut observed_usage = TokenUsage::default();
        let mut source_ended = false;
        let mut transport_failed = false;
        let mut protocol_completed = false;
        let mut protocol_terminal = false;
        let mut protocol_failed = false;
        let mut response_id = None;
        let mut response_trace = (streaming && protocol == ProxyProtocol::OpenAiResponses)
            .then(diagnostics::protocol::ResponseTrace::default);
        loop {
            let next = tokio::select! {
                _ = sender.closed() => break,
                _ = config_changed.changed() => break,
                result = source.next() => result,
            };
            let result: Result<Bytes, BoxError> = match next {
                Some(result) => result.map_err(|err| Box::new(err) as BoxError),
                None => {
                    source_ended = true;
                    break;
                }
            };
            if let Ok(chunk) = &result {
                if streaming {
                    let signals = observe_sse_usage_traced(
                        protocol,
                        &mut usage_event_buffer,
                        chunk,
                        &mut observed_usage,
                        response_trace.as_mut(),
                    );
                    protocol_completed |= signals.completed;
                    protocol_terminal |= signals.terminal;
                    protocol_failed |= signals.failed;
                    response_id = signals.response_id.or(response_id);
                } else {
                    merge_usage(&mut observed_usage, usage_from_wire_bytes(protocol, chunk));
                }
                tail.extend_from_slice(chunk);
                const USAGE_TAIL_LIMIT: usize = 256 * 1024;
                if tail.len() > USAGE_TAIL_LIMIT {
                    tail.drain(..tail.len() - USAGE_TAIL_LIMIT);
                }
            }
            transport_failed = result.is_err();
            if let Err(err) = &result {
                mark_failure(&failure_state, target_id, &retry_policy);
                set_error(&failure_state, err.to_string());
            }
            if sender.send(result).await.is_err() {
                break;
            }
            if transport_failed || (streaming && (protocol_terminal || protocol_failed)) {
                break;
            }
        }
        let stream_succeeded = if streaming {
            (protocol_terminal || (source_ended && protocol_completed))
                && !protocol_failed
                && !transport_failed
        } else {
            source_ended && !transport_failed
        };
        if stream_succeeded {
            if protocol == ProxyProtocol::OpenAiResponses {
                websocket::capability::http_success(&failure_state, ws_evidence.as_ref());
            }
            if streaming {
                complete_target_success(
                    &failure_state,
                    route_id,
                    session_key.as_deref(),
                    target_id,
                    attempt_started,
                    response_id.as_deref(),
                );
            }
        } else if protocol_failed {
            mark_failure(&failure_state, target_id, &retry_policy);
            set_error(
                &failure_state,
                "upstream returned an error event after stream commit".into(),
            );
        } else if streaming && source_ended {
            mark_failure(&failure_state, target_id, &retry_policy);
            set_error(
                &failure_state,
                "upstream stream ended before protocol completion".into(),
            );
        }
        merge_usage(&mut observed_usage, usage_from_wire_bytes(protocol, &tail));
        if let Some(trace) = response_trace {
            trace.log(&store, record.id, "http_sse_upstream");
        }
        record_recent_tokens(
            &failure_state,
            observed_usage
                .input_tokens
                .saturating_add(observed_usage.output_tokens)
                .saturating_add(observed_usage.cache_read_tokens)
                .saturating_add(observed_usage.cache_creation_tokens),
        );
        record.duration_ms = started.elapsed().as_millis() as u64;
        if !stream_succeeded {
            record.status = StatusCode::BAD_GATEWAY.as_u16();
        }
        if stream_succeeded || transport_failed || protocol_failed || source_ended {
            record_request(&failure_state, stream_succeeded, record.first_token_ms);
        }
        record.input_tokens = observed_usage.input_tokens;
        record.output_tokens = observed_usage.output_tokens;
        record.cache_read_tokens = observed_usage.cache_read_tokens;
        record.cache_creation_tokens = observed_usage.cache_creation_tokens;
        record.estimated_cost_micros = pricing
            .as_ref()
            .map(|pricing| estimate_cost(&observed_usage, pricing))
            .unwrap_or(0);
        let _ = store.record(&record);
        if let Some((mut attempt, attempt_started)) = attempt {
            attempt.duration_ms = attempt_started.elapsed().as_millis() as u64;
            attempt.success = if stream_succeeded {
                Some(true)
            } else if transport_failed || protocol_failed || source_ended {
                Some(false)
            } else {
                None
            };
            let _ = store.record_attempt(&attempt);
        }
    });
    Box::pin(stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|item| (item, receiver))
    }))
}

fn estimate_cost(usage: &TokenUsage, pricing: &ModelPricing) -> u64 {
    let total = usage
        .input_tokens
        .saturating_mul(pricing.input_micros_per_million)
        .saturating_add(
            usage
                .output_tokens
                .saturating_mul(pricing.output_micros_per_million),
        )
        .saturating_add(
            usage
                .cache_read_tokens
                .saturating_mul(pricing.cache_read_micros_per_million),
        )
        .saturating_add(
            usage
                .cache_creation_tokens
                .saturating_mul(pricing.cache_creation_micros_per_million),
        );
    total / 1_000_000
}

fn usage_from_wire_bytes(protocol: ProxyProtocol, bytes: &[u8]) -> TokenUsage {
    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) {
        return usage_from_wire_value(protocol, &value);
    }
    let text = String::from_utf8_lossy(bytes);
    let mut total = TokenUsage::default();
    for line in text
        .lines()
        .filter_map(|line| line.strip_prefix("data:").map(str::trim))
    {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        merge_usage(&mut total, usage_from_wire_value(protocol, &value));
    }
    total
}

fn usage_from_wire_value(protocol: ProxyProtocol, value: &serde_json::Value) -> TokenUsage {
    let mut usage = BuiltinConversionPlugin.extract_usage(protocol, value);
    let nested = match protocol {
        ProxyProtocol::OpenAiResponses => value.get("response"),
        ProxyProtocol::AnthropicMessages => value.get("message"),
        ProxyProtocol::OpenAiChatCompletions => None,
    };
    if let Some(nested) = nested {
        merge_usage(
            &mut usage,
            BuiltinConversionPlugin.extract_usage(protocol, nested),
        );
    }
    usage
}

#[derive(Default)]
struct SseSignals {
    response_id: Option<String>,
    completed: bool,
    terminal: bool,
    failed: bool,
}

#[cfg(test)]
fn observe_sse_usage(
    protocol: ProxyProtocol,
    buffer: &mut Vec<u8>,
    chunk: &[u8],
    usage: &mut TokenUsage,
) -> SseSignals {
    observe_sse_usage_traced(protocol, buffer, chunk, usage, None)
}

fn observe_sse_usage_traced(
    protocol: ProxyProtocol,
    buffer: &mut Vec<u8>,
    chunk: &[u8],
    usage: &mut TokenUsage,
    mut trace: Option<&mut diagnostics::protocol::ResponseTrace>,
) -> SseSignals {
    buffer.extend_from_slice(chunk);
    let mut consumed = 0;
    let mut signals = SseSignals::default();
    while let Some(relative_end) = sse_event_boundary_end(&buffer[consumed..]) {
        let end = consumed + relative_end;
        let event = &buffer[consumed..end];
        signals.failed |= sse_event_reports_error(event);
        signals.completed |= sse_event_reports_completion(protocol, event);
        signals.terminal |= sse_event_is_terminal(protocol, event);
        if let Some(data) = sse_event_data(event) {
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&data) {
                if let Some(trace) = trace.as_deref_mut() {
                    trace.observe(&value);
                }
                if protocol == ProxyProtocol::OpenAiResponses {
                    if let Some(id) = value
                        .pointer("/response/id")
                        .and_then(serde_json::Value::as_str)
                        .and_then(normalize_session_affinity_key)
                    {
                        signals.response_id = Some(id);
                    }
                }
                merge_usage(usage, usage_from_wire_value(protocol, &value));
            }
        }
        consumed = end;
    }
    if consumed > 0 {
        buffer.drain(..consumed);
    }
    const USAGE_EVENT_BUFFER_LIMIT: usize = 1024 * 1024;
    if buffer.len() > USAGE_EVENT_BUFFER_LIMIT {
        if let Some(trace) = trace {
            trace.limited();
        }
        buffer.clear();
    }
    signals
}

fn merge_usage(total: &mut TokenUsage, usage: TokenUsage) {
    total.input_tokens = total.input_tokens.max(usage.input_tokens);
    total.output_tokens = total.output_tokens.max(usage.output_tokens);
    total.cache_read_tokens = total.cache_read_tokens.max(usage.cache_read_tokens);
    total.cache_creation_tokens = total.cache_creation_tokens.max(usage.cache_creation_tokens);
}

#[cfg(test)]
fn upstream_url(base_url: &str, path: &str) -> Result<String, ProxyError> {
    upstream_url_with_query(base_url, path, None)
}

pub fn upstream_url_with_query(
    base_url: &str,
    path: &str,
    query: Option<&str>,
) -> Result<String, ProxyError> {
    let base =
        reqwest::Url::parse(base_url).map_err(|err| ProxyError::InvalidConfig(err.to_string()))?;
    let base_path = base.path().trim_end_matches('/').to_string();
    // Respect an explicit /v1 path segment in the user's API base. Other
    // versions and names such as /openai do not imply /v1 is already present.
    // The official Codex OAuth backend has its own unversioned resource path.
    let strip_version_prefix = base_path.split('/').any(|segment| segment == "v1")
        || base_path.ends_with("/backend-api/codex");
    let suffix = if strip_version_prefix && (path == "/v1" || path.starts_with("/v1/")) {
        &path[3..]
    } else {
        path
    };
    let mut url = base;
    url.set_path(&format!("{}{}", base_path, suffix));
    if let Some(query) = query.filter(|query| !query.is_empty()) {
        let merged = match url.query().filter(|existing| !existing.is_empty()) {
            Some(existing) => format!("{existing}&{query}"),
            None => query.to_string(),
        };
        url.set_query(Some(&merged));
    }
    Ok(url.to_string())
}

fn round_robin_start(state: &RuntimeState, route_id: Uuid, weights: &[u32]) -> usize {
    let counter = {
        let Ok(mut counters) = state.rr_counters.lock() else {
            return 0;
        };
        counters
            .entry(route_id)
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed)
    };
    weighted_start_index(counter, weights)
}

fn weighted_start_index(counter: u64, weights: &[u32]) -> usize {
    if weights.is_empty() {
        return 0;
    }
    let total: u64 = weights.iter().map(|weight| u64::from(*weight)).sum();
    if total == 0 {
        return (counter % weights.len() as u64) as usize;
    }
    let mut position = counter % total;
    for (index, weight) in weights.iter().enumerate() {
        let weight = u64::from(*weight);
        if position < weight {
            return index;
        }
        position -= weight;
    }
    0
}

fn circuit_open(state: &RuntimeState, target_id: Uuid) -> bool {
    state
        .health
        .lock()
        .ok()
        .and_then(|mut health| {
            health
                .get_mut(&target_id)
                .map(|target| target.circuit_open(Instant::now()))
        })
        .unwrap_or(false)
}

fn mark_failure(state: &RuntimeState, target_id: Uuid, policy: &RetryPolicy) {
    if let Ok(mut health) = state.health.lock() {
        let target = health.entry(target_id).or_default();
        let previous = target.open_until;
        let now = Instant::now();
        target.fail(policy, now);
        if target.open_until != previous {
            if let Some(until) = target.open_until {
                state.usage.log_diagnostic("warn", format!(
                    "event=proxy.target.cooldown target_id={target_id} cooldown_ms={} reopen_count={}",
                    until.saturating_duration_since(now).as_millis(), target.reopen_count,
                ));
            }
        }
        clear_affinity_for_target(state, target_id);
    }
}

#[cfg(test)]
fn mark_success(state: &RuntimeState, target_id: Uuid) {
    complete_target_success(state, Uuid::nil(), None, target_id, Instant::now(), None);
}

fn is_retryable_status(status: StatusCode) -> bool {
    // A response that reached the upstream can still be a target failure:
    // quota/authentication errors (including 403 insufficient balance),
    // client errors, redirects and server errors all need to advance the
    // fallback chain before anything is committed to the caller.
    status.is_redirection() || status.is_client_error() || status.is_server_error()
}

fn status_affects_circuit(status: StatusCode) -> bool {
    status.is_server_error()
        || matches!(
            status,
            StatusCode::UNAUTHORIZED
                | StatusCode::FORBIDDEN
                | StatusCode::REQUEST_TIMEOUT
                | StatusCode::TOO_MANY_REQUESTS
        )
}

fn build_upstream_headers(
    incoming: &HeaderMap,
    target: &ResolvedTarget,
    protocol: ProxyProtocol,
) -> Result<HeaderMap, String> {
    let incoming_hop_headers = connection_header_names(incoming);
    let anthropic_upstream = protocol == ProxyProtocol::AnthropicMessages;
    let mut headers = HeaderMap::new();
    for (name, value) in incoming.iter() {
        // Local metadata and product identity never belong on provider traffic.
        if is_local_proxy_header(name, value) {
            continue;
        }
        // Anthropic-specific headers are meaningless (and leaking them is
        // confusing) to an OpenAI-wire upstream after conversion.
        if !anthropic_upstream && (name == "anthropic-version" || name == ANTHROPIC_BETA_HEADER) {
            continue;
        }
        if !is_hop_header(name)
            && !incoming_hop_headers.contains(name)
            && name != header::AUTHORIZATION
            && name != "x-api-key"
            && name != "api-key"
            && name != header::ACCEPT_ENCODING
            && name != header::CONTENT_LENGTH
            && name != header::HOST
        {
            headers.append(name.clone(), value.clone());
        }
    }

    let mut configured = HeaderMap::new();
    for (name, value) in &target.config.headers {
        let name = header::HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| format!("invalid configured upstream header name: {name}"))?;
        let mut value = HeaderValue::from_str(value)
            .map_err(|_| format!("invalid value for configured upstream header {name}"))?;
        value.set_sensitive(true);
        configured.append(name, value);
    }
    let configured_hop_headers = connection_header_names(&configured);
    for (name, value) in configured.iter() {
        if !is_local_proxy_header(name, value)
            && !(is_client_identity_header(name) && headers.contains_key(name))
            && !is_hop_header(name)
            && !configured_hop_headers.contains(name)
            && name != header::ACCEPT_ENCODING
            && name != header::CONTENT_LENGTH
            && name != header::CONTENT_TYPE
            && name != header::HOST
        {
            if name == ANTHROPIC_BETA_HEADER && headers.contains_key(name) {
                let merged = merge_anthropic_beta_values(headers.get_all(name).iter(), value)?;
                headers.insert(name.clone(), merged);
            } else {
                headers.insert(name.clone(), value.clone());
            }
        }
    }

    let (auth_name, mut auth_value) = match target.config.auth_scheme.as_str() {
        "bearer" => {
            let mut bearer = format!("Bearer {}", target.api_key);
            let value = HeaderValue::from_str(&bearer)
                .map_err(|_| "invalid bearer credential for upstream request".to_string());
            bearer.zeroize();
            (header::AUTHORIZATION, value?)
        }
        "custom_header" => (
            header::AUTHORIZATION,
            HeaderValue::from_str(&target.api_key)
                .map_err(|_| "invalid custom authorization credential".to_string())?,
        ),
        "x_api_key" => (
            header::HeaderName::from_static("x-api-key"),
            HeaderValue::from_str(&target.api_key)
                .map_err(|_| "invalid x-api-key credential".to_string())?,
        ),
        "azure_api_key" => (
            header::HeaderName::from_static("api-key"),
            HeaderValue::from_str(&target.api_key)
                .map_err(|_| "invalid Azure API credential".to_string())?,
        ),
        scheme => return Err(format!("unsupported proxy authentication scheme: {scheme}")),
    };
    auth_value.set_sensitive(true);
    headers.insert(auth_name, auth_value);
    if !headers.contains_key(header::CONTENT_TYPE) {
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
    }
    if protocol == ProxyProtocol::AnthropicMessages && !headers.contains_key("anthropic-version") {
        headers.insert(
            header::HeaderName::from_static("anthropic-version"),
            HeaderValue::from_static("2023-06-01"),
        );
    }
    Ok(headers)
}

fn is_client_identity_header(name: &header::HeaderName) -> bool {
    matches!(
        name.as_str(),
        "user-agent"
            | "x-user-agent"
            | "originator"
            | "http-referer"
            | "x-title"
            | "x-app-name"
            | "x-client-name"
    )
}

fn is_local_proxy_header(name: &header::HeaderName, value: &HeaderValue) -> bool {
    name.as_str().starts_with("x-aipass-")
        || name.as_str().starts_with("aipass-")
        || (is_client_identity_header(name)
            && value
                .as_bytes()
                .windows(6)
                .any(|part| part.eq_ignore_ascii_case(b"aipass")))
}

const ANTHROPIC_BETA_HEADER: &str = "anthropic-beta";

/// `anthropic-beta` is a comma-separated feature-flag list. Clients such as
/// Claude Code send their own flags while imported OAuth entries configure
/// `oauth-2025-04-20`; merging (incoming first, then configured additions,
/// deduped) keeps both instead of letting the configured value replace the
/// client's list.
fn merge_anthropic_beta_values<'a>(
    incoming: impl Iterator<Item = &'a HeaderValue>,
    configured: &'a HeaderValue,
) -> Result<HeaderValue, String> {
    let mut tokens: Vec<String> = Vec::new();
    for value in incoming.chain(std::iter::once(configured)) {
        let text = value
            .to_str()
            .map_err(|_| "invalid anthropic-beta header value".to_string())?;
        for token in text
            .split(',')
            .map(str::trim)
            .filter(|token| !token.is_empty())
        {
            if !tokens.iter().any(|existing| existing == token) {
                tokens.push(token.to_string());
            }
        }
    }
    let mut merged = HeaderValue::from_str(&tokens.join(", "))
        .map_err(|_| "invalid merged anthropic-beta header value".to_string())?;
    merged.set_sensitive(true);
    Ok(merged)
}

fn connection_header_names(headers: &HeaderMap) -> HashSet<header::HeaderName> {
    headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|name| header::HeaderName::from_bytes(name.trim().as_bytes()).ok())
        .collect()
}

fn is_hop_header(name: &header::HeaderName) -> bool {
    matches!(
        name.as_str(),
        "connection"
            | "keep-alive"
            | "proxy-connection"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}
fn error_response(status: StatusCode, message: &str) -> Response<BoxBody> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(BodyExt::boxed_unsync(
            Full::new(Bytes::from(
                serde_json::json!({"error":{"message":message,"type":"aipass_proxy_error"}})
                    .to_string(),
            ))
            .map_err(|never| -> BoxError { match never {} }),
        ))
        .unwrap()
}
fn set_error(state: &RuntimeState, error: String) {
    // Raw errors can embed upstream URLs, userinfo, headers or response bodies.
    state
        .usage
        .log_diagnostic("error", "event=proxy.runtime.failed".into());
    if let Ok(mut stats) = state.stats.lock() {
        stats.last_error = Some(error);
    }
}

fn record_request(state: &RuntimeState, success: bool, first_token_ms: Option<u64>) {
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

fn record_recent_tokens(state: &RuntimeState, tokens: u64) {
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

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn local_day_start(timestamp: i64, timezone_offset_seconds: i64) -> i64 {
    (timestamp + timezone_offset_seconds).div_euclid(86_400) * 86_400 - timezone_offset_seconds
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    mod concurrency;
    mod image_api;
    mod runtime_status;
    mod stability;
    mod transparency;

    #[test]
    fn route_protocol_scope_keeps_codex_tokens_on_responses_only() {
        let route = ProxyRouteConfig {
            id: Uuid::new_v4(),
            name: "Codex".into(),
            token: "route-token".into(),
            inbound_protocol: ProxyProtocol::OpenAiResponses,
            upstream_protocol: ProxyProtocol::OpenAiResponses,
            conversion_enabled: false,
            strategy: RouteStrategy::Fallback,
            targets: Vec::new(),
            retry: RetryPolicy::default(),
            enabled: true,
        };

        assert!(route_accepts_inbound_protocol(
            &route,
            ProxyProtocol::OpenAiResponses
        ));
        assert!(!route_accepts_inbound_protocol(
            &route,
            ProxyProtocol::OpenAiChatCompletions
        ));
        assert!(!route_accepts_inbound_protocol(
            &route,
            ProxyProtocol::AnthropicMessages
        ));
    }

    #[test]
    fn route_selection_rejects_codex_token_on_other_http_protocols() {
        let token = "codex-responses-token";
        let route = single_target_route(
            token,
            "http://127.0.0.1:1/v1".into(),
            RetryPolicy::default(),
        );
        let proxy = start_proxy(available_addr(), route);
        assert!(select_route(
            &proxy.state,
            Some(token),
            None,
            Some(ProxyProtocol::OpenAiResponses)
        )
        .is_some());
        assert!(select_route(
            &proxy.state,
            Some(token),
            None,
            Some(ProxyProtocol::OpenAiChatCompletions)
        )
        .is_none());
        assert!(select_route(
            &proxy.state,
            Some(token),
            None,
            Some(ProxyProtocol::AnthropicMessages)
        )
        .is_none());
    }

    #[test]
    fn in_flight_guard_tracks_nested_request_lifetimes() {
        let counter = Arc::new(AtomicU64::new(0));
        {
            let _first = InFlightGuard::new(counter.clone());
            assert_eq!(counter.load(Ordering::Relaxed), 1);
            {
                let _second = InFlightGuard::new(counter.clone());
                assert_eq!(counter.load(Ordering::Relaxed), 2);
            }
            assert_eq!(counter.load(Ordering::Relaxed), 1);
        }
        assert_eq!(counter.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn proxy_config_defaults_upstream_proxy_to_system_for_legacy_json() {
        let config: ProxyConfig = serde_json::from_str(
            r#"{"enabled":true,"bindAddr":"127.0.0.1:8787","routes":[],"pricing":[]}"#,
        )
        .expect("legacy config without upstreamProxy still deserializes");
        assert_eq!(config.upstream_proxy.mode, UpstreamProxyMode::System);
        assert_eq!(config.upstream_proxy.custom_url, None);
    }

    #[test]
    fn upstream_proxy_config_serde_roundtrip() {
        let config = UpstreamProxyConfig {
            mode: UpstreamProxyMode::Custom,
            custom_url: Some("http://user:pass@127.0.0.1:7890".into()),
        };
        let json = serde_json::to_string(&config).unwrap();
        assert_eq!(
            json,
            r#"{"mode":"custom","customUrl":"http://user:pass@127.0.0.1:7890"}"#
        );
        let parsed: UpstreamProxyConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, config);
    }

    #[test]
    fn apply_upstream_proxy_rejects_custom_mode_without_url() {
        let builder = reqwest::Client::builder();
        let result = apply_upstream_proxy(
            builder,
            &UpstreamProxyConfig {
                mode: UpstreamProxyMode::Custom,
                custom_url: None,
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn apply_upstream_proxy_rejects_invalid_custom_url() {
        let builder = reqwest::Client::builder();
        let result = apply_upstream_proxy(
            builder,
            &UpstreamProxyConfig {
                mode: UpstreamProxyMode::Custom,
                custom_url: Some("not a url".into()),
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn apply_upstream_proxy_accepts_valid_modes() {
        for config in [
            UpstreamProxyConfig::default(),
            UpstreamProxyConfig {
                mode: UpstreamProxyMode::Direct,
                custom_url: None,
            },
            UpstreamProxyConfig {
                mode: UpstreamProxyMode::Environment,
                custom_url: None,
            },
            UpstreamProxyConfig {
                mode: UpstreamProxyMode::Custom,
                custom_url: Some("socks5://127.0.0.1:1080".into()),
            },
        ] {
            let builder = reqwest::Client::builder();
            let builder = apply_upstream_proxy(builder, &config).expect("valid proxy config");
            builder.build().expect("client builds");
        }
    }

    #[tokio::test]
    async fn custom_upstream_proxy_routes_http_traffic_through_proxy() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = listener.local_addr().unwrap();
        let capture = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 1024];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let read = socket.read(&mut chunk).await.unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
            }
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .await
                .unwrap();
            String::from_utf8_lossy(&request).to_string()
        });

        let builder = apply_upstream_proxy(
            reqwest::Client::builder(),
            &UpstreamProxyConfig {
                mode: UpstreamProxyMode::Custom,
                custom_url: Some(format!("http://{proxy_addr}")),
            },
        )
        .expect("custom proxy config");
        let body = builder
            .build()
            .unwrap()
            .get("http://example.com/upstream")
            .send()
            .await
            .expect("request through proxy")
            .text()
            .await
            .unwrap();
        assert_eq!(body, "ok");
        let request = capture.await.unwrap();
        assert!(
            request.starts_with("GET http://example.com/upstream"),
            "proxy received an absolute-URI request, got: {request:?}"
        );
    }

    fn available_addr() -> SocketAddr {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        addr
    }

    fn single_target_route(token: &str, base_url: String, retry: RetryPolicy) -> ResolvedRoute {
        ResolvedRoute {
            config: ProxyRouteConfig {
                id: Uuid::new_v4(),
                name: "test".into(),
                token: String::new(),
                inbound_protocol: ProxyProtocol::OpenAiResponses,
                upstream_protocol: ProxyProtocol::OpenAiResponses,
                conversion_enabled: false,
                strategy: RouteStrategy::Fallback,
                targets: Vec::new(),
                retry,
                enabled: true,
            },
            local_token: token.into(),
            targets: vec![test_target(base_url, 0)],
        }
    }

    pub(crate) fn test_target(base_url: String, priority: u16) -> ResolvedTarget {
        ResolvedTarget {
            // These fixtures exercise the HTTP retry/streaming pipeline.
            // WS preference and fallback have dedicated adaptive WS tests.
            max_concurrent_requests: None,
            supports_websockets: false,
            config: ProxyTargetConfig {
                id: Uuid::new_v4(),
                provider_entry_id: Uuid::new_v4(),
                secret_id: "primary".into(),
                label: "primary".into(),
                base_url,
                auth_scheme: "bearer".into(),
                headers: Vec::new(),
                group: None,
                priority,
                weight: 1,
                enabled: true,
                protocol: None,
            },
            api_key: "upstream-secret".into(),
        }
    }

    fn fallback_route(token: &str, upstreams: &[SocketAddr], retry: RetryPolicy) -> ResolvedRoute {
        let mut route = single_target_route(token, String::new(), retry);
        route.targets = upstreams
            .iter()
            .enumerate()
            .map(|(index, addr)| test_target(format!("http://{addr}/v1"), index as u16))
            .collect();
        route
    }

    fn read_http_request(stream: &mut std::net::TcpStream) -> (String, Vec<u8>) {
        let mut received = Vec::new();
        let mut buffer = [0_u8; 8192];
        let header_end = loop {
            let read = stream.read(&mut buffer).unwrap();
            assert!(
                read > 0,
                "connection closed before request headers completed"
            );
            received.extend_from_slice(&buffer[..read]);
            if let Some(index) = received.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let headers = String::from_utf8(received[..header_end].to_vec()).unwrap();
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap_or_default();
        while received.len() - header_end < content_length {
            let read = stream.read(&mut buffer).unwrap();
            assert!(read > 0, "connection closed before request body completed");
            received.extend_from_slice(&buffer[..read]);
        }
        (
            headers,
            received[header_end..header_end + content_length].to_vec(),
        )
    }

    #[test]
    fn plaintext_tokens_use_constant_time_comparison() {
        assert!(tokens_match("local-test-token", "local-test-token"));
        assert!(!tokens_match("local-test-token", "other"));
        assert!(!tokens_match("local-test-token", "local-test-token-longer"));

        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer wrong-token"),
        );
        headers.insert("x-api-key", HeaderValue::from_static("local-test-token"));
        let (bearer, api_key) = local_proxy_tokens(&headers);
        assert_eq!(bearer, Some("wrong-token"));
        assert_eq!(api_key, Some("local-test-token"));
        assert!(
            bearer.is_some_and(|token| tokens_match("local-test-token", token))
                || api_key.is_some_and(|token| tokens_match("local-test-token", token))
        );

        headers.insert(header::AUTHORIZATION, HeaderValue::from_static("Bearer "));
        headers.insert("x-api-key", HeaderValue::from_static(""));
        assert_eq!(local_proxy_tokens(&headers), (None, None));
    }

    #[test]
    fn every_upstream_error_status_is_eligible_for_failover() {
        assert!(is_retryable_status(StatusCode::BAD_REQUEST));
        assert!(is_retryable_status(StatusCode::NOT_FOUND));
        assert!(is_retryable_status(StatusCode::UNPROCESSABLE_ENTITY));
        assert!(is_retryable_status(StatusCode::FORBIDDEN));
        assert!(is_retryable_status(StatusCode::INTERNAL_SERVER_ERROR));
        assert!(is_retryable_status(StatusCode::MOVED_PERMANENTLY));
        assert!(!is_retryable_status(StatusCode::OK));
        assert!(!status_affects_circuit(StatusCode::BAD_REQUEST));
        assert!(!status_affects_circuit(StatusCode::NOT_FOUND));
        assert!(status_affects_circuit(StatusCode::UNAUTHORIZED));
        assert!(status_affects_circuit(StatusCode::TOO_MANY_REQUESTS));
        assert!(status_affects_circuit(StatusCode::BAD_GATEWAY));
    }

    #[test]
    fn connection_declared_headers_are_treated_as_hop_by_hop() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            header::CONNECTION,
            http::HeaderValue::from_static("keep-alive, x-internal-hop"),
        );
        headers.append(
            header::CONNECTION,
            http::HeaderValue::from_static("x-second-hop"),
        );

        let declared = connection_header_names(&headers);

        assert!(declared.contains(&header::HeaderName::from_static("x-internal-hop")));
        assert!(declared.contains(&header::HeaderName::from_static("x-second-hop")));
        assert!(!declared.contains(&header::CONTENT_TYPE));

        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer local-token"),
        );
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"));
        headers.insert(header::ACCEPT_ENCODING, HeaderValue::from_static("gzip"));
        headers.insert(
            header::HeaderName::from_static("x-internal-hop"),
            HeaderValue::from_static("must-not-forward"),
        );
        headers.insert(
            header::HeaderName::from_static("x-end-to-end"),
            HeaderValue::from_static("keep"),
        );
        let mut target = test_target("https://api.example.test/v1".into(), 0);
        target.config.headers = vec![
            ("connection".into(), "x-target-hop".into()),
            ("x-target-hop".into(), "must-not-forward".into()),
            ("content-type".into(), "application/octet-stream".into()),
        ];

        let forwarded =
            build_upstream_headers(&headers, &target, ProxyProtocol::OpenAiResponses).unwrap();

        assert!(!forwarded.contains_key("x-internal-hop"));
        assert!(!forwarded.contains_key("x-target-hop"));
        assert!(!forwarded.contains_key(header::ACCEPT_ENCODING));
        assert_eq!(forwarded["x-end-to-end"], "keep");
        assert_eq!(forwarded[header::CONTENT_TYPE], "text/plain");
        assert_eq!(forwarded[header::AUTHORIZATION], "Bearer upstream-secret");

        headers.remove(header::CONTENT_TYPE);
        let forwarded =
            build_upstream_headers(&headers, &target, ProxyProtocol::OpenAiResponses).unwrap();
        assert_eq!(forwarded[header::CONTENT_TYPE], "application/json");
    }

    #[test]
    fn sse_prefetch_classifies_heartbeats_errors_and_completion_markers() {
        assert!(sse_event_is_heartbeat(
            b"event: ping\ndata: {\"type\":\"ping\"}\n\n"
        ));
        assert!(sse_event_reports_error(
            b"event: response.failed\ndata: {\"type\":\"response.failed\"}\n\n"
        ));
        assert!(stream_reports_completion(
            ProxyProtocol::OpenAiResponses,
            b"event: response.completed\ndata: {\"type\":\"response.completed\"}\n\n"
        ));
        assert!(stream_reports_completion(
            ProxyProtocol::OpenAiChatCompletions,
            b"data: [DONE]\n\n"
        ));
        assert!(sse_event_reports_completion(
            ProxyProtocol::OpenAiChatCompletions,
            b"data: {\"choices\":[{\"finish_reason\":\"stop\"}]}\n\n"
        ));
        assert!(!sse_event_is_terminal(
            ProxyProtocol::OpenAiChatCompletions,
            b"data: {\"choices\":[{\"finish_reason\":\"stop\"}]}\n\n"
        ));
        assert!(sse_event_is_terminal(
            ProxyProtocol::OpenAiChatCompletions,
            b"data: [DONE]\n\n"
        ));
        assert!(stream_reports_completion(
            ProxyProtocol::AnthropicMessages,
            b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
        ));
        assert!(!stream_reports_completion(
            ProxyProtocol::OpenAiResponses,
            b"data: {\"type\":\"response.output_text.delta\"}\n\n"
        ));
        assert!(!sse_event_reports_output(
            ProxyProtocol::OpenAiResponses,
            b"data: {\"type\":\"response.created\"}\n\n"
        ));
        assert!(sse_event_reports_output(
            ProxyProtocol::OpenAiResponses,
            b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n"
        ));
        assert!(!sse_event_reports_output(
            ProxyProtocol::OpenAiChatCompletions,
            b"data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n"
        ));
        assert!(sse_event_reports_output(
            ProxyProtocol::OpenAiChatCompletions,
            b"data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n"
        ));
        assert!(!sse_event_reports_output(
            ProxyProtocol::AnthropicMessages,
            b"event: message_start\ndata: {\"type\":\"message_start\"}\n\n"
        ));
        assert!(sse_event_reports_output(
            ProxyProtocol::AnthropicMessages,
            b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n"
        ));
    }

    #[test]
    fn streaming_usage_is_extracted_incrementally_from_nested_events() {
        let anthropic = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":12,\"cache_read_input_tokens\":4}}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":7}}\n\n"
        )
        .as_bytes();
        let mut buffer = Vec::new();
        let mut usage = TokenUsage::default();
        for chunk in anthropic.chunks(23) {
            observe_sse_usage(
                ProxyProtocol::AnthropicMessages,
                &mut buffer,
                chunk,
                &mut usage,
            );
        }
        assert_eq!(usage.input_tokens, 12);
        assert_eq!(usage.output_tokens, 7);
        assert_eq!(usage.cache_read_tokens, 4);

        let chat = concat!(
            "data: {\"choices\":[{\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":4}}\n\n",
            "data: [DONE]\n\n"
        )
        .as_bytes();
        let mut buffer = Vec::new();
        let mut usage = TokenUsage::default();
        let mut completed = false;
        let mut terminal = false;
        for chunk in chat.chunks(19) {
            let signals = observe_sse_usage(
                ProxyProtocol::OpenAiChatCompletions,
                &mut buffer,
                chunk,
                &mut usage,
            );
            completed |= signals.completed;
            terminal |= signals.terminal;
        }
        assert!(completed);
        assert!(terminal);
        assert_eq!(usage.input_tokens, 9);
        assert_eq!(usage.output_tokens, 4);

        let responses = usage_from_wire_bytes(
            ProxyProtocol::OpenAiResponses,
            b"data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":20,\"output_tokens\":5,\"input_tokens_details\":{\"cached_tokens\":3}}}}\n\n",
        );
        assert_eq!(responses.input_tokens, 17);
        assert_eq!(responses.output_tokens, 5);
        assert_eq!(responses.cache_read_tokens, 3);
    }

    #[test]
    fn chat_stream_requests_enable_usage_reporting() {
        let payload = Bytes::from_static(
            br#"{"model":"gpt-test","stream":true,"stream_options":{"custom":true}}"#,
        );
        let updated =
            request_stream_usage(ProxyProtocol::OpenAiChatCompletions, true, payload.clone());
        let value: serde_json::Value = serde_json::from_slice(&updated).unwrap();
        assert_eq!(value["stream_options"]["include_usage"], true);
        assert_eq!(value["stream_options"]["custom"], true);
        assert_eq!(
            request_stream_usage(ProxyProtocol::OpenAiResponses, true, payload.clone()),
            payload
        );
        assert_eq!(
            request_stream_usage(
                ProxyProtocol::OpenAiChatCompletions,
                false,
                Bytes::from_static(br#"{"stream":false}"#),
            ),
            Bytes::from_static(br#"{"stream":false}"#)
        );
    }

    #[test]
    fn degraded_targets_follow_recent_failures_circuits_and_recovery() {
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let retry = RetryPolicy::default();
        let mut route = single_target_route(
            "aipass_health_status_test",
            "http://127.0.0.1:1/v1".into(),
            retry.clone(),
        );
        let target_id = route.targets[0].config.id;
        let mut disabled_target = test_target("http://127.0.0.1:2/v1".into(), 1);
        disabled_target.config.enabled = false;
        let disabled_target_id = disabled_target.config.id;
        route.targets.push(disabled_target);
        let mut disabled_route = single_target_route(
            "aipass_disabled_health_status_test",
            "http://127.0.0.1:3/v1".into(),
            retry.clone(),
        );
        disabled_route.config.enabled = false;
        let disabled_route_target_id = disabled_route.targets[0].config.id;
        let proxy = ProxyHandle::start(
            RuntimeConfig::from_routes("127.0.0.1:0", vec![route, disabled_route]),
            usage,
        )
        .unwrap();

        assert!(!proxy.status().degraded);
        for id in [
            target_id,
            disabled_target_id,
            disabled_route_target_id,
            Uuid::new_v4(),
        ] {
            mark_failure(&proxy.state, id, &retry);
        }
        assert!(proxy.status().degraded);
        assert_eq!(proxy.status().degraded_target_ids, vec![target_id]);

        // Recovery belongs to the affected target; a healthy fallback cannot clear it.
        mark_success(&proxy.state, disabled_target_id);
        assert_eq!(proxy.status().degraded_target_ids, vec![target_id]);
        mark_success(&proxy.state, target_id);
        mark_success(&proxy.state, target_id);
        assert!(!proxy.status().degraded);

        mark_failure(&proxy.state, target_id, &retry);
        {
            let mut health = proxy.state.health.lock().unwrap();
            let target = health.get_mut(&target_id).unwrap();
            target.last_failure_at = Some(Instant::now() - Duration::from_secs(61));
        }
        assert_eq!(proxy.status().degraded_target_ids, vec![target_id]);
        assert!(proxy.status().degraded);
        {
            let mut health = proxy.state.health.lock().unwrap();
            health.get_mut(&target_id).unwrap().open_until =
                Some(Instant::now() + Duration::from_secs(120));
        }
        assert_eq!(proxy.status().degraded_target_ids, vec![target_id]);
        {
            let mut health = proxy.state.health.lock().unwrap();
            health.get_mut(&target_id).unwrap().open_until =
                Some(Instant::now() - Duration::from_secs(1));
        }
        assert!(proxy.status().degraded);

        // The additive status field remains compatible with older serialized statuses.
        let mut legacy = serde_json::to_value(proxy.status()).unwrap();
        legacy.as_object_mut().unwrap().remove("degradedTargetIds");
        assert!(serde_json::from_value::<ProxyStatus>(legacy)
            .unwrap()
            .degraded_target_ids
            .is_empty());
    }

    #[test]
    fn circuit_recovery_requires_consecutive_successes() {
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let retry = RetryPolicy {
            failure_threshold: 1,
            circuit_open_seconds: 60,
            ..RetryPolicy::default()
        };
        let route = single_target_route(
            "aipass_recovery_threshold_test",
            "http://127.0.0.1:1/v1".into(),
            retry.clone(),
        );
        let target_id = route.targets[0].config.id;
        let proxy = ProxyHandle::start(
            RuntimeConfig::from_routes("127.0.0.1:0", vec![route]),
            usage,
        )
        .unwrap();

        mark_failure(&proxy.state, target_id, &retry);
        {
            let mut health = proxy.state.health.lock().unwrap();
            health.get_mut(&target_id).unwrap().open_until =
                Some(Instant::now() - Duration::from_secs(1));
        }
        assert!(!circuit_open(&proxy.state, target_id));
        mark_success(&proxy.state, target_id);
        assert!(proxy.state.health.lock().unwrap().contains_key(&target_id));
        mark_success(&proxy.state, target_id);
        assert!(!proxy.state.health.lock().unwrap()[&target_id].degraded());
    }

    #[test]
    fn runtime_config_update_resets_circuit_and_round_robin_state() {
        let bind_addr = available_addr();
        let dead_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let retry = RetryPolicy {
            failure_threshold: 1,
            circuit_open_seconds: 60,
            ..RetryPolicy::default()
        };
        let route = single_target_route(
            "aipass_runtime_reset_test",
            format!("http://{dead_addr}/v1"),
            retry.clone(),
        );
        let route_id = route.config.id;
        let target_id = route.targets[0].config.id;
        let proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage,
        )
        .unwrap();

        mark_failure(&proxy.state, target_id, &retry);
        let _ = round_robin_start(&proxy.state, route_id, &[1]);
        remember_affinity_target(&proxy.state, route_id, Some("session"), target_id);
        assert!(circuit_open(&proxy.state, target_id));
        assert!(!proxy.state.rr_counters.lock().unwrap().is_empty());
        assert!(!proxy.state.session_affinity.lock().unwrap().is_empty());
        assert_eq!(proxy.status().degraded_target_ids, vec![target_id]);

        let mut replacement = single_target_route(
            "aipass_runtime_reset_test",
            format!("http://{dead_addr}/v1"),
            retry,
        );
        replacement.config.id = route_id;
        replacement.targets[0].config.id = target_id;
        proxy
            .update_config(RuntimeConfig::from_routes(
                bind_addr.to_string(),
                vec![replacement],
            ))
            .unwrap();

        assert!(!circuit_open(&proxy.state, target_id));
        assert!(proxy.state.rr_counters.lock().unwrap().is_empty());
        assert!(proxy.state.session_affinity.lock().unwrap().is_empty());
        assert!(proxy.status().degraded_target_ids.is_empty());
        assert!(!proxy.status().degraded);
    }

    #[test]
    fn upstream_url_does_not_duplicate_v1() {
        assert_eq!(
            upstream_url("https://api.example.test/v1", "/v1/messages").unwrap(),
            "https://api.example.test/v1/messages"
        );
    }

    #[test]
    fn upstream_url_adds_v1_for_root_endpoint() {
        assert_eq!(
            upstream_url("https://api.example.test", "/v1/messages").unwrap(),
            "https://api.example.test/v1/messages"
        );
    }

    #[test]
    fn upstream_url_strips_v1_for_chatgpt_codex_backend() {
        assert_eq!(
            upstream_url("https://chatgpt.com/backend-api/codex", "/v1/responses").unwrap(),
            "https://chatgpt.com/backend-api/codex/responses"
        );
    }

    #[test]
    fn upstream_url_preserves_azure_query_without_v1_path() {
        assert_eq!(
            upstream_url_with_query(
                "https://example.openai.azure.com/openai/deployments/gpt?api-version=2024-10-21",
                "/chat/completions",
                Some("trace=enabled"),
            )
            .unwrap(),
            "https://example.openai.azure.com/openai/deployments/gpt/chat/completions?api-version=2024-10-21&trace=enabled"
        );
    }

    fn beta_target(configured_beta: Option<&str>) -> ResolvedTarget {
        let mut target = test_target("https://api.anthropic.com".into(), 0);
        if let Some(value) = configured_beta {
            target
                .config
                .headers
                .push(("anthropic-beta".into(), value.into()));
        }
        target
    }

    fn beta_header(headers: &HeaderMap) -> String {
        headers
            .get("anthropic-beta")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string()
    }

    #[test]
    fn anthropic_beta_incoming_only_is_preserved() {
        let mut incoming = HeaderMap::new();
        incoming.insert(
            "anthropic-beta",
            HeaderValue::from_static("fine-grained-tool-results-2025-05-14"),
        );
        let target = beta_target(None);

        let headers =
            build_upstream_headers(&incoming, &target, ProxyProtocol::AnthropicMessages).unwrap();

        assert_eq!(
            beta_header(&headers),
            "fine-grained-tool-results-2025-05-14"
        );
    }

    #[test]
    fn anthropic_beta_configured_only_is_applied() {
        let incoming = HeaderMap::new();
        let target = beta_target(Some("oauth-2025-04-20"));

        let headers =
            build_upstream_headers(&incoming, &target, ProxyProtocol::AnthropicMessages).unwrap();

        assert_eq!(beta_header(&headers), "oauth-2025-04-20");
    }

    #[test]
    fn anthropic_beta_incoming_and_configured_are_merged_and_deduped() {
        let mut incoming = HeaderMap::new();
        incoming.insert(
            "anthropic-beta",
            HeaderValue::from_static("claude-code-20250219, oauth-2025-04-20"),
        );
        let target = beta_target(Some("oauth-2025-04-20, interleaved-thinking-2025-05-14"));

        let headers =
            build_upstream_headers(&incoming, &target, ProxyProtocol::AnthropicMessages).unwrap();

        assert_eq!(
            beta_header(&headers),
            "claude-code-20250219, oauth-2025-04-20, interleaved-thinking-2025-05-14"
        );
    }

    #[test]
    fn unrelated_configured_headers_still_replace_incoming() {
        let mut incoming = HeaderMap::new();
        incoming.insert("x-custom-flag", HeaderValue::from_static("incoming"));
        let mut target = beta_target(Some("oauth-2025-04-20"));
        target
            .config
            .headers
            .push(("x-custom-flag".into(), "configured".into()));

        let headers =
            build_upstream_headers(&incoming, &target, ProxyProtocol::AnthropicMessages).unwrap();

        assert_eq!(
            headers
                .get("x-custom-flag")
                .and_then(|value| value.to_str().ok()),
            Some("configured")
        );
    }

    #[test]
    fn model_discovery_uses_route_token_and_adds_cursor_protocol_metadata() {
        let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let upstream_addr = upstream.local_addr().unwrap();
        let upstream_thread = std::thread::spawn(move || {
            let (mut stream, _) = upstream.accept().unwrap();
            let (headers, body) = read_http_request(&mut stream);
            assert!(headers.starts_with("GET /v1/models HTTP/1.1\r\n"));
            assert!(headers
                .lines()
                .any(|line| line.eq_ignore_ascii_case("authorization: Bearer upstream-secret")));
            assert!(body.is_empty());
            let response = r#"{"object":"list","data":[{"id":"local-model","object":"model"}]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response.len(),
                response
            )
            .unwrap();
        });

        let bind_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let token = "aipass_cursor_models_test";
        let mut route = single_target_route(
            token,
            format!("http://{upstream_addr}/v1"),
            RetryPolicy::default(),
        );
        route.config.inbound_protocol = ProxyProtocol::OpenAiChatCompletions;
        route.config.upstream_protocol = ProxyProtocol::OpenAiChatCompletions;
        let _proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage,
        )
        .unwrap();

        let response = reqwest::blocking::Client::new()
            .get(format!("http://{bind_addr}/v1/models"))
            .bearer_auth(token)
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let payload: serde_json::Value = response.json().unwrap();
        assert_eq!(payload["data"][0]["id"], "local-model");
        assert_eq!(payload["data"][0]["api_types"][0], "chat_completions");
        assert_eq!(
            payload["data"][0]["capabilities"]["supports_tool_use"],
            true
        );
        upstream_thread.join().unwrap();
    }

    #[test]
    fn model_discovery_empty_and_error_success_bodies_fail_over() {
        for body in ["", r#"{"error":{"message":"quota exhausted"}}"#] {
            let (bad_addr, bad) = mock_upstream("200 OK", "application/json", body.into());
            let (good_addr, good) = mock_upstream(
                "200 OK",
                "application/json",
                r#"{"object":"list","data":[{"id":"fallback","object":"model"}]}"#.into(),
            );
            let token = "models_payload_fallback";
            let route = fallback_route(token, &[bad_addr, good_addr], RetryPolicy::default());
            let failed_id = route.targets[0].config.id;
            let bind_addr = available_addr();
            let proxy = start_proxy(bind_addr, route);
            let response = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap()
                .get(format!("http://{bind_addr}/v1/models"))
                .bearer_auth(token)
                .send()
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let payload: serde_json::Value = response.json().unwrap();
            assert_eq!(payload["data"][0]["id"], "fallback");
            assert!(proxy.status().degraded_target_ids.contains(&failed_id));
            bad.join().unwrap();
            good.join().unwrap();
        }
    }

    #[test]
    fn model_discovery_not_found_does_not_pollute_proxy_health() {
        let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let upstream_addr = upstream.local_addr().unwrap();
        let upstream_thread = std::thread::spawn(move || {
            let (mut stream, _) = upstream.accept().unwrap();
            let (headers, body) = read_http_request(&mut stream);
            assert!(headers.starts_with("GET /v1/models HTTP/1.1\r\n"));
            assert!(body.is_empty());
            write!(
                stream,
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
        });

        let bind_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let token = "aipass_models_not_found_health_test";
        let route = single_target_route(
            token,
            format!("http://{upstream_addr}/v1"),
            RetryPolicy::default(),
        );
        let proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage,
        )
        .unwrap();

        let response = reqwest::blocking::Client::new()
            .get(format!("http://{bind_addr}/v1/models"))
            .bearer_auth(token)
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(proxy.status().failures, 0);
        assert_eq!(proxy.status().last_error, None);
        upstream_thread.join().unwrap();
    }

    #[test]
    fn local_health_endpoint_does_not_forward_to_upstream() {
        let bind_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let route = single_target_route(
            "aipass_health_endpoint_test",
            "http://127.0.0.1:1/v1".into(),
            RetryPolicy::default(),
        );
        let proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage,
        )
        .unwrap();

        let response = reqwest::blocking::Client::new()
            .get(format!("http://{bind_addr}/health"))
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.json::<serde_json::Value>().unwrap();
        assert_eq!(body["status"], "ok");
        assert_eq!(body["service"], "aipass-proxy");
        assert_eq!(body["activeRoutes"], 1);
        assert_eq!(proxy.status().requests, 0);
        assert_eq!(proxy.status().failures, 0);
        assert_eq!(proxy.status().last_error, None);
    }

    #[test]
    fn usage_store_persists_records() {
        let temp = tempfile::tempdir().unwrap();
        let store = UsageStore::open(temp.path().join("usage.sqlite")).unwrap();
        store
            .record(&UsageRecord {
                id: Uuid::new_v4(),
                started_at: 1,
                duration_ms: 2,
                first_token_ms: None,
                route_id: Uuid::new_v4(),
                provider_entry_id: Uuid::new_v4(),
                secret_id: "key".into(),
                model: Some("gpt".into()),
                inbound_protocol: ProxyProtocol::OpenAiResponses,
                upstream_protocol: ProxyProtocol::OpenAiResponses,
                status: 200,
                attempts: 1,
                input_tokens: 1,
                output_tokens: 2,
                cache_read_tokens: 3,
                cache_creation_tokens: 4,
                estimated_cost_micros: 5,
            })
            .unwrap();
        assert_eq!(store.count().unwrap(), 1);
    }

    #[test]
    fn usage_store_can_clear_records_without_reopening_database() {
        let temp = tempfile::tempdir().unwrap();
        let store = UsageStore::open(temp.path().join("usage.sqlite")).unwrap();
        store
            .record(&UsageRecord {
                id: Uuid::new_v4(),
                started_at: 1,
                duration_ms: 2,
                first_token_ms: None,
                route_id: Uuid::new_v4(),
                provider_entry_id: Uuid::new_v4(),
                secret_id: "key".into(),
                model: None,
                inbound_protocol: ProxyProtocol::OpenAiResponses,
                upstream_protocol: ProxyProtocol::OpenAiResponses,
                status: 200,
                attempts: 1,
                input_tokens: 1,
                output_tokens: 1,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                estimated_cost_micros: 0,
            })
            .unwrap();
        store.clear().unwrap();
        assert_eq!(store.count().unwrap(), 0);
        store.clear().unwrap();
    }

    #[test]
    fn start_reports_bind_conflicts() {
        let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let result = ProxyHandle::start(
            RuntimeConfig::from_routes(occupied.local_addr().unwrap().to_string(), Vec::new()),
            usage,
        );
        assert!(matches!(result, Err(ProxyError::InvalidConfig(_))));
    }

    #[tokio::test]
    async fn request_body_spills_to_disk_and_remains_replayable() {
        let chunks = stream::iter(vec![
            Ok::<Bytes, std::io::Error>(Bytes::from_static(b"1234")),
            Ok(Bytes::from_static(b"56789")),
        ]);
        let body = read_replayable_request_chunks(chunks, 16, 4).await.unwrap();
        assert!(matches!(body, ReplayableRequestBody::File { .. }));
        assert_eq!(body.len(), 9);

        let first = body
            .request_body()
            .await
            .unwrap()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let second = body
            .request_body()
            .await
            .unwrap()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        assert_eq!(first, Bytes::from_static(b"123456789"));
        assert_eq!(second, first);
    }

    #[tokio::test]
    async fn request_body_limit_is_enforced_while_reading() {
        let chunks = stream::iter(vec![
            Ok::<Bytes, std::io::Error>(Bytes::from_static(b"1234")),
            Ok(Bytes::from_static(b"56789")),
        ]);
        let result = read_replayable_request_chunks(chunks, 8, 4).await;
        assert!(matches!(result, Err(RequestBodyReadError::TooLarge)));
    }

    #[test]
    fn responses_image_inputs_are_forwarded_without_rewriting() {
        for (case, image) in [
            (
                "url",
                serde_json::json!({
                    "type": "input_image",
                    "image_url": "https://example.test/image.png",
                    "detail": "high"
                }),
            ),
            (
                "base64",
                serde_json::json!({
                    "type": "input_image",
                    "image_url": "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAAB",
                    "detail": "auto"
                }),
            ),
        ] {
            let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let upstream_addr = upstream.local_addr().unwrap();
            let expected = serde_json::json!({
                "model": "gpt-vision-test",
                "input": [{
                    "role": "user",
                    "content": [
                        {"type": "input_text", "text": "describe this image"},
                        image
                    ]
                }]
            });
            let expected_upstream = expected.clone();
            let upstream_thread = std::thread::spawn(move || {
                let (mut stream, _) = upstream.accept().unwrap();
                let (headers, body) = read_http_request(&mut stream);
                assert!(headers
                    .lines()
                    .any(|line| line.eq_ignore_ascii_case("content-type: application/json")));
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
                    expected_upstream
                );
                let response = r#"{"id":"resp_test","status":"completed","output":[]}"#;
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response.len(),
                    response
                )
                .unwrap();
            });

            let bind_addr = available_addr();
            let temp = tempfile::tempdir().unwrap();
            let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
            let token = format!("aipass_image_passthrough_{case}");
            let route = single_target_route(
                &token,
                format!("http://{upstream_addr}/v1"),
                RetryPolicy::default(),
            );
            let _proxy = ProxyHandle::start(
                RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
                usage,
            )
            .unwrap();

            let response = reqwest::blocking::Client::new()
                .post(format!("http://{bind_addr}/v1/responses"))
                .bearer_auth(&token)
                .json(&expected)
                .send()
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            upstream_thread.join().unwrap();
        }
    }

    #[test]
    fn unsupported_multimodal_primary_fails_over_with_the_same_request() {
        let base64_image = "A".repeat(REQUEST_BODY_MEMORY_THRESHOLD + 1024);
        let request = serde_json::json!({
            "model": "gpt-vision-test",
            "input": [{
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "describe this image"},
                    {
                        "type": "input_image",
                        "image_url": format!("data:image/png;base64,{base64_image}")
                    }
                ]
            }]
        });

        let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let primary_addr = primary.local_addr().unwrap();
        let primary_request = request.clone();
        let primary_thread = std::thread::spawn(move || {
            let (mut stream, _) = primary.accept().unwrap();
            let (_, body) = read_http_request(&mut stream);
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
                primary_request
            );
            let response = r#"{"error":{"message":"Unsupported content type","type":"invalid_request_error"}}"#;
            write!(
                stream,
                "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response.len(),
                response
            )
            .unwrap();
        });

        let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let fallback_addr = fallback.local_addr().unwrap();
        let fallback_request = request.clone();
        let fallback_thread = std::thread::spawn(move || {
            let (mut stream, _) = fallback.accept().unwrap();
            let (_, body) = read_http_request(&mut stream);
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
                fallback_request
            );
            let response = r#"{"source":"fallback","status":"completed"}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response.len(),
                response
            )
            .unwrap();
        });

        let bind_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let token = "aipass_multimodal_fallback_test";
        let route = fallback_route(
            token,
            &[primary_addr, fallback_addr],
            RetryPolicy {
                max_attempts: 2,
                ..RetryPolicy::default()
            },
        );
        let _proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage,
        )
        .unwrap();

        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/responses"))
            .bearer_auth(token)
            .json(&request)
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let value = response.json::<serde_json::Value>().unwrap();
        assert_eq!(value["source"], "fallback");
        assert!(!value.to_string().contains("Unsupported content type"));
        primary_thread.join().unwrap();
        fallback_thread.join().unwrap();
    }

    #[test]
    fn forbidden_insufficient_balance_fails_over_before_returning_an_empty_response() {
        let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let primary_addr = primary.local_addr().unwrap();
        let primary_thread = std::thread::spawn(move || {
            let (mut stream, _) = primary.accept().unwrap();
            let _ = read_http_request(&mut stream);
            let body = r#"{"error":{"message":"insufficient balance: upstream-secret", "code":"balance_exhausted"},"input":"private prompt"}"#;
            write!(
                stream,
                "HTTP/1.1 403 Forbidden\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let fallback_addr = fallback.local_addr().unwrap();
        let fallback_thread = std::thread::spawn(move || {
            let (mut stream, _) = fallback.accept().unwrap();
            let _ = read_http_request(&mut stream);
            let body = r#"{"status":"completed","source":"fallback"}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        let bind_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let token = "aipass_forbidden_balance_fallback_test";
        let route = fallback_route(
            token,
            &[primary_addr, fallback_addr],
            RetryPolicy {
                max_attempts: 2,
                ..RetryPolicy::default()
            },
        );
        let provider_id = route.targets[0].config.provider_entry_id;
        let _proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage.clone(),
        )
        .unwrap();

        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/responses"))
            .bearer_auth(token)
            .json(&serde_json::json!({"model":"balance-test","input":[]}))
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.text().unwrap();
        assert!(body.contains("fallback"));
        assert!(!body.contains("insufficient balance"));
        let logs = usage.logs().unwrap();
        let error = logs
            .iter()
            .find(|entry| entry.message.contains("event=proxy.upstream.rejected"))
            .unwrap();
        assert!(error
            .message
            .contains(&format!("provider_id={provider_id}")));
        assert!(error.message.contains("status=403"));
        assert!(error.message.contains("insufficient balance"));
        assert!(error.message.contains("balance_exhausted"));
        assert!(!error.message.contains("upstream-secret"));
        assert!(!error.message.contains("private prompt"));
        primary_thread.join().unwrap();
        fallback_thread.join().unwrap();
    }

    #[test]
    fn successful_empty_upstream_body_fails_over_before_returning_to_client() {
        let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let primary_addr = primary.local_addr().unwrap();
        let primary_thread = std::thread::spawn(move || {
            let (mut stream, _) = primary.accept().unwrap();
            let _ = read_http_request(&mut stream);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
        });

        let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let fallback_addr = fallback.local_addr().unwrap();
        let fallback_thread = std::thread::spawn(move || {
            let (mut stream, _) = fallback.accept().unwrap();
            let _ = read_http_request(&mut stream);
            let body = r#"{"status":"completed","source":"fallback"}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        let bind_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let token = "aipass_empty_body_fallback_test";
        let route = fallback_route(
            token,
            &[primary_addr, fallback_addr],
            RetryPolicy {
                max_attempts: 2,
                ..RetryPolicy::default()
            },
        );
        let _proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage,
        )
        .unwrap();

        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/responses"))
            .bearer_auth(token)
            .json(&serde_json::json!({"model":"empty-body-test","input":[]}))
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.text().unwrap().contains("fallback"));
        primary_thread.join().unwrap();
        fallback_thread.join().unwrap();
    }

    #[test]
    fn silent_retry_replays_a_failed_upstream_round_before_returning_error() {
        let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let upstream_addr = upstream.local_addr().unwrap();
        let upstream_thread = std::thread::spawn(move || {
            for (index, status) in ["HTTP/1.1 503 Service Unavailable", "HTTP/1.1 200 OK"]
                .into_iter()
                .enumerate()
            {
                let (mut stream, _) = upstream.accept().unwrap();
                let (_, body) = read_http_request(&mut stream);
                assert!(!body.is_empty());
                let response = if index == 0 {
                    r#"{"error":{"message":"temporary upstream failure"}}"#
                } else {
                    r#"{"status":"completed"}"#
                };
                write!(
                    stream,
                    "{status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response.len(),
                    response
                )
                .unwrap();
            }
        });

        let bind_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let token = "aipass_silent_retry_test";
        let route = single_target_route(
            token,
            format!("http://{upstream_addr}/v1"),
            RetryPolicy {
                silent_retry: true,
                max_silent_retries: 1,
                ..RetryPolicy::default()
            },
        );
        let _proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage,
        )
        .unwrap();

        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/responses"))
            .bearer_auth(token)
            .json(&serde_json::json!({"model":"retry-test","input":[]}))
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.json::<serde_json::Value>().unwrap()["status"],
            "completed"
        );
        upstream_thread.join().unwrap();
    }

    #[test]
    fn retry_policy_defaults_hold_fields_for_legacy_json() {
        let retry: RetryPolicy = serde_json::from_value(serde_json::json!({
            "maxAttempts": 3,
            "failureThreshold": 3,
            "circuitOpenSeconds": 30,
            "connectTimeoutMs": 10000,
            "firstByteTimeoutMs": 30000,
            "streamIdleTimeoutMs": 120000
        }))
        .unwrap();
        assert!(!retry.hold_on_failure);
        assert_eq!(retry.hold_initial_delay_ms, 500);
        assert_eq!(retry.hold_max_delay_ms, 10_000);
        assert_eq!(retry.hold_max_duration_ms, 300_000);
    }

    #[test]
    fn hold_on_failure_retries_with_backoff_until_upstream_recovers() {
        let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let upstream_addr = upstream.local_addr().unwrap();
        let upstream_thread = std::thread::spawn(move || {
            for index in 0..3 {
                let (mut stream, _) = upstream.accept().unwrap();
                let _ = read_http_request(&mut stream);
                let (status, body) = if index < 2 {
                    (
                        "HTTP/1.1 500 Internal Server Error",
                        r#"{"error":{"message":"upstream down"}}"#,
                    )
                } else {
                    ("HTTP/1.1 200 OK", r#"{"status":"completed"}"#)
                };
                write!(
                    stream,
                    "{status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });

        let bind_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let token = "aipass_hold_backoff_test";
        let route = single_target_route(
            token,
            format!("http://{upstream_addr}/v1"),
            RetryPolicy {
                max_attempts: 1,
                hold_on_failure: true,
                hold_initial_delay_ms: 50,
                hold_max_delay_ms: 200,
                hold_max_duration_ms: 10_000,
                ..RetryPolicy::default()
            },
        );
        let _proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage,
        )
        .unwrap();

        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/responses"))
            .bearer_auth(token)
            .json(&serde_json::json!({"model":"hold-test","input":[]}))
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.json::<serde_json::Value>().unwrap()["status"],
            "completed"
        );
        upstream_thread.join().unwrap();
    }

    #[test]
    fn hold_on_failure_returns_502_after_max_duration() {
        let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let upstream_addr = upstream.local_addr().unwrap();
        let request_count = Arc::new(AtomicU64::new(0));
        let counter = request_count.clone();
        std::thread::spawn(move || {
            for _ in 0..20 {
                let Ok((mut stream, _)) = upstream.accept() else {
                    return;
                };
                let _ = read_http_request(&mut stream);
                counter.fetch_add(1, Ordering::SeqCst);
                let body = r#"{"error":{"message":"upstream down"}}"#;
                write!(
                    stream,
                    "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });

        let bind_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let token = "aipass_hold_timeout_test";
        let route = single_target_route(
            token,
            format!("http://{upstream_addr}/v1"),
            RetryPolicy {
                max_attempts: 1,
                hold_on_failure: true,
                hold_initial_delay_ms: 50,
                hold_max_delay_ms: 200,
                hold_max_duration_ms: 300,
                ..RetryPolicy::default()
            },
        );
        let _proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage,
        )
        .unwrap();

        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/responses"))
            .bearer_auth(token)
            .json(&serde_json::json!({"model":"hold-test","input":[]}))
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert!(request_count.load(Ordering::SeqCst) >= 2);
    }

    #[tokio::test]
    async fn hold_deadline_bounds_backoff_after_confirmed_failure() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let count = Arc::new(AtomicU64::new(0));
        let counter = count.clone();
        let server = tokio::spawn(async move {
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                let counter = counter.clone();
                tokio::spawn(async move {
                    let service = service_fn(move |request: Request<Incoming>| {
                        let counter = counter.clone();
                        async move {
                            if request.method() == http::Method::POST {
                                request.into_body().collect().await.unwrap();
                                counter.fetch_add(1, Ordering::SeqCst);
                            }
                            Ok::<_, Infallible>(error_response(
                                StatusCode::SERVICE_UNAVAILABLE,
                                "unavailable",
                            ))
                        }
                    });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(TokioIo::new(socket), service)
                        .await;
                });
            }
        });
        let route = single_target_route(
            "hold",
            format!("http://{addr}/v1"),
            RetryPolicy {
                hold_on_failure: true,
                hold_initial_delay_ms: 5_000,
                hold_max_delay_ms: 5_000,
                hold_max_duration_ms: 300,
                ..RetryPolicy::default()
            },
        );
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let mut config = RuntimeConfig::from_routes(available_addr().to_string(), vec![route]);
        config.upstream_proxy.mode = UpstreamProxyMode::Direct;
        let proxy = ProxyHandle::start(config, usage).unwrap();
        let response = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap()
            .post(format!("http://{}/v1/responses", proxy.bind_addr))
            .bearer_auth("hold")
            .body("{}")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "hold expiry must prevent the next submission"
        );
        server.abort();
    }

    #[tokio::test]
    async fn hold_deadline_does_not_cut_off_a_committed_stream() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let service = service_fn(|_: Request<Incoming>| async {
                let source = stream::iter([0, 1]).then(|index| async move {
                    let data = if index == 0 {
                        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n"
                    } else {
                        tokio::time::sleep(Duration::from_millis(300)).await;
                        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n"
                    };
                    Ok::<_, BoxError>(Frame::data(Bytes::from_static(data.as_bytes())))
                });
                Ok::<_, Infallible>(
                    Response::builder()
                        .header(header::CONTENT_TYPE, "text/event-stream")
                        .body(BodyExt::boxed_unsync(StreamBody::new(source)))
                        .unwrap(),
                )
            });
            let _ = hyper::server::conn::http1::Builder::new()
                .serve_connection(TokioIo::new(socket), service)
                .await;
        });
        let token = "hold_live_stream_test";
        let route = single_target_route(
            token,
            format!("http://{addr}/v1"),
            RetryPolicy {
                hold_on_failure: true,
                hold_max_duration_ms: 150,
                stream_idle_timeout_ms: 2_000,
                ..RetryPolicy::default()
            },
        );
        let bind_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let mut config = RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]);
        config.upstream_proxy.mode = UpstreamProxyMode::Direct;
        let proxy = ProxyHandle::start(config, usage.clone()).unwrap();
        let response = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap()
            .post(format!("http://{bind_addr}/v1/responses"))
            .bearer_auth(token)
            .json(&serde_json::json!({"model":"test","input":[],"stream":true}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response
            .text()
            .await
            .unwrap()
            .contains("response.completed"));
        for _ in 0..100 {
            if usage.count().unwrap() == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(proxy.status().requests, 1);
        assert_eq!(proxy.status().failures, 0);
        server.abort();
    }

    #[test]
    fn hold_on_failure_waits_for_circuit_cooldown() {
        let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let upstream_addr = upstream.local_addr().unwrap();
        let upstream_thread = std::thread::spawn(move || {
            for index in 0..2 {
                let (mut stream, _) = upstream.accept().unwrap();
                let _ = read_http_request(&mut stream);
                let (status, body) = if index == 0 {
                    (
                        "HTTP/1.1 500 Internal Server Error",
                        r#"{"error":{"message":"upstream down"}}"#,
                    )
                } else {
                    ("HTTP/1.1 200 OK", r#"{"status":"completed"}"#)
                };
                write!(
                    stream,
                    "{status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });

        let bind_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let token = "aipass_hold_circuit_test";
        let route = single_target_route(
            token,
            format!("http://{upstream_addr}/v1"),
            RetryPolicy {
                max_attempts: 1,
                failure_threshold: 1,
                circuit_open_seconds: 1,
                hold_on_failure: true,
                hold_initial_delay_ms: 50,
                hold_max_delay_ms: 100,
                hold_max_duration_ms: 5_000,
                ..RetryPolicy::default()
            },
        );
        let _proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage,
        )
        .unwrap();

        let started = Instant::now();
        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/responses"))
            .bearer_auth(token)
            .json(&serde_json::json!({"model":"hold-test","input":[]}))
            .send()
            .unwrap();
        assert!(started.elapsed() >= Duration::from_secs(1));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.json::<serde_json::Value>().unwrap()["status"],
            "completed"
        );
        upstream_thread.join().unwrap();
    }

    #[test]
    fn silent_retry_does_not_replay_an_incomplete_stream() {
        let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let primary_addr = primary.local_addr().unwrap();
        let primary_thread = std::thread::spawn(move || {
            let (mut stream, _) = primary.accept().unwrap();
            let _ = read_http_request(&mut stream);
            let partial =
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n";
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                partial.len(),
                partial
            )
            .unwrap();
        });

        let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let fallback_addr = fallback.local_addr().unwrap();

        let bind_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let token = "aipass_silent_stream_retry_test";
        let mut route = fallback_route(
            token,
            &[primary_addr, fallback_addr],
            RetryPolicy {
                max_attempts: 1,
                silent_retry: true,
                max_silent_retries: 1,
                ..RetryPolicy::default()
            },
        );
        route.config.strategy = RouteStrategy::RoundRobin;
        let _proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage,
        )
        .unwrap();

        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/responses"))
            .bearer_auth(token)
            .json(&serde_json::json!({"model":"stream-retry-test","stream":true}))
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let _ = response.text().unwrap();
        primary_thread.join().unwrap();
        fallback.set_nonblocking(true).unwrap();
        assert_eq!(
            fallback.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn truncated_non_stream_response_is_rejected_after_disconnect() {
        let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let upstream_addr = upstream.local_addr().unwrap();
        let upstream_thread = std::thread::spawn(move || {
            let (mut stream, _) = upstream.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n"
            )
            .unwrap();
            stream.flush().unwrap();
            std::thread::sleep(Duration::from_millis(250));
        });

        let bind_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let token = "aipass_stream_timeout_test";
        let route = single_target_route(
            token,
            format!("http://{upstream_addr}/v1"),
            RetryPolicy {
                max_attempts: 1,
                first_byte_timeout_ms: 500,
                stream_idle_timeout_ms: 50,
                ..RetryPolicy::default()
            },
        );
        let _proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage,
        )
        .unwrap();

        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/responses"))
            .bearer_auth(token)
            .body("{}")
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(
            response.json::<serde_json::Value>().unwrap()["error"]["message"],
            "all upstream targets failed"
        );
        upstream_thread.join().unwrap();
    }

    #[test]
    fn upstream_error_status_fails_over_without_reaching_the_client() {
        let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let primary_addr = primary.local_addr().unwrap();
        let primary_thread = std::thread::spawn(move || {
            let (mut stream, _) = primary.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            write!(
                stream,
                "HTTP/1.1 400 Bad Request\r\nContent-Length: 15\r\nConnection: close\r\n\r\nprimary failed"
            )
            .unwrap();
        });
        let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let fallback_addr = fallback.local_addr().unwrap();
        let fallback_thread = std::thread::spawn(move || {
            let (mut stream, _) = fallback.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            let body = r#"{"source":"fallback"}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        let bind_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let token = "aipass_status_failover_test";
        let route = fallback_route(
            token,
            &[primary_addr, fallback_addr],
            RetryPolicy {
                max_attempts: 2,
                ..RetryPolicy::default()
            },
        );
        let _proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage,
        )
        .unwrap();

        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/responses"))
            .bearer_auth(token)
            .body("{}")
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.json::<serde_json::Value>().unwrap()["source"],
            "fallback"
        );
        primary_thread.join().unwrap();
        fallback_thread.join().unwrap();
    }

    #[test]
    fn upstream_redirect_is_not_followed_and_fails_over_internally() {
        let redirect_target = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        redirect_target.set_nonblocking(true).unwrap();
        let redirect_target_addr = redirect_target.local_addr().unwrap();
        let (redirect_hit_tx, redirect_hit_rx) = std::sync::mpsc::channel();
        let (redirect_stop_tx, redirect_stop_rx) = std::sync::mpsc::channel();
        let redirect_target_thread = std::thread::spawn(move || loop {
            if redirect_stop_rx.try_recv().is_ok() {
                break;
            }
            match redirect_target.accept() {
                Ok((mut stream, _)) => {
                    let _ = redirect_hit_tx.send(());
                    let mut request = [0_u8; 4096];
                    let _ = stream.read(&mut request);
                    let _ = write!(
                        stream,
                        "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    );
                    break;
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break,
            }
        });

        let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let primary_addr = primary.local_addr().unwrap();
        let primary_thread = std::thread::spawn(move || {
            let (mut stream, _) = primary.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            write!(
                stream,
                "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://{redirect_target_addr}/v1/responses\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
        });
        let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let fallback_addr = fallback.local_addr().unwrap();
        let fallback_thread = std::thread::spawn(move || {
            let (mut stream, _) = fallback.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            let body = r#"{"source":"fallback"}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        let bind_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let token = "aipass_redirect_failover_test";
        let route = fallback_route(
            token,
            &[primary_addr, fallback_addr],
            RetryPolicy {
                max_attempts: 2,
                ..RetryPolicy::default()
            },
        );
        let _proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage,
        )
        .unwrap();

        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/responses"))
            .bearer_auth(token)
            .body("{}")
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.json::<serde_json::Value>().unwrap()["source"],
            "fallback"
        );
        assert!(redirect_hit_rx.try_recv().is_err());

        let _ = redirect_stop_tx.send(());
        redirect_target_thread.join().unwrap();
        primary_thread.join().unwrap();
        fallback_thread.join().unwrap();
    }

    #[test]
    fn successful_json_error_payload_fails_over_internally() {
        let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let primary_addr = primary.local_addr().unwrap();
        let primary_thread = std::thread::spawn(move || {
            let (mut stream, _) = primary.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            let body = r#"{"error":{"message":"primary failed"}}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let fallback_addr = fallback.local_addr().unwrap();
        let fallback_thread = std::thread::spawn(move || {
            let (mut stream, _) = fallback.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            let body = r#"{"source":"fallback"}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        let bind_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let token = "aipass_payload_failover_test";
        let route = fallback_route(
            token,
            &[primary_addr, fallback_addr],
            RetryPolicy {
                max_attempts: 2,
                ..RetryPolicy::default()
            },
        );
        let _proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage,
        )
        .unwrap();

        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/responses"))
            .bearer_auth(token)
            .body("{}")
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.json::<serde_json::Value>().unwrap()["source"],
            "fallback"
        );
        primary_thread.join().unwrap();
        fallback_thread.join().unwrap();
    }

    #[test]
    fn compressed_json_error_payload_fails_over_internally() {
        let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let primary_addr = primary.local_addr().unwrap();
        let primary_thread = std::thread::spawn(move || {
            let (mut stream, _) = primary.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            let compressed = [
                31, 139, 8, 0, 0, 0, 0, 0, 0, 19, 171, 86, 74, 45, 42, 202, 47, 82, 178, 170, 86,
                202, 77, 45, 46, 78, 76, 79, 85, 178, 82, 42, 40, 202, 204, 77, 44, 170, 84, 72,
                75, 204, 204, 73, 77, 81, 170, 173, 5, 0, 53, 129, 192, 235, 38, 0, 0, 0,
            ];
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                compressed.len()
            )
            .unwrap();
            stream.write_all(&compressed).unwrap();
        });
        let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let fallback_addr = fallback.local_addr().unwrap();
        let fallback_thread = std::thread::spawn(move || {
            let (mut stream, _) = fallback.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            let body = r#"{"source":"fallback"}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        let bind_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let token = "aipass_compressed_failover_test";
        let route = fallback_route(
            token,
            &[primary_addr, fallback_addr],
            RetryPolicy {
                max_attempts: 2,
                ..RetryPolicy::default()
            },
        );
        let _proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage,
        )
        .unwrap();

        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/responses"))
            .bearer_auth(token)
            .body("{}")
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.json::<serde_json::Value>().unwrap()["source"],
            "fallback"
        );
        primary_thread.join().unwrap();
        fallback_thread.join().unwrap();
    }

    #[test]
    fn lost_response_headers_do_not_replay_a_submitted_request() {
        let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let primary_addr = primary.local_addr().unwrap();
        let primary_thread = std::thread::spawn(move || {
            let (mut stream, _) = primary.accept().unwrap();
            let _ = read_http_request(&mut stream);
            std::thread::sleep(Duration::from_millis(200));
        });
        let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let fallback_addr = fallback.local_addr().unwrap();

        let bind_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let token = "aipass_header_timeout_test";
        let route = fallback_route(
            token,
            &[primary_addr, fallback_addr],
            RetryPolicy {
                max_attempts: 2,
                first_byte_timeout_ms: 50,
                ..RetryPolicy::default()
            },
        );
        let _proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage,
        )
        .unwrap();

        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/responses"))
            .bearer_auth(token)
            .body("{}")
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let _ = response.text().unwrap();
        primary_thread.join().unwrap();
        fallback.set_nonblocking(true).unwrap();
        assert_eq!(
            fallback.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn truncated_non_stream_body_does_not_replay_a_submitted_request() {
        let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let primary_addr = primary.local_addr().unwrap();
        let primary_thread = std::thread::spawn(move || {
            let (mut stream, _) = primary.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 64\r\nConnection: close\r\n\r\npartial"
            )
            .unwrap();
        });
        let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let fallback_addr = fallback.local_addr().unwrap();

        let bind_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let token = "aipass_body_failover_test";
        let route = fallback_route(
            token,
            &[primary_addr, fallback_addr],
            RetryPolicy {
                max_attempts: 2,
                ..RetryPolicy::default()
            },
        );
        let _proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage,
        )
        .unwrap();

        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/responses"))
            .bearer_auth(token)
            .body("{}")
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let _ = response.text().unwrap();
        primary_thread.join().unwrap();
        fallback.set_nonblocking(true).unwrap();
        assert_eq!(
            fallback.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn stream_request_with_truncated_json_does_not_replay() {
        let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let primary_addr = primary.local_addr().unwrap();
        let primary_thread = std::thread::spawn(move || {
            let (mut stream, _) = primary.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 64\r\nConnection: close\r\n\r\npartial"
            )
            .unwrap();
        });
        let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let fallback_addr = fallback.local_addr().unwrap();

        let bind_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let token = "aipass_stream_json_failover_test";
        let route = fallback_route(
            token,
            &[primary_addr, fallback_addr],
            RetryPolicy {
                max_attempts: 2,
                ..RetryPolicy::default()
            },
        );
        let _proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage,
        )
        .unwrap();

        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/responses"))
            .bearer_auth(token)
            .json(&serde_json::json!({"stream": true}))
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let _ = response.text().unwrap();
        primary_thread.join().unwrap();
        fallback.set_nonblocking(true).unwrap();
        assert_eq!(
            fallback.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn incomplete_first_sse_event_does_not_replay_a_submitted_request() {
        let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let primary_addr = primary.local_addr().unwrap();
        let primary_thread = std::thread::spawn(move || {
            let (mut stream, _) = primary.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            let partial = r#"data: {"partial":true}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n{:X}\r\n{}\r\n",
                partial.len(),
                partial
            )
            .unwrap();
            stream.flush().unwrap();
            std::thread::sleep(Duration::from_millis(200));
        });
        let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let fallback_addr = fallback.local_addr().unwrap();

        let bind_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let token = "aipass_sse_failover_test";
        let route = fallback_route(
            token,
            &[primary_addr, fallback_addr],
            RetryPolicy {
                max_attempts: 2,
                first_byte_timeout_ms: 50,
                ..RetryPolicy::default()
            },
        );
        let _proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage,
        )
        .unwrap();

        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/responses"))
            .bearer_auth(token)
            .json(&serde_json::json!({"stream": true}))
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let _ = response.text().unwrap();
        primary_thread.join().unwrap();
        fallback.set_nonblocking(true).unwrap();
        assert_eq!(
            fallback.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn first_sse_error_event_fails_over_before_stream_commit() {
        let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let primary_addr = primary.local_addr().unwrap();
        let primary_thread = std::thread::spawn(move || {
            let (mut stream, _) = primary.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            let body = "event: error\ndata: {\"error\":{\"message\":\"primary failed\"}}\n\n";
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let fallback_addr = fallback.local_addr().unwrap();
        let fallback_thread = std::thread::spawn(move || {
            let (mut stream, _) = fallback.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            let body = "data: {\"source\":\"fallback\"}\n\n";
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        let bind_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let token = "aipass_sse_error_failover_test";
        let route = fallback_route(
            token,
            &[primary_addr, fallback_addr],
            RetryPolicy {
                max_attempts: 2,
                ..RetryPolicy::default()
            },
        );
        let _proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage.clone(),
        )
        .unwrap();

        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/responses"))
            .bearer_auth(token)
            .json(&serde_json::json!({"stream": true}))
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.text().unwrap();
        assert!(body.contains("fallback"));
        assert!(!body.contains("primary failed"));
        let logs = usage.logs().unwrap();
        let error = logs
            .iter()
            .find(|entry| entry.message.contains("event=proxy.upstream.rejected"))
            .unwrap();
        assert!(error.message.contains("transport=sse"));
        assert!(error.message.contains("primary failed"));
        primary_thread.join().unwrap();
        fallback_thread.join().unwrap();
    }

    #[test]
    fn metadata_then_error_fails_over_before_stream_commit() {
        let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let primary_addr = primary.local_addr().unwrap();
        let primary_thread = std::thread::spawn(move || {
            let (mut stream, _) = primary.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            let body = concat!(
                "event: response.created\n",
                "data: {\"type\":\"response.created\"}\n\n",
                "event: response.failed\n",
                "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"message\":\"primary failed\"}}}\n\n"
            );
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let fallback_addr = fallback.local_addr().unwrap();
        let fallback_thread = std::thread::spawn(move || {
            let (mut stream, _) = fallback.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            let body = concat!(
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"fallback\"}\n\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n"
            );
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        let bind_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let token = "aipass_sse_metadata_failover_test";
        let route = fallback_route(
            token,
            &[primary_addr, fallback_addr],
            RetryPolicy {
                max_attempts: 2,
                ..RetryPolicy::default()
            },
        );
        let _proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage,
        )
        .unwrap();

        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/responses"))
            .bearer_auth(token)
            .json(&serde_json::json!({"stream": true}))
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.text().unwrap();
        assert!(body.contains("fallback"));
        assert!(!body.contains("primary failed"));
        assert!(!body.contains("response.created"));
        primary_thread.join().unwrap();
        fallback_thread.join().unwrap();
    }

    #[test]
    fn stream_failure_after_commit_opens_the_target_circuit() {
        let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let upstream_addr = upstream.local_addr().unwrap();
        let upstream_thread = std::thread::spawn(move || {
            let (mut stream, _) = upstream.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            let body = "data: {\"delta\":\"started\"}\n\n";
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n{:X}\r\n{}\r\n",
                body.len(),
                body
            )
            .unwrap();
            stream.flush().unwrap();
        });

        let bind_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let token = "aipass_stream_circuit_test";
        let route = single_target_route(
            token,
            format!("http://{upstream_addr}/v1"),
            RetryPolicy {
                max_attempts: 1,
                failure_threshold: 1,
                ..RetryPolicy::default()
            },
        );
        let target_id = route.targets[0].config.id;
        let proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage,
        )
        .unwrap();

        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/responses"))
            .bearer_auth(token)
            .json(&serde_json::json!({"stream": true}))
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.text().is_err());
        assert!(circuit_open(&proxy.state, target_id));
        upstream_thread.join().unwrap();
    }

    #[test]
    fn stream_completion_controls_circuit_health_and_usage_duration() {
        let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let upstream_addr = upstream.local_addr().unwrap();
        let upstream_thread = std::thread::spawn(move || {
            let (mut stream, _) = upstream.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {{\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}}\n\n"
            )
            .unwrap();
            stream.flush().unwrap();
            std::thread::sleep(Duration::from_millis(80));
            write!(
                stream,
                "event: response.completed\ndata: {{\"type\":\"response.completed\",\"response\":{{\"status\":\"completed\"}}}}\n\n"
            )
            .unwrap();
            stream.flush().unwrap();
            std::thread::sleep(Duration::from_millis(350));
        });

        let bind_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let token = "aipass_stream_completion_test";
        let route = single_target_route(
            token,
            format!("http://{upstream_addr}/v1"),
            RetryPolicy {
                failure_threshold: 1,
                stream_idle_timeout_ms: 150,
                ..RetryPolicy::default()
            },
        );
        let target_id = route.targets[0].config.id;
        let proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage.clone(),
        )
        .unwrap();

        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/responses"))
            .bearer_auth(token)
            .json(&serde_json::json!({"stream": true}))
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.text().unwrap().contains("response.completed"));
        upstream_thread.join().unwrap();
        for _ in 0..30 {
            if usage.count().unwrap() == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let duration_ms = usage
            .connection
            .lock()
            .unwrap()
            .query_row("SELECT duration_ms FROM proxy_usage", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap();
        assert!(duration_ms >= 60, "recorded duration was {duration_ms}ms");
        assert!(!circuit_open(&proxy.state, target_id));
    }

    #[test]
    fn natural_stream_eof_without_terminal_event_opens_the_circuit() {
        let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let upstream_addr = upstream.local_addr().unwrap();
        let upstream_thread = std::thread::spawn(move || {
            let (mut stream, _) = upstream.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            let body = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n";
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        let bind_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let token = "aipass_stream_incomplete_test";
        let route = single_target_route(
            token,
            format!("http://{upstream_addr}/v1"),
            RetryPolicy {
                failure_threshold: 1,
                ..RetryPolicy::default()
            },
        );
        let target_id = route.targets[0].config.id;
        let proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage,
        )
        .unwrap();

        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/responses"))
            .bearer_auth(token)
            .json(&serde_json::json!({"stream": true}))
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let _ = response.text().unwrap();
        for _ in 0..30 {
            if circuit_open(&proxy.state, target_id) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(circuit_open(&proxy.state, target_id));
        upstream_thread.join().unwrap();
    }

    #[test]
    fn proxy_authenticates_fails_over_and_records_usage() {
        let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let upstream_addr = upstream.local_addr().unwrap();
        let (request_tx, request_rx) = std::sync::mpsc::channel();
        let upstream_thread = std::thread::spawn(move || {
            let (mut stream, _) = upstream.accept().unwrap();
            let mut request = vec![0_u8; 8192];
            let count = stream.read(&mut request).unwrap();
            request.truncate(count);
            request_tx
                .send(String::from_utf8_lossy(&request).to_string())
                .unwrap();
            let body = serde_json::json!({
                "id": "response-test",
                "status": "completed",
                "output": [],
                "usage": {
                    "input_tokens": 12,
                    "output_tokens": 4,
                    "input_tokens_details": {"cached_tokens": 7, "cache_creation_tokens": 2}
                }
            })
            .to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        });

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let bind_addr = probe.local_addr().unwrap();
        drop(probe);
        let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let dead_addr = dead.local_addr().unwrap();
        drop(dead);
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let route_id = Uuid::new_v4();
        let provider_id = Uuid::new_v4();
        let target = |id, base_url, priority| ResolvedTarget {
            max_concurrent_requests: None,
            supports_websockets: false,
            config: ProxyTargetConfig {
                id,
                provider_entry_id: provider_id,
                secret_id: "primary".into(),
                label: "primary".into(),
                base_url,
                auth_scheme: "bearer".into(),
                headers: Vec::new(),
                group: Some("default".into()),
                priority,
                weight: 1,
                enabled: true,
                protocol: None,
            },
            api_key: "upstream-secret".into(),
        };
        let token = "aipass_local_test";
        let route = ResolvedRoute {
            config: ProxyRouteConfig {
                id: route_id,
                name: "test".into(),
                token: String::new(),
                inbound_protocol: ProxyProtocol::OpenAiResponses,
                upstream_protocol: ProxyProtocol::OpenAiResponses,
                conversion_enabled: false,
                strategy: RouteStrategy::Fallback,
                targets: Vec::new(),
                retry: RetryPolicy {
                    max_attempts: 2,
                    connect_timeout_ms: 100,
                    ..RetryPolicy::default()
                },
                enabled: true,
            },
            local_token: token.to_string(),
            targets: vec![
                target(Uuid::new_v4(), format!("http://{dead_addr}/v1"), 0),
                target(Uuid::new_v4(), format!("http://{upstream_addr}/v1"), 1),
            ],
        };
        let failed_target_id = route.targets[0].config.id;
        let proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage.clone(),
        )
        .unwrap();
        let client = reqwest::blocking::Client::new();
        let url = format!("http://{bind_addr}/v1/responses");
        let mut response = None;
        for _ in 0..30 {
            match client
                .post(&url)
                .bearer_auth(token)
                .header("api-key", "local-credential-must-not-forward")
                .json(&serde_json::json!({"model":"gpt-test","input":"hello"}))
                .send()
            {
                Ok(value) => {
                    response = Some(value);
                    break;
                }
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        let response = response.expect("proxy response");
        assert_eq!(response.status(), StatusCode::OK);
        let _ = response.text().unwrap();
        let upstream_request = request_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(upstream_request
            .to_ascii_lowercase()
            .contains("authorization: bearer upstream-secret"));
        assert!(!upstream_request.contains("local-credential-must-not-forward"));
        upstream_thread.join().unwrap();
        for _ in 0..30 {
            if usage.count().unwrap() == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(usage.count().unwrap(), 1);
        let summary = usage.summary(|_| 0).unwrap();
        assert_eq!(summary.attempt_count, 2);
        assert_eq!(summary.successful_attempts, 1);
        assert_eq!(summary.success_rate_bps, 10_000);
        assert!(proxy.status().degraded);
        assert_eq!(proxy.status().degraded_target_ids, vec![failed_target_id]);
        assert_eq!(proxy.status().failures, 0);
        let request_id: String = usage
            .connection
            .lock()
            .unwrap()
            .query_row("SELECT id FROM proxy_usage", [], |row| row.get(0))
            .unwrap();
        let linked_attempts: i64 = usage
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM proxy_attempts WHERE request_id = ?1",
                [&request_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(linked_attempts, 2);
        drop(proxy);
        let reopened = UsageStore::open(usage.path()).unwrap();
        let logs = reopened.logs().unwrap();
        let text = logs
            .iter()
            .map(|entry| entry.message.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("event=proxy.stopped"));
        assert!(text.contains("outcome=failed"));
        assert!(text.contains("outcome=success"));
        assert!(text.contains(&format!(
            "event=proxy.request.completed request_id={request_id}"
        )));
        for forbidden in [
            token,
            "upstream-secret",
            "local-credential-must-not-forward",
            "gpt-test",
            "hello",
            "http://",
        ] {
            assert!(!text.contains(forbidden));
        }
    }

    #[test]
    fn route_config_defaults_keep_fallback_strategy() {
        let route: ProxyRouteConfig = serde_json::from_value(serde_json::json!({
            "id": Uuid::new_v4(),
            "name": "legacy",
            "token": "legacy-plaintext-token",
            "tokenFingerprint": "abc",
            "inboundProtocol": "open_ai_responses",
            "upstreamProtocol": "open_ai_responses",
            "conversionEnabled": false,
            "targets": [{
                "id": Uuid::new_v4(),
                "providerEntryId": Uuid::new_v4(),
                "secretId": "key",
                "label": "primary",
                "baseUrl": "https://api.example.test",
                "authScheme": "bearer",
                "group": null,
                "priority": 0,
                "enabled": true
            }],
            "retry": {
                "maxAttempts": 3,
                "failureThreshold": 3,
                "circuitOpenSeconds": 30,
                "connectTimeoutMs": 10000,
                "firstByteTimeoutMs": 30000,
                "streamIdleTimeoutMs": 120000
            },
            "enabled": true
        }))
        .unwrap();
        assert_eq!(route.strategy, RouteStrategy::Fallback);
        assert_eq!(route.token, "legacy-plaintext-token");
        assert_eq!(route.targets[0].weight, 1);
    }

    #[test]
    fn round_robin_redistributes_weight_among_available_targets() {
        let mut route = fallback_route(
            "healthy_weight_test",
            &[
                "127.0.0.1:1".parse().unwrap(),
                "127.0.0.1:2".parse().unwrap(),
                "127.0.0.1:3".parse().unwrap(),
            ],
            RetryPolicy {
                failure_threshold: 1,
                max_attempts: 1,
                ..RetryPolicy::default()
            },
        );
        route.config.strategy = RouteStrategy::RoundRobin;
        route.targets[0].config.weight = 100;
        let proxy = start_proxy(available_addr(), route.clone());
        mark_failure(
            &proxy.state,
            route.targets[0].config.id,
            &route.config.retry,
        );
        let mut counts = [0; 2];
        for _ in 0..20 {
            let selected = select_route_targets(&proxy.state, &route);
            assert_eq!(selected.len(), 1);
            let index = route.targets[1..]
                .iter()
                .position(|target| target.config.id == selected[0].config.id)
                .unwrap();
            counts[index] += 1;
        }
        assert_eq!(counts, [10, 10]);
    }

    #[test]
    fn session_affinity_prefers_last_successful_target_across_round_robin() {
        let mut route = fallback_route(
            "session_affinity_test",
            &[
                "127.0.0.1:1".parse().unwrap(),
                "127.0.0.1:2".parse().unwrap(),
            ],
            RetryPolicy {
                max_attempts: 1,
                ..RetryPolicy::default()
            },
        );
        route.config.strategy = RouteStrategy::RoundRobin;
        let proxy = start_proxy(available_addr(), route.clone());
        let session = "conversation-1";
        let first = select_route_targets_with_affinity(&proxy.state, &route, Some(session));
        assert_eq!(first.len(), 1);
        let target = first[0].config.id;
        remember_affinity_target(&proxy.state, route.config.id, Some(session), target);

        // Round-robin advances on every selection, but the remembered healthy
        // target remains first for this session.
        for _ in 0..4 {
            let selected = select_route_targets_with_affinity(&proxy.state, &route, Some(session));
            assert_eq!(selected[0].config.id, target);
        }
    }

    #[test]
    fn session_affinity_is_cleared_when_a_target_fails() {
        let route = fallback_route(
            "session_affinity_failure_test",
            &["127.0.0.1:1".parse().unwrap()],
            RetryPolicy::default(),
        );
        let proxy = start_proxy(available_addr(), route.clone());
        let session = "conversation-1";
        let target = route.targets[0].config.id;
        remember_affinity_target(&proxy.state, route.config.id, Some(session), target);
        assert_eq!(
            affinity_target(&proxy.state, route.config.id, Some(session), &route.targets),
            Some(target)
        );
        mark_failure(&proxy.state, target, &route.config.retry);
        assert_eq!(
            affinity_target(&proxy.state, route.config.id, Some(session), &route.targets),
            None
        );
    }

    #[test]
    fn session_affinity_key_accepts_headers_and_prompt_cache_fields() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-aipass-session-id",
            HeaderValue::from_static(" header-session "),
        );
        assert_eq!(
            session_affinity_key(&headers, None).as_deref(),
            Some("header-session")
        );
        let metadata: RequestMetadata =
            serde_json::from_str(r#"{"prompt_cache_key":"prompt-session"}"#).unwrap();
        assert_eq!(
            session_affinity_key(&HeaderMap::new(), Some(&metadata)).as_deref(),
            Some("prompt-session")
        );
        let metadata: RequestMetadata =
            serde_json::from_str(r#"{"conversation":{"id":"conversation-session"}}"#).unwrap();
        assert_eq!(
            session_affinity_key(&HeaderMap::new(), Some(&metadata)).as_deref(),
            Some("conversation-session")
        );
    }

    #[test]
    fn weighted_start_index_follows_weight_distribution() {
        let weights = [1_u32, 3];
        let mut counts = [0_usize; 2];
        for counter in 0..8_u64 {
            counts[weighted_start_index(counter, &weights)] += 1;
        }
        assert_eq!(counts, [2, 6]);
        assert_eq!(weighted_start_index(0, &[]), 0);
        assert_eq!(weighted_start_index(5, &[0, 0]), 1);
    }

    #[test]
    fn usage_timeseries_groups_records_by_day() {
        let temp = tempfile::tempdir().unwrap();
        let store = UsageStore::open(temp.path().join("usage.sqlite")).unwrap();
        let record = |started_at, input_tokens, model: Option<&str>| UsageRecord {
            id: Uuid::new_v4(),
            started_at,
            duration_ms: 1,
            first_token_ms: None,
            route_id: Uuid::new_v4(),
            provider_entry_id: Uuid::new_v4(),
            secret_id: "key".into(),
            model: model.map(str::to_string),
            inbound_protocol: ProxyProtocol::OpenAiResponses,
            upstream_protocol: ProxyProtocol::OpenAiResponses,
            status: 200,
            attempts: 1,
            input_tokens,
            output_tokens: 2,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            estimated_cost_micros: 3,
        };
        let today_start = now_unix() / 86_400 * 86_400;
        store
            .record(&record(today_start + 60, 10, Some("gpt-4o")))
            .unwrap();
        store
            .record(&record(today_start + 120, 5, Some("claude-3-7-sonnet")))
            .unwrap();
        store
            .record(&record(today_start - 86_400, 7, None))
            .unwrap();
        store
            .record(&record(today_start - 10 * 86_400, 99, None))
            .unwrap();

        let points = store
            .timeseries(7, 0, UsageGranularity::Day, |_| 3)
            .unwrap();
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].request_count, 1);
        assert_eq!(points[0].input_tokens, 7);
        assert_eq!(points[1].request_count, 2);
        assert_eq!(points[1].input_tokens, 15);
        assert_eq!(points[1].output_tokens, 4);
        assert_eq!(points[1].estimated_cost_micros, 6);
        assert_eq!(points[0].models.len(), 1);
        assert_eq!(points[0].models[0].model, None);
        assert_eq!(points[0].models[0].request_count, 1);
        assert_eq!(points[0].models[0].input_tokens, 7);
        assert_eq!(points[1].models.len(), 2);
        assert_eq!(points[1].models[0].model.as_deref(), Some("gpt-4o"));
        assert_eq!(points[1].models[0].input_tokens, 10);
        assert_eq!(
            points[1].models[1].model.as_deref(),
            Some("claude-3-7-sonnet")
        );
        assert_eq!(points[1].models[1].input_tokens, 5);
        assert!(points.iter().all(|point| point.input_tokens != 99));
    }

    #[test]
    fn usage_timeseries_uses_the_requested_local_timezone() {
        let temp = tempfile::tempdir().unwrap();
        let store = UsageStore::open(temp.path().join("usage.sqlite")).unwrap();
        let record = |started_at| UsageRecord {
            id: Uuid::new_v4(),
            started_at,
            duration_ms: 1,
            first_token_ms: None,
            route_id: Uuid::new_v4(),
            provider_entry_id: Uuid::new_v4(),
            secret_id: "key".into(),
            model: None,
            inbound_protocol: ProxyProtocol::OpenAiResponses,
            upstream_protocol: ProxyProtocol::OpenAiResponses,
            status: 200,
            attempts: 1,
            input_tokens: 1,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            estimated_cost_micros: 3,
        };
        let offset_seconds = 8 * 60 * 60;
        let local_today_start = local_day_start(now_unix(), offset_seconds);
        store.record(&record(local_today_start - 60)).unwrap();
        store.record(&record(local_today_start + 60)).unwrap();

        let points = store
            .timeseries(2, 8 * 60, UsageGranularity::Day, |_| 3)
            .unwrap();
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].input_tokens, 1);
        assert_eq!(points[1].input_tokens, 1);
    }

    #[test]
    fn usage_timeseries_groups_the_last_24_hours_in_local_hour_buckets() {
        // Include both whole-hour and fractional timezone offsets.
        for offset_minutes in [0, 480, 330, -210, 345] {
            let temp = tempfile::tempdir().unwrap();
            let store = UsageStore::open(temp.path().join("usage.sqlite")).unwrap();
            let offset_seconds = i64::from(offset_minutes) * 60;
            let current_hour =
                (now_unix() + offset_seconds).div_euclid(3_600) * 3_600 - offset_seconds;
            let cutoff = current_hour - 23 * 3_600;
            let record = |started_at, model: Option<&str>| UsageRecord {
                id: Uuid::new_v4(),
                started_at,
                duration_ms: 1,
                first_token_ms: None,
                route_id: Uuid::new_v4(),
                provider_entry_id: Uuid::new_v4(),
                secret_id: "key".into(),
                model: model.map(str::to_string),
                inbound_protocol: ProxyProtocol::OpenAiResponses,
                upstream_protocol: ProxyProtocol::OpenAiResponses,
                status: 200,
                attempts: 1,
                input_tokens: 10,
                output_tokens: 2,
                cache_read_tokens: 3,
                cache_creation_tokens: 4,
                estimated_cost_micros: 99,
            };
            for (timestamp, model) in [
                (cutoff - 1, None),
                (cutoff, None),
                (current_hour - 3_600, Some("model-a")),
                (current_hour - 1, Some("model-a")),
                (current_hour - 2, Some("model-b")),
                (current_hour, Some("model-b")),
            ] {
                store.record(&record(timestamp, model)).unwrap();
            }
            let points = store
                .timeseries(1, offset_minutes, UsageGranularity::Hour, |_| 7)
                .unwrap();
            assert_eq!(points.len(), 3);
            assert_eq!(points[0].request_count, 1);
            assert_eq!(points[0].models[0].model, None);
            assert_eq!(points[1].request_count, 3);
            assert_eq!(points[1].input_tokens, 30);
            assert_eq!(points[1].output_tokens, 6);
            assert_eq!(points[1].cache_read_tokens, 9);
            assert_eq!(points[1].cache_creation_tokens, 12);
            assert_eq!(points[1].estimated_cost_micros, 21);
            assert_eq!(points[1].models.len(), 2);
            assert_eq!(points[1].models[0].model.as_deref(), Some("model-a"));
            assert_eq!(points[1].models[0].request_count, 2);
            assert_eq!(points[1].models[0].estimated_cost_micros, 14);
            assert_eq!(points[2].request_count, 1);
            let conn = store.connection.lock().unwrap();
            for (point, timestamp) in
                points
                    .iter()
                    .zip([cutoff, current_hour - 3_600, current_hour])
            {
                let bucket_start: i64 = conn
                    .query_row("SELECT unixepoch(?1)", params![point.date], |row| {
                        row.get(0)
                    })
                    .unwrap();
                assert_eq!(bucket_start, timestamp);
            }
        }
    }

    #[test]
    fn usage_summary_matches_chart_periods_and_filters_attempts() {
        for offset in [0, 480, 330, -210, 345] {
            for (days, granularity) in [
                (1, UsageGranularity::Hour),
                (7, UsageGranularity::Day),
                (30, UsageGranularity::Day),
            ] {
                let temp = tempfile::tempdir().unwrap();
                let store = UsageStore::open(temp.path().join("usage.sqlite")).unwrap();
                let cutoff = usage_window_start(days, offset, granularity);
                let provider = Uuid::new_v4();
                let old_provider = Uuid::new_v4();
                for (started_at, provider_entry_id, status, latency) in [
                    (cutoff - 1, old_provider, 502, 900),
                    (cutoff - 1, provider, 502, 900),
                    (cutoff, provider, 200, 120),
                    (now_unix(), provider, 200, 240),
                ] {
                    let record = UsageRecord {
                        id: Uuid::new_v4(),
                        started_at,
                        duration_ms: 1000,
                        first_token_ms: Some(latency),
                        route_id: Uuid::new_v4(),
                        provider_entry_id,
                        secret_id: "key".into(),
                        model: Some("model".into()),
                        inbound_protocol: ProxyProtocol::OpenAiResponses,
                        upstream_protocol: ProxyProtocol::OpenAiResponses,
                        status,
                        attempts: 1,
                        input_tokens: 10,
                        output_tokens: 2,
                        cache_read_tokens: 3,
                        cache_creation_tokens: 4,
                        estimated_cost_micros: 99,
                    };
                    store.record(&record).unwrap();
                    store
                        .record_attempt(&AttemptRecord {
                            id: Uuid::new_v4(),
                            request_id: Some(record.id),
                            started_at,
                            duration_ms: 1000,
                            first_token_ms: Some(latency),
                            route_id: record.route_id,
                            target_id: Uuid::new_v4(),
                            provider_entry_id,
                            secret_id: "key".into(),
                            model: Some("model".into()),
                            status: Some(status),
                            success: Some(status == 200),
                        })
                        .unwrap();
                }
                let summary = store.summary_since(Some(cutoff), |_| 7).unwrap();
                let points = store.timeseries(days, offset, granularity, |_| 7).unwrap();
                assert_eq!(summary.request_count, 2);
                assert_eq!(
                    summary.request_count,
                    points.iter().map(|p| p.request_count).sum::<u64>()
                );
                assert_eq!(
                    summary.input_tokens,
                    points.iter().map(|p| p.input_tokens).sum::<u64>()
                );
                assert_eq!(
                    summary.output_tokens,
                    points.iter().map(|p| p.output_tokens).sum::<u64>()
                );
                assert_eq!(
                    summary.cache_read_tokens,
                    points.iter().map(|p| p.cache_read_tokens).sum::<u64>()
                );
                assert_eq!(
                    summary.cache_creation_tokens,
                    points.iter().map(|p| p.cache_creation_tokens).sum::<u64>()
                );
                assert_eq!(
                    summary.estimated_cost_micros,
                    points.iter().map(|p| p.estimated_cost_micros).sum::<u64>()
                );
                assert_eq!(summary.attempt_count, 2);
                assert_eq!(summary.completed_attempts, 2);
                assert_eq!(summary.successful_attempts, 2);
                assert_eq!(summary.success_rate_bps, 10_000);
                assert_eq!(summary.average_first_token_ms, Some(180));
                assert_eq!(summary.providers.len(), 1);
                assert_eq!(summary.providers[0].provider_entry_id, provider);
                assert_eq!(summary.providers[0].request_count, 2);
                assert_eq!(summary.providers[0].attempt_count, 2);
                assert_eq!(summary.providers[0].success_rate_bps, 10_000);
                assert_eq!(summary.providers[0].average_first_token_ms, Some(180));
                assert_eq!(summary.models.len(), 1);
                assert_eq!(summary.models[0].attempt_count, 2);
                assert_eq!(store.summary(|_| 7).unwrap().request_count, 4);
            }
        }
    }

    #[test]
    fn usage_summary_recomputes_cost_with_injected_resolver() {
        let temp = tempfile::tempdir().unwrap();
        let store = UsageStore::open(temp.path().join("usage.sqlite")).unwrap();
        let provider_a = Uuid::new_v4();
        let provider_b = Uuid::new_v4();
        let record =
            |provider_entry_id, secret_id: &str, model: Option<&str>, started_at, input_tokens| {
                UsageRecord {
                    id: Uuid::new_v4(),
                    started_at,
                    duration_ms: 1,
                    first_token_ms: None,
                    route_id: Uuid::new_v4(),
                    provider_entry_id,
                    secret_id: secret_id.into(),
                    model: model.map(str::to_string),
                    inbound_protocol: ProxyProtocol::OpenAiResponses,
                    upstream_protocol: ProxyProtocol::OpenAiResponses,
                    status: 200,
                    attempts: 1,
                    input_tokens,
                    output_tokens: 2,
                    cache_read_tokens: 0,
                    cache_creation_tokens: 0,
                    estimated_cost_micros: 0,
                }
            };
        store
            .record(&record(provider_a, "key", Some("gpt-test"), 10, 100))
            .unwrap();
        store
            .record(&record(provider_a, "key", Some("claude-test"), 20, 50))
            .unwrap();
        store
            .record(&record(provider_b, "key", Some("gpt-test"), 30, 10))
            .unwrap();
        store
            .record(&record(provider_b, "key", None, 40, 5))
            .unwrap();

        // Stored estimated_cost_micros is 0; the injected resolver recomputes
        // cost per row at query time.
        let summary = store.summary(|row| row.input_tokens * 2).unwrap();
        assert_eq!(summary.request_count, 4);
        assert_eq!(summary.input_tokens, 165);
        assert_eq!(summary.output_tokens, 8);
        assert_eq!(summary.estimated_cost_micros, 330);
        assert_eq!(summary.providers.len(), 2);
        // Providers are ordered by most recent usage first.
        assert_eq!(summary.providers[0].provider_entry_id, provider_b);
        assert_eq!(summary.providers[0].estimated_cost_micros, 30);
        assert_eq!(summary.providers[0].request_count, 2);
        assert_eq!(summary.providers[1].provider_entry_id, provider_a);
        assert_eq!(summary.providers[1].request_count, 2);
        assert_eq!(summary.providers[1].estimated_cost_micros, 300);
        assert_eq!(summary.models.len(), 4);
        // Models are aggregated per (provider, model) and ordered by most
        // recent usage first, including records without a detected model.
        assert_eq!(summary.models[0].model, None);
        assert_eq!(summary.models[0].provider_entry_id, provider_b);
        assert_eq!(summary.models[0].request_count, 1);
        assert_eq!(summary.models[0].estimated_cost_micros, 10);
        assert_eq!(summary.models[1].model.as_deref(), Some("gpt-test"));
        assert_eq!(summary.models[1].provider_entry_id, provider_b);
        assert_eq!(summary.models[1].request_count, 1);
        assert_eq!(summary.models[1].input_tokens, 10);
        assert_eq!(summary.models[1].estimated_cost_micros, 20);
        assert_eq!(summary.models[2].model.as_deref(), Some("claude-test"));
        assert_eq!(summary.models[2].provider_entry_id, provider_a);
        assert_eq!(summary.models[2].estimated_cost_micros, 100);
        // The same model on a different provider stays a separate row.
        assert_eq!(summary.models[3].model.as_deref(), Some("gpt-test"));
        assert_eq!(summary.models[3].provider_entry_id, provider_a);
        assert_eq!(summary.models[3].request_count, 1);
        assert_eq!(summary.models[3].input_tokens, 100);
        assert_eq!(summary.models[3].estimated_cost_micros, 200);

        let rows = store.iter_rows().unwrap();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].started_at, 10);
        assert_eq!(rows[0].model.as_deref(), Some("gpt-test"));
    }

    #[test]
    fn usage_summary_aggregates_attempt_health_and_first_token_latency() {
        let temp = tempfile::tempdir().unwrap();
        let store = UsageStore::open(temp.path().join("usage.sqlite")).unwrap();
        let provider_a = Uuid::new_v4();
        let provider_b = Uuid::new_v4();
        let route_id = Uuid::new_v4();
        let request =
            |provider_entry_id, secret_id: &str, started_at, status, first_token_ms| UsageRecord {
                id: Uuid::new_v4(),
                started_at,
                duration_ms: 10,
                first_token_ms,
                route_id,
                provider_entry_id,
                secret_id: secret_id.into(),
                model: Some("gpt-test".into()),
                inbound_protocol: ProxyProtocol::OpenAiResponses,
                upstream_protocol: ProxyProtocol::OpenAiResponses,
                status,
                attempts: 1,
                input_tokens: 0,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                estimated_cost_micros: 0,
            };
        store
            .record(&request(provider_a, "primary", 10, 502, None))
            .unwrap();
        store
            .record(&request(provider_a, "primary", 20, 200, Some(120)))
            .unwrap();
        store
            .record(&request(provider_b, "backup", 40, 502, None))
            .unwrap();
        let attempt = |provider_entry_id, secret_id: &str, started_at, success, first_token_ms| {
            AttemptRecord {
                id: Uuid::new_v4(),
                request_id: None,
                started_at,
                duration_ms: 10,
                first_token_ms,
                route_id,
                target_id: Uuid::new_v4(),
                provider_entry_id,
                secret_id: secret_id.into(),
                model: Some("gpt-test".into()),
                status: Some(if success == Some(true) { 200 } else { 502 }),
                success,
            }
        };
        store
            .record_attempt(&attempt(provider_a, "primary", 10, Some(false), None))
            .unwrap();
        store
            .record_attempt(&attempt(provider_a, "primary", 20, Some(true), Some(120)))
            .unwrap();
        store
            .record_attempt(&attempt(provider_a, "primary", 30, None, Some(90)))
            .unwrap();
        store
            .record_attempt(&attempt(provider_b, "backup", 40, Some(false), None))
            .unwrap();

        let summary = store.summary(|_| 0).unwrap();
        assert_eq!(summary.request_count, 3);
        assert_eq!(summary.attempt_count, 4);
        assert_eq!(summary.completed_attempts, 3);
        assert_eq!(summary.successful_attempts, 1);
        assert_eq!(summary.success_rate_bps, 3_333);
        assert_eq!(summary.average_first_token_ms, Some(120));
        let provider = summary
            .providers
            .iter()
            .find(|row| row.provider_entry_id == provider_a)
            .unwrap();
        assert_eq!(provider.attempt_count, 3);
        assert_eq!(provider.completed_attempts, 2);
        assert_eq!(provider.successful_attempts, 1);
        assert_eq!(provider.success_rate_bps, 5_000);
        assert_eq!(provider.average_first_token_ms, Some(120));
        let model = summary
            .models
            .iter()
            .find(|row| row.provider_entry_id == provider_a)
            .unwrap();
        assert_eq!(model.attempt_count, 3);
        assert_eq!(model.success_rate_bps, 5_000);
        assert_eq!(model.average_first_token_ms, Some(120));
    }

    #[test]
    fn request_stats_use_request_denominator_and_last_100_first_tokens() {
        let provider = Uuid::new_v4();
        let mut stats = RequestStats::default();
        for index in 0..101 {
            stats.observe(&UsageRow {
                started_at: index,
                provider_entry_id: provider,
                secret_id: "key".into(),
                model: None,
                input_tokens: 0,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                status: if index == 0 { 502 } else { 200 },
                first_token_ms: Some(index as u64),
            });
        }
        assert_eq!(stats.request_count, 101);
        assert_eq!(stats.success_rate_bps(), 9_901);
        assert_eq!(stats.average_first_token_ms(), Some(50));
    }

    // --- cross-protocol conversion end-to-end ------------------------------

    fn conversion_target(
        base_url: String,
        priority: u16,
        protocol: ProxyProtocol,
    ) -> ResolvedTarget {
        let mut target = test_target(base_url, priority);
        target.config.protocol = Some(protocol);
        target
    }

    fn conversion_route(
        token: &str,
        inbound: ProxyProtocol,
        upstream_fallback: ProxyProtocol,
        targets: Vec<ResolvedTarget>,
        max_attempts: u8,
    ) -> ResolvedRoute {
        ResolvedRoute {
            config: ProxyRouteConfig {
                id: Uuid::new_v4(),
                name: "conversion".into(),
                token: String::new(),
                inbound_protocol: inbound,
                upstream_protocol: upstream_fallback,
                conversion_enabled: true,
                strategy: RouteStrategy::Fallback,
                targets: Vec::new(),
                retry: RetryPolicy {
                    max_attempts,
                    ..RetryPolicy::default()
                },
                enabled: true,
            },
            local_token: token.into(),
            targets,
        }
    }

    /// Runs a mock upstream that captures one request and replies with the
    /// given status, content type, and body.
    fn mock_upstream(
        status: &str,
        content_type: &str,
        body: String,
    ) -> (SocketAddr, std::thread::JoinHandle<(String, Vec<u8>)>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let status = status.to_string();
        let content_type = content_type.to_string();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let captured = read_http_request(&mut stream);
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            captured
        });
        (addr, handle)
    }

    fn start_proxy(bind_addr: SocketAddr, route: ResolvedRoute) -> ProxyHandle {
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage,
        )
        .unwrap()
    }

    #[test]
    fn anthropic_client_to_chat_completions_upstream_non_streaming() {
        let (upstream_addr, upstream) = mock_upstream(
            "200 OK",
            "application/json",
            serde_json::json!({
                "id": "chatcmpl_1", "object": "chat.completion", "model": "gpt-test",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "Hi there"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 12, "completion_tokens": 3}
            })
            .to_string(),
        );
        let bind_addr = available_addr();
        let token = "aipass_conv_am_cc_test";
        let route = conversion_route(
            token,
            ProxyProtocol::AnthropicMessages,
            ProxyProtocol::AnthropicMessages,
            vec![conversion_target(
                format!("http://{upstream_addr}/v1"),
                0,
                ProxyProtocol::OpenAiChatCompletions,
            )],
            1,
        );
        let _proxy = start_proxy(bind_addr, route);

        let request = serde_json::json!({
            "model": "claude-test",
            "system": "Be terse.",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "Hello"}]}],
            "max_tokens": 64
        });
        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/messages"))
            .bearer_auth(token)
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", "fine-grained-tool-results-2025-05-14")
            .json(&request)
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let payload: serde_json::Value = response.json().unwrap();
        assert_eq!(payload["type"], "message");
        assert_eq!(payload["role"], "assistant");
        assert_eq!(
            payload["content"][0],
            serde_json::json!({"type": "text", "text": "Hi there"})
        );
        assert_eq!(payload["stop_reason"], "end_turn");
        assert_eq!(payload["usage"]["input_tokens"], 12);
        assert_eq!(payload["usage"]["output_tokens"], 3);

        let (headers, body) = upstream.join().unwrap();
        // The upstream saw a converted Chat Completions request at the CC path.
        let sent: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            sent["messages"][0],
            serde_json::json!({"role": "system", "content": "Be terse."})
        );
        assert_eq!(
            sent["messages"][1],
            serde_json::json!({"role": "user", "content": "Hello"})
        );
        assert_eq!(sent["max_tokens"], 64);
        assert!(sent.get("system").is_none());
        assert!(sent.get("thinking").is_none());
        assert!(headers.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"));
        // Anthropic-only headers must not leak to the OpenAI-wire upstream.
        assert!(!headers.lines().any(|line| line
            .to_ascii_lowercase()
            .starts_with("anthropic-version:")
            || line.to_ascii_lowercase().starts_with("anthropic-beta:")));
    }

    #[test]
    fn anthropic_client_to_chat_completions_upstream_streaming_tool_call() {
        let sse = concat!(
            "data: {\"id\":\"chatcmpl_1\",\"model\":\"gpt-test\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Let me check.\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"lookup\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"city\\\"\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\":\\\"Paris\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":20,\"completion_tokens\":9}}\n\n",
            "data: [DONE]\n\n",
        );
        let (upstream_addr, upstream) =
            mock_upstream("200 OK", "text/event-stream", sse.to_string());
        let bind_addr = available_addr();
        let token = "aipass_conv_am_cc_stream_test";
        let route = conversion_route(
            token,
            ProxyProtocol::AnthropicMessages,
            ProxyProtocol::AnthropicMessages,
            vec![conversion_target(
                format!("http://{upstream_addr}/v1"),
                0,
                ProxyProtocol::OpenAiChatCompletions,
            )],
            1,
        );
        let _proxy = start_proxy(bind_addr, route);

        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/messages"))
            .bearer_auth(token)
            .json(&serde_json::json!({
                "model": "claude-test",
                "messages": [{"role": "user", "content": "weather?"}],
                "max_tokens": 64,
                "stream": true
            }))
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.text().unwrap();

        for expected in [
            "event: message_start",
            "event: content_block_start",
            "event: content_block_delta",
            "event: content_block_stop",
            "event: message_delta",
            "event: message_stop",
        ] {
            assert!(body.contains(expected), "missing {expected} in:\n{body}");
        }
        assert!(!body.contains("[DONE]"));
        // serde_json emits object keys in sorted order.
        assert!(
            body.contains("\"text\":\"Let me check.\",\"type\":\"text_delta\""),
            "{body}"
        );
        assert!(
            body.contains("\"id\":\"call_1\",\"name\":\"lookup\",\"type\":\"tool_use\""),
            "{body}"
        );
        assert!(
            body.contains("\"partial_json\":\"{\\\"city\\\"\""),
            "{body}"
        );
        assert!(
            body.contains("\"partial_json\":\":\\\"Paris\\\"}\""),
            "{body}"
        );
        assert!(body.contains("\"stop_reason\":\"tool_use\""), "{body}");
        assert!(body.contains("\"output_tokens\":9"), "{body}");

        let (headers, body) = upstream.join().unwrap();
        let sent: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(sent["stream"], true);
        // Usage reporting is requested on the converted CC stream.
        assert_eq!(sent["stream_options"]["include_usage"], true);
        assert!(headers.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"));
    }

    #[test]
    fn chat_completions_client_to_anthropic_upstream_non_streaming() {
        let (upstream_addr, upstream) = mock_upstream(
            "200 OK",
            "application/json",
            serde_json::json!({
                "id": "msg_1", "type": "message", "role": "assistant", "model": "claude-test",
                "content": [
                    {"type": "text", "text": "Checking."},
                    {"type": "tool_use", "id": "toolu_1", "name": "lookup", "input": {"city": "Paris"}}
                ],
                "stop_reason": "tool_use",
                "usage": {"input_tokens": 10, "output_tokens": 8}
            })
            .to_string(),
        );
        let bind_addr = available_addr();
        let token = "aipass_conv_cc_am_test";
        let route = conversion_route(
            token,
            ProxyProtocol::OpenAiChatCompletions,
            ProxyProtocol::OpenAiChatCompletions,
            vec![conversion_target(
                format!("http://{upstream_addr}/v1"),
                0,
                ProxyProtocol::AnthropicMessages,
            )],
            1,
        );
        let _proxy = start_proxy(bind_addr, route);

        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/chat/completions"))
            .bearer_auth(token)
            .json(&serde_json::json!({
                "model": "gpt-test",
                "messages": [
                    {"role": "system", "content": "Be terse."},
                    {"role": "user", "content": "weather?"}
                ]
            }))
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let payload: serde_json::Value = response.json().unwrap();
        assert_eq!(payload["object"], "chat.completion");
        assert_eq!(payload["choices"][0]["message"]["content"], "Checking.");
        assert_eq!(
            payload["choices"][0]["message"]["tool_calls"][0],
            serde_json::json!({"id": "toolu_1", "type": "function", "function": {"name": "lookup", "arguments": "{\"city\":\"Paris\"}"}})
        );
        assert_eq!(payload["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(payload["usage"]["prompt_tokens"], 10);
        assert_eq!(payload["usage"]["completion_tokens"], 8);

        let (headers, body) = upstream.join().unwrap();
        let sent: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(sent["system"], "Be terse.");
        // max_tokens is required by the Anthropic API and defaults when absent.
        assert_eq!(sent["max_tokens"], 4096);
        assert_eq!(
            sent["messages"][0],
            serde_json::json!({"role": "user", "content": [{"type": "text", "text": "weather?"}]})
        );
        assert!(headers.starts_with("POST /v1/messages HTTP/1.1\r\n"));
        // The Anthropic upstream gets the required version header.
        assert!(headers
            .lines()
            .any(|line| line.eq_ignore_ascii_case("anthropic-version: 2023-06-01")));
    }

    #[test]
    fn anthropic_client_to_responses_upstream_non_streaming() {
        let (upstream_addr, upstream) = mock_upstream(
            "200 OK",
            "application/json",
            serde_json::json!({
                "id": "resp_1", "status": "completed", "model": "gpt-test",
                "output": [
                    {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Hi there"}]}
                ],
                "usage": {"input_tokens": 12, "output_tokens": 3}
            })
            .to_string(),
        );
        let bind_addr = available_addr();
        let token = "aipass_conv_am_rs_test";
        let route = conversion_route(
            token,
            ProxyProtocol::AnthropicMessages,
            ProxyProtocol::AnthropicMessages,
            vec![conversion_target(
                format!("http://{upstream_addr}/v1"),
                0,
                ProxyProtocol::OpenAiResponses,
            )],
            1,
        );
        let _proxy = start_proxy(bind_addr, route);

        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/messages"))
            .bearer_auth(token)
            .header("anthropic-version", "2023-06-01")
            .json(&serde_json::json!({
                "model": "claude-test",
                "system": "Be terse.",
                "messages": [{"role": "user", "content": "Hello"}],
                "max_tokens": 64
            }))
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let payload: serde_json::Value = response.json().unwrap();
        assert_eq!(payload["type"], "message");
        assert_eq!(
            payload["content"][0],
            serde_json::json!({"type": "text", "text": "Hi there"})
        );
        assert_eq!(payload["stop_reason"], "end_turn");

        let (headers, body) = upstream.join().unwrap();
        let sent: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(sent["instructions"], "Be terse.");
        assert_eq!(sent["max_output_tokens"], 64);
        assert_eq!(
            sent["input"][0],
            serde_json::json!({"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Hello"}]})
        );
        assert!(headers.starts_with("POST /v1/responses HTTP/1.1\r\n"));
        assert!(!headers
            .lines()
            .any(|line| line.to_ascii_lowercase().starts_with("anthropic-version:")));
    }

    #[test]
    fn invalid_converted_response_fails_over_without_recording_success() {
        let (bad_addr, bad) = mock_upstream("200 OK", "application/json", "not json".into());
        let (good_addr, good) = mock_upstream(
            "200 OK",
            "application/json",
            serde_json::json!({
                "id":"msg_ok", "type":"message", "role":"assistant", "model":"test",
                "content":[{"type":"text","text":"fallback"}],
                "stop_reason":"end_turn", "usage":{"input_tokens":5,"output_tokens":2}
            })
            .to_string(),
        );
        let token = "conversion_failure_test";
        let route = conversion_route(
            token,
            ProxyProtocol::AnthropicMessages,
            ProxyProtocol::AnthropicMessages,
            vec![
                conversion_target(
                    format!("http://{bad_addr}/v1"),
                    0,
                    ProxyProtocol::OpenAiChatCompletions,
                ),
                conversion_target(
                    format!("http://{good_addr}/v1"),
                    1,
                    ProxyProtocol::AnthropicMessages,
                ),
            ],
            2,
        );
        let failed_id = route.targets[0].config.id;
        let bind_addr = available_addr();
        let proxy = start_proxy(bind_addr, route);
        let response = reqwest::blocking::Client::builder().timeout(Duration::from_secs(5)).build().unwrap()
            .post(format!("http://{bind_addr}/v1/messages"))
            .bearer_auth(token)
            .json(&serde_json::json!({"model":"test","messages":[{"role":"user","content":"hello"}],"max_tokens":32}))
            .send().unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.text().unwrap().contains("fallback"));
        bad.join().unwrap();
        good.join().unwrap();
        for _ in 0..100 {
            if proxy.status().requests == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(proxy.status().requests, 1);
        assert_eq!(proxy.status().failures, 0);
        assert!(proxy.status().degraded_target_ids.contains(&failed_id));
        let conn = proxy.state.usage.connection.lock().unwrap();
        let (failed, succeeded): (i64, i64) = conn
            .query_row(
                "SELECT SUM(success = 0), SUM(success = 1) FROM proxy_attempts",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!((failed, succeeded), (1, 1));
    }

    #[test]
    fn mixed_protocol_route_passes_native_through_and_converts_on_failover() {
        // Target 0 is Anthropic-native and fails; target 1 is Chat
        // Completions-native and must receive a converted request.
        let (native_addr, native) = mock_upstream(
            "503 Service Unavailable",
            "application/json",
            r#"{"error":{"message":"down"}}"#.to_string(),
        );
        let (foreign_addr, foreign) = mock_upstream(
            "200 OK",
            "application/json",
            serde_json::json!({
                "id": "chatcmpl_2", "object": "chat.completion", "model": "gpt-test",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "from fallback"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 5, "completion_tokens": 2}
            })
            .to_string(),
        );
        let bind_addr = available_addr();
        let token = "aipass_conv_mixed_test";
        let route = conversion_route(
            token,
            ProxyProtocol::AnthropicMessages,
            ProxyProtocol::AnthropicMessages,
            vec![
                conversion_target(
                    format!("http://{native_addr}/v1"),
                    0,
                    ProxyProtocol::AnthropicMessages,
                ),
                conversion_target(
                    format!("http://{foreign_addr}/v1"),
                    1,
                    ProxyProtocol::OpenAiChatCompletions,
                ),
            ],
            2,
        );
        let _proxy = start_proxy(bind_addr, route);

        let request = serde_json::json!({
            "model": "claude-test",
            "messages": [{"role": "user", "content": "Hello"}],
            "max_tokens": 32
        });
        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/messages"))
            .bearer_auth(token)
            .json(&request)
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let payload: serde_json::Value = response.json().unwrap();
        assert_eq!(
            payload["content"][0],
            serde_json::json!({"type": "text", "text": "from fallback"})
        );

        // The native target received the request bytes losslessly.
        let (_, native_body) = native.join().unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&native_body).unwrap(),
            request
        );
        // The foreign target received a converted Chat Completions request.
        let (_, foreign_body) = foreign.join().unwrap();
        let converted: serde_json::Value = serde_json::from_slice(&foreign_body).unwrap();
        assert_eq!(
            converted["messages"][0],
            serde_json::json!({"role": "user", "content": "Hello"})
        );
        assert!(converted.get("max_tokens").is_some());
    }

    #[test]
    fn start_gate_rejects_unsupported_pairs_and_accepts_supported_conversion() {
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());

        // Chat Completions <-> Responses conversion is not implemented.
        let mut unsupported = conversion_route(
            "aipass_gate_unsupported",
            ProxyProtocol::OpenAiChatCompletions,
            ProxyProtocol::OpenAiChatCompletions,
            vec![conversion_target(
                "http://127.0.0.1:1/v1".into(),
                0,
                ProxyProtocol::OpenAiResponses,
            )],
            1,
        );
        assert!(matches!(
            ProxyHandle::start(
                RuntimeConfig::from_routes(available_addr().to_string(), vec![unsupported.clone()]),
                usage.clone(),
            ),
            Err(ProxyError::InvalidConfig(_))
        ));

        // A protocol mismatch without conversion enabled is a misconfiguration.
        unsupported.config.conversion_enabled = false;
        unsupported.targets[0].config.protocol = Some(ProxyProtocol::AnthropicMessages);
        assert!(matches!(
            ProxyHandle::start(
                RuntimeConfig::from_routes(available_addr().to_string(), vec![unsupported]),
                usage.clone(),
            ),
            Err(ProxyError::InvalidConfig(_))
        ));

        // Anthropic <-> Chat Completions conversion starts fine.
        let supported = conversion_route(
            "aipass_gate_supported",
            ProxyProtocol::AnthropicMessages,
            ProxyProtocol::AnthropicMessages,
            vec![conversion_target(
                "http://127.0.0.1:1/v1".into(),
                0,
                ProxyProtocol::OpenAiChatCompletions,
            )],
            1,
        );
        assert!(ProxyHandle::start(
            RuntimeConfig::from_routes(available_addr().to_string(), vec![supported]),
            usage,
        )
        .is_ok());
    }

    #[test]
    fn start_gate_ignores_disabled_targets_when_validating_protocols() {
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let mut route = single_target_route(
            "aipass_disabled_target_protocol_test",
            "http://127.0.0.1:9/v1".into(),
            RetryPolicy::default(),
        );
        let mut disabled = route.targets[0].clone();
        disabled.config.id = Uuid::new_v4();
        disabled.config.enabled = false;
        disabled.config.protocol = Some(ProxyProtocol::AnthropicMessages);
        route.targets.push(disabled);
        assert!(ProxyHandle::start(
            RuntimeConfig::from_routes(available_addr().to_string(), vec![route]),
            usage,
        )
        .is_ok());
    }
}
