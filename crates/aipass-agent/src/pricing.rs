use crate::session::{map_vault_error, ServiceError, ServiceResult};
use aipass_agent_protocol::{
    GroupPriceVersion, ModelPriceRule, OffPeakWindow, PricingConfig, PricingGroup,
};
use aipass_crypto::Ciphertext;
use aipass_proxy::ModelPricing;
use aipass_storage::atomic_write_bytes;
use aipass_vault::Vault;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use time::OffsetDateTime;
use uuid::Uuid;

mod list_prices;
pub use list_prices::{builtin_list_prices, load_list_prices, spawn_list_price_refresh};

const NEWAPI_RATIO_MICROS_PER_UNIT: f64 = 2_000_000.0;

const CONFIG_FILE: &str = "pricing.aipstate";
const CONFIG_PURPOSE: &str = "proxy-pricing";

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PersistedPricingConfig {
    version: u32,
    payload: Ciphertext,
}

pub fn load_pricing_config(vault_dir: &Path, vault: &Vault) -> ServiceResult<PricingConfig> {
    let path = vault_dir.join(CONFIG_FILE);
    if !path.exists() {
        return Ok(PricingConfig {
            list_price_updated_at: list_prices::updated_at(vault_dir),
            ..Default::default()
        });
    }
    let persisted: PersistedPricingConfig =
        serde_json::from_slice(&std::fs::read(path).map_err(ServiceError::internal)?)
            .map_err(ServiceError::internal)?;
    let bytes = vault
        .decrypt_local_state(CONFIG_PURPOSE, &persisted.payload)
        .map_err(map_vault_error)?;
    let mut config: PricingConfig =
        serde_json::from_slice(&bytes).map_err(ServiceError::internal)?;
    // Public price updates run even while the vault is locked. Their timestamp
    // belongs to the same public snapshot, not the encrypted user settings.
    config.list_price_updated_at = list_prices::updated_at(vault_dir);
    Ok(config)
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

/// Best-effort synchronization of New API's public pricing table. New API's
/// `model_ratio` uses the same convention as one-api: ratio 1 equals
/// $0.002/1K tokens, or $2/M tokens. The resulting rules are stored as
/// namespaced pricing groups so a remote refresh cannot overwrite a user's
/// manually-created group with the same name.
pub fn fetch_newapi_pricing(endpoint: &str, api_key: &str, timeout_seconds: u64) -> Option<Value> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(timeout_seconds.clamp(1, 120)))
        .build()
        .ok()?;
    crate::usage_probe::newapi_pricing_urls(endpoint)
        .into_iter()
        .find_map(|url| {
            // Pricing sync is allowed to contact the same HTTPS/loopback
            // endpoints as usage probing. In particular, never put a vault
            // API key into an arbitrary HTTP URL from provider metadata.
            crate::usage_probe::validate_probe_url(&url).ok()?;
            // New API exposes `/api/pricing` through a public-or-dashboard
            // middleware, while an ordinary relay key is not a dashboard PAT.
            // Try the public form first so a relay key cannot accidentally turn
            // a public pricing request into a dashboard-auth failure.
            let response = client.get(&url).send().ok()?;
            let response = if response.status().is_success() {
                response
            } else {
                client.get(&url).bearer_auth(api_key).send().ok()?
            };
            if !response.status().is_success() {
                return None;
            }
            response.json::<Value>().ok()
        })
}

/// Fetch the key-scoped billing multiplier exposed by SubAPI. SubAPI does not
/// expose a model-price table to relay keys; its billing endpoint only tells us
/// which multiplier applies to this key.
pub fn fetch_subapi_billing(endpoint: &str, api_key: &str, timeout_seconds: u64) -> Option<Value> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(timeout_seconds.clamp(1, 120)))
        .build()
        .ok()?;
    crate::usage_probe::subapi_billing_urls(endpoint)
        .into_iter()
        .find_map(|url| {
            crate::usage_probe::validate_probe_url(&url).ok()?;
            let response = client.get(&url).bearer_auth(api_key).send().ok()?;
            if !response.status().is_success() {
                return None;
            }
            response.json::<Value>().ok()
        })
}

