use aipass_agent_protocol::{SensitiveString, UsageProbeResult, UsageProbeSource};
use reqwest::blocking::Client;
use reqwest::header::ACCEPT;
use serde_json::Value;

use super::errors::validation_failure;
use super::urls::validate_probe_url;

pub(super) fn get_json(
    client: &Client,
    url: &str,
    bearer: &str,
    headers: &[(&str, &str)],
    provider_id: Option<String>,
    source: UsageProbeSource,
    redactions: &[String],
) -> Result<(u16, Value), Box<UsageProbeResult>> {
    if let Err(error) = validate_probe_url(url) {
        return Err(Box::new(validation_failure(
            provider_id,
            Some(url.to_string()),
            source,
            error,
        )));
    }

    let mut request = client
        .get(url)
        .bearer_auth(bearer)
        .header(ACCEPT, "application/json");
    for (key, value) in headers {
        request = request.header(*key, *value);
    }

    let response = match request.send() {
        Ok(response) => response,
        Err(err) => {
            return Err(Box::new(UsageProbeResult {
                ok: false,
                provider_id,
                source,
                endpoint: Some(url.to_string()),
                status: None,
                quota: None,
                gateway: None,
                plan_name: None,
                message: None,
                error: Some(redact_many(&err.to_string(), redactions)),
            }));
        }
    };

    let status = response.status().as_u16();
    let text = match response.text() {
        Ok(text) => text,
        Err(err) => {
            return Err(Box::new(UsageProbeResult {
                ok: false,
                provider_id,
                source,
                endpoint: Some(url.to_string()),
                status: Some(status),
                quota: None,
                gateway: None,
                plan_name: None,
                message: None,
                error: Some(redact_many(&err.to_string(), redactions)),
            }));
        }
    };

    if !(200..300).contains(&status) {
        return Err(Box::new(UsageProbeResult {
            ok: false,
            provider_id,
            source,
            endpoint: Some(url.to_string()),
            status: Some(status),
            quota: None,
            gateway: None,
            plan_name: None,
            message: None,
            error: Some(format!(
                "API error (HTTP {status}): {}",
                redact_many(&preview(&text), redactions)
            )),
        }));
    }

    let json = serde_json::from_str::<Value>(&text).map_err(|err| {
        Box::new(UsageProbeResult {
            ok: false,
            provider_id,
            source,
            endpoint: Some(url.to_string()),
            status: Some(status),
            quota: None,
            gateway: None,
            plan_name: None,
            message: None,
            error: Some(format!("Failed to parse response: {err}")),
        })
    })?;

    Ok((status, json))
}

pub(super) fn redactions(api_key: &str, access_token: Option<&SensitiveString>) -> Vec<String> {
    let mut values = Vec::new();
    if !api_key.trim().is_empty() {
        values.push(api_key.to_string());
    }
    if let Some(access_token) = access_token {
        let value = access_token.expose();
        if !value.trim().is_empty() {
            values.push(value.to_string());
        }
    }
    values
}

fn redact_many(value: &str, redactions: &[String]) -> String {
    redactions.iter().fold(value.to_string(), |acc, secret| {
        if secret.is_empty() {
            acc
        } else {
            acc.replace(secret, "[redacted]")
        }
    })
}

fn preview(value: &str) -> String {
    const LIMIT: usize = 300;
    if value.len() <= LIMIT {
        return value.to_string();
    }
    let mut end = LIMIT;
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!("{}...", &value[..end])
}
