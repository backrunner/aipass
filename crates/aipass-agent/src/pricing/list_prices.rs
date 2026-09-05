use crate::logging::{write_component_log, AGENT_LOG};
use crate::session::{shutdown_requested, AgentState};
use aipass_agent_protocol::ModelPriceRule;
use aipass_storage::atomic_write_bytes;
use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use time::OffsetDateTime;

const LIST_PRICES_FILE: &str = "list-prices.json";
const LITELLM_PRICES_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
const REFRESH_INTERVAL_SECONDS: i64 = 24 * 60 * 60;
const RETRY_INTERVAL_SECONDS: i64 = 60 * 60;
const POLL_INTERVAL: Duration = Duration::from_secs(30);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_SNAPSHOT_BYTES: u64 = 32 * 1024 * 1024;

/// Only public catalog data lives here; credential assignments and custom
/// pricing remain in the encrypted pricing config. The timestamp and rules
/// are replaced atomically, including when the vault is locked.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListPriceSnapshot {
    version: u32,
    updated_at: i64,
    rules: Vec<ModelPriceRule>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum CachedListPrices {
    Snapshot(ListPriceSnapshot),
    Legacy(Vec<ModelPriceRule>),
}

impl CachedListPrices {
    fn updated_at(&self) -> Option<i64> {
        match self {
            Self::Snapshot(snapshot) => Some(snapshot.updated_at),
            Self::Legacy(_) => None,
        }
    }

    fn into_rules(self) -> Vec<ModelPriceRule> {
        match self {
            Self::Snapshot(snapshot) => snapshot.rules,
            Self::Legacy(rules) => rules,
        }
    }
}

/// Built-in list-price snapshot shipped with the app; used if no valid cache
/// has been downloaded yet.
pub fn builtin_list_prices() -> &'static [ModelPriceRule] {
    static PRICES: OnceLock<Vec<ModelPriceRule>> = OnceLock::new();
    PRICES.get_or_init(|| {
        serde_json::from_str(include_str!("../list_prices.json"))
            .expect("built-in list price snapshot must be valid")
    })
}

pub fn load_list_prices(vault_dir: &Path) -> Vec<ModelPriceRule> {
    load_cached_prices(vault_dir)
        .map(CachedListPrices::into_rules)
        .unwrap_or_else(|| builtin_list_prices().to_vec())
}

pub(super) fn updated_at(vault_dir: &Path) -> Option<i64> {
    load_cached_prices(vault_dir).and_then(|cache| cache.updated_at())
}

fn load_cached_prices(vault_dir: &Path) -> Option<CachedListPrices> {
    let file = std::fs::File::open(vault_dir.join(LIST_PRICES_FILE)).ok()?;
    let bytes = read_bounded(file).ok()?;
    let cache: CachedListPrices = serde_json::from_slice(&bytes).ok()?;
    let rules = match &cache {
        CachedListPrices::Snapshot(snapshot) => {
            if snapshot.version != 1 || snapshot.updated_at <= 0 {
                return None;
            }
            &snapshot.rules
        }
        CachedListPrices::Legacy(rules) => rules,
    };
    validate_rules(rules).ok()?;
    Some(cache)
}

fn validate_rules(rules: &[ModelPriceRule]) -> anyhow::Result<()> {
    if rules.is_empty() {
        bail!("list price table contains no rules");
    }
    let mut names = HashSet::new();
    for rule in rules {
        if rule.model.trim().is_empty() || !names.insert(&rule.model) {
            bail!("list price table contains empty or duplicate model names");
        }
        if rule
            .off_peak
            .as_ref()
            .is_some_and(|window| window.start_minute_utc >= 1440 || window.end_minute_utc >= 1440)
        {
            bail!("list price table contains an invalid off-peak window");
        }
    }
    Ok(())
}

struct RefreshSchedule {
    scheduled_at: i64,
    next_attempt_at: i64,
}