pub fn sync_newapi_pricing(
    vault_dir: &Path,
    vault: &Vault,
    entry_id: Uuid,
    endpoint: &str,
    secret_groups: &[(String, Option<String>)],
    payload: &Value,
) -> ServiceResult<PricingConfig> {
    let mut config = load_pricing_config(vault_dir, vault)?;
    if payload
        .get("success")
        .and_then(Value::as_bool)
        .is_some_and(|success| !success)
    {
        return Ok(config);
    }
    let data = payload.get("data").and_then(Value::as_array);
    let ratios = payload.get("group_ratio").and_then(Value::as_object);
    let (Some(models), Some(ratios)) = (data, ratios) else {
        return Ok(config);
    };

    let now = OffsetDateTime::now_utc().unix_timestamp();
    let mut changed = false;
    let mut group_ids = HashMap::new();
    for (group_name, raw_ratio) in ratios {
        let Some(group_ratio) =
            json_number(raw_ratio).filter(|value| value.is_finite() && *value >= 0.0)
        else {
            continue;
        };
        let mut rules = models
            .iter()
            .filter_map(|model| newapi_model_rule(model, group_name, group_ratio))
            .collect::<Vec<_>>();
        rules.sort_by(|left, right| left.model.cmp(&right.model));
        if rules.is_empty() {
            continue;
        }
        // Keep independently hosted gateways from sharing a pricing group
        // just because they use the same public group name (for example,
        // both may expose a group called `default`).
        let name = format!("New API / {group_name} @ {}", pricing_namespace(endpoint));
        let group_id = if let Some(group) = config.groups.iter().find(|group| group.name == name) {
            group.id
        } else {
            config.groups.push(PricingGroup {
                id: Uuid::new_v4(),
                name,
                versions: Vec::new(),
            });
            changed = true;
            config.groups.last().expect("just pushed pricing group").id
        };
        let group = config
            .groups
            .iter_mut()
            .find(|group| group.id == group_id)
            .expect("pricing group id must exist");
        let current_rules = group.versions.last().map(|version| &version.rules);
        if current_rules != Some(&rules) {
            group
                .versions
                .retain(|version| version.effective_from != now);
            group.versions.push(GroupPriceVersion {
                effective_from: now,
                rules,
            });
            changed = true;
        }
        group_ids.insert(group_name.clone(), group_id);
    }
    for (secret_id, secret_group) in secret_groups {
        let assignment = config
            .assignments
            .iter_mut()
            .find(|item| item.entry_id == entry_id && item.secret_id == *secret_id);
        let Some(assignment) = assignment else {
            if let Some(group_id) = secret_group.as_deref().and_then(|name| group_ids.get(name)) {
                config
                    .assignments
                    .push(aipass_agent_protocol::CredentialAssignment {
                        entry_id,
                        secret_id: secret_id.clone(),
                        group_id: Some(*group_id),
                        multiplier: 1.0,
                    });
                changed = true;
            }
            continue;
        };

        // Only assignments created by this synchronizer may be changed. A
        // user-created group remains authoritative even if its name happens
        // to mention New API. When the key group disappears or changes, clear
        // the old managed assignment so stale prices cannot be applied.
        let managed_assignment = assignment
            .group_id
            .and_then(|group_id| config.groups.iter().find(|group| group.id == group_id))
            .is_some_and(|group| is_managed_newapi_group(group, endpoint));
        let next_group_id = secret_group
            .as_deref()
            .and_then(|name| group_ids.get(name))
            .copied();
        let can_auto_update =
            managed_assignment || (assignment.group_id.is_none() && assignment.multiplier == 1.0);
        if next_group_id.is_some() && can_auto_update {
            if assignment.group_id != next_group_id || assignment.multiplier != 1.0 {
                assignment.group_id = next_group_id;
                assignment.multiplier = 1.0;
                changed = true;
            }
        } else if next_group_id.is_none() && managed_assignment {
            assignment.group_id = None;
            assignment.multiplier = 1.0;
            changed = true;
        }
    }
    if changed {
        save_pricing_config(vault_dir, vault, &config)?;
    }
    Ok(config)
}

fn is_managed_newapi_group(group: &PricingGroup, endpoint: &str) -> bool {
    let Some(rest) = group.name.strip_prefix("New API / ") else {
        return false;
    };
    let Some((group_name, namespace)) = rest.rsplit_once(" @ ") else {
        return false;
    };
    !group_name.trim().is_empty() && namespace == pricing_namespace(endpoint)
}

