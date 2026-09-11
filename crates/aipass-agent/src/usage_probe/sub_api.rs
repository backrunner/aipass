use aipass_agent_protocol::{UsageProbeQuota, UsageProbeResult, UsageProbeSource};
use aipass_provider_registry::{GatewayMetadata, SubscriptionSnapshot, SubscriptionWindow};
use reqwest::blocking::Client;
use serde_json::Value;

use super::errors::{parse_failure, validation_failure};
use super::http::get_json;
use super::urls::{subapi_billing_urls, subapi_usage_urls};
use super::values::{
    data_object, expires_at, format_amount, is_valid_like, is_wallet_plan_name, number_field,
    response_message, string_field,
};

pub(super) fn run_subapi_probe(
    client: &Client,
    endpoint: &str,
    api_key: &str,
    provider_id: Option<String>,
    redactions: &[String],
) -> UsageProbeResult {
    let mut last_failure = None;
    for url in subapi_usage_urls(endpoint) {
        let source = UsageProbeSource::SubApiV1Usage;
        match get_json(
            client,
            &url,
            api_key,
            &[],
            provider_id.clone(),
            source,
            redactions,
        ) {
            Ok((status, body)) => {
                match parse_subapi_usage(&body, provider_id.clone(), url.clone(), status) {
                    Ok(mut result) => {
                        // Billing metadata is key-scoped and optional. A
                        // wallet-only key legitimately returns 403 here; in
                        // that case keep the balance result and omit group/rate.
                        if let Some((group, rate)) = probe_subapi_billing(
                            client,
                            endpoint,
                            api_key,
                            provider_id.clone(),
                            redactions,
                        ) {
                            let gateway = result.gateway.get_or_insert(GatewayMetadata {
                                group: None,
                                rate: None,
                            });
                            gateway.group = group.or_else(|| gateway.group.take());
                            gateway.rate = rate.or_else(|| gateway.rate.take());
                        }
                        return result;
                    }
                    Err(error) => {
                        last_failure = Some(parse_failure(
                            provider_id.clone(),
                            Some(url),
                            source,
                            status,
                            error,
                        ));
                    }
                }
            }
            Err(result) => last_failure = Some(*result),
        }
    }
    last_failure.unwrap_or_else(|| {
        validation_failure(
            provider_id,
            Some(endpoint.to_string()),
            UsageProbeSource::SubApiV1Usage,
            "unable to build SubAPI usage URL",
        )
    })
}

fn probe_subapi_billing(
    client: &Client,
    endpoint: &str,
    api_key: &str,
    provider_id: Option<String>,
    redactions: &[String],
) -> Option<(Option<String>, Option<String>)> {
    for url in subapi_billing_urls(endpoint) {
        let source = UsageProbeSource::SubApiV1Usage;
        let Ok((_status, body)) = get_json(
            client,
            &url,
            api_key,
            &[],
            provider_id.clone(),
            source,
            redactions,
        ) else {
            continue;
        };
        let data = data_object(&body);
        if !is_valid_like(data) {
            continue;
        }
        let rate = number_field(data, "effective_rate_multiplier")
            .or_else(|| number_field(data, "resolved_rate_multiplier"))
            .or_else(|| number_field(data, "group_rate_multiplier"))
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map(format_amount)
            .map(|value| format!("{value}x"));
        let group = string_field(data, "group").or_else(|| string_field(data, "group_name"));
        if group.is_some() || rate.is_some() {
            return Some((group, rate));
        }
    }
    None
}