impl RefreshSchedule {
    fn new(last_success: Option<i64>, now: i64) -> Self {
        let next_attempt_at = last_success
            .filter(|updated_at| *updated_at > 0 && *updated_at <= now)
            .map(|updated_at| updated_at.saturating_add(REFRESH_INTERVAL_SECONDS))
            .unwrap_or(now);
        Self {
            scheduled_at: now,
            next_attempt_at,
        }
    }

    fn is_due(&self, now: i64) -> bool {
        // A clock correction must not suppress refreshes indefinitely. Wall
        // time also lets sleeping desktops refresh promptly after waking.
        now >= self.next_attempt_at || now < self.scheduled_at
    }

    fn record_attempt(&mut self, now: i64, succeeded: bool) {
        self.scheduled_at = now;
        self.next_attempt_at = now.saturating_add(if succeeded {
            REFRESH_INTERVAL_SECONDS
        } else {
            RETRY_INTERVAL_SECONDS
        });
    }
}

/// Refresh daily for the lifetime of the agent, independently of app updates
/// and vault unlocks. Failures retain the last usable snapshot and retry hourly.
pub fn spawn_list_price_refresh(state: Arc<AgentState>) {
    let state = Arc::downgrade(&state);
    std::thread::spawn(move || {
        let Some(initial_state) = state.upgrade() else {
            return;
        };
        let vault_dir = initial_state.vault_dir.clone();
        drop(initial_state);
        let mut schedule = RefreshSchedule::new(updated_at(&vault_dir), now());
        loop {
            let Some(state) = state.upgrade() else {
                break;
            };
            if shutdown_requested(&state) {
                break;
            }
            if schedule.is_due(now()) {
                let operation =
                    crate::operation_log::OperationLog::background("pricing.catalog.refresh");
                let result = fetch_list_prices(LITELLM_PRICES_URL);
                if shutdown_requested(&state) {
                    break;
                }
                let result = result.and_then(|rules| save_snapshot(&vault_dir, rules, now()));
                schedule.record_attempt(now(), result.is_ok());
                if let Some(operation) = operation {
                    operation.finish(&if result.is_ok() {
                        aipass_agent_protocol::AgentResponse::empty()
                    } else {
                        aipass_agent_protocol::AgentResponse::error(
                            aipass_agent_protocol::AgentErrorCode::Internal,
                            "price refresh failed",
                        )
                    });
                }
                if result.is_err() {
                    write_component_log(
                        AGENT_LOG,
                        "WARN",
                        "list price refresh failed; retaining cached or built-in prices, retrying in one hour",
                    );
                }
            }
            drop(state);
            std::thread::sleep(POLL_INTERVAL);
        }
    });
}

fn now() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

fn fetch_list_prices(url: &str) -> anyhow::Result<Vec<ModelPriceRule>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()?;
    let response = client.get(url).send()?.error_for_status()?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_SNAPSHOT_BYTES)
    {
        bail!("list price response exceeds size limit");
    }
    let bytes = read_bounded(response)?;
    merge_litellm_prices(&serde_json::from_slice(&bytes)?)
}

fn read_bounded(reader: impl Read) -> anyhow::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_SNAPSHOT_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_SNAPSHOT_BYTES {
        bail!("list price snapshot exceeds size limit");
    }
    Ok(bytes)
}

fn save_snapshot(
    vault_dir: &Path,
    rules: Vec<ModelPriceRule>,
    updated_at: i64,
) -> anyhow::Result<()> {
    validate_rules(&rules)?;
    let snapshot = ListPriceSnapshot {
        version: 1,
        updated_at,
        rules,
    };
    let bytes = serde_json::to_vec(&snapshot)?;
    if bytes.len() as u64 > MAX_SNAPSHOT_BYTES {
        bail!("list price snapshot exceeds size limit");
    }
    atomic_write_bytes(vault_dir.join(LIST_PRICES_FILE), &bytes)?;
    Ok(())
}