/// Apply a SubAPI key's effective billing multiplier. The upstream billing
/// endpoint is key-scoped and intentionally does not publish model prices, so
/// `resolve_cost` keeps using local overrides or the official list snapshot as
/// the base. The empty marker group also prevents an automatic sync from
/// overriding a user's manually configured model prices.
pub fn sync_subapi_pricing(
    vault_dir: &Path,
    vault: &Vault,
    entry_id: Uuid,
    secret_id: &str,
    endpoint: &str,
    payload: &Value,
) -> ServiceResult<PricingConfig> {
    let mut config = load_pricing_config(vault_dir, vault)?;
    let Some(multiplier) = subapi_billing_multiplier(payload) else {
        return Ok(config);
    };

    let name = format!("SubAPI / {}", pricing_namespace(endpoint));
    let mut changed = false;
    let group_id = if let Some(group) = config.groups.iter().find(|group| group.name == name) {
        group.id
    } else {
        config.groups.push(PricingGroup {
            id: Uuid::new_v4(),
            name,
            versions: Vec::new(),
        });
        changed = true;
        config.groups.last().expect("just pushed pricing group").id
    };

    let assignment = config
        .assignments
        .iter_mut()
        .find(|item| item.entry_id == entry_id && item.secret_id == secret_id);
    if let Some(assignment) = assignment {
        let managed_assignment = assignment
            .group_id
            .and_then(|group_id| config.groups.iter().find(|group| group.id == group_id))
            .is_some_and(|group| is_managed_subapi_group(group, endpoint));
        let can_update_assignment =
            managed_assignment || (assignment.group_id.is_none() && assignment.multiplier == 1.0);
        if can_update_assignment
            && (assignment.group_id != Some(group_id) || assignment.multiplier != multiplier)
        {
            assignment.group_id = Some(group_id);
            assignment.multiplier = multiplier;
            changed = true;
        }
    } else {
        config
            .assignments
            .push(aipass_agent_protocol::CredentialAssignment {
                entry_id,
                secret_id: secret_id.to_string(),
                group_id: Some(group_id),
                multiplier,
            });
        changed = true;
    }
    if changed {
        save_pricing_config(vault_dir, vault, &config)?;
    }
    Ok(config)
}

fn is_managed_subapi_group(group: &PricingGroup, endpoint: &str) -> bool {
    group
        .name
        .strip_prefix("SubAPI / ")
        .is_some_and(|namespace| namespace == pricing_namespace(endpoint))
}

fn subapi_billing_multiplier(payload: &Value) -> Option<f64> {
    let data = payload.get("data").unwrap_or(payload);
    [
        "effective_rate_multiplier",
        "resolved_rate_multiplier",
        "group_rate_multiplier",
    ]
    .iter()
    .find_map(|field| data.get(*field).and_then(json_number))
    .filter(|value| value.is_finite() && *value >= 0.0)
}

fn pricing_namespace(endpoint: &str) -> String {
    let Ok(url) = reqwest::Url::parse(endpoint) else {
        return endpoint.trim().trim_end_matches('/').to_string();
    };
    let mut namespace = url.host_str().unwrap_or("unknown-host").to_string();
    if let Some(port) = url.port() {
        namespace.push(':');
        namespace.push_str(&port.to_string());
    }
    let path = url.path().trim_end_matches('/');
    let path = path.strip_suffix("/v1").unwrap_or(path);
    if !path.is_empty() {
        namespace.push_str(path);
    }
    namespace
}

fn newapi_model_rule(model: &Value, group_name: &str, group_ratio: f64) -> Option<ModelPriceRule> {
    let model_name = model.get("model_name").and_then(Value::as_str)?.trim();
    if model_name.is_empty() {
        return None;
    }
    let groups = model.get("enable_groups").and_then(Value::as_array);
    let enabled = groups
        .map(|groups| {
            groups.iter().any(|group| {
                group
                    .as_str()
                    .is_some_and(|group| group == "all" || group == group_name)
            })
        })
        .unwrap_or(true);
    if !enabled {
        return None;
    }
    // QuotaType 1 is per-call pricing and cannot be represented by the
    // token-based proxy pricing schema without inventing a token equivalent.
    let quota_type = model
        .get("quota_type")
        .and_then(json_number)
        .map(|value| value as i64)
        .unwrap_or(0);
    if quota_type == 1 {
        return None;
    }
    let ratio = json_number(model.get("model_ratio")?)
        .filter(|value| value.is_finite() && *value >= 0.0)?;
    let input = ratio * group_ratio * NEWAPI_RATIO_MICROS_PER_UNIT;
    let completion = model
        .get("completion_ratio")
        .and_then(json_number)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(1.0);
    let cache = model
        .get("cache_ratio")
        .and_then(json_number)
        .filter(|value| value.is_finite() && *value >= 0.0)
        // New API defaults an omitted cache ratio to 1 (same as normal input).
        .unwrap_or(1.0);
    let cache_creation = model
        .get("create_cache_ratio")
        .and_then(json_number)
        .filter(|value| value.is_finite() && *value >= 0.0)
        // New API defaults an omitted cache-creation ratio to 1.25.
        .unwrap_or(1.25);
    Some(ModelPriceRule {
        model: model_name.to_string(),
        input_micros_per_million: rounded_price(input),
        output_micros_per_million: rounded_price(input * completion),
        cache_read_micros_per_million: rounded_price(input * cache),
        cache_creation_micros_per_million: rounded_price(input * cache_creation),
        off_peak: None,
    })
}