fn parse_subapi_usage(
    body: &Value,
    provider_id: Option<String>,
    endpoint: String,
    status: u16,
) -> Result<UsageProbeResult, String> {
    let data = data_object(body);
    if !is_valid_like(data) {
        return Err(
            response_message(data).unwrap_or_else(|| "SubAPI usage query failed".to_string())
        );
    }

    let mode = string_field(data, "mode");
    let plan_name = string_field(data, "planName")
        .or_else(|| string_field(data, "plan_name"))
        .or_else(|| string_field(data, "name"));
    let unit = string_field(data, "unit").unwrap_or_else(|| "USD".to_string());
    let quota_obj = data.get("quota").and_then(Value::as_object);
    let subscription = data
        .get("subscription")
        .and_then(|value| parse_subapi_subscription(value, plan_name.clone()));

    let quota = if let Some(quota_obj) = quota_obj {
        let quota_value = Value::Object(quota_obj.clone());
        Some(UsageProbeQuota {
            label: plan_name.clone().or_else(|| Some("SubAPI".to_string())),
            limit: number_field(&quota_value, "limit")
                .filter(|v| v.is_finite() && *v >= 0.0)
                .map(format_amount),
            used: number_field(&quota_value, "used")
                .filter(|v| v.is_finite() && *v >= 0.0)
                .map(format_amount),
            remaining: number_field(&quota_value, "remaining")
                .filter(|v| v.is_finite() && *v >= 0.0)
                .map(format_amount),
            reset_at: string_field(&quota_value, "reset_at")
                .or_else(|| string_field(&quota_value, "resetAt")),
            unit: string_field(&quota_value, "unit").or_else(|| Some(unit.clone())),
        })
    } else {
        // Unrestricted mode: `remaining` is the wallet balance or the smallest
        // subscription headroom. Upstream uses -1 for a subscription with no
        // configured period limits; surface that as "unlimited" instead of
        // dropping the quota entirely.
        let remaining = number_field(data, "remaining").or_else(|| number_field(data, "balance"));
        match remaining.filter(|value| value.is_finite()) {
            Some(remaining) if remaining >= 0.0 => Some(UsageProbeQuota {
                label: plan_name.clone().or_else(|| Some("SubAPI".to_string())),
                limit: number_field(data, "total")
                    .filter(|v| v.is_finite() && *v >= 0.0)
                    .map(format_amount),
                used: number_field(data, "used")
                    .filter(|v| v.is_finite() && *v >= 0.0)
                    .map(format_amount),
                remaining: Some(format_amount(remaining)),
                reset_at: None,
                unit: Some(unit.clone()),
            }),
            Some(_) if subscription.is_some() => Some(UsageProbeQuota {
                label: plan_name.clone().or_else(|| Some("SubAPI".to_string())),
                limit: Some("unlimited".to_string()),
                used: None,
                remaining: None,
                reset_at: None,
                unit: Some(unit.clone()),
            }),
            _ => None,
        }
    };

    let gateway = plan_name
        .as_deref()
        .filter(|name| !is_wallet_plan_name(name))
        .map(|name| GatewayMetadata {
            group: Some(name.to_string()),
            rate: None,
        });

    if quota.is_none() && gateway.is_none() && subscription.is_none() {
        return Err("response is missing SubAPI quota fields".to_string());
    }

    Ok(UsageProbeResult {
        ok: true,
        provider_id,
        source: UsageProbeSource::SubApiV1Usage,
        endpoint: Some(endpoint),
        status: Some(status),
        quota,
        gateway,
        subscription,
        plan_name,
        message: mode.map(|mode| format!("SubAPI usage mode: {mode}")),
        error: None,
    })
}

