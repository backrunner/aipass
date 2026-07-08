mod errors;
mod http;
mod new_api;
mod sub_api;
mod urls;
mod values;

use aipass_agent_protocol::{
    endpoint_url, SensitiveString, UsageProbeMode, UsageProbeResult, UsageProbeSource,
};
use aipass_vault::EntrySummary;
use reqwest::blocking::Client;
use std::time::Duration;

use self::errors::{missing_api_key, validation_failure};
use self::http::redactions;
use self::new_api::{run_newapi_token_probe, run_newapi_user_self_probe};
use self::sub_api::run_subapi_probe;
use self::values::non_empty;

const USER_AGENT: &str = "AIPass/1.0";

pub(crate) struct UsageProbeOptions {
    pub(crate) mode: UsageProbeMode,
    pub(crate) timeout_seconds: u64,
    pub(crate) base_url: Option<String>,
    pub(crate) access_token: Option<SensitiveString>,
    pub(crate) user_id: Option<String>,
}

pub(crate) fn probe_provider_usage(
    entry: EntrySummary,
    api_key: String,
    options: UsageProbeOptions,
) -> UsageProbeResult {
    let endpoint = options
        .base_url
        .as_deref()
        .and_then(non_empty)
        .map(str::to_string)
        .or_else(|| endpoint_url(&entry.endpoints));

    let Some(endpoint) = endpoint else {
        return UsageProbeResult {
            ok: false,
            provider_id: entry.provider_id,
            source: UsageProbeSource::Unknown,
            endpoint: None,
            status: None,
            quota: None,
            gateway: None,
            plan_name: None,
            message: None,
            error: Some("provider has no API endpoint".to_string()),
        };
    };

    let client = match Client::builder()
        .timeout(Duration::from_secs(options.timeout_seconds.clamp(1, 120)))
        .user_agent(USER_AGENT)
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            return UsageProbeResult {
                ok: false,
                provider_id: entry.provider_id,
                source: UsageProbeSource::Unknown,
                endpoint: Some(endpoint),
                status: None,
                quota: None,
                gateway: None,
                plan_name: None,
                message: None,
                error: Some(err.to_string()),
            }
        }
    };

    let provider_id = entry.provider_id.clone();
    let redactions = redactions(&api_key, options.access_token.as_ref());

    match options.mode {
        UsageProbeMode::NewApi => {
            if api_key.trim().is_empty() {
                return missing_api_key(
                    provider_id,
                    Some(endpoint),
                    UsageProbeSource::NewApiTokenUsage,
                );
            }
            run_newapi_token_probe(&client, &endpoint, &api_key, provider_id, &redactions)
        }
        UsageProbeMode::SubApi => {
            if api_key.trim().is_empty() {
                return missing_api_key(
                    provider_id,
                    Some(endpoint),
                    UsageProbeSource::SubApiV1Usage,
                );
            }
            run_subapi_probe(&client, &endpoint, &api_key, provider_id, &redactions)
        }
        UsageProbeMode::NewApiAdvanced => {
            let access_token = options
                .access_token
                .as_ref()
                .map(SensitiveString::expose)
                .and_then(non_empty);
            let user_id = options.user_id.as_deref().and_then(non_empty);
            let Some(access_token) = access_token else {
                return validation_failure(
                    provider_id,
                    Some(endpoint),
                    UsageProbeSource::NewApiUserSelf,
                    "New API advanced probe requires an access token",
                );
            };
            let Some(user_id) = user_id else {
                return validation_failure(
                    provider_id,
                    Some(endpoint),
                    UsageProbeSource::NewApiUserSelf,
                    "New API advanced probe requires a user id",
                );
            };
            run_newapi_user_self_probe(
                &client,
                &endpoint,
                access_token,
                user_id,
                provider_id,
                &redactions,
            )
        }
        UsageProbeMode::Auto => {
            if api_key.trim().is_empty() {
                return missing_api_key(provider_id, Some(endpoint), UsageProbeSource::Unknown);
            }
            run_auto_probe(&client, &endpoint, &api_key, provider_id, &redactions)
        }
    }
}

type KeyProbe = fn(&Client, &str, &str, Option<String>, &[String]) -> UsageProbeResult;

fn run_auto_probe(
    client: &Client,
    endpoint: &str,
    api_key: &str,
    provider_id: Option<String>,
    redactions: &[String],
) -> UsageProbeResult {
    let endpoint_lower = endpoint.to_ascii_lowercase();
    let prefer_newapi = endpoint_lower.contains("newapi")
        || endpoint_lower.contains("new-api")
        || endpoint_lower.contains("one-api");
    let prefer_subapi = endpoint_lower.contains("sub2api") || endpoint_lower.contains("subapi");

    let mut failures = Vec::new();
    let mut attempts: Vec<KeyProbe> = if prefer_newapi && !prefer_subapi {
        vec![run_newapi_token_probe, run_subapi_probe]
    } else {
        vec![run_subapi_probe, run_newapi_token_probe]
    };

    if prefer_subapi {
        attempts = vec![run_subapi_probe, run_newapi_token_probe];
    }

    for attempt in attempts {
        let result = attempt(client, endpoint, api_key, provider_id.clone(), redactions);
        if result.ok {
            return result;
        }
        failures.push(result);
    }

    let message = failures
        .iter()
        .filter_map(|result| {
            result.error.as_ref().map(|error| {
                let endpoint = result.endpoint.as_deref().unwrap_or("unknown endpoint");
                format!("{endpoint}: {error}")
            })
        })
        .collect::<Vec<_>>()
        .join("; ");

    UsageProbeResult {
        ok: false,
        provider_id,
        source: UsageProbeSource::Unknown,
        endpoint: Some(endpoint.to_string()),
        status: failures.iter().find_map(|result| result.status),
        quota: None,
        gateway: None,
        plan_name: None,
        message: None,
        error: Some(if message.is_empty() {
            "all usage probes failed".to_string()
        } else {
            format!("all usage probes failed: {message}")
        }),
    }
}
