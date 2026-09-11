use super::*;

pub(crate) fn upstream_client(
    state: &RuntimeState,
    connect_timeout_ms: u64,
) -> Result<reqwest::Client, String> {
    upstream_client_for_transport(state, connect_timeout_ms, false)
}

pub(crate) fn upstream_client_for_transport(
    state: &RuntimeState,
    connect_timeout_ms: u64,
    http1_only: bool,
) -> Result<reqwest::Client, String> {
    let connect_timeout_ms = connect_timeout_ms.max(1);
    let upstream_proxy = state
        .config
        .read()
        .map_err(|_| "proxy config lock poisoned".to_string())?
        .upstream_proxy
        .clone();
    let cache_key = (connect_timeout_ms, upstream_proxy.clone(), http1_only);
    let mut clients = state
        .clients
        .lock()
        .map_err(|_| "proxy HTTP client cache lock poisoned".to_string())?;
    if let Some(client) = clients.get(&cache_key) {
        return Ok(client.clone());
    }
    let builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_millis(connect_timeout_ms))
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never());
    let builder = if http1_only {
        builder.http1_only()
    } else {
        builder
    };
    let client = apply_upstream_proxy(builder, &upstream_proxy)?
        .build()
        .map_err(|err| err.to_string())?;
    clients.insert(cache_key, client.clone());
    Ok(client)
}

/// Resolve outbound proxy selection once for async forwarding and blocking probes.
pub fn upstream_proxy_rules(
    config: &UpstreamProxyConfig,
) -> Result<Option<Vec<reqwest::Proxy>>, String> {
    match config.mode {
        UpstreamProxyMode::System => Ok(None),
        UpstreamProxyMode::Direct => Ok(Some(Vec::new())),
        UpstreamProxyMode::Custom => {
            let url = config
                .custom_url
                .as_deref()
                .map(str::trim)
                .filter(|url| !url.is_empty())
                .ok_or_else(|| {
                    "upstream proxy mode is custom but no proxy URL is configured".to_string()
                })?;
            let proxy = reqwest::Proxy::all(url)
                .map_err(|err| format!("invalid upstream proxy URL: {err}"))?;
            Ok(Some(vec![proxy]))
        }
        UpstreamProxyMode::Environment => {
            let vars = shell_env::proxy_env();
            let no_proxy = shell_env::lookup(&vars, &["NO_PROXY", "no_proxy"])
                .map(reqwest::NoProxy::from_string);
            let mut proxies = Vec::new();
            type ProxyCtor = fn(&str) -> reqwest::Result<reqwest::Proxy>;
            let constructors: [(&[&str], ProxyCtor); 3] = [
                (&["HTTPS_PROXY", "https_proxy"][..], |url| {
                    reqwest::Proxy::https(url)
                }),
                (&["HTTP_PROXY", "http_proxy"][..], |url| {
                    reqwest::Proxy::http(url)
                }),
                (&["ALL_PROXY", "all_proxy"][..], |url| {
                    reqwest::Proxy::all(url)
                }),
            ];
            for (keys, ctor) in constructors {
                let Some(url) = shell_env::lookup(&vars, keys) else {
                    continue;
                };
                if let Ok(proxy) = ctor(url) {
                    proxies.push(match no_proxy.clone() {
                        Some(no_proxy) => proxy.no_proxy(no_proxy),
                        None => proxy,
                    });
                }
            }
            Ok(Some(proxies))
        }
    }
}

pub(crate) type BoxError = Box<dyn StdError + Send + Sync>;
pub(crate) type BoxBody = http_body_util::combinators::UnsyncBoxBody<Bytes, BoxError>;
pub(crate) type UpstreamBodyStream =
    Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static>>;

pub(crate) const MAX_REQUEST_BODY_BYTES: usize = 512 * 1024 * 1024;
pub(crate) const REQUEST_BODY_MEMORY_THRESHOLD: usize = 8 * 1024 * 1024;
pub(crate) const MAX_BUFFERED_RESPONSE_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_PROXY_LOG_ENTRIES: usize = 1_000;
/// Session affinity is intentionally ephemeral. Provider prompt caches are
/// useful while a client session is active, but retaining arbitrary client
/// supplied keys indefinitely would make the proxy's memory usage unbounded.
pub(crate) const SESSION_AFFINITY_TTL: Duration = Duration::from_secs(30 * 60);
pub(crate) const MAX_SESSION_AFFINITY_ENTRIES: usize = 4_096;
pub(crate) const MAX_SESSION_AFFINITY_KEY_BYTES: usize = 512;