/// Map a SubAPI `subscription` object into a snapshot of daily/weekly/monthly
/// windows. The endpoint reports usage and limits per period but not the
/// window boundaries, so only the weekly reset (anchored at
/// `weekly_window_start`) and the subscription expiry carry timestamps.
fn parse_subapi_subscription(
    subscription: &Value,
    plan_name: Option<String>,
) -> Option<SubscriptionSnapshot> {
    let object = subscription.as_object()?;

    let window = |id: &str, label: &str, minutes: u64, used_field: &str, limit_field: &str| {
        let used = number_field(subscription, used_field).filter(|v| v.is_finite() && *v >= 0.0);
        let limit = number_field(subscription, limit_field).filter(|v| v.is_finite() && *v > 0.0);
        (used.is_some() || limit.is_some()).then(|| SubscriptionWindow {
            id: id.to_string(),
            label: label.to_string(),
            used_percent: match (used, limit) {
                (Some(used), Some(limit)) => Some((used / limit * 100.0).clamp(0.0, 100.0)),
                _ => None,
            },
            resets_at: None,
            window_minutes: Some(minutes),
            source: Some("sub2api-usage".to_string()),
        })
    };

    let mut windows = Vec::new();
    for item in [
        window(
            "daily",
            "Daily",
            24 * 60,
            "daily_usage_usd",
            "daily_limit_usd",
        ),
        window(
            "weekly",
            "Weekly",
            7 * 24 * 60,
            "weekly_usage_usd",
            "weekly_limit_usd",
        ),
        window(
            "monthly",
            "Monthly",
            30 * 24 * 60,
            "monthly_usage_usd",
            "monthly_limit_usd",
        ),
    ]
    .into_iter()
    .flatten()
    {
        windows.push(item);
    }

    // Weekly windows roll over seven days after their anchor.
    if let Some(weekly) = windows.iter_mut().find(|window| window.id == "weekly") {
        weekly.resets_at = subscription
            .get("weekly_window_start")
            .and_then(Value::as_str)
            .and_then(|raw| {
                time::OffsetDateTime::parse(raw, &time::format_description::well_known::Rfc3339)
                    .ok()
            })
            .map(|start| start + time::Duration::days(7))
            .and_then(|reset| {
                reset
                    .format(&time::format_description::well_known::Rfc3339)
                    .ok()
            });
    }

    if object.is_empty() && windows.is_empty() {
        return None;
    }
    Some(SubscriptionSnapshot {
        plan: plan_name,
        subscription_expires_at: expires_at(subscription.get("expires_at")),
        windows,
        observed_at: time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
        source: "sub2api-usage".to_string(),
        ..SubscriptionSnapshot::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_subapi_quota_limited_usage() {
        let value = json!({
            "mode": "quota_limited",
            "isValid": true,
            "status": "active",
            "quota": {
                "limit": 10,
                "used": 2.5,
                "remaining": 7.5,
                "unit": "USD"
            }
        });

        let result = parse_subapi_usage(&value, None, "https://s/v1/usage".to_string(), 200)
            .expect("parsed");

        let quota = result.quota.expect("quota");
        assert_eq!(quota.label.as_deref(), Some("SubAPI"));
        assert_eq!(quota.limit.as_deref(), Some("10"));
        assert_eq!(quota.used.as_deref(), Some("2.5"));
        assert_eq!(quota.remaining.as_deref(), Some("7.5"));
    }

    #[test]
    fn subapi_subscription_plan_becomes_group() {
        let value = json!({
            "mode": "unrestricted",
            "isValid": true,
            "planName": "pro",
            "remaining": 42,
            "unit": "USD"
        });

        let result = parse_subapi_usage(&value, None, "https://s/v1/usage".to_string(), 200)
            .expect("parsed");

        assert_eq!(
            result.gateway.and_then(|gateway| gateway.group).as_deref(),
            Some("pro")
        );
    }

    #[test]
    fn subapi_wallet_balance_does_not_invent_used_or_group() {
        let value = json!({
            "mode": "unrestricted",
            "isValid": true,
            "planName": "钱包余额",
            "balance": 42,
            "unit": "USD"
        });

        let result = parse_subapi_usage(&value, None, "https://s/v1/usage".to_string(), 200)
            .expect("parsed");
        let quota = result.quota.expect("quota");
        assert_eq!(quota.remaining.as_deref(), Some("42"));
        assert_eq!(quota.used, None);
        assert_eq!(result.gateway, None);
    }
}
