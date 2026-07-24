use crate::logging::{write_component_log, AGENT_LOG};
use crate::session::{map_vault_error, with_vault, AgentState, ServiceError, ServiceResult};
use aipass_agent_protocol::{ModelPriceRule, OffPeakWindow, PricingConfig};
use aipass_crypto::Ciphertext;
use aipass_proxy::ModelPricing;
use aipass_storage::atomic_write_bytes;
use aipass_vault::Vault;
use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use time::OffsetDateTime;
use uuid::Uuid;

const CONFIG_FILE: &str = "pricing.aipstate";
const CONFIG_PURPOSE: &str = "proxy-pricing";
const LIST_PRICES_FILE: &str = "list-prices.json";
const LITELLM_PRICES_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
const LIST_PRICE_TIMEOUT: Duration = Duration::from_secs(15);
const TRACKED_PREFIXES: [&str; 10] = [
    "gpt-",
    "o1",
    "o3",
    "o4",
    "claude-",
    "deepseek-",
    "moonshot-",
    "kimi-",
    "qwen-",
    "gemini-",
];

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PersistedPricingConfig {
    version: u32,
    payload: Ciphertext,
}

pub fn load_pricing_config(vault_dir: &Path, vault: &Vault) -> ServiceResult<PricingConfig> {
    let path = vault_dir.join(CONFIG_FILE);
    if !path.exists() {
        return Ok(PricingConfig::default());
    }
    let persisted: PersistedPricingConfig =
        serde_json::from_slice(&std::fs::read(path).map_err(ServiceError::internal)?)
            .map_err(ServiceError::internal)?;
    let bytes = vault
        .decrypt_local_state(CONFIG_PURPOSE, &persisted.payload)
        .map_err(map_vault_error)?;
    serde_json::from_slice(&bytes).map_err(ServiceError::internal)
}

pub fn save_pricing_config(
    vault_dir: &Path,
    vault: &Vault,
    config: &PricingConfig,
) -> ServiceResult<()> {
    let mut config = config.clone();
    for group in &mut config.groups {
        group.versions.sort_by_key(|version| version.effective_from);
    }
    let bytes = serde_json::to_vec(&config).map_err(ServiceError::internal)?;
    let payload = vault
        .encrypt_local_state(CONFIG_PURPOSE, &bytes)
        .map_err(map_vault_error)?;
    let persisted = PersistedPricingConfig {
        version: 1,
        payload,
    };
    atomic_write_bytes(
        vault_dir.join(CONFIG_FILE),
        &serde_json::to_vec_pretty(&persisted).map_err(ServiceError::internal)?,
    )
    .map_err(ServiceError::internal)
}

/// Built-in list-price snapshot shipped with the app; used as fallback when no
/// refreshed price table has been downloaded yet.
pub fn builtin_list_prices() -> &'static [ModelPriceRule] {
    static PRICES: OnceLock<Vec<ModelPriceRule>> = OnceLock::new();
    PRICES.get_or_init(|| {
        serde_json::from_str(include_str!("list_prices.json"))
            .expect("built-in list price snapshot must be valid")
    })
}

/// List prices effective for cost resolution: the refreshed snapshot written by
/// the background updater when present, otherwise the built-in snapshot.
pub fn load_list_prices(vault_dir: &Path) -> Vec<ModelPriceRule> {
    let path = vault_dir.join(LIST_PRICES_FILE);
    if let Ok(bytes) = std::fs::read(&path) {
        if let Ok(rules) = serde_json::from_slice::<Vec<ModelPriceRule>>(&bytes) {
            return rules;
        }
    }
    builtin_list_prices().to_vec()
}

