use aipass_proxy_conversion::{
    BuiltinConversionPlugin, ConversionPlugin, ProxyProtocol, TokenUsage,
};
use bytes::Bytes;
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AttemptRecord {
    pub id: Uuid,
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
    first_token_total_ms: u64,
    first_token_samples: u64,
}

impl AttemptStats {
    fn observe(&mut self, row: &AttemptRow) {
        self.attempt_count = self.attempt_count.saturating_add(1);
        if let Some(success) = row.success {
            self.completed_attempts = self.completed_attempts.saturating_add(1);
            if success {
                self.successful_attempts = self.successful_attempts.saturating_add(1);
                if let Some(first_token_ms) = row.first_token_ms {
                    self.first_token_total_ms =
                        self.first_token_total_ms.saturating_add(first_token_ms);
                    self.first_token_samples = self.first_token_samples.saturating_add(1);
                }
            }
        }
    }

    fn success_rate_bps(self) -> u16 {
        if self.completed_attempts == 0 {
            return 0;
        }
        let bps = self
            .successful_attempts
            .saturating_mul(10_000)
            .saturating_add(self.completed_attempts / 2)
            / self.completed_attempts;
        u16::try_from(bps.min(10_000)).unwrap_or(10_000)
    }

    fn average_first_token_ms(self) -> Option<u64> {
        (self.first_token_samples > 0).then(|| self.first_token_total_ms / self.first_token_samples)
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
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; CREATE TABLE IF NOT EXISTS proxy_usage (id TEXT PRIMARY KEY, started_at INTEGER NOT NULL, duration_ms INTEGER NOT NULL, route_id TEXT NOT NULL, provider_entry_id TEXT NOT NULL, secret_id TEXT NOT NULL, model TEXT, inbound_protocol TEXT NOT NULL, upstream_protocol TEXT NOT NULL, status INTEGER NOT NULL, attempts INTEGER NOT NULL, input_tokens INTEGER NOT NULL, output_tokens INTEGER NOT NULL, cache_read_tokens INTEGER NOT NULL, cache_creation_tokens INTEGER NOT NULL, estimated_cost_micros INTEGER NOT NULL); CREATE INDEX IF NOT EXISTS proxy_usage_started_at_idx ON proxy_usage(started_at); CREATE TABLE IF NOT EXISTS proxy_attempts (id TEXT PRIMARY KEY, started_at INTEGER NOT NULL, duration_ms INTEGER NOT NULL, first_token_ms INTEGER, route_id TEXT NOT NULL, target_id TEXT NOT NULL, provider_entry_id TEXT NOT NULL, secret_id TEXT NOT NULL, model TEXT, status INTEGER, success INTEGER); CREATE INDEX IF NOT EXISTS proxy_attempts_started_at_idx ON proxy_attempts(started_at)").map_err(ProxyError::Sqlite)?;
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

    pub fn record_attempt(&self, item: &AttemptRecord) -> Result<(), ProxyError> {
        let conn = self.connection.lock().map_err(|_| ProxyError::Poisoned)?;
        conn.execute(
            "INSERT OR REPLACE INTO proxy_attempts (id, started_at, duration_ms, first_token_ms, route_id, target_id, provider_entry_id, secret_id, model, status, success) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                item.id.to_string(),
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
        let mut aggregate = UsageAggregate::default();
        let mut providers: HashMap<(Uuid, String), (ProviderUsageAggregate, i64)> = HashMap::new();
        let mut models: HashMap<(Uuid, String, Option<String>), (ModelUsageAggregate, i64)> =
            HashMap::new();
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

            let model_key = (
                row.provider_entry_id,
                row.secret_id.clone(),
                row.model.clone(),
            );
            let (model, last_started) = models.entry(model_key.clone()).or_insert_with(|| {
                (
                    ModelUsageAggregate {
                        provider_entry_id: model_key.0,
                        secret_id: model_key.1,
                        model: model_key.2,
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
        })?;
        let mut attempt_stats = AttemptStats::default();
        let mut provider_attempts: HashMap<(Uuid, String), AttemptStats> = HashMap::new();
        let mut model_attempts: HashMap<(Uuid, String, Option<String>), AttemptStats> =
            HashMap::new();
        self.visit_attempt_rows(|row| {
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
            provider.success_rate_bps = stats.success_rate_bps();
            provider.average_first_token_ms = stats.average_first_token_ms();

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
            model.success_rate_bps = stats.success_rate_bps();
            model.average_first_token_ms = stats.average_first_token_ms();
        })?;
        aggregate.attempt_count = attempt_stats.attempt_count;
        aggregate.completed_attempts = attempt_stats.completed_attempts;
        aggregate.successful_attempts = attempt_stats.successful_attempts;
        aggregate.success_rate_bps = attempt_stats.success_rate_bps();
        aggregate.average_first_token_ms = attempt_stats.average_first_token_ms();
        let mut providers: Vec<(ProviderUsageAggregate, i64)> = providers.into_values().collect();
        providers.sort_by_key(|provider| std::cmp::Reverse(provider.1));
        aggregate.providers = providers
            .into_iter()
            .map(|(provider, _)| provider)
            .collect();
        let mut models: Vec<(ModelUsageAggregate, i64)> = models.into_values().collect();
        models.sort_by_key(|model| std::cmp::Reverse(model.1));
        aggregate.models = models.into_iter().map(|(model, _)| model).collect();
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

    fn visit_attempt_rows(&self, mut visit: impl FnMut(AttemptRow)) -> Result<(), ProxyError> {
        let conn = self.connection.lock().map_err(|_| ProxyError::Poisoned)?;
        let mut statement = conn
            .prepare(
                "SELECT started_at, provider_entry_id, secret_id, model, success, first_token_ms FROM proxy_attempts ORDER BY started_at",
            )
            .map_err(ProxyError::Sqlite)?;
        let rows = statement
            .query_map([], decode_attempt_row)
            .map_err(ProxyError::Sqlite)?;
        for row in rows {
            visit(row.map_err(ProxyError::Sqlite)?);
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

#[derive(Clone, Debug)]
pub struct ResolvedTarget {
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

#[derive(Clone, Debug)]
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
    clients: Arc<Mutex<HashMap<u64, reqwest::Client>>>,
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
            clients: Arc::new(Mutex::new(HashMap::new())),
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
        let config = self
            .state
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
            .unwrap_or_default();
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
            enabled: config.0,
            bind_addr: self.bind_addr.clone(),
            active_routes: config.1,
            requests: stats.0,
            failures: stats.1,
            last_error: stats.2,
            recent_requests: 0,
            recent_tokens: 0,
        }
    }

    pub fn update_config(&self, config: RuntimeConfig) -> Result<(), ProxyError> {
        let mut current = self
            .state
            .config
            .write()
            .map_err(|_| ProxyError::Poisoned)?;
        let mut health = self.state.health.lock().map_err(|_| ProxyError::Poisoned)?;
        let mut rr_counters = self
            .state
            .rr_counters
            .lock()
            .map_err(|_| ProxyError::Poisoned)?;
        *current = config;
        health.clear();
        rr_counters.clear();
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
    let connect_timeout_ms = connect_timeout_ms.max(1);
    let mut clients = state
        .clients
        .lock()
        .map_err(|_| "proxy HTTP client cache lock poisoned".to_string())?;
    if let Some(client) = clients.get(&connect_timeout_ms) {
        return Ok(client.clone());
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_millis(connect_timeout_ms))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|err| err.to_string())?;
    clients.insert(connect_timeout_ms, client.clone());
    Ok(client)
}

type BoxError = Box<dyn StdError + Send + Sync>;
type BoxBody = http_body_util::combinators::UnsyncBoxBody<Bytes, BoxError>;
type UpstreamBodyStream =
    Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static>>;

const MAX_REQUEST_BODY_BYTES: usize = 512 * 1024 * 1024;
const REQUEST_BODY_MEMORY_THRESHOLD: usize = 8 * 1024 * 1024;
const MAX_BUFFERED_RESPONSE_BYTES: usize = 64 * 1024 * 1024;

enum ReplayableRequestBody {
    Memory(Bytes),
    File { file: std::fs::File, len: u64 },
}

#[derive(Default, Deserialize)]
struct RequestMetadata {
    #[serde(default)]
    stream: bool,
    model: Option<String>,
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

async fn handle_request(
    request: Request<Incoming>,
    state: RuntimeState,
) -> Result<Response<BoxBody>, Infallible> {
    let started = Instant::now();
    let started_at = now_unix();
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
    let (bearer_token, api_key_token) = local_proxy_tokens(&incoming_headers);
    if bearer_token.is_none() && api_key_token.is_none() {
        return Ok(error_response(
            StatusCode::UNAUTHORIZED,
            "missing local proxy token",
        ));
    }
    let selected = state.config.read().ok().and_then(|config| {
        config.enabled.then(|| {
            config
                .routes
                .iter()
                .find(|route| {
                    route.config.enabled
                        && route.config.inbound_protocol == inbound
                        && (bearer_token
                            .is_some_and(|token| tokens_match(&route.local_token, token))
                            || api_key_token
                                .is_some_and(|token| tokens_match(&route.local_token, token)))
                })
                .cloned()
                .map(|route| (route, config.pricing.clone()))
        })?
    });
    let Some((mut route, pricing)) = selected else {
        return Ok(error_response(
            StatusCode::UNAUTHORIZED,
            "invalid local proxy token or route",
        ));
    };
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
    let request_json = if route.config.conversion_enabled
        && route.config.inbound_protocol != route.config.upstream_protocol
    {
        body.json().await
    } else {
        None
    };
    let request_metadata = body.metadata().await.unwrap_or_default();
    let streaming_request = request_metadata.stream;
    let model = request_metadata.model;
    let mut last_error = None;
    let mut targets = std::mem::take(&mut route.targets);
    targets.retain(|target| target.config.enabled);
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
    targets.retain(|target| !circuit_open(&state, target.config.id));
    targets.truncate(usize::from(route.config.retry.max_attempts.max(1)));
    for (attempt_index, target) in targets.into_iter().enumerate() {
        let attempt_started_at = now_unix();
        let attempt_started = Instant::now();
        let attempts = u8::try_from(attempt_index + 1).unwrap_or(u8::MAX);
        let client = match upstream_client(&state, route.config.retry.connect_timeout_ms) {
            Ok(client) => client,
            Err(err) => {
                last_error = Some(err);
                mark_failure(&state, target.config.id, &route.config.retry);
                persist_attempt(
                    &state.usage,
                    route.config.id,
                    &target,
                    model.as_deref(),
                    attempt_started_at,
                    attempt_started,
                    AttemptOutcome::failure(None, None),
                );
                continue;
            }
        };
        let mut rewritten_payload = None;
        if route.config.conversion_enabled
            && route.config.inbound_protocol != route.config.upstream_protocol
        {
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
                    Err(err) => {
                        return Ok(error_response(StatusCode::BAD_REQUEST, &err.to_string()))
                    }
                },
            );
        }
        if let Some(payload) = rewritten_payload.take().or_else(|| body.bytes().cloned()) {
            let updated = request_stream_usage(
                route.config.upstream_protocol,
                streaming_request,
                payload.clone(),
            );
            if updated != payload {
                rewritten_payload = Some(updated);
            } else if route.config.conversion_enabled
                && route.config.inbound_protocol != route.config.upstream_protocol
            {
                rewritten_payload = Some(payload);
            }
        }
        let upstream_path = if target.config.auth_scheme == "azure_api_key" {
            route
                .config
                .upstream_protocol
                .path()
                .strip_prefix("/v1")
                .unwrap_or(route.config.upstream_protocol.path())
        } else {
            route.config.upstream_protocol.path()
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
                    route.config.id,
                    &target,
                    model.as_deref(),
                    attempt_started_at,
                    attempt_started,
                    AttemptOutcome::failure(None, None),
                );
                continue;
            }
        };
        let upstream_headers = match build_upstream_headers(
            &incoming_headers,
            &target,
            route.config.upstream_protocol,
        ) {
            Ok(headers) => headers,
            Err(err) => {
                last_error = Some(err);
                mark_failure(&state, target.config.id, &route.config.retry);
                persist_attempt(
                    &state.usage,
                    route.config.id,
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
                        route.config.id,
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
                    route.config.id,
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
        let first_byte_timeout =
            Duration::from_millis(route.config.retry.first_byte_timeout_ms.max(1));
        let response = match tokio::time::timeout(first_byte_timeout, upstream.send()).await {
            Ok(Ok(response)) => response,
            Ok(Err(err)) => {
                last_error = Some(err.to_string());
                mark_failure(&state, target.config.id, &route.config.retry);
                persist_attempt(
                    &state.usage,
                    route.config.id,
                    &target,
                    model.as_deref(),
                    attempt_started_at,
                    attempt_started,
                    AttemptOutcome::failure(None, None),
                );
                continue;
            }
            Err(_) => {
                last_error = Some("upstream response header timeout".into());
                mark_failure(&state, target.config.id, &route.config.retry);
                persist_attempt(
                    &state.usage,
                    route.config.id,
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
            last_error = Some(format!("upstream returned {status}"));
            if status_affects_circuit(status) {
                mark_failure(&state, target.config.id, &route.config.retry);
            }
            persist_attempt(
                &state.usage,
                route.config.id,
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
        let mut upstream_stream: UpstreamBodyStream = Box::pin(response.bytes_stream());
        let first_event_deadline = tokio::time::Instant::now() + first_byte_timeout;
        let first_chunk =
            match tokio::time::timeout_at(first_event_deadline, upstream_stream.next()).await {
                Ok(Some(Ok(chunk))) => Some(chunk),
                Ok(Some(Err(err))) => {
                    last_error = Some(err.to_string());
                    mark_failure(&state, target.config.id, &route.config.retry);
                    persist_attempt(
                        &state.usage,
                        route.config.id,
                        &target,
                        model.as_deref(),
                        attempt_started_at,
                        attempt_started,
                        AttemptOutcome::failure(Some(status), None),
                    );
                    continue;
                }
                Ok(None) => None,
                Err(_) => {
                    last_error = Some("upstream first-byte timeout".into());
                    mark_failure(&state, target.config.id, &route.config.retry);
                    persist_attempt(
                        &state.usage,
                        route.config.id,
                        &target,
                        model.as_deref(),
                        attempt_started_at,
                        attempt_started,
                        AttemptOutcome::failure(Some(status), None),
                    );
                    continue;
                }
            };
        let stream_idle_timeout =
            Duration::from_millis(route.config.retry.stream_idle_timeout_ms.max(1));
        let (first_chunk, first_token_observed) = if streaming_response {
            // Once this event is returned to the client, replaying on another target is unsafe.
            match prefetch_sse_event(
                route.config.upstream_protocol,
                first_chunk,
                &mut upstream_stream,
                first_event_deadline,
            )
            .await
            {
                Ok(Some(prefetched)) => (Some(prefetched.bytes), prefetched.first_token_observed),
                Ok(None) => {
                    last_error = Some("upstream stream ended before the first event".into());
                    mark_failure(&state, target.config.id, &route.config.retry);
                    persist_attempt(
                        &state.usage,
                        route.config.id,
                        &target,
                        model.as_deref(),
                        attempt_started_at,
                        attempt_started,
                        AttemptOutcome::failure(Some(status), None),
                    );
                    continue;
                }
                Err(err) => {
                    last_error = Some(err);
                    mark_failure(&state, target.config.id, &route.config.retry);
                    persist_attempt(
                        &state.usage,
                        route.config.id,
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
        let conversion = route.config.conversion_enabled
            && route.config.inbound_protocol != route.config.upstream_protocol;
        let upstream_protocol = route.config.upstream_protocol;
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
            id: Uuid::new_v4(),
            started_at,
            duration_ms: started.elapsed().as_millis() as u64,
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
        let body_stream: UpstreamBodyStream = if streaming_response {
            if let Some(first_chunk) = first_chunk {
                Box::pin(stream::once(async move { Ok(first_chunk) }).chain(upstream_stream))
            } else {
                Box::pin(stream::empty())
            }
        } else {
            let buffered =
                match collect_upstream_body(first_chunk, &mut upstream_stream, stream_idle_timeout)
                    .await
                {
                    Ok(buffered) => buffered,
                    Err(err) => {
                        last_error = Some(err);
                        mark_failure(&state, target.config.id, &route.config.retry);
                        persist_attempt(
                            &state.usage,
                            route.config.id,
                            &target,
                            model.as_deref(),
                            attempt_started_at,
                            attempt_started,
                            AttemptOutcome::failure(Some(status), first_token_ms),
                        );
                        continue;
                    }
                };
            if status.is_success() && is_upstream_error_payload(&buffered) {
                last_error = Some("upstream returned an error payload".into());
                mark_failure(&state, target.config.id, &route.config.retry);
                persist_attempt(
                    &state.usage,
                    route.config.id,
                    &target,
                    model.as_deref(),
                    attempt_started_at,
                    attempt_started,
                    AttemptOutcome::failure(Some(status), first_token_ms),
                );
                continue;
            }
            Box::pin(stream::once(async move {
                Ok::<Bytes, reqwest::Error>(buffered)
            }))
        };
        if !streaming_response {
            mark_success(&state, target.config.id);
            persist_attempt(
                &state.usage,
                route.config.id,
                &target,
                model.as_deref(),
                attempt_started_at,
                attempt_started,
                AttemptOutcome::success(status, first_token_ms),
            );
        }
        let streaming_attempt = streaming_response.then(|| {
            (
                AttemptRecord {
                    id: Uuid::new_v4(),
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
                store: state.usage.clone(),
                record,
                pricing: model_pricing,
                stream_idle_timeout,
                streaming: streaming_response,
                started,
                failure_state: state.clone(),
                target_id: target.config.id,
                retry_policy: route.config.retry.clone(),
                attempt: streaming_attempt,
            },
        );
        let output_stream: Pin<Box<dyn Stream<Item = Result<Bytes, BoxError>> + Send>> =
            if conversion && streaming_response {
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
        let response = builder.body(stream_body).unwrap_or_else(|_| {
            error_response(StatusCode::BAD_GATEWAY, "failed to build proxy response")
        });
        if let Ok(mut stats) = state.stats.lock() {
            stats.requests += 1;
            if !status.is_success() {
                stats.failures += 1;
            } else {
                stats.last_error = None;
            }
        }
        return Ok(response);
    }
    let diagnostic = last_error.unwrap_or_else(|| "all upstream targets failed".into());
    if let Ok(mut stats) = state.stats.lock() {
        stats.requests += 1;
        stats.failures += 1;
        stats.last_error = Some(diagnostic);
    }
    Ok(error_response(
        StatusCode::BAD_GATEWAY,
        "all upstream targets failed",
    ))
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
    route_id: Uuid,
    target: &ResolvedTarget,
    model: Option<&str>,
    started_at: i64,
    started: Instant,
    outcome: AttemptOutcome,
) {
    let _ = store.record_attempt(&AttemptRecord {
        id: Uuid::new_v4(),
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

async fn prefetch_sse_event(
    protocol: ProxyProtocol,
    first_chunk: Option<Bytes>,
    source: &mut UpstreamBodyStream,
    deadline: tokio::time::Instant,
) -> Result<Option<PrefetchedSse>, String> {
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
                return Err("upstream returned an error event".into());
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
        match tokio::time::timeout_at(deadline, source.next()).await {
            Ok(Some(Ok(chunk))) => buffered.extend_from_slice(&chunk),
            Ok(Some(Err(err))) => return Err(err.to_string()),
            Ok(None) => return Err("upstream stream ended before the first complete event".into()),
            Err(_) => return Err("upstream first-event timeout".into()),
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

#[cfg(test)]
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
            value.get("type").and_then(serde_json::Value::as_str) == Some("response.completed")
                || value
                    .pointer("/response/status")
                    .and_then(serde_json::Value::as_str)
                    == Some("completed")
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
    idle_timeout: Duration,
) -> Result<Bytes, String> {
    let mut buffered = first_chunk.map_or_else(Vec::new, |chunk| chunk.to_vec());
    loop {
        if buffered.len() > MAX_BUFFERED_RESPONSE_BYTES {
            return Err("upstream response exceeds proxy buffer limit".into());
        }
        match tokio::time::timeout(idle_timeout, source.next()).await {
            Ok(Some(Ok(chunk))) => buffered.extend_from_slice(&chunk),
            Ok(Some(Err(err))) => return Err(err.to_string()),
            Ok(None) => return Ok(Bytes::from(buffered)),
            Err(_) => return Err("upstream response body idle timeout".into()),
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
    streaming: bool,
    started: Instant,
    failure_state: RuntimeState,
    target_id: Uuid,
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
        protocol,
        store,
        mut record,
        pricing,
        stream_idle_timeout,
        streaming,
        started,
        failure_state,
        target_id,
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
        loop {
            let next = if streaming {
                tokio::select! {
                    _ = sender.closed() => break,
                    result = tokio::time::timeout(stream_idle_timeout, source.next()) => result,
                }
            } else {
                tokio::time::timeout(stream_idle_timeout, source.next()).await
            };
            let result: Result<Bytes, BoxError> = match next {
                Ok(Some(result)) => result.map_err(|err| Box::new(err) as BoxError),
                Ok(None) => {
                    source_ended = true;
                    break;
                }
                Err(_) => Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "upstream stream idle timeout",
                ))),
            };
            if let Ok(chunk) = &result {
                if streaming {
                    let signals = observe_sse_usage(
                        protocol,
                        &mut usage_event_buffer,
                        chunk,
                        &mut observed_usage,
                    );
                    protocol_completed |= signals.completed;
                    protocol_terminal |= signals.terminal;
                    protocol_failed |= signals.failed;
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
            mark_success(&failure_state, target_id);
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
        record.duration_ms = started.elapsed().as_millis() as u64;
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
    completed: bool,
    terminal: bool,
    failed: bool,
}

fn observe_sse_usage(
    protocol: ProxyProtocol,
    buffer: &mut Vec<u8>,
    chunk: &[u8],
    usage: &mut TokenUsage,
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
    !status.is_success()
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
    let mut headers = HeaderMap::new();
    for (name, value) in incoming.iter() {
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
        if !is_hop_header(name)
            && !configured_hop_headers.contains(name)
            && name != header::ACCEPT_ENCODING
            && name != header::CONTENT_LENGTH
            && name != header::CONTENT_TYPE
            && name != header::HOST
        {
            headers.insert(name.clone(), value.clone());
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

    fn test_target(base_url: String, priority: u16) -> ResolvedTarget {
        ResolvedTarget {
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
        assert!(circuit_open(&proxy.state, target_id));
        assert!(!proxy.state.rr_counters.lock().unwrap().is_empty());

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
    fn non_stream_idle_timeout_is_rejected_before_response_commit() {
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
    fn response_header_timeout_fails_over_internally() {
        let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let primary_addr = primary.local_addr().unwrap();
        let primary_thread = std::thread::spawn(move || {
            let (_stream, _) = primary.accept().unwrap();
            std::thread::sleep(Duration::from_millis(200));
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
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.json::<serde_json::Value>().unwrap()["source"],
            "fallback"
        );
        primary_thread.join().unwrap();
        fallback_thread.join().unwrap();
    }

    #[test]
    fn truncated_non_stream_body_fails_over_before_response_commit() {
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
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.json::<serde_json::Value>().unwrap()["source"],
            "fallback"
        );
        primary_thread.join().unwrap();
        fallback_thread.join().unwrap();
    }

    #[test]
    fn stream_request_with_json_response_fails_over_before_commit() {
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
        let fallback_thread = std::thread::spawn(move || {
            let (mut stream, _) = fallback.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            let body = "data: {\"source\":\"fallback\"}\n\n";
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

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
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.text().unwrap();
        assert!(body.contains("fallback"));
        assert!(!body.contains("partial"));
        primary_thread.join().unwrap();
        fallback_thread.join().unwrap();
    }

    #[test]
    fn incomplete_first_sse_event_fails_over_before_stream_commit() {
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
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.text().unwrap();
        assert!(body.contains("fallback"));
        assert!(!body.contains("partial"));
        primary_thread.join().unwrap();
        fallback_thread.join().unwrap();
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
        let summary = usage.summary(|_| 0).unwrap();
        assert_eq!(summary.attempt_count, 2);
        assert_eq!(summary.successful_attempts, 1);
        assert_eq!(summary.success_rate_bps, 5_000);
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
        let record =
            |provider_entry_id, secret_id: &str, model: Option<&str>, started_at, input_tokens| {
                UsageRecord {
                    id: Uuid::new_v4(),
                    started_at,
                    duration_ms: 1,
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
        let attempt = |provider_entry_id, secret_id: &str, started_at, success, first_token_ms| {
            AttemptRecord {
                id: Uuid::new_v4(),
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
        assert_eq!(summary.request_count, 0);
        assert_eq!(summary.attempt_count, 4);
        assert_eq!(summary.completed_attempts, 3);
        assert_eq!(summary.successful_attempts, 1);
        assert_eq!(summary.success_rate_bps, 3_333);
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
}