fn merge_litellm_prices(payload: &Value) -> anyhow::Result<Vec<ModelPriceRule>> {
    let table = payload
        .as_object()
        .context("litellm price table is not a json object")?;
    let mut merged: HashMap<String, ModelPriceRule> = table
        .iter()
        .filter_map(|(name, info)| litellm_rule(name, info).map(|rule| (name.clone(), rule)))
        .collect();
    // Check the remote data before adding fallback rules, otherwise an empty
    // or changed upstream schema can silently replace a good downloaded table.
    if merged.is_empty() {
        bail!("litellm price table produced no usable rules");
    }
    for builtin in builtin_list_prices() {
        merged
            .entry(builtin.model.clone())
            .and_modify(|rule| rule.off_peak = builtin.off_peak.clone())
            .or_insert_with(|| builtin.clone());
    }
    let mut rules: Vec<_> = merged.into_values().collect();
    rules.sort_by(|left, right| {
        right
            .model
            .len()
            .cmp(&left.model.len())
            .then_with(|| left.model.cmp(&right.model))
    });
    validate_rules(&rules)?;
    Ok(rules)
}

fn litellm_rule(name: &str, info: &Value) -> Option<ModelPriceRule> {
    if name.trim().is_empty()
        || (info.get("input_cost_per_token").is_none()
            && info.get("output_cost_per_token").is_none())
    {
        return None;
    }
    Some(ModelPriceRule {
        model: name.to_string(),
        input_micros_per_million: litellm_micros(info.get("input_cost_per_token"))?,
        output_micros_per_million: litellm_micros(info.get("output_cost_per_token"))?,
        cache_read_micros_per_million: litellm_micros(info.get("cache_read_input_token_cost"))?,
        cache_creation_micros_per_million: litellm_micros(
            info.get("cache_creation_input_token_cost"),
        )?,
        off_peak: None,
    })
}