/// Recompute the cost of a single usage row at query time. Group rules win
/// when the credential is assigned to a group with a matching version/rule;
/// otherwise per-config overrides win over the official list prices. A
/// credential multiplier applies in every branch.
#[allow(clippy::too_many_arguments)]
pub fn resolve_cost(
    config: &PricingConfig,
    overrides: &[ModelPricing],
    list_prices: &[ModelPriceRule],
    entry_id: Uuid,
    secret_id: &str,
    model: Option<&str>,
    started_at: i64,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
) -> u64 {
    let assignment = config
        .assignments
        .iter()
        .find(|item| item.entry_id == entry_id && item.secret_id == secret_id);
    let group_cost = assignment
        .and_then(|item| item.group_id)
        .and_then(|group_id| config.groups.iter().find(|group| group.id == group_id))
        .and_then(|group| {
            group
                .versions
                .iter()
                .rev()
                .find(|version| version.effective_from <= started_at)
        })
        .and_then(|version| model.and_then(|model| find_rule(&version.rules, model)))
        .map(|rule| {
            rule_cost(
                rule,
                started_at,
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_creation_tokens,
            )
        });
    let base = group_cost.unwrap_or_else(|| {
        model
            .and_then(|model| {
                overrides
                    .iter()
                    .filter(|item| item.model == model || model.starts_with(&item.model))
                    .max_by_key(|item| item.model.len())
            })
            .map(|pricing| {
                tokens_cost(
                    pricing.input_micros_per_million,
                    pricing.output_micros_per_million,
                    pricing.cache_read_micros_per_million,
                    pricing.cache_creation_micros_per_million,
                    input_tokens,
                    output_tokens,
                    cache_read_tokens,
                    cache_creation_tokens,
                )
            })
            .or_else(|| {
                model
                    .and_then(|model| find_rule(list_prices, model))
                    .map(|rule| {
                        rule_cost(
                            rule,
                            started_at,
                            input_tokens,
                            output_tokens,
                            cache_read_tokens,
                            cache_creation_tokens,
                        )
                    })
            })
            .unwrap_or(0)
    });
    let multiplier = assignment.map(|item| item.multiplier).unwrap_or(1.0);
    if multiplier == 1.0 {
        return base;
    }
    let scaled = base as f64 * multiplier;
    if scaled.is_finite() && scaled > 0.0 {
        scaled.round() as u64
    } else {
        0
    }
}

fn find_rule<'a>(rules: &'a [ModelPriceRule], model: &str) -> Option<&'a ModelPriceRule> {
    rules
        .iter()
        .filter(|rule| rule.model == model || model.starts_with(&rule.model))
        .max_by_key(|rule| rule.model.len())
}

fn rule_cost(
    rule: &ModelPriceRule,
    started_at: i64,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
) -> u64 {
    let (input, output, cache_read, cache_creation) = match &rule.off_peak {
        Some(window) if off_peak_contains(window, started_at) => (
            window.input_micros_per_million,
            window.output_micros_per_million,
            window.cache_read_micros_per_million,
            window.cache_creation_micros_per_million,
        ),
        _ => (
            rule.input_micros_per_million,
            rule.output_micros_per_million,
            rule.cache_read_micros_per_million,
            rule.cache_creation_micros_per_million,
        ),
    };
    tokens_cost(
        input,
        output,
        cache_read,
        cache_creation,
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_creation_tokens,
    )
}

fn off_peak_contains(window: &OffPeakWindow, started_at: i64) -> bool {
    let minute = (started_at.rem_euclid(86_400) / 60) as u16;
    if window.start_minute_utc <= window.end_minute_utc {
        minute >= window.start_minute_utc && minute < window.end_minute_utc
    } else {
        minute >= window.start_minute_utc || minute < window.end_minute_utc
    }
}

#[allow(clippy::too_many_arguments)]
fn tokens_cost(
    input_micros: u64,
    output_micros: u64,
    cache_read_micros: u64,
    cache_creation_micros: u64,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
) -> u64 {
    input_tokens
        .saturating_mul(input_micros)
        .saturating_add(output_tokens.saturating_mul(output_micros))
        .saturating_add(cache_read_tokens.saturating_mul(cache_read_micros))
        .saturating_add(cache_creation_tokens.saturating_mul(cache_creation_micros))
        / 1_000_000
}

/// Refresh the official list-price table from LiteLLM in the background. Any
/// failure is logged and silently falls back to the built-in snapshot.
pub fn spawn_list_price_refresh(state: Arc<AgentState>) {
    std::thread::spawn(move || {
        if let Err(err) = refresh_list_prices(&state) {
            write_component_log(
                AGENT_LOG,
                "WARN",
                &format!("list price refresh failed, using built-in snapshot: {err:#}"),
            );
        }
    });
}

