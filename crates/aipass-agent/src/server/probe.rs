//! Provider HTTP and WebSocket capability probes.
use super::*;

pub(super) fn probe_entry(
    mut entry: EntrySummary,
    secret: String,
    timeout_seconds: u64,
    headers: Vec<(String, String)>,
    outbound: aipass_proxy::UpstreamProxyConfig,
    state: Option<&Arc<AgentState>>,
) -> ProbeResult {
    if let Some(interface) = entry
        .secret_refs
        .first()
        .and_then(|secret| secret.interface_type.as_ref())
    {
        entry.interface_type = interface.clone();
    }
    let secret = zeroize::Zeroizing::new(secret);
    let mut headers = zeroize::Zeroizing::new(headers);
    let started = Instant::now();
    let budget = Duration::from_secs(timeout_seconds.clamp(1, 120));
    let ws_interface = matches!(
        entry.interface_type,
        InterfaceType::OpenAiCompatible | InterfaceType::AzureOpenAi
    );
    let endpoint = endpoint_url(&entry.endpoints);
    let Some(endpoint) = endpoint.clone() else {
        return ProbeResult {
            ok: false,
            provider_id: entry.provider_id,
            interface_type: entry.interface_type,
            status: None,
            endpoint: None,
            model_count: None,
            websocket: None,
            error: Some("provider has no API endpoint".to_string()),
        };
    };

    let endpoint = crate::proxy_service::pinned_official_oauth_endpoint(
        &entry.provider_kind,
        &entry.credential_kind,
        entry.provider_id.as_deref(),
    )
    .map(str::to_owned)
    .unwrap_or(endpoint);
    let target = ws_interface.then(|| aipass_proxy::ResolvedTarget {
        upstream_proxy: None,
        quota: Vec::new(),
        model_override: None,
        profile: aipass_proxy::ProviderProfile::Generic,
        upstream_kind: crate::proxy_service::upstream_kind(&entry),
        max_concurrent_requests: None,
        supports_websockets: true,
        api_key: secret.to_string(),
        config: aipass_proxy::ProxyTargetConfig {
            id: entry.id,
            provider_entry_id: entry.id,
            secret_id: entry
                .secret_refs
                .first()
                .map(|secret| secret.id.clone())
                .unwrap_or_default(),
            label: String::new(),
            base_url: endpoint.clone(),
            auth_scheme: crate::proxy_service::proxy_auth_scheme(&entry.auth_scheme)
                .unwrap_or("bearer")
                .into(),
            headers: std::mem::take(&mut *headers),
            group: None,
            priority: 0,
            weight: 1,
            enabled: true,
            protocol: Some(ProxyProtocol::OpenAiResponses),
            prefer_ws: false,
            model: None,
        },
    });
    let observation = state.and_then(|state| {
        let target = target.as_ref()?;
        state
            .proxy
            .lock()
            .ok()?
            .begin_ws_probe(aipass_proxy::websocket_config_key(target, &outbound))
    });
    let default_model = entry.default_model.clone();
    let client = match aipass_proxy::upstream_proxy_rules(&outbound).and_then(|rules| {
        let mut builder = reqwest::blocking::Client::builder()
            .timeout(if ws_interface { budget / 2 } else { budget })
            .redirect(reqwest::redirect::Policy::none());
        if let Some(proxies) = rules {
            builder = builder.no_proxy();
            for proxy in proxies {
                builder = builder.proxy(proxy);
            }
        }
        builder.build().map_err(|err| err.to_string())
    }) {
        Ok(client) => client,
        Err(err) => {
            return ProbeResult {
                ok: false,
                provider_id: entry.provider_id,
                interface_type: entry.interface_type,
                status: None,
                endpoint: Some(endpoint),
                model_count: None,
                websocket: None,
                error: Some(err.to_string()),
            };
        }
    };

    let (display_url, request) = match entry.interface_type {
        InterfaceType::OpenAiCompatible | InterfaceType::AzureOpenAi => {
            let path = if entry.auth_scheme == AuthScheme::AzureApiKey {
                "/models"
            } else {
                "/v1/models"
            };
            let url = aipass_proxy::upstream_url_with_query(&endpoint, path, None)
                .unwrap_or_else(|_| join_url(&endpoint, "models"));
            let request = apply_auth(client.get(&url), &entry.auth_scheme, &secret);
            (url, request)
        }
        InterfaceType::AnthropicMessages => {
            let url = aipass_proxy::upstream_url_with_query(&endpoint, "/v1/models", None)
                .unwrap_or_else(|_| join_url(&endpoint, "v1/models"));
            let request = apply_auth(client.get(&url), &entry.auth_scheme, &secret)
                .header("anthropic-version", "2023-06-01");
            (url, request)
        }
        InterfaceType::Gemini => {
            let url = join_url(&endpoint, "v1beta/models");
            let display_url = append_query_param(&url, "key", "[redacted]");
            let request_url = append_query_param(&url, "key", &secret);
            let request = client.get(&request_url);
            (display_url, request)
        }
        InterfaceType::Bedrock | InterfaceType::CustomHttp => {
            return ProbeResult {
                ok: false,
                provider_id: entry.provider_id,
                interface_type: entry.interface_type,
                status: None,
                endpoint: Some(endpoint),
                model_count: None,
                websocket: None,
                error: Some("probe is not supported for this interface".to_string()),
            };
        }
    };

    let mut request = request;
    for (name, value) in target
        .as_ref()
        .map(|target| target.config.headers.as_slice())
        .unwrap_or(&headers)
    {
        request = request.header(name, value);
    }
    let mut probe_model = default_model;
    let mut result = match request.send() {
        Ok(response) => {
            let status = response.status().as_u16();
            let json = response
                .text()
                .ok()
                .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok());
            if probe_model.is_none() {
                probe_model = json
                    .as_ref()
                    .and_then(|json| json.pointer("/data/0/id"))
                    .and_then(|id| id.as_str())
                    .map(str::to_owned);
            }
            ProbeResult {
                ok: (200..300).contains(&status),
                provider_id: entry.provider_id,
                interface_type: entry.interface_type,
                status: Some(status),
                endpoint: Some(display_url),
                model_count: json.as_ref().and_then(model_count),
                websocket: None,
                error: None,
            }
        }
        Err(err) => ProbeResult {
            ok: false,
            provider_id: entry.provider_id,
            interface_type: entry.interface_type,
            status: None,
            endpoint: Some(display_url),
            model_count: None,
            websocket: None,
            error: Some(redact_error(&err.to_string(), &secret)),
        },
    };
    if let Some(target) = target {
        result.websocket = Some(aipass_proxy::probe_websocket(
            target,
            &outbound,
            budget.saturating_sub(started.elapsed()),
            probe_model.as_deref(),
        ));
    }
    if result.websocket.as_ref().and_then(|ws| ws.supported) == Some(true) {
        if let Some(state) = state {
            if let Ok(proxy) = state.proxy.lock() {
                proxy.confirm_ws_probe(observation);
            }
        }
    }
    result
}

