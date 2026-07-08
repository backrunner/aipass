use aipass_agent_protocol::{UsageProbeQuota, UsageProbeResult, UsageProbeSource};
use aipass_provider_registry::GatewayMetadata;
use reqwest::blocking::Client;
use serde_json::Value;

use super::errors::{parse_failure, validation_failure};
use super::http::get_json;
use super::urls::subapi_usage_urls;
use super::values::{
    data_object, format_amount, is_valid_like, is_wallet_plan_name, number_field, response_message,
    string_field,
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
                    Ok(result) => return result,
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

    let quota = if let Some(quota_obj) = quota_obj {
        let quota_value = Value::Object(quota_obj.clone());
        Some(UsageProbeQuota {
            label: plan_name.clone().or_else(|| Some("SubAPI".to_string())),
            limit: number_field(&quota_value, "limit").map(format_amount),
            used: number_field(&quota_value, "used").map(format_amount),
            remaining: number_field(&quota_value, "remaining").map(format_amount),
            reset_at: string_field(&quota_value, "reset_at")
                .or_else(|| string_field(&quota_value, "resetAt")),
            unit: string_field(&quota_value, "unit").or_else(|| Some(unit.clone())),
        })
    } else {
        let remaining = number_field(data, "remaining").or_else(|| number_field(data, "balance"));
        remaining.map(|remaining| UsageProbeQuota {
            label: plan_name.clone().or_else(|| Some("SubAPI".to_string())),
            limit: number_field(data, "total").map(format_amount),
            used: number_field(data, "used").map(format_amount),
            remaining: Some(format_amount(remaining)),
            reset_at: None,
            unit: Some(unit.clone()),
        })
    };

    let gateway = plan_name
        .as_deref()
        .filter(|name| !is_wallet_plan_name(name))
        .map(|name| GatewayMetadata {
            group: Some(name.to_string()),
            rate: None,
        });

    if quota.is_none() && gateway.is_none() {
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
        plan_name,
        message: mode.map(|mode| format!("SubAPI usage mode: {mode}")),
        error: None,
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
}