pub(crate) const SESSION_AFFINITY_HEADERS: [&str; 11] = [
    "x-aipass-session-id",
    "x-aipass-session",
    "x-session-id",
    "x-client-session-id",
    "x-codex-session-id",
    "x-openai-session-id",
    "x-anthropic-session-id",
    "x-claude-session-id",
    "session-id",
    "session_id",
    "prompt-cache-key",
];

pub(crate) const SESSION_AFFINITY_FIELDS: [&str; 10] = [
    "prompt_cache_key",
    "promptCacheKey",
    "session_id",
    "sessionId",
    "session",
    "sessionKey",
    "conversation_id",
    "conversationId",
    "previous_response_id",
    "previousResponseId",
];

pub(crate) fn apply_upstream_proxy(
    mut builder: reqwest::ClientBuilder,
    config: &UpstreamProxyConfig,
) -> Result<reqwest::ClientBuilder, String> {
    if let Some(proxies) = upstream_proxy_rules(config)? {
        builder = builder.no_proxy();
        for proxy in proxies {
            builder = builder.proxy(proxy);
        }
    }
    Ok(builder)
}

#[cfg(test)]
pub(crate) fn upstream_url(base_url: &str, path: &str) -> Result<String, ProxyError> {
    upstream_url_with_query(base_url, path, None)
}

pub fn upstream_url_with_query(
    base_url: &str,
    path: &str,
    query: Option<&str>,
) -> Result<String, ProxyError> {
    let base =
        reqwest::Url::parse(base_url).map_err(|err| ProxyError::InvalidConfig(err.to_string()))?;
    let base_path = base.path().trim_end_matches('/').to_string();
    // Respect an explicit /v1 path segment in the user's API base. Other
    // versions and names such as /openai do not imply /v1 is already present.
    // The official Codex OAuth backend has its own unversioned resource path.
    let strip_version_prefix = base_path.split('/').any(|segment| segment == "v1")
        || base_path.ends_with("/backend-api/codex");
    let suffix = if strip_version_prefix && (path == "/v1" || path.starts_with("/v1/")) {
        &path[3..]
    } else {
        path
    };
    let mut url = base;
    url.set_path(&format!("{}{}", base_path, suffix));
    if let Some(query) = query.filter(|query| !query.is_empty()) {
        let merged = match url.query().filter(|existing| !existing.is_empty()) {
            Some(existing) => format!("{existing}&{query}"),
            None => query.to_string(),
        };
        url.set_query(Some(&merged));
    }
    Ok(url.to_string())
}

