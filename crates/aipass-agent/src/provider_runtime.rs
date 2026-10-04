//! Encrypted provider preferences, bounded balance reads and opt-in notifications.
use crate::session::{map_vault_error, with_vault, AgentState, ServiceError, ServiceResult};
use aipass_agent_protocol::{
    ProviderRuntimeOptions, ProviderWebhook, ProviderWebhookEvent as Event,
};
use aipass_crypto::SecretString;
use aipass_proxy::{UpstreamProxyConfig, UpstreamProxyMode};
use aipass_vault::{EntrySummary, Vault};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};
use uuid::Uuid;
const KEY: &str = "provider_runtime_v1";
pub(crate) fn load(vault: &Vault, id: Uuid) -> ServiceResult<ProviderRuntimeOptions> {
    vault
        .provider_runtime_extension(id, KEY)
        .map_err(map_vault_error)?
        .map(|raw| serde_json::from_str(raw.expose()).map_err(ServiceError::internal))
        .transpose()
        .map(|v| v.unwrap_or_default())
}
pub(crate) fn view(mut options: ProviderRuntimeOptions) -> ProviderRuntimeOptions {
    if let Some(proxy) = options.proxy.as_mut() {
        proxy.has_credentials = proxy.username.is_some() || proxy.password.is_some();
        proxy.username = None;
        proxy.password = None;
    }
    if let Some(balance) = options.balance.as_mut() {
        balance.body = None;
        for (_, value) in &mut balance.headers {
            *value = None;
        }
    }
    for hook in &mut options.webhooks {
        hook.has_secret = hook.secret.is_some();
        hook.secret = None;
    }
    options
}
fn validate_url(raw: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw).map_err(|_| "invalid HTTP endpoint")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("endpoint must be HTTP(S) without credentials or a fragment".into());
    }
    Ok(url)
}
pub(crate) fn outbound(
    options: &ProviderRuntimeOptions,
) -> Result<Option<UpstreamProxyConfig>, String> {
    options
        .proxy
        .as_ref()
        .map(|proxy| {
            let custom_url = if proxy.mode == UpstreamProxyMode::Custom {
                let mut url = reqwest::Url::parse(proxy.url.as_deref().unwrap_or(""))
                    .map_err(|_| "invalid provider proxy URL")?;
                if !matches!(url.scheme(), "http" | "https" | "socks5" | "socks5h")
                    || url.host_str().is_none()
                    || !url.username().is_empty()
                    || url.password().is_some()
                {
                    return Err(
                        "proxy URL must use HTTP(S)/SOCKS5; enter authentication separately".into(),
                    );
                }
                if let Some(username) = proxy.username.as_ref() {
                    url.set_username(username.expose())
                        .map_err(|_| "invalid proxy username")?;
                }
                if let Some(password) = proxy.password.as_ref() {
                    url.set_password(Some(password.expose()))
                        .map_err(|_| "invalid proxy password")?;
                }
                Some(url.to_string())
            } else {
                None
            };
            Ok(UpstreamProxyConfig {
                mode: proxy.mode,
                custom_url,
            })
        })
        .transpose()
}
pub(crate) fn save(
    vault: &Vault,
    id: Uuid,
    mut options: ProviderRuntimeOptions,
) -> ServiceResult<ProviderRuntimeOptions> {
    let old = load(vault, id)?;
    let validation = (|| -> Result<(), String> {
        if !(60..=3600).contains(&options.quota_refresh_seconds) {
            return Err("quota refresh interval must be 60–3600 seconds".into());
        }
        if let Some(proxy) = options.proxy.as_mut() {
            if !proxy.clear_credentials {
                if let Some(old) = old
                    .proxy
                    .as_ref()
                    .filter(|old| old.url == proxy.url && old.mode == proxy.mode)
                {
                    if proxy.username.is_none() {
                        proxy.username = old.username.clone();
                    }
                    if proxy.password.is_none() {
                        proxy.password = old.password.clone();
                    }
                }
            } else {
                proxy.username = None;
                proxy.password = None;
            }
            proxy.clear_credentials = false;
        }
        outbound(&options)?;
        if let Some(balance) = options.balance.as_mut() {
            validate_url(&balance.url)?;
            if balance.unit.len() > 32
                || balance.json_path.len() > 512
                || balance.headers.len() > 32
            {
                return Err("balance configuration exceeds limits".into());
            }
            if let Some(old) = old.balance.as_ref().filter(|old| old.url == balance.url) {
                if balance.body.is_none() {
                    balance.body = old.body.clone();
                }
                for (name, value) in &mut balance.headers {
                    if value.is_none() {
                        *value = old
                            .headers
                            .iter()
                            .find(|(n, _)| n.eq_ignore_ascii_case(name))
                            .and_then(|(_, v)| v.clone());
                    }
                }
            }
            for (name, value) in &balance.headers {
                reqwest::header::HeaderName::from_bytes(name.as_bytes())
                    .map_err(|_| "invalid balance header name")?;
                if value.as_ref().is_some_and(|v| v.expose().len() > 16 * 1024) {
                    return Err("balance header exceeds limit".into());
                }
            }
            if balance
                .body
                .as_ref()
                .is_some_and(|b| b.expose().len() > 64 * 1024)
            {
                return Err("balance request exceeds limit".into());
            }
        }
        if options.webhooks.len() > 8 {
            return Err("at most eight webhooks per provider".into());
        }
        let mut ids = HashSet::new();
        for hook in &mut options.webhooks {
            validate_url(&hook.url)?;
            if !ids.insert(hook.id) {
                return Err("duplicate webhook ID".into());
            }
            if hook.clear_secret {
                hook.secret = None;
            } else if hook.secret.is_none() {
                hook.secret = old
                    .webhooks
                    .iter()
                    .find(|h| h.id == hook.id && h.url == hook.url)
                    .and_then(|h| h.secret.clone());
            }
            hook.clear_secret = false;
        }
        Ok(())
    })();
    validation.map_err(|e| {
        ServiceError::new(aipass_agent_protocol::AgentErrorCode::ValidationFailed, e)
    })?;
    let encoded =
        SecretString::new(serde_json::to_string(&options).map_err(ServiceError::internal)?);
    vault
        .set_provider_runtime_extension(id, KEY, Some(&encoded))
        .map_err(map_vault_error)?;
    Ok(view(options))
}
fn client(
    options: &ProviderRuntimeOptions,
    default_proxy: UpstreamProxyConfig,
) -> Result<reqwest::blocking::Client, String> {
    let mut builder = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none());
    {
        let proxy = outbound(options)?.unwrap_or(default_proxy);
        if let Some(proxies) = aipass_proxy::upstream_proxy_rules(&proxy)? {
            builder = builder.no_proxy();
            for proxy in proxies {
                builder = builder.proxy(proxy);
            }
        }
    }
    builder
        .build()
        .map_err(|_| "could not create provider HTTP client".into())
}
fn balance_value(value: &Value, path: &str) -> Option<f64> {
    let selected = if path.starts_with('/') {
        value.pointer(path)
    } else {
        let mut cursor = value;
        for part in path
            .trim_start_matches("$.")
            .split('.')
            .filter(|s| !s.is_empty())
        {
            cursor = if let Ok(index) = part.parse::<usize>() {
                cursor.get(index)?
            } else {
                cursor.get(part)?
            };
        }
        Some(cursor)
    }?;
    selected
        .as_f64()
        .or_else(|| selected.as_str()?.parse().ok())
        .filter(|v| v.is_finite())
}
pub(crate) fn probe(state: &Arc<AgentState>, id: Uuid) -> ServiceResult<Value> {
    let (options, credential, fingerprint, default_proxy) = with_vault(state, false, |vault| {
        let entry = vault.get_provider_summary(id).map_err(map_vault_error)?;
        let secret =
            aipass_provider_registry::primary_secret_ref(&entry.secret_refs).ok_or_else(|| {
                ServiceError::internal(anyhow::anyhow!("provider credential missing"))
            })?;
        let credentials = vault
            .runtime_provider_credentials(id, &secret.id)
            .map_err(map_vault_error)?;
        let fingerprint = vault.fingerprint_secret(credentials.secret.expose());
        let default_proxy = state
            .proxy
            .lock()
            .map_err(|_| ServiceError::internal(anyhow::anyhow!("proxy lock poisoned")))?
            .config(vault)?
            .upstream_proxy;
        Ok((
            load(vault, id)?,
            credentials.secret,
            fingerprint,
            default_proxy,
        ))
    })?;
    let result = (|| -> Result<f64, String> {
        let config = options
            .balance
            .as_ref()
            .ok_or("custom balance endpoint is not configured")?;
        validate_url(&config.url)?;
        let client = client(&options, default_proxy)?;
        let mut request = if config.post {
            client.post(&config.url)
        } else {
            client.get(&config.url)
        };
        for (name, value) in &config.headers {
            if let Some(value) = value {
                request = request.header(
                    name,
                    value.expose().replace("${api_key}", credential.expose()),
                );
            }
        }
        if config.post {
            if let Some(body) = config.body.as_ref() {
                request = request
                    .header("content-type", "application/json")
                    .body(body.expose().to_owned());
            }
        }
        let response = request
            .send()
            .map_err(|_| "balance endpoint request failed")?;
        if !response.status().is_success() {
            return Err(format!(
                "balance endpoint returned HTTP {}",
                response.status()
            ));
        }
        use std::io::Read;
        let mut bytes = zeroize::Zeroizing::new(Vec::new());
        response
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "balance response failed")?;
        if bytes.len() > 1024 * 1024 {
            return Err("balance response exceeds limit".into());
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| "balance response is not JSON")?;
        balance_value(&value, &config.json_path)
            .ok_or_else(|| "balance path is missing or not a finite number".into())
    })()
    .map_err(|e| ServiceError::new(aipass_agent_protocol::AgentErrorCode::ValidationFailed, e))?;
    with_vault(state, false, |vault| {
        let entry = vault.get_provider_summary(id).map_err(map_vault_error)?;
        let secret =
            aipass_provider_registry::primary_secret_ref(&entry.secret_refs).ok_or_else(|| {
                ServiceError::internal(anyhow::anyhow!("provider credential missing"))
            })?;
        let current = vault
            .runtime_provider_credentials(id, &secret.id)
            .map_err(map_vault_error)?;
        if fingerprint != vault.fingerprint_secret(current.secret.expose())
            || serde_json::to_value(load(vault, id)?).ok() != serde_json::to_value(&options).ok()
        {
            return Err(ServiceError::new(
                aipass_agent_protocol::AgentErrorCode::Conflict,
                "provider changed during balance request",
            ));
        }
        let unit = options.balance.as_ref().unwrap().unit.clone();
        vault
            .update_provider_usage(
                id,
                Some(aipass_provider_registry::QuotaInfo {
                    unit: Some(unit.clone()),
                    label: Some("Balance".into()),
                    remaining: Some(result.to_string()),
                    limit: None,
                    used: None,
                    reset_at: None,
                }),
                None,
                Some("custom"),
            )
            .map_err(map_vault_error)?;
        Ok(json!({"balance":result,"unit":unit}))
    })
}
fn send_hook(hook: &ProviderWebhook, payload: &Value) -> Result<(), String> {
    validate_url(&hook.url)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "webhook client unavailable")?;
    let mut request = client.post(&hook.url).json(payload);
    if let Some(secret) = hook.secret.as_ref() {
        request = request.bearer_auth(secret.expose());
    }
    let response = request.send().map_err(|_| "webhook delivery failed")?;
    if !response.status().is_success() {
        return Err(format!("webhook returned HTTP {}", response.status()));
    }
    Ok(())
}
pub(crate) fn test_webhook(state: &Arc<AgentState>, id: Uuid, hook_id: Uuid) -> ServiceResult<()> {
    let hook = with_vault(state, true, |vault| {
        load(vault, id)?
            .webhooks
            .into_iter()
            .find(|h| h.id == hook_id)
            .ok_or_else(|| {
                ServiceError::new(
                    aipass_agent_protocol::AgentErrorCode::NotFound,
                    "webhook missing",
                )
            })
    })?;
    send_hook(
        &hook,
        &json!({"event":"test","providerId":id,"source":"aipass"}),
    )
    .map_err(|e| ServiceError::new(aipass_agent_protocol::AgentErrorCode::ServiceUnavailable, e))
}
#[derive(Default)]
pub(crate) struct Background {
    last_probe: HashMap<Uuid, Instant>,
    delivered: HashSet<(Uuid, Event)>,
}
fn events(entry: &EntrySummary, channels: &[aipass_proxy::ChannelStatus]) -> HashSet<Event> {
    let mut events = HashSet::new();
    let now = time::OffsetDateTime::now_utc();
    if let Some(s) = entry
        .subscription
        .as_ref()
        .filter(|s| !s.stale && s.error.is_none())
    {
        let fresh = time::OffsetDateTime::parse(
            &s.observed_at,
            &time::format_description::well_known::Rfc3339,
        )
        .ok()
        .is_some_and(|at| at <= now && (now - at).whole_seconds() <= 300);
        if fresh {
            for window in &s.windows {
                if let Some(used) = window.used_percent {
                    if used >= 100.0 {
                        events.insert(Event::QuotaExhausted);
                    } else if used >= 80.0 {
                        events.insert(Event::QuotaLow);
                    }
                }
            }
        }
        if let Some(expiry) = s.subscription_expires_at.as_deref().and_then(|s| {
            time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
        }) {
            if expiry <= now {
                events.insert(Event::SubscriptionExpired);
            } else if (expiry - now).whole_hours() <= 72 {
                events.insert(Event::SubscriptionExpiring);
            }
        }
    }
    for channel in channels.iter().filter(|c| c.provider_entry_id == entry.id) {
        if channel.degraded {
            events.insert(Event::ProviderDegraded);
        }
        match channel.last_status {
            Some(429) => {
                events.insert(Event::RateLimitDetected);
            }
            Some(401 | 403) => {
                events.insert(Event::CredentialFailed);
            }
            Some(500..=599) => {
                events.insert(Event::ProviderError);
            }
            _ => {}
        }
    }
    events
}
pub(crate) fn refresh(state: &Arc<AgentState>, cache: &mut Background) -> ServiceResult<()> {
    let entries = with_vault(state, false, |vault| {
        vault
            .list_provider_summaries()
            .map_err(map_vault_error)?
            .into_iter()
            .map(|e| load(vault, e.id).map(|o| (e, o)))
            .collect::<ServiceResult<Vec<_>>>()
    })?;
    let channels = state
        .proxy
        .lock()
        .map_err(|_| ServiceError::internal(anyhow::anyhow!("proxy unavailable")))?
        .status()
        .channels;
    let current: HashSet<_> = entries.iter().map(|(entry, _)| entry.id).collect();
    cache.last_probe.retain(|id, _| current.contains(id));
    let mut active = HashSet::new();
    for (entry, options) in entries {
        if options.quota_tracking
            && options.balance.is_some()
            && cache.last_probe.get(&entry.id).is_none_or(|at| {
                at.elapsed() >= Duration::from_secs(options.quota_refresh_seconds as u64)
            })
        {
            cache.last_probe.insert(entry.id, Instant::now());
            let _ = probe(state, entry.id);
        }
        let events = events(&entry, &channels);
        for hook in options.webhooks.iter().filter(|h| h.enabled) {
            for event in hook.events.iter().filter(|event| events.contains(event)) {
                let key = (hook.id, *event);
                active.insert(key);
                if !cache.delivered.contains(&key) {
                    // Recheck lock before sending any configured external notification.
                    if crate::session::session_status(state)?.locked {
                        return Ok(());
                    }
                    if send_hook(hook,&json!({"event":event,"providerId":entry.id,"source":"aipass","observedAt":time::OffsetDateTime::now_utc().unix_timestamp()})).is_ok(){cache.delivered.insert(key);}
                }
            }
        }
    }
    cache.delivered.retain(|key| active.contains(key));
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use aipass_agent_protocol::SensitiveString;
    #[test]
    fn balances_are_strict_and_secrets_are_omitted_from_views() {
        assert_eq!(
            balance_value(&json!({"data":{"credit":"3.25"}}), "$.data.credit"),
            Some(3.25)
        );
        assert_eq!(balance_value(&json!({"a":[7]}), "/a/0"), Some(7.0));
        assert_eq!(balance_value(&json!({"a":"NaN"}), "a"), None);
        let options = ProviderRuntimeOptions {
            webhooks: vec![ProviderWebhook {
                id: Uuid::nil(),
                url: "https://example.test".into(),
                enabled: true,
                events: vec![Event::QuotaLow],
                secret: Some(SensitiveString::new("never-return")),
                has_secret: false,
                clear_secret: false,
            }],
            ..Default::default()
        };
        let view = serde_json::to_string(&view(options)).unwrap();
        assert!(!view.contains("never-return"));
        assert!(view.contains("\"hasSecret\":true"));
    }
}
