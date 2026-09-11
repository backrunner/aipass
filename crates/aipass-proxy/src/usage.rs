use super::*;

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
pub(crate) struct AttemptRow {
    pub(crate) started_at: i64,
    pub(crate) provider_entry_id: Uuid,
    pub(crate) secret_id: String,
    pub(crate) model: Option<String>,
    pub(crate) success: Option<bool>,
    pub(crate) first_token_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct AttemptStats {
    pub(crate) attempt_count: u64,
    pub(crate) completed_attempts: u64,
    pub(crate) successful_attempts: u64,
}

#[derive(Default)]
pub(crate) struct RequestStats {
    pub(crate) request_count: u64,
    pub(crate) successful_requests: u64,
    pub(crate) recent_first_tokens: VecDeque<Option<u64>>,
}

impl RequestStats {
    pub(crate) fn observe(&mut self, row: &UsageRow) {
        self.request_count = self.request_count.saturating_add(1);
        if (200..300).contains(&row.status) {
            self.successful_requests = self.successful_requests.saturating_add(1);
        }
        self.recent_first_tokens.push_back(row.first_token_ms);
        while self.recent_first_tokens.len() > 100 {
            self.recent_first_tokens.pop_front();
        }
    }

    pub(crate) fn success_rate_bps(&self) -> u16 {
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

    pub(crate) fn average_first_token_ms(&self) -> Option<u64> {
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
    pub(crate) fn observe(&mut self, row: &AttemptRow) {
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
    pub(crate) path: PathBuf,
    pub(crate) connection: Mutex<Connection>,
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

    pub(crate) fn rows_since(
        &self,
        since: Option<i64>,
    ) -> Result<Vec<(String, UsageRow)>, ProxyError> {
        let mut rows = Vec::new();
        self.visit_rows_since(since, |date, row| rows.push((date, row)))?;
        Ok(rows)
    }

    pub(crate) fn visit_rows_since(
        &self,
        since: Option<i64>,
        visit: impl FnMut(String, UsageRow),
    ) -> Result<(), ProxyError> {
        self.visit_rows_since_with_offset(since, 0, UsageGranularity::Day, visit)
    }

    pub(crate) fn visit_rows_since_with_offset(
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

    pub(crate) fn visit_attempt_rows(
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

pub(crate) fn decode_usage_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(String, UsageRow)> {
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

pub(crate) fn decode_attempt_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AttemptRow> {
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