fn json_number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_i64().map(|value| value as f64))
        .or_else(|| value.as_u64().map(|value| value as f64))
        .or_else(|| value.as_str().and_then(|value| value.parse::<f64>().ok()))
}

fn rounded_price(value: f64) -> u64 {
    if value.is_finite() && value > 0.0 && value < u64::MAX as f64 {
        value.round() as u64
    } else {
        0
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use aipass_agent_protocol::{CredentialAssignment, GroupPriceVersion, PricingGroup};
    use serde_json::json;

    const DAY: i64 = 86_400 * 20_000;

    #[test]
    fn converts_newapi_ratios_to_token_prices() {
        let model = json!({
            "model_name": "gpt-test",
            "model_ratio": 1.5,
            "completion_ratio": 2.0,
            "cache_ratio": 0.25,
            "create_cache_ratio": 0.5,
            "enable_groups": ["default"]
        });
        let rule = newapi_model_rule(&model, "default", 0.8).expect("pricing rule");
        assert_eq!(rule.input_micros_per_million, 2_400_000);
        assert_eq!(rule.output_micros_per_million, 4_800_000);
        assert_eq!(rule.cache_read_micros_per_million, 600_000);
        assert_eq!(rule.cache_creation_micros_per_million, 1_200_000);
    }

    #[test]
    fn skips_newapi_models_not_enabled_for_group() {
        let model = json!({
            "model_name": "gpt-test",
            "model_ratio": 1.0,
            "enable_groups": ["vip"]
        });
        assert!(newapi_model_rule(&model, "default", 1.0).is_none());
    }

    #[test]
    fn skips_newapi_per_call_models() {
        let model = json!({
            "model_name": "image-model",
            "quota_type": 1,
            "model_price": 0.02,
            "model_ratio": 1.0
        });
        assert!(newapi_model_rule(&model, "default", 1.0).is_none());
    }

    #[test]
    fn keeps_free_newapi_models_free() {
        let model = json!({
            "model_name": "free-model",
            "model_ratio": 0,
            "completion_ratio": 0
        });
        let rule = newapi_model_rule(&model, "default", 1.0).expect("free pricing rule");
        assert_eq!(rule.input_micros_per_million, 0);
        assert_eq!(rule.output_micros_per_million, 0);
    }

    #[test]
    fn uses_newapi_default_cache_ratios_when_omitted() {
        let model = json!({
            "model_name": "gpt-test",
            "model_ratio": 1.0
        });
        let rule = newapi_model_rule(&model, "default", 1.0).expect("pricing rule");
        assert_eq!(rule.cache_read_micros_per_million, 2_000_000);
        assert_eq!(rule.cache_creation_micros_per_million, 2_500_000);
    }

    #[test]
    fn namespaces_remote_groups_by_endpoint() {
        assert_eq!(
            pricing_namespace("https://example.com/proxy/v1"),
            "example.com/proxy"
        );
        assert_eq!(
            pricing_namespace("https://example.com:8443/v1"),
            "example.com:8443"
        );
    }

    #[test]
    fn managed_newapi_groups_are_scoped_to_the_endpoint() {
        let group = PricingGroup {
            id: Uuid::nil(),
            name: "New API / default @ example.com/proxy".to_string(),
            versions: Vec::new(),
        };
        assert!(is_managed_newapi_group(
            &group,
            "https://example.com/proxy/v1"
        ));
        assert!(!is_managed_newapi_group(&group, "https://other.example/v1"));
        assert!(!is_managed_newapi_group(
            &PricingGroup {
                id: Uuid::nil(),
                name: "New API / default".to_string(),
                versions: Vec::new(),
            },
            "https://example.com/proxy/v1"
        ));
    }

    #[test]
    fn parses_subapi_effective_billing_multiplier() {
        assert_eq!(
            subapi_billing_multiplier(&json!({
                "group_rate_multiplier": 0.8,
                "resolved_rate_multiplier": 0.6,
                "effective_rate_multiplier": 0.9
            })),
            Some(0.9)
        );
        assert_eq!(
            subapi_billing_multiplier(&json!({
                "data": {"resolved_rate_multiplier": "0.6"}
            })),
            Some(0.6)
        );
        assert_eq!(
            subapi_billing_multiplier(&json!({"effective_rate_multiplier": -1})),
            None
        );
    }

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