pub(crate) fn build_upstream_headers(
    incoming: &HeaderMap,
    target: &ResolvedTarget,
    protocol: ProxyProtocol,
) -> Result<HeaderMap, String> {
    let incoming_hop_headers = connection_header_names(incoming);
    let anthropic_upstream = protocol == ProxyProtocol::AnthropicMessages;
    let mut headers = HeaderMap::new();
    for (name, value) in incoming.iter() {
        // Local metadata and product identity never belong on provider traffic.
        if is_local_proxy_header(name, value) {
            continue;
        }
        // Anthropic-specific headers are meaningless (and leaking them is
        // confusing) to an OpenAI-wire upstream after conversion.
        if !anthropic_upstream && (name == "anthropic-version" || name == ANTHROPIC_BETA_HEADER) {
            continue;
        }
        if !is_hop_header(name)
            && !incoming_hop_headers.contains(name)
            && name != header::AUTHORIZATION
            && name != "x-api-key"
            && name != "api-key"
            && name != header::ACCEPT_ENCODING
            && name != header::CONTENT_LENGTH
            && name != header::HOST
        {
            headers.append(name.clone(), value.clone());
        }
    }

    let mut configured = HeaderMap::new();
    for (name, value) in &target.config.headers {
        let name = header::HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| format!("invalid configured upstream header name: {name}"))?;
        let mut value = HeaderValue::from_str(value)
            .map_err(|_| format!("invalid value for configured upstream header {name}"))?;
        value.set_sensitive(true);
        configured.append(name, value);
    }
    let configured_hop_headers = connection_header_names(&configured);
    for (name, value) in configured.iter() {
        if !is_local_proxy_header(name, value)
            && !(is_client_identity_header(name) && headers.contains_key(name))
            && !is_hop_header(name)
            && !configured_hop_headers.contains(name)
            && name != header::ACCEPT_ENCODING
            && name != header::CONTENT_LENGTH
            && name != header::CONTENT_TYPE
            && name != header::HOST
        {
            if name == ANTHROPIC_BETA_HEADER && headers.contains_key(name) {
                let merged = merge_anthropic_beta_values(headers.get_all(name).iter(), value)?;
                headers.insert(name.clone(), merged);
            } else {
                headers.insert(name.clone(), value.clone());
            }
        }
    }

    let (auth_name, mut auth_value) = match target.config.auth_scheme.as_str() {
        "bearer" => {
            let mut bearer = format!("Bearer {}", target.api_key);
            let value = HeaderValue::from_str(&bearer)
                .map_err(|_| "invalid bearer credential for upstream request".to_string());
            bearer.zeroize();
            (header::AUTHORIZATION, value?)
        }
        "custom_header" => (
            header::AUTHORIZATION,
            HeaderValue::from_str(&target.api_key)
                .map_err(|_| "invalid custom authorization credential".to_string())?,
        ),
        "x_api_key" => (
            header::HeaderName::from_static("x-api-key"),
            HeaderValue::from_str(&target.api_key)
                .map_err(|_| "invalid x-api-key credential".to_string())?,
        ),
        "azure_api_key" => (
            header::HeaderName::from_static("api-key"),
            HeaderValue::from_str(&target.api_key)
                .map_err(|_| "invalid Azure API credential".to_string())?,
        ),
        scheme => return Err(format!("unsupported proxy authentication scheme: {scheme}")),
    };
    auth_value.set_sensitive(true);
    headers.insert(auth_name, auth_value);
    if !headers.contains_key(header::CONTENT_TYPE) {
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
    }
    if protocol == ProxyProtocol::AnthropicMessages && !headers.contains_key("anthropic-version") {
        headers.insert(
            header::HeaderName::from_static("anthropic-version"),
            HeaderValue::from_static("2023-06-01"),
        );
    }
    Ok(headers)
}

pub(crate) fn is_client_identity_header(name: &header::HeaderName) -> bool {
    matches!(
        name.as_str(),
        "user-agent"
            | "x-user-agent"
            | "originator"
            | "http-referer"
            | "x-title"
            | "x-app-name"
            | "x-client-name"
    )
}

pub(crate) fn is_local_proxy_header(name: &header::HeaderName, value: &HeaderValue) -> bool {
    name.as_str().starts_with("x-aipass-")
        || name.as_str().starts_with("aipass-")
        || (is_client_identity_header(name)
            && value
                .as_bytes()
                .windows(6)
                .any(|part| part.eq_ignore_ascii_case(b"aipass")))
}

pub(crate) const ANTHROPIC_BETA_HEADER: &str = "anthropic-beta";

/// `anthropic-beta` is a comma-separated feature-flag list. Clients such as
/// Claude Code send their own flags while imported OAuth entries configure
/// `oauth-2025-04-20`; merging (incoming first, then configured additions,
/// deduped) keeps both instead of letting the configured value replace the
/// client's list.
pub(crate) fn merge_anthropic_beta_values<'a>(
    incoming: impl Iterator<Item = &'a HeaderValue>,
    configured: &'a HeaderValue,
) -> Result<HeaderValue, String> {
    let mut tokens: Vec<String> = Vec::new();
    for value in incoming.chain(std::iter::once(configured)) {
        let text = value
            .to_str()
            .map_err(|_| "invalid anthropic-beta header value".to_string())?;
        for token in text
            .split(',')
            .map(str::trim)
            .filter(|token| !token.is_empty())
        {
            if !tokens.iter().any(|existing| existing == token) {
                tokens.push(token.to_string());
            }
        }
    }
    let mut merged = HeaderValue::from_str(&tokens.join(", "))
        .map_err(|_| "invalid merged anthropic-beta header value".to_string())?;
    merged.set_sensitive(true);
    Ok(merged)
}

pub(crate) fn connection_header_names(headers: &HeaderMap) -> HashSet<header::HeaderName> {
    headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|name| header::HeaderName::from_bytes(name.trim().as_bytes()).ok())
        .collect()
}

pub(crate) fn is_hop_header(name: &header::HeaderName) -> bool {
    matches!(
        name.as_str(),
        "connection"
            | "keep-alive"
            | "proxy-connection"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}