fn refresh_list_prices(state: &Arc<AgentState>) -> anyhow::Result<()> {
    let client = reqwest::blocking::Client::builder()
        .timeout(LIST_PRICE_TIMEOUT)
        .build()?;
    let payload: serde_json::Value = client
        .get(LITELLM_PRICES_URL)
        .send()?
        .error_for_status()?
        .json()?;
    let table = payload
        .as_object()
        .context("litellm price table is not a json object")?;
    let mut merged = builtin_list_prices().to_vec();
    let mut extra: Vec<ModelPriceRule> = Vec::new();
    for (name, info) in table {
        if !TRACKED_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
        {
            continue;
        }
        let Some(rule) = litellm_rule(name, info) else {
            continue;
        };
        if let Some(existing) = merged.iter_mut().find(|item| item.model == *name) {
            // Network values override the numbers; built-in off-peak windows
            // (e.g. deepseek) stay in place.
            existing.input_micros_per_million = rule.input_micros_per_million;
            existing.output_micros_per_million = rule.output_micros_per_million;
            existing.cache_read_micros_per_million = rule.cache_read_micros_per_million;
            existing.cache_creation_micros_per_million = rule.cache_creation_micros_per_million;
        } else {
            extra.push(rule);
        }
    }
    if extra.is_empty() && merged.is_empty() {
        bail!("litellm price table produced no usable rules");
    }
    // Longer (more specific) model names first so prefix matching picks the
    // most specific rule before the built-in generic prefixes.
    extra.sort_by_key(|rule| std::cmp::Reverse(rule.model.len()));
    extra.extend(merged);
    atomic_write_bytes(
        state.vault_dir.join(LIST_PRICES_FILE),
        &serde_json::to_vec_pretty(&extra)?,
    )?;
    let updated_at = OffsetDateTime::now_utc().unix_timestamp();
    // Best effort: when the vault is locked the refreshed table still lands on
    // disk, only the encrypted timestamp update is skipped.
    let _ = with_vault(state, false, |vault| {
        let mut config = load_pricing_config(&state.vault_dir, vault)?;
        config.list_price_updated_at = Some(updated_at);
        save_pricing_config(&state.vault_dir, vault, &config)
    });
    Ok(())
}

fn litellm_rule(name: &str, info: &serde_json::Value) -> Option<ModelPriceRule> {
    let rule = ModelPriceRule {
        model: name.to_string(),
        input_micros_per_million: litellm_micros(info.get("input_cost_per_token")),
        output_micros_per_million: litellm_micros(info.get("output_cost_per_token")),
        cache_read_micros_per_million: litellm_micros(info.get("cache_read_input_token_cost")),
        cache_creation_micros_per_million: litellm_micros(
            info.get("cache_creation_input_token_cost"),
        ),
        off_peak: None,
    };
    (rule.input_micros_per_million > 0 || rule.output_micros_per_million > 0).then_some(rule)
}