pub(super) fn apply_auth(
    request: RequestBuilder,
    auth_scheme: &AuthScheme,
    secret: &str,
) -> RequestBuilder {
    match auth_scheme {
        AuthScheme::Bearer => request.bearer_auth(secret),
        AuthScheme::XApiKey => request.header("x-api-key", secret),
        AuthScheme::AzureApiKey => request.header("api-key", secret),
        AuthScheme::CustomHeader => request.header("authorization", secret),
        AuthScheme::GoogleApiKey | AuthScheme::AwsProfile => request,
    }
}

pub(super) fn endpoint_url(endpoints: &[ProviderEndpoint]) -> Option<String> {
    protocol_endpoint_url(endpoints)
}

pub(super) fn join_url(base: &str, suffix: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        suffix.trim_start_matches('/')
    )
}

pub(super) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(super) fn append_query_param(url: &str, key: &str, value: &str) -> String {
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}{key}={value}")
}

pub(super) fn model_count(value: &serde_json::Value) -> Option<usize> {
    value
        .get("data")
        .or_else(|| value.get("models"))
        .and_then(|value| value.as_array())
        .map(Vec::len)
}

pub(super) fn redact_error(value: &str, secret: &str) -> String {
    if secret.is_empty() {
        value.to_string()
    } else {
        value.replace(secret, "[redacted]")
    }
}
