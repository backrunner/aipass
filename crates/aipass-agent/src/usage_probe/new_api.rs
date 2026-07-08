use aipass_agent_protocol::{UsageProbeQuota, UsageProbeResult, UsageProbeSource};
use aipass_provider_registry::GatewayMetadata;
use reqwest::blocking::Client;
use serde_json::Value;

use super::errors::{parse_failure, validation_failure};
use super::http::get_json;
use super::urls::{newapi_token_urls, newapi_user_self_urls};
use super::values::{
    bool_field, data_object, expires_at, format_newapi_quota, is_success_like, number_field,
    response_message, string_field,
};

pub(super) fn run_newapi_token_probe(
    client: &Client,
    endpoint: &str,
    api_key: &str,
    provider_id: Option<String>,
    redactions: &[String],
) -> UsageProbeResult {
    let mut last_failure = None;
    for url in newapi_token_urls(endpoint) {
        let source = UsageProbeSource::NewApiTokenUsage;
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
                match parse_newapi_token_usage(&body, provider_id.clone(), url.clone(), status) {
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
            UsageProbeSource::NewApiTokenUsage,
            "unable to build New API token usage URL",
        )
    })
}

pub(super) fn run_newapi_user_self_probe(
    client: &Client,
    endpoint: &str,
    access_token: &str,
    user_id: &str,
    provider_id: Option<String>,
    redactions: &[String],
) -> UsageProbeResult {
    let mut last_failure = None;
    for url in newapi_user_self_urls(endpoint) {
        let source = UsageProbeSource::NewApiUserSelf;
        let headers = [("New-Api-User", user_id)];
        match get_json(
            client,
            &url,
            access_token,
            &headers,
            provider_id.clone(),
            source,
            redactions,
        ) {
            Ok((status, body)) => {
                match parse_newapi_user_self(&body, provider_id.clone(), url.clone(), status) {
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
            UsageProbeSource::NewApiUserSelf,
            "unable to build New API user URL",
        )
    })
}

fn parse_newapi_token_usage(
    body: &Value,
    provider_id: Option<String>,
    endpoint: String,
    status: u16,
) -> Result<UsageProbeResult, String> {
    if !is_success_like(body) {
        return Err(
            response_message(body).unwrap_or_else(|| "New API usage query failed".to_string())
        );
    }

    let data = data_object(body);
    let label = string_field(data, "name")
        .or_else(|| Some("New API token".to_string()))
        .filter(|value| !value.trim().is_empty());
    let granted = number_field(data, "total_granted");
    let used = number_field(data, "total_used");
    let available = number_field(data, "total_available");
    let unlimited = bool_field(data, "unlimited_quota").unwrap_or(false);
    let reset_at = expires_at(data.get("expires_at"));

    if granted.is_none() && used.is_none() && available.is_none() && !unlimited {
        return Err("response is missing token usage fields".to_string());
    }

    Ok(UsageProbeResult {
        ok: true,
        provider_id,
        source: UsageProbeSource::NewApiTokenUsage,
        endpoint: Some(endpoint),
        status: Some(status),
        quota: Some(UsageProbeQuota {
            label: label.clone(),
            limit: if unlimited {
                Some("unlimited".to_string())
            } else {
                granted.map(format_newapi_quota)
            },
            used: used.map(format_newapi_quota),
            remaining: available.map(format_newapi_quota),
            reset_at,
            unit: Some("USD".to_string()),
        }),
        gateway: None,
        plan_name: label,
        message: Some("New API token usage".to_string()),
        error: None,
    })
}

fn parse_newapi_user_self(
    body: &Value,
    provider_id: Option<String>,
    endpoint: String,
    status: u16,
) -> Result<UsageProbeResult, String> {
    if !is_success_like(body) {
        return Err(
            response_message(body).unwrap_or_else(|| "New API user query failed".to_string())
        );
    }

    let data = data_object(body);
    let group = string_field(data, "group");
    let quota_remaining = number_field(data, "quota");
    let quota_used = number_field(data, "used_quota");
    let quota_total = match (quota_remaining, quota_used) {
        (Some(remaining), Some(used)) => Some(remaining + used),
        _ => None,
    };
    let label = group.clone().unwrap_or_else(|| "New API".to_string());

    if group.is_none() && quota_remaining.is_none() && quota_used.is_none() {
        return Err("response is missing New API user quota fields".to_string());
    }

    Ok(UsageProbeResult {
        ok: true,
        provider_id,
        source: UsageProbeSource::NewApiUserSelf,
        endpoint: Some(endpoint),
        status: Some(status),
        quota: Some(UsageProbeQuota {
            label: Some(label.clone()),
            limit: quota_total.map(format_newapi_quota),
            used: quota_used.map(format_newapi_quota),
            remaining: quota_remaining.map(format_newapi_quota),
            reset_at: None,
            unit: Some("USD".to_string()),
        }),
        gateway: group.clone().map(|group| GatewayMetadata {
            group: Some(group),
            rate: None,
        }),
        plan_name: Some(label),
        message: Some("New API user quota".to_string()),
        error: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_newapi_token_usage() {
        let value = json!({
            "code": true,
            "message": "ok",
            "data": {
                "object": "token_usage",
                "name": "Default Token",
                "total_granted": 1000000,
                "total_used": 12345,
                "total_available": 987655,
                "unlimited_quota": false,
                "expires_at": 0
            }
        });

        let result = parse_newapi_token_usage(
            &value,
            Some("newapi".to_string()),
            "https://n/api/usage/token".to_string(),
            200,
        )
        .expect("parsed");

        let quota = result.quota.expect("quota");
        assert_eq!(quota.label.as_deref(), Some("Default Token"));
        assert_eq!(quota.limit.as_deref(), Some("2"));
        assert_eq!(quota.used.as_deref(), Some("0.0247"));
        assert_eq!(quota.remaining.as_deref(), Some("1.9753"));
        assert_eq!(quota.reset_at, None);
    }

    #[test]
    fn parses_newapi_user_self_group() {
        let value = json!({
            "success": true,
            "data": {
                "group": "vip",
                "quota": 1500000,
                "used_quota": 500000
            }
        });

        let result =
            parse_newapi_user_self(&value, None, "https://n/api/user/self".to_string(), 200)
                .expect("parsed");

        assert_eq!(
            result.gateway.and_then(|gateway| gateway.group).as_deref(),
            Some("vip")
        );
        let quota = result.quota.expect("quota");
        assert_eq!(quota.limit.as_deref(), Some("4"));
        assert_eq!(quota.used.as_deref(), Some("1"));
        assert_eq!(quota.remaining.as_deref(), Some("3"));
    }
}
