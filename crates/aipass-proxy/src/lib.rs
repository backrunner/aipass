use aipass_proxy_conversion::{
    BuiltinConversionPlugin, ConversionPlugin, ProxyProtocol, TokenUsage,
};
use bytes::Bytes;
use futures_util::{stream, Stream, StreamExt};
use http::{header, Request, Response, StatusCode};
use http_body_util::{BodyExt, Full, LengthLimitError, Limited, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::convert::Infallible;
use std::error::Error as StdError;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use uuid::Uuid;
use zeroize::Zeroize;

pub use aipass_proxy_conversion::{ConversionError, ProxyProtocol as Protocol};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RetryPolicy {
    pub max_attempts: u8,
    pub failure_threshold: u8,
    pub circuit_open_seconds: u64,
    pub connect_timeout_ms: u64,
    pub first_byte_timeout_ms: u64,
    pub stream_idle_timeout_ms: u64,
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
    pub token_fingerprint: String,
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProxyConfig {
    pub enabled: bool,
    pub bind_addr: String,
    pub routes: Vec<ProxyRouteConfig>,
    #[serde(default)]
    pub pricing: Vec<ModelPricing>,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind_addr: "127.0.0.1:8787".into(),
            routes: Vec::new(),
            pricing: Vec::new(),
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
    pub providers: Vec<ProviderUsageAggregate>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UsageTimeseriesPoint {
    pub date: String,
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
    /// Requests completed in the last 60 seconds (for RPM display).
    #[serde(default)]
    pub recent_requests: u64,
    /// Tokens (input + output + cache) consumed in the last 60 seconds (for TPM display).
    #[serde(default)]
    pub recent_tokens: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UsageRecord {
    pub id: Uuid,
    pub started_at: i64,
    pub duration_ms: u64,
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
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; CREATE TABLE IF NOT EXISTS proxy_usage (id TEXT PRIMARY KEY, started_at INTEGER NOT NULL, duration_ms INTEGER NOT NULL, route_id TEXT NOT NULL, provider_entry_id TEXT NOT NULL, secret_id TEXT NOT NULL, model TEXT, inbound_protocol TEXT NOT NULL, upstream_protocol TEXT NOT NULL, status INTEGER NOT NULL, attempts INTEGER NOT NULL, input_tokens INTEGER NOT NULL, output_tokens INTEGER NOT NULL, cache_read_tokens INTEGER NOT NULL, cache_creation_tokens INTEGER NOT NULL, estimated_cost_micros INTEGER NOT NULL); CREATE INDEX IF NOT EXISTS proxy_usage_started_at_idx ON proxy_usage(started_at)").map_err(ProxyError::Sqlite)?;
        Ok(Self {
            path,
            connection: Mutex::new(connection),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn record(&self, item: &UsageRecord) -> Result<(), ProxyError> {
        let conn = self.connection.lock().map_err(|_| ProxyError::Poisoned)?;
        conn.execute("INSERT OR REPLACE INTO proxy_usage (id, started_at, duration_ms, route_id, provider_entry_id, secret_id, model, inbound_protocol, upstream_protocol, status, attempts, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens, estimated_cost_micros) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)", params![item.id.to_string(), item.started_at, item.duration_ms, item.route_id.to_string(), item.provider_entry_id.to_string(), item.secret_id, item.model, serde_json::to_string(&item.inbound_protocol).unwrap_or_default(), serde_json::to_string(&item.upstream_protocol).unwrap_or_default(), item.status, item.attempts, item.input_tokens, item.output_tokens, item.cache_read_tokens, item.cache_creation_tokens, item.estimated_cost_micros]).map_err(ProxyError::Sqlite)?;
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
        conn.execute("DELETE FROM proxy_usage", [])
            .map_err(ProxyError::Sqlite)
            .and_then(|_| {
                conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                    .map_err(ProxyError::Sqlite)
            })
    }

    pub fn iter_rows(&self) -> Result<Vec<UsageRow>, ProxyError> {
        Ok(self
            .rows_since(None)?
            .into_iter()
            .map(|(_, row)| row)
            .collect())
    }

    pub fn summary(&self, cost: impl Fn(&UsageRow) -> u64) -> Result<UsageAggregate, ProxyError> {
        let mut aggregate = UsageAggregate::default();
        let mut providers: HashMap<(Uuid, String), (ProviderUsageAggregate, i64)> = HashMap::new();
        self.visit_rows_since(None, |_, row| {
            let row_cost = cost(&row);
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
            let (provider, last_started) = providers.entry(key).or_insert_with(|| {
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
        })?;
        let mut providers: Vec<(ProviderUsageAggregate, i64)> = providers.into_values().collect();
        providers.sort_by_key(|provider| std::cmp::Reverse(provider.1));
        aggregate.providers = providers
            .into_iter()
            .map(|(provider, _)| provider)
            .collect();
        Ok(aggregate)
    }

    pub fn timeseries(
        &self,
        days: u32,
        cost: impl Fn(&UsageRow) -> u64,
    ) -> Result<Vec<UsageTimeseriesPoint>, ProxyError> {
        let days = i64::from(days.max(1));
        let today_start = now_unix() / 86_400 * 86_400;
        let cutoff = today_start - (days - 1) * 86_400;
        let mut buckets: std::collections::BTreeMap<String, UsageTimeseriesPoint> =
            std::collections::BTreeMap::new();
        self.visit_rows_since(Some(cutoff), |date, row| {
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
            point.estimated_cost_micros = point.estimated_cost_micros.saturating_add(cost(&row));
        })?;
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
        mut visit: impl FnMut(String, UsageRow),
    ) -> Result<(), ProxyError> {
        let conn = self.connection.lock().map_err(|_| ProxyError::Poisoned)?;
        let columns = "date(started_at, 'unixepoch'), started_at, provider_entry_id, secret_id, model, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens";
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
        },
    ))
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

#[derive(Clone, Debug)]
pub struct ResolvedTarget {
    pub config: ProxyTargetConfig,
    pub api_key: String,
}

impl Drop for ResolvedTarget {
    fn drop(&mut self) {
        self.api_key.zeroize();
    }
}

#[derive(Clone, Debug)]
pub struct ResolvedRoute {
    pub config: ProxyRouteConfig,
    pub local_token: String,
    pub targets: Vec<ResolvedTarget>,
}

#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    pub enabled: bool,
    pub bind_addr: String,
    pub routes: Vec<ResolvedRoute>,
    pub pricing: Vec<ModelPricing>,
}

impl RuntimeConfig {
    pub fn from_routes(bind_addr: impl Into<String>, routes: Vec<ResolvedRoute>) -> Self {
        Self {
            enabled: true,
            bind_addr: bind_addr.into(),
            routes,
            pricing: Vec::new(),
        }
    }
}

#[derive(Clone)]
struct RuntimeState {
    config: Arc<RwLock<RuntimeConfig>>,
    stats: Arc<Mutex<RuntimeStats>>,
    usage: Arc<UsageStore>,
    health: Arc<Mutex<HashMap<Uuid, TargetHealth>>>,
    rr_counters: Arc<Mutex<HashMap<Uuid, AtomicU64>>>,
}

#[derive(Default)]
struct RuntimeStats {
    requests: u64,
    failures: u64,
    last_error: Option<String>,
}

#[derive(Default)]
struct TargetHealth {
    consecutive_failures: u8,
    open_until: Option<Instant>,
}

pub struct ProxyHandle {
    state: RuntimeState,
    stop: Option<oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
    bind_addr: String,
}

impl ProxyHandle {
    pub fn start(config: RuntimeConfig, usage: Arc<UsageStore>) -> Result<Self, ProxyError> {
        if config.routes.iter().any(|route| {
            route.config.conversion_enabled
                || route.config.inbound_protocol != route.config.upstream_protocol
        }) {
            return Err(ProxyError::InvalidConfig(
                "protocol conversion is not available in this release".into(),
            ));
        }
        let bind_addr = config.bind_addr.clone();
        let socket: SocketAddr = bind_addr
            .parse()
            .map_err(|_| ProxyError::InvalidConfig("bind address must be host:port".into()))?;
        let state = RuntimeState {
            config: Arc::new(RwLock::new(config)),
            stats: Arc::new(Mutex::new(RuntimeStats::default())),
            usage,
            health: Arc::new(Mutex::new(HashMap::new())),
            rr_counters: Arc::new(Mutex::new(HashMap::new())),
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
                                    let _ = hyper::server::conn::http1::Builder::new().serve_connection(io, service).await;
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
        Ok(Self {
            state,
            stop: Some(stop_tx),
            thread: Some(thread),
            bind_addr,
        })
    }

    pub fn status(&self) -> ProxyStatus {
        let config = self.state.config.read().map(|c| c.clone()).ok();
        let stats = self
            .state
            .stats
            .lock()
            .map(|s| (s.requests, s.failures, s.last_error.clone()))
            .unwrap_or_default();
        ProxyStatus {
            running: self
                .thread
                .as_ref()
                .is_some_and(|thread| !thread.is_finished()),
            enabled: config.as_ref().is_some_and(|c| c.enabled),
            bind_addr: self.bind_addr.clone(),
            active_routes: config
                .map(|c| c.routes.iter().filter(|r| r.config.enabled).count())
                .unwrap_or(0),
            requests: stats.0,
            failures: stats.1,
            last_error: stats.2,
            recent_requests: 0,
            recent_tokens: 0,
        }
    }

    pub fn update_config(&self, config: RuntimeConfig) -> Result<(), ProxyError> {
        *self
            .state
            .config
            .write()
            .map_err(|_| ProxyError::Poisoned)? = config;
        Ok(())
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
    }
}

pub fn fingerprint_token(token: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(token.as_bytes());
    hex_encode(&digest.finalize())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

type BoxError = Box<dyn StdError + Send + Sync>;
type BoxBody = http_body_util::combinators::UnsyncBoxBody<Bytes, BoxError>;

async fn handle_request(
    request: Request<Incoming>,
    state: RuntimeState,
) -> Result<Response<BoxBody>, Infallible> {
    let started = Instant::now();
    let path = request.uri().path().to_string();
    let method = request.method().clone();
    let request_query = request.uri().query().map(str::to_owned);
    let Some(inbound) = ProxyProtocol::from_path(&path) else {
        return Ok(error_response(
            StatusCode::NOT_FOUND,
            "unsupported proxy path",
        ));
    };
    let incoming_headers = request.headers().clone();
    let token = incoming_headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            let (scheme, token) = value.split_once(' ')?;
            scheme
                .eq_ignore_ascii_case("bearer")
                .then_some(token.trim())
        })
        .or_else(|| {
            incoming_headers
                .get("x-api-key")
                .and_then(|value| value.to_str().ok())
        });
    let Some(token) = token else {
        return Ok(error_response(
            StatusCode::UNAUTHORIZED,
            "missing local proxy token",
        ));
    };
    let selected = state.config.read().ok().and_then(|config| {
        config
            .routes
            .iter()
            .find(|route| {
                route.config.enabled
                    && route.config.inbound_protocol == inbound
                    && route.config.token_fingerprint == fingerprint_token(token)
            })
            .cloned()
            .map(|route| (route, config.pricing.clone()))
    });
    let Some((route, pricing)) = selected else {
        return Ok(error_response(
            StatusCode::UNAUTHORIZED,
            "invalid local proxy token or route",
        ));
    };
    let max_body = 16 * 1024 * 1024;
    let body = match Limited::new(request.into_body(), max_body).collect().await {
        Ok(body) => body.to_bytes(),
        Err(err) if err.downcast_ref::<LengthLimitError>().is_some() => {
            return Ok(error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                "proxy request body too large",
            ))
        }
        Err(err) => return Ok(error_response(StatusCode::BAD_REQUEST, &err.to_string())),
    };
    let request_json = serde_json::from_slice::<serde_json::Value>(&body).ok();
    let mut attempts = 0_u8;
    let mut last_error = None;
    let mut targets: Vec<ResolvedTarget> = route
        .targets
        .iter()
        .filter(|target| target.config.enabled)
        .cloned()
        .collect();
    targets.sort_by_key(|target| target.config.priority);
    if route.config.strategy == RouteStrategy::RoundRobin {
        let start = round_robin_start(
            &state,
            route.config.id,
            &targets
                .iter()
                .map(|target| target.config.weight)
                .collect::<Vec<_>>(),
        );
        targets.rotate_left(start);
    }
    for target in targets {
        if circuit_open(&state, target.config.id) {
            continue;
        }
        if attempts >= route.config.retry.max_attempts.max(1) {
            break;
        }
        attempts += 1;
        let client = match reqwest::Client::builder()
            .connect_timeout(Duration::from_millis(
                route.config.retry.connect_timeout_ms.max(1),
            ))
            .build()
        {
            Ok(client) => client,
            Err(err) => {
                last_error = Some(err.to_string());
                mark_failure(&state, target.config.id, &route.config.retry);
                continue;
            }
        };
        let mut payload = body.clone();
        if route.config.conversion_enabled
            && route.config.inbound_protocol != route.config.upstream_protocol
        {
            let Some(json_payload) = request_json.clone() else {
                return Ok(error_response(
                    StatusCode::BAD_REQUEST,
                    "protocol conversion requires a JSON request",
                ));
            };
            payload = match BuiltinConversionPlugin
                .convert_request(
                    route.config.inbound_protocol,
                    route.config.upstream_protocol,
                    json_payload,
                )
                .and_then(|value| {
                    serde_json::to_vec(&value).map_err(|err| {
                        aipass_proxy_conversion::ConversionError::InvalidPayload {
                            protocol: route.config.inbound_protocol,
                            message: err.to_string(),
                        }
                    })
                }) {
                Ok(payload) => Bytes::from(payload),
                Err(err) => return Ok(error_response(StatusCode::BAD_REQUEST, &err.to_string())),
            };
        }
        let url = match upstream_url_with_query(
            &target.config.base_url,
            route.config.upstream_protocol.path(),
            request_query.as_deref(),
        ) {
            Ok(url) => url,
            Err(err) => {
                last_error = Some(err.to_string());
                mark_failure(&state, target.config.id, &route.config.retry);
                continue;
            }
        };
        let mut upstream = client.request(method.clone(), url).body(payload);
        for (name, value) in incoming_headers.iter() {
            if !is_hop_header(name)
                && name != header::AUTHORIZATION
                && name != "x-api-key"
                && name != "api-key"
                && name != header::CONTENT_LENGTH
                && name != header::HOST
            {
                upstream = upstream.header(name, value);
            }
        }
        upstream = match target.config.auth_scheme.as_str() {
            "x_api_key" => upstream.header("x-api-key", &target.api_key),
            "azure_api_key" => upstream.header("api-key", &target.api_key),
            _ => upstream.bearer_auth(&target.api_key),
        };
        for (name, value) in &target.config.headers {
            upstream = upstream.header(name, value);
        }
        if route.config.upstream_protocol == ProxyProtocol::AnthropicMessages
            && !target
                .config
                .headers
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("anthropic-version"))
        {
            upstream = upstream.header("anthropic-version", "2023-06-01");
        }
        upstream = upstream.header(header::CONTENT_TYPE, "application/json");
        let response = match upstream.send().await {
            Ok(response) => response,
            Err(err) => {
                last_error = Some(err.to_string());
                mark_failure(&state, target.config.id, &route.config.retry);
                continue;
            }
        };
        let status = response.status();
        let retryable_status = is_retryable_status(status);
        if retryable_status {
            mark_failure(&state, target.config.id, &route.config.retry);
            if attempts < route.config.retry.max_attempts.max(1) {
                let _ = response.bytes().await;
                last_error = Some(format!("upstream returned {status}"));
                continue;
            }
        }
        let response_headers = response.headers().clone();
        let mut upstream_stream = response.bytes_stream();
        let first_chunk = match tokio::time::timeout(
            Duration::from_millis(route.config.retry.first_byte_timeout_ms.max(1)),
            upstream_stream.next(),
        )
        .await
        {
            Ok(Some(Ok(chunk))) => Some(chunk),
            Ok(Some(Err(err))) => {
                last_error = Some(err.to_string());
                mark_failure(&state, target.config.id, &route.config.retry);
                continue;
            }
            Ok(None) => None,
            Err(_) => {
                last_error = Some("upstream first-byte timeout".into());
                mark_failure(&state, target.config.id, &route.config.retry);
                continue;
            }
        };
        if !retryable_status {
            mark_success(&state, target.config.id);
        }
        let content_type = response_headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        let conversion = route.config.conversion_enabled
            && route.config.inbound_protocol != route.config.upstream_protocol;
        let upstream_protocol = route.config.upstream_protocol;
        let inbound_protocol = route.config.inbound_protocol;
        let model = request_json
            .as_ref()
            .and_then(|value| value.get("model"))
            .and_then(|value| value.as_str())
            .map(str::to_owned);
        let model_pricing = model.as_deref().and_then(|model| {
            pricing
                .iter()
                .filter(|item| item.model == model || model.starts_with(&item.model))
                .max_by_key(|item| item.model.len())
                .cloned()
        });
        let record = UsageRecord {
            id: Uuid::new_v4(),
            started_at: now_unix(),
            duration_ms: started.elapsed().as_millis() as u64,
            route_id: route.config.id,
            provider_entry_id: target.config.provider_entry_id,
            secret_id: target.config.secret_id.clone(),
            model,
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
        let body_stream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>> =
            if let Some(first_chunk) = first_chunk {
                Box::pin(stream::once(async move { Ok(first_chunk) }).chain(upstream_stream))
            } else {
                Box::pin(stream::empty())
            };
        let body_stream = track_usage_stream(
            body_stream,
            UsageTrackingContext {
                protocol: upstream_protocol,
                store: state.usage.clone(),
                record,
                pricing: model_pricing,
                stream_idle_timeout: Duration::from_millis(
                    route.config.retry.stream_idle_timeout_ms.max(1),
                ),
                failure_state: state.clone(),
                target_id: target.config.id,
                retry_policy: route.config.retry.clone(),
            },
        );
        let output_stream: Pin<Box<dyn Stream<Item = Result<Bytes, BoxError>> + Send>> =
            if conversion && content_type.contains("text/event-stream") {
                convert_sse_stream(body_stream, upstream_protocol, inbound_protocol)
            } else if conversion {
                let bytes = match body_stream
                    .collect::<Vec<_>>()
                    .await
                    .into_iter()
                    .collect::<Result<Vec<_>, _>>()
                {
                    Ok(parts) => parts.into_iter().fold(Bytes::new(), |all, part| {
                        let mut data = all.to_vec();
                        data.extend_from_slice(&part);
                        Bytes::from(data)
                    }),
                    Err(err) => {
                        return Ok(error_response(StatusCode::BAD_GATEWAY, &err.to_string()))
                    }
                };
                let converted = match serde_json::from_slice::<serde_json::Value>(&bytes)
                    .ok()
                    .and_then(|value| {
                        BuiltinConversionPlugin
                            .convert_response(upstream_protocol, inbound_protocol, value)
                            .ok()
                    })
                    .and_then(|value| serde_json::to_vec(&value).ok())
                {
                    Some(value) => Bytes::from(value),
                    None => {
                        return Ok(error_response(
                            StatusCode::BAD_GATEWAY,
                            "protocol conversion failed for upstream response",
                        ))
                    }
                };
                Box::pin(stream::once(async move { Ok(converted) }))
            } else {
                Box::pin(body_stream)
            };
        let frame_stream = output_stream.map(|result| result.map(Frame::data));
        let stream_body = BodyExt::boxed_unsync(StreamBody::new(frame_stream));
        let mut builder = Response::builder().status(status);
        for (name, value) in response_headers.iter() {
            if !is_hop_header(name)
                && name != header::CONTENT_LENGTH
                && !(conversion && name == header::CONTENT_ENCODING)
            {
                builder = builder.header(name, value);
            }
        }
        let response = builder.body(stream_body).unwrap_or_else(|_| {
            error_response(StatusCode::BAD_GATEWAY, "failed to build proxy response")
        });
        if let Ok(mut stats) = state.stats.lock() {
            stats.requests += 1;
            if !status.is_success() {
                stats.failures += 1;
            }
        }
        return Ok(response);
    }
    if let Ok(mut stats) = state.stats.lock() {
        stats.requests += 1;
        stats.failures += 1;
        stats.last_error = last_error.clone();
    }
    Ok(error_response(
        StatusCode::BAD_GATEWAY,
        &last_error.unwrap_or_else(|| "all upstream targets failed".into()),
    ))
}

fn convert_sse_stream<S>(
    source: S,
    from: ProxyProtocol,
    to: ProxyProtocol,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, BoxError>> + Send>>
where
    S: Stream<Item = Result<Bytes, BoxError>> + Send + 'static,
{
    let source = Box::pin(source);
    Box::pin(stream::unfold(
        (source, Vec::<u8>::new(), VecDeque::<Bytes>::new(), false),
        move |(mut source, mut buffer, mut pending, mut done)| async move {
            loop {
                if let Some(value) = pending.pop_front() {
                    return Some((Ok(value), (source, buffer, pending, done)));
                }
                if done {
                    return None;
                }
                if let Some(index) = buffer.windows(2).position(|window| window == b"\n\n") {
                    let event = String::from_utf8_lossy(&buffer[..index + 2]).to_string();
                    buffer.drain(..index + 2);
                    match BuiltinConversionPlugin.convert_stream_event(from, to, &event) {
                        Ok(events) => pending.extend(events.into_iter().map(Bytes::from)),
                        Err(err) => {
                            done = true;
                            return Some((
                                Err(Box::new(err) as BoxError),
                                (source, buffer, pending, done),
                            ));
                        }
                    }
                    continue;
                }
                match source.next().await {
                    Some(Ok(chunk)) => buffer.extend_from_slice(&chunk),
                    Some(Err(err)) => {
                        done = true;
                        return Some((Err(err), (source, buffer, pending, done)));
                    }
                    None => {
                        done = true;
                        if !buffer.is_empty() {
                            let event = String::from_utf8_lossy(&buffer).to_string();
                            buffer.clear();
                            match BuiltinConversionPlugin.convert_stream_event(from, to, &event) {
                                Ok(events) => pending.extend(events.into_iter().map(Bytes::from)),
                                Err(err) => {
                                    return Some((
                                        Err(Box::new(err) as BoxError),
                                        (source, buffer, pending, done),
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
    protocol: ProxyProtocol,
    store: Arc<UsageStore>,
    record: UsageRecord,
    pricing: Option<ModelPricing>,
    stream_idle_timeout: Duration,
    failure_state: RuntimeState,
    target_id: Uuid,
    retry_policy: RetryPolicy,
}

fn track_usage_stream<S>(
    source: S,
    context: UsageTrackingContext,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, BoxError>> + Send>>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    let UsageTrackingContext {
        protocol,
        store,
        mut record,
        pricing,
        stream_idle_timeout,
        failure_state,
        target_id,
        retry_policy,
    } = context;
    let mut source = Box::pin(source);
    let (sender, receiver) = tokio::sync::mpsc::channel(8);
    tokio::spawn(async move {
        let mut tail = Vec::new();
        loop {
            let result: Result<Bytes, BoxError> =
                match tokio::time::timeout(stream_idle_timeout, source.next()).await {
                    Ok(Some(result)) => result.map_err(|err| Box::new(err) as BoxError),
                    Ok(None) => break,
                    Err(_) => Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "upstream stream idle timeout",
                    ))),
                };
            if let Ok(chunk) = &result {
                tail.extend_from_slice(chunk);
                const USAGE_TAIL_LIMIT: usize = 256 * 1024;
                if tail.len() > USAGE_TAIL_LIMIT {
                    tail.drain(..tail.len() - USAGE_TAIL_LIMIT);
                }
            }
            let failed = result.is_err();
            if let Err(err) = &result {
                mark_failure(&failure_state, target_id, &retry_policy);
                set_error(&failure_state, err.to_string());
            }
            if sender.send(result).await.is_err() || failed {
                break;
            }
        }
        let usage = usage_from_wire_bytes(protocol, &tail);
        record.input_tokens = usage.input_tokens;
        record.output_tokens = usage.output_tokens;
        record.cache_read_tokens = usage.cache_read_tokens;
        record.cache_creation_tokens = usage.cache_creation_tokens;
        record.estimated_cost_micros = pricing
            .as_ref()
            .map(|pricing| estimate_cost(&usage, pricing))
            .unwrap_or(0);
        let _ = store.record(&record);
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
        return BuiltinConversionPlugin.extract_usage(protocol, &value);
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
        let usage = BuiltinConversionPlugin.extract_usage(protocol, &value);
        total.input_tokens = total.input_tokens.max(usage.input_tokens);
        total.output_tokens = total.output_tokens.max(usage.output_tokens);
        total.cache_read_tokens = total.cache_read_tokens.max(usage.cache_read_tokens);
        total.cache_creation_tokens = total.cache_creation_tokens.max(usage.cache_creation_tokens);
    }
    total
}

#[cfg(test)]
fn upstream_url(base_url: &str, path: &str) -> Result<String, ProxyError> {
    upstream_url_with_query(base_url, path, None)
}

fn upstream_url_with_query(
    base_url: &str,
    path: &str,
    query: Option<&str>,
) -> Result<String, ProxyError> {
    let base =
        reqwest::Url::parse(base_url).map_err(|err| ProxyError::InvalidConfig(err.to_string()))?;
    let base_path = base.path().trim_end_matches('/').to_string();
    let suffix = if base_path == "/v1" || base_path.ends_with("/v1") {
        path.strip_prefix("/v1").unwrap_or(path)
    } else {
        path
    };
    let mut url = base;
    url.set_path(&format!("{}{}", base_path, suffix));
    if query.is_some() {
        url.set_query(query);
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
    let Ok(mut health) = state.health.lock() else {
        return false;
    };
    let Some(target) = health.get_mut(&target_id) else {
        return false;
    };
    if let Some(open_until) = target.open_until {
        if Instant::now() < open_until {
            return true;
        }
        target.open_until = None;
        target.consecutive_failures = 0;
    }
    false
}

fn mark_failure(state: &RuntimeState, target_id: Uuid, policy: &RetryPolicy) {
    let Ok(mut health) = state.health.lock() else {
        return;
    };
    let target = health.entry(target_id).or_default();
    target.consecutive_failures = target.consecutive_failures.saturating_add(1);
    if target.consecutive_failures >= policy.failure_threshold.max(1) {
        target.open_until = Some(Instant::now() + Duration::from_secs(policy.circuit_open_seconds));
    }
}

fn mark_success(state: &RuntimeState, target_id: Uuid) {
    if let Ok(mut health) = state.health.lock() {
        health.remove(&target_id);
    }
}

fn is_retryable_status(status: StatusCode) -> bool {
    status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
        || status == StatusCode::UNAUTHORIZED
        || status == StatusCode::FORBIDDEN
}
fn is_hop_header(name: &header::HeaderName) -> bool {
    matches!(
        name.as_str(),
        "connection"
            | "keep-alive"
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
    if let Ok(mut stats) = state.stats.lock() {
        stats.failures += 1;
        stats.last_error = Some(error);
    }
}
fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn available_addr() -> SocketAddr {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        addr
    }

    fn single_target_route(token: &str, base_url: String, retry: RetryPolicy) -> ResolvedRoute {
        let target_id = Uuid::new_v4();
        ResolvedRoute {
            config: ProxyRouteConfig {
                id: Uuid::new_v4(),
                name: "test".into(),
                token_fingerprint: fingerprint_token(token),
                token: token.into(),
                inbound_protocol: ProxyProtocol::OpenAiResponses,
                upstream_protocol: ProxyProtocol::OpenAiResponses,
                conversion_enabled: false,
                strategy: RouteStrategy::Fallback,
                targets: Vec::new(),
                retry,
                enabled: true,
            },
            local_token: String::new(),
            targets: vec![ResolvedTarget {
                config: ProxyTargetConfig {
                    id: target_id,
                    provider_entry_id: Uuid::new_v4(),
                    secret_id: "primary".into(),
                    label: "primary".into(),
                    base_url,
                    auth_scheme: "bearer".into(),
                    headers: Vec::new(),
                    group: None,
                    priority: 0,
                    weight: 1,
                    enabled: true,
                },
                api_key: "upstream-secret".into(),
            }],
        }
    }

    #[test]
    fn fingerprints_are_stable_without_retaining_token() {
        assert_eq!(fingerprint_token("local-test-token").len(), 64);
        assert_ne!(
            fingerprint_token("local-test-token"),
            fingerprint_token("other")
        );
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
    fn usage_store_persists_records() {
        let temp = tempfile::tempdir().unwrap();
        let store = UsageStore::open(temp.path().join("usage.sqlite")).unwrap();
        store
            .record(&UsageRecord {
                id: Uuid::new_v4(),
                started_at: 1,
                duration_ms: 2,
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

    #[test]
    fn request_body_limit_is_enforced_while_reading() {
        let bind_addr = available_addr();
        let dead_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let token = "aipass_body_limit_test";
        let route = single_target_route(
            token,
            format!("http://{dead_addr}/v1"),
            RetryPolicy::default(),
        );
        let _proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage,
        )
        .unwrap();

        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/responses"))
            .bearer_auth(token)
            .body(vec![b'x'; 16 * 1024 * 1024 + 1])
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[test]
    fn stream_idle_timeout_is_reported_as_truncated_body() {
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
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.bytes().is_err());
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
            },
            api_key: "upstream-secret".into(),
        };
        let token = "aipass_local_test";
        let route = ResolvedRoute {
            config: ProxyRouteConfig {
                id: route_id,
                name: "test".into(),
                token_fingerprint: fingerprint_token(token),
                token: token.to_string(),
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
            local_token: String::new(),
            targets: vec![
                target(Uuid::new_v4(), format!("http://{dead_addr}/v1"), 0),
                target(Uuid::new_v4(), format!("http://{upstream_addr}/v1"), 1),
            ],
        };
        let _proxy = ProxyHandle::start(
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
    }

    #[test]
    fn route_config_defaults_keep_fallback_strategy() {
        let route: ProxyRouteConfig = serde_json::from_value(serde_json::json!({
            "id": Uuid::new_v4(),
            "name": "legacy",
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
        assert_eq!(route.token, "");
        assert_eq!(route.targets[0].weight, 1);
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
        let record = |started_at, input_tokens| UsageRecord {
            id: Uuid::new_v4(),
            started_at,
            duration_ms: 1,
            route_id: Uuid::new_v4(),
            provider_entry_id: Uuid::new_v4(),
            secret_id: "key".into(),
            model: None,
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
        store.record(&record(today_start + 60, 10)).unwrap();
        store.record(&record(today_start + 120, 5)).unwrap();
        store.record(&record(today_start - 86_400, 7)).unwrap();
        store
            .record(&record(today_start - 10 * 86_400, 99))
            .unwrap();

        let points = store.timeseries(7, |_| 3).unwrap();
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].request_count, 1);
        assert_eq!(points[0].input_tokens, 7);
        assert_eq!(points[1].request_count, 2);
        assert_eq!(points[1].input_tokens, 15);
        assert_eq!(points[1].output_tokens, 4);
        assert_eq!(points[1].estimated_cost_micros, 6);
        assert!(points.iter().all(|point| point.input_tokens != 99));
    }

    #[test]
    fn usage_summary_recomputes_cost_with_injected_resolver() {
        let temp = tempfile::tempdir().unwrap();
        let store = UsageStore::open(temp.path().join("usage.sqlite")).unwrap();
        let provider_a = Uuid::new_v4();
        let provider_b = Uuid::new_v4();
        let record = |provider_entry_id, secret_id: &str, started_at, input_tokens| UsageRecord {
            id: Uuid::new_v4(),
            started_at,
            duration_ms: 1,
            route_id: Uuid::new_v4(),
            provider_entry_id,
            secret_id: secret_id.into(),
            model: Some("gpt-test".into()),
            inbound_protocol: ProxyProtocol::OpenAiResponses,
            upstream_protocol: ProxyProtocol::OpenAiResponses,
            status: 200,
            attempts: 1,
            input_tokens,
            output_tokens: 2,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            estimated_cost_micros: 0,
        };
        store.record(&record(provider_a, "key", 10, 100)).unwrap();
        store.record(&record(provider_a, "key", 20, 50)).unwrap();
        store.record(&record(provider_b, "key", 30, 10)).unwrap();

        // Stored estimated_cost_micros is 0; the injected resolver recomputes
        // cost per row at query time.
        let summary = store.summary(|row| row.input_tokens * 2).unwrap();
        assert_eq!(summary.request_count, 3);
        assert_eq!(summary.input_tokens, 160);
        assert_eq!(summary.output_tokens, 6);
        assert_eq!(summary.estimated_cost_micros, 320);
        assert_eq!(summary.providers.len(), 2);
        // Providers are ordered by most recent usage first.
        assert_eq!(summary.providers[0].provider_entry_id, provider_b);
        assert_eq!(summary.providers[0].estimated_cost_micros, 20);
        assert_eq!(summary.providers[1].provider_entry_id, provider_a);
        assert_eq!(summary.providers[1].request_count, 2);
        assert_eq!(summary.providers[1].estimated_cost_micros, 300);

        let rows = store.iter_rows().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].started_at, 10);
        assert_eq!(rows[0].model.as_deref(), Some("gpt-test"));
    }
}