fn litellm_micros(value: Option<&serde_json::Value>) -> u64 {
    match value.and_then(serde_json::Value::as_f64) {
        // cost_per_token * 1e6 micros/USD * 1e6 tokens/million
        Some(price) if price.is_finite() && price > 0.0 => (price * 1e12).round() as u64,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aipass_agent_protocol::{CredentialAssignment, GroupPriceVersion, PricingGroup};

    const DAY: i64 = 86_400 * 20_000;

    fn rule(model: &str, input: u64, output: u64) -> ModelPriceRule {
        ModelPriceRule {
            model: model.into(),
            input_micros_per_million: input,
            output_micros_per_million: output,
            cache_read_micros_per_million: 0,
            cache_creation_micros_per_million: 0,
            off_peak: None,
        }
    }

    fn config_with_group(group: PricingGroup, multiplier: f64) -> PricingConfig {
        PricingConfig {
            groups: vec![group],
            assignments: vec![CredentialAssignment {
                entry_id: Uuid::nil(),
                secret_id: "key".into(),
                group_id: Some(Uuid::nil()),
                multiplier,
            }],
            list_price_updated_at: None,
        }
    }

    fn resolve(
        config: &PricingConfig,
        overrides: &[ModelPricing],
        list_prices: &[ModelPriceRule],
        model: Option<&str>,
        started_at: i64,
    ) -> u64 {
        resolve_cost(
            config,
            overrides,
            list_prices,
            Uuid::nil(),
            "key",
            model,
            started_at,
            1_000_000,
            1_000_000,
            0,
            0,
        )
    }

    #[test]
    fn prefix_matching_picks_most_specific_rule() {
        let rules = vec![rule("gpt-4o", 300, 400), rule("gpt-4o-mini", 100, 200)];
        assert_eq!(
            resolve(
                &PricingConfig::default(),
                &[],
                &rules,
                Some("gpt-4o-mini-2024"),
                DAY
            ),
            300
        );
        assert_eq!(
            resolve(
                &PricingConfig::default(),
                &[],
                &rules,
                Some("gpt-4o-2024"),
                DAY
            ),
            700
        );
    }

    #[test]
    fn group_versions_segment_history_by_effective_from() {
        let group = PricingGroup {
            id: Uuid::nil(),
            name: "discounted".into(),
            versions: vec![
                GroupPriceVersion {
                    effective_from: DAY,
                    rules: vec![rule("gpt-x", 1_000_000, 0)],
                },
                GroupPriceVersion {
                    effective_from: DAY + 86_400,
                    rules: vec![rule("gpt-x", 2_000_000, 0)],
                },
            ],
        };
        let config = config_with_group(group, 1.0);
        // Before the first version: no group rule applies, no fallback either.
        assert_eq!(resolve(&config, &[], &[], Some("gpt-x"), DAY - 1), 0);
        assert_eq!(
            resolve(&config, &[], &[], Some("gpt-x"), DAY + 60),
            1_000_000
        );
        assert_eq!(
            resolve(&config, &[], &[], Some("gpt-x"), DAY + 86_400 + 60),
            2_000_000
        );
    }

    #[test]
    fn off_peak_window_crossing_midnight_applies_utc_prices() {
        let mut off_peak_rule = rule("deepseek-chat", 1_000_000, 0);
        off_peak_rule.off_peak = Some(OffPeakWindow {
            start_minute_utc: 990,
            end_minute_utc: 30,
            input_micros_per_million: 500_000,
            output_micros_per_million: 0,
            cache_read_micros_per_million: 0,
            cache_creation_micros_per_million: 0,
        });
        let group = PricingGroup {
            id: Uuid::nil(),
            name: "deepseek".into(),
            versions: vec![GroupPriceVersion {
                effective_from: 0,
                rules: vec![off_peak_rule],
            }],
        };
        let config = config_with_group(group, 1.0);
        // 23:00 UTC (minute 1380) is inside 990 -> 30: 2M tokens at the
        // off-peak price of $0.5/M cost 1_000_000 micros.
        assert_eq!(
            resolve_cost(
                &config,
                &[],
                &[],
                Uuid::nil(),
                "key",
                Some("deepseek-chat"),
                DAY + 1_380 * 60,
                2_000_000,
                0,
                0,
                0,
            ),
            1_000_000
        );
        // 10:00 UTC (minute 600) is outside the window: full $1/M price.
        assert_eq!(
            resolve_cost(
                &config,
                &[],
                &[],
                Uuid::nil(),
                "key",
                Some("deepseek-chat"),
                DAY + 600 * 60,
                2_000_000,
                0,
                0,
                0,
            ),
            2_000_000
        );
    }

    #[test]
    fn multiplier_scales_group_and_fallback_costs() {
        let group = PricingGroup {
            id: Uuid::nil(),
            name: "reseller".into(),
            versions: vec![GroupPriceVersion {
                effective_from: 0,
                rules: vec![rule("gpt-x", 1_000_000, 0)],
            }],
        };
        let config = config_with_group(group, 1.5);
        // 2M tokens at $1/M = 2_000_000 micros, scaled by 1.5.
        assert_eq!(
            resolve_cost(
                &config,
                &[],
                &[],
                Uuid::nil(),
                "key",
                Some("gpt-x"),
                DAY,
                2_000_000,
                0,
                0,
                0,
            ),
            3_000_000
        );
        // Model without a group rule falls back to list prices, still scaled.
        assert_eq!(
            resolve_cost(
                &config,
                &[],
                &[rule("gpt-y", 1_000_000, 0)],
                Uuid::nil(),
                "key",
                Some("gpt-y"),
                DAY,
                2_000_000,
                0,
                0,
                0,
            ),
            3_000_000
        );
    }

    #[test]
    fn overrides_win_over_list_prices_without_assignment() {
        let overrides = vec![ModelPricing {
            model: "gpt-x".into(),
            input_micros_per_million: 1_000_000,
            output_micros_per_million: 0,
            cache_read_micros_per_million: 0,
            cache_creation_micros_per_million: 0,
        }];
        let list = vec![rule("gpt-x", 9_000_000, 0)];
        assert_eq!(
            resolve(
                &PricingConfig::default(),
                &overrides,
                &list,
                Some("gpt-x"),
                DAY
            ),
            1_000_000
        );
        assert_eq!(
            resolve(&PricingConfig::default(), &[], &list, Some("gpt-x"), DAY),
            9_000_000
        );
    }

    #[test]
    fn unmatched_models_and_missing_models_cost_zero() {
        assert_eq!(
            resolve(&PricingConfig::default(), &[], &[], Some("unknown"), DAY),
            0
        );
        assert_eq!(resolve(&PricingConfig::default(), &[], &[], None, DAY), 0);
    }

    #[test]
    fn normalized_input_is_not_reduced_by_cache_tokens_again() {
        let rules = vec![ModelPriceRule {
            model: "claude-sonnet-4".into(),
            input_micros_per_million: 3_000_000,
            output_micros_per_million: 15_000_000,
            cache_read_micros_per_million: 300_000,
            cache_creation_micros_per_million: 3_750_000,
            off_peak: None,
        }];
        // input_tokens is normalized to exclude cache reads and creation.
        let cost = resolve_cost(
            &PricingConfig::default(),
            &[],
            &rules,
            Uuid::nil(),
            "key",
            Some("claude-sonnet-4"),
            DAY,
            400_000,
            0,
            500_000,
            100_000,
        );
        assert_eq!(cost, 400_000 * 3 + 500_000 * 3 / 10 + 100_000 * 15 / 4);
    }

    #[test]
    fn litellm_entries_convert_to_micros_per_million() {
        let info = serde_json::json!({
            "input_cost_per_token": 0.0000025,
            "output_cost_per_token": 0.00001,
            "cache_read_input_token_cost": 0.00000125
        });
        let rule = litellm_rule("gpt-4o", &info).unwrap();
        assert_eq!(rule.input_micros_per_million, 2_500_000);
        assert_eq!(rule.output_micros_per_million, 10_000_000);
        assert_eq!(rule.cache_read_micros_per_million, 1_250_000);
        assert_eq!(rule.cache_creation_micros_per_million, 0);
        assert!(litellm_rule("free-model", &serde_json::json!({})).is_none());
    }

    #[test]
    fn builtin_snapshot_is_valid_and_covers_mainstream_models() {
        let rules = builtin_list_prices();
        assert!(rules.len() >= 15);
        for prefix in ["gpt-4o-mini", "gpt-4o", "deepseek-chat", "claude-sonnet-4"] {
            assert!(rules.iter().any(|rule| rule.model == prefix));
        }
        let deepseek = rules
            .iter()
            .find(|rule| rule.model == "deepseek-chat")
            .unwrap();
        let off_peak = deepseek.off_peak.as_ref().unwrap();
        assert_eq!(off_peak.start_minute_utc, 990);
        assert_eq!(off_peak.end_minute_utc, 30);
        // More specific prefixes must precede their generic counterparts.
        let position = |model: &str| rules.iter().position(|rule| rule.model == model).unwrap();
        assert!(position("gpt-4o-mini") < position("gpt-4o"));
        assert!(position("gpt-5-mini") < position("gpt-5"));
    }
}