fn litellm_micros(value: Option<&Value>) -> Option<u64> {
    let Some(value) = value else {
        return Some(0);
    };
    let price = value.as_f64()?;
    // cost_per_token * 1e6 micros/USD * 1e6 tokens/million
    let micros = (price * 1e12).round();
    (price >= 0.0 && micros.is_finite() && micros < u64::MAX as f64).then_some(micros as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;
    use std::net::TcpListener;
    use std::time::Instant;
    use tempfile::tempdir;

    const NOW: i64 = 1_800_000_000;

    fn fetch_fixture(status: &str, body: &str) -> anyhow::Result<Vec<ModelPriceRule>> {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/prices.json", listener.local_addr().unwrap());
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + REQUEST_TIMEOUT;
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "fixture did not receive request");
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(err) => panic!("fixture accept: {err}"),
                }
            };
            stream.set_read_timeout(Some(REQUEST_TIMEOUT)).unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            let request = String::from_utf8(request).unwrap();
            assert!(request.starts_with("GET /prices.json HTTP/1.1\r\n"));
            assert!(!request.to_ascii_lowercase().contains("authorization:"));
            stream.write_all(response.as_bytes()).unwrap();
        });
        let result = fetch_list_prices(&url);
        server.join().unwrap();
        result
    }

    #[test]
    fn refresh_schedule_survives_restarts_and_retries_without_restarting() {
        for timestamp in [None, Some(0), Some(NOW + 1)] {
            assert!(RefreshSchedule::new(timestamp, NOW).is_due(NOW));
        }
        let mut schedule = RefreshSchedule::new(None, NOW);
        schedule.record_attempt(NOW, true);
        assert!(!schedule.is_due(NOW + REFRESH_INTERVAL_SECONDS - 1));
        assert!(schedule.is_due(NOW + REFRESH_INTERVAL_SECONDS));

        let restarted = RefreshSchedule::new(Some(NOW), NOW + 60);
        assert!(!restarted.is_due(NOW + 60));
        assert!(restarted.is_due(NOW + REFRESH_INTERVAL_SECONDS));

        let failed_at = NOW + REFRESH_INTERVAL_SECONDS;
        schedule.record_attempt(failed_at, false);
        assert!(!schedule.is_due(failed_at + RETRY_INTERVAL_SECONDS - 1));
        assert!(schedule.is_due(failed_at + RETRY_INTERVAL_SECONDS));
        schedule.record_attempt(failed_at + RETRY_INTERVAL_SECONDS, true);
        assert!(!schedule.is_due(failed_at + 2 * RETRY_INTERVAL_SECONDS));
        assert!(schedule.is_due(failed_at + RETRY_INTERVAL_SECONDS + REFRESH_INTERVAL_SECONDS));
        assert!(schedule.is_due(NOW - 60));
    }

    #[test]
    fn download_replaces_prices_and_failures_preserve_the_last_good_snapshot() {
        let dir = tempdir().unwrap();
        let body =
            r#"{"new-model":{"input_cost_per_token":0.0000025,"output_cost_per_token":0.00001}}"#;
        let rules = fetch_fixture("200 OK", body).unwrap();
        save_snapshot(dir.path(), rules, NOW).unwrap();
        let good_snapshot = std::fs::read(dir.path().join(LIST_PRICES_FILE)).unwrap();
        let rules = load_list_prices(dir.path());
        let rule = rules.iter().find(|rule| rule.model == "new-model").unwrap();
        assert_eq!(rule.input_micros_per_million, 2_500_000);
        assert_eq!(rule.output_micros_per_million, 10_000_000);
        assert_eq!(updated_at(dir.path()), Some(NOW));

        for (status, body) in [
            ("503 Service Unavailable", "temporarily unavailable"),
            ("200 OK", "not json"),
            ("200 OK", "[]"),
            ("200 OK", "{}"),
            ("200 OK", r#"{"schema-changed":{"price":3}}"#),
        ] {
            let result = fetch_fixture(status, body)
                .and_then(|rules| save_snapshot(dir.path(), rules, NOW + 1));
            assert!(result.is_err());
            assert_eq!(
                std::fs::read(dir.path().join(LIST_PRICES_FILE)).unwrap(),
                good_snapshot
            );
        }

        let free = r#"{"new-model":{"input_cost_per_token":0,"output_cost_per_token":0}}"#;
        let rules = fetch_fixture("200 OK", free).unwrap();
        save_snapshot(dir.path(), rules, NOW + RETRY_INTERVAL_SECONDS).unwrap();
        let rules = load_list_prices(dir.path());
        let rule = rules.iter().find(|rule| rule.model == "new-model").unwrap();
        assert_eq!(rule.input_micros_per_million, 0);
        assert_eq!(rule.output_micros_per_million, 0);
        assert_eq!(updated_at(dir.path()), Some(NOW + RETRY_INTERVAL_SECONDS));
    }

    #[test]
    fn legacy_cache_still_works_and_invalid_caches_fall_back() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(LIST_PRICES_FILE);
        let legacy = vec![ModelPriceRule {
            model: "legacy-model".into(),
            input_micros_per_million: 42,
            ..Default::default()
        }];
        std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        assert_eq!(load_list_prices(dir.path()), legacy);
        assert_eq!(updated_at(dir.path()), None);
        for invalid in [
            json!([]),
            json!([{}]),
            json!([legacy[0], legacy[0]]),
            json!({"version": 2, "updatedAt": NOW, "rules": legacy}),
            json!({"version": 1, "updatedAt": 0, "rules": legacy}),
        ] {
            std::fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
            assert_eq!(load_list_prices(dir.path()), builtin_list_prices());
            assert_eq!(updated_at(dir.path()), None);
        }
        std::fs::write(&path, b"truncated json {").unwrap();
        assert_eq!(load_list_prices(dir.path()), builtin_list_prices());
        std::fs::remove_file(path).unwrap();
        assert_eq!(load_list_prices(dir.path()), builtin_list_prices());
    }

    #[test]
    fn refresh_while_locked_reports_timestamp_without_modifying_user_pricing() {
        use crate::pricing::{load_pricing_config, save_pricing_config};
        use aipass_agent_protocol::{CredentialAssignment, PricingConfig};
        use aipass_crypto::SecretString;
        use aipass_vault::Vault;

        let dir = tempdir().unwrap();
        let password = SecretString::new("correct horse battery staple");
        let creation = Vault::create(dir.path(), &password).unwrap();
        let assignments = vec![CredentialAssignment {
            entry_id: uuid::Uuid::new_v4(),
            secret_id: "test-credential".into(),
            multiplier: 0.5,
            group_id: None,
        }];
        save_pricing_config(
            dir.path(),
            &creation.vault,
            &PricingConfig {
                assignments: assignments.clone(),
                list_price_updated_at: Some(NOW - REFRESH_INTERVAL_SECONDS),
                ..Default::default()
            },
        )
        .unwrap();
        drop(creation);
        let config_path = dir.path().join(crate::pricing::CONFIG_FILE);
        let encrypted_config = std::fs::read(&config_path).unwrap();

        save_snapshot(dir.path(), builtin_list_prices().to_vec(), NOW).unwrap();
        assert_eq!(std::fs::read(&config_path).unwrap(), encrypted_config);
        let vault = Vault::open(dir.path(), &password).unwrap();
        let config = load_pricing_config(dir.path(), &vault).unwrap();
        assert_eq!(config.list_price_updated_at, Some(NOW));
        assert_eq!(config.assignments, assignments);

        std::fs::remove_file(config_path).unwrap();
        let config = load_pricing_config(dir.path(), &vault).unwrap();
        assert_eq!(config.list_price_updated_at, Some(NOW));
        assert!(config.assignments.is_empty());
    }

    #[test]
    fn remote_prices_keep_builtin_fallbacks_and_off_peak_windows() {
        let rules = merge_litellm_prices(&json!({
            "deepseek-chat": {
                "input_cost_per_token": 0.000003,
                "output_cost_per_token": 0.000004,
                "cache_read_input_token_cost": 0.00000125,
                "cache_creation_input_token_cost": 0.000002
            }
        }))
        .unwrap();
        let rule = rules
            .iter()
            .find(|rule| rule.model == "deepseek-chat")
            .unwrap();
        assert_eq!(rule.input_micros_per_million, 3_000_000);
        assert_eq!(rule.output_micros_per_million, 4_000_000);
        assert_eq!(rule.cache_read_micros_per_million, 1_250_000);
        assert_eq!(rule.cache_creation_micros_per_million, 2_000_000);
        let builtin = builtin_list_prices()
            .iter()
            .find(|rule| rule.model == "deepseek-chat")
            .unwrap();
        assert_eq!(rule.off_peak, builtin.off_peak);
        assert!(rules.iter().any(|rule| rule.model == "gpt-4o"));
    }

    #[test]
    fn invalid_remote_rates_are_rejected_instead_of_becoming_free() {
        for rate in [json!(-1), json!("0.01"), json!(null), json!(1e100)] {
            assert!(litellm_rule(
                "invalid-model",
                &json!({"input_cost_per_token": rate, "output_cost_per_token": 0.01})
            )
            .is_none());
        }
        assert!(litellm_rule("metadata", &json!({})).is_none());
        assert!(litellm_rule("", &json!({"input_cost_per_token": 0.01})).is_none());
    }

    #[test]
    fn oversized_streams_are_rejected_without_content_length() {
        assert!(read_bounded(std::io::repeat(b' ').take(MAX_SNAPSHOT_BYTES + 1)).is_err());
    }
}
