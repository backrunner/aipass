//! Bounded public-address favicon discovery.
use super::*;

pub(super) fn backfill_provider_favicons(
    state: &Arc<AgentState>,
    request: FaviconBackfillRequest,
) -> ServiceResult<FaviconBackfillResponse> {
    let _backfill_guard = state.favicon_backfill.lock().map_err(|_| {
        ServiceError::new(AgentErrorCode::Internal, "favicon backfill lock poisoned")
    })?;
    let limit = request
        .limit
        .unwrap_or(FAVICON_BACKFILL_DEFAULT_LIMIT)
        .min(FAVICON_BACKFILL_MAX_LIMIT);
    let mut response = FaviconBackfillResponse::default();
    let entries = with_vault(state, false, |vault| {
        favicon_backfill_entries(vault, request.entry_ids, &mut response).map_err(map_vault_error)
    })?;
    let client = HttpClient::builder()
        .timeout(FAVICON_REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("AIPass/1.0")
        .build()
        .map_err(ServiceError::internal)?;

    for entry in entries {
        if favicon_backfill_entry_is_skippable(&entry) {
            response.skipped += 1;
            continue;
        }
        if response.checked >= limit {
            response.skipped += 1;
            continue;
        }
        response.checked += 1;
        let Some(favicon_url) = resolve_favicon_data_url(&client, &entry) else {
            response.skipped += 1;
            continue;
        };
        match with_vault(state, false, |vault| {
            vault
                .replace_provider_favicon_url(entry.id, favicon_url)
                .map_err(map_vault_error)
        }) {
            Ok(Some(updated)) => {
                response.updated += 1;
                response.entries.push(updated);
            }
            Ok(None) => response.skipped += 1,
            Err(err) if err.code == AgentErrorCode::Locked => return Err(err),
            Err(err) => response.errors.push(FaviconBackfillError {
                entry_id: Some(entry.id),
                message: err.message,
            }),
        }
    }

    Ok(response)
}

pub(super) fn favicon_backfill_entries(
    vault: &Vault,
    entry_ids: Option<Vec<Uuid>>,
    response: &mut FaviconBackfillResponse,
) -> Result<Vec<EntrySummary>, aipass_vault::VaultError> {
    match entry_ids {
        Some(entry_ids) => {
            let mut seen = HashSet::new();
            let mut entries = Vec::new();
            for entry_id in entry_ids {
                if !seen.insert(entry_id) {
                    response.skipped += 1;
                    continue;
                }
                match vault.get_provider_summary(entry_id) {
                    Ok(entry) => entries.push(entry),
                    Err(err) => response.errors.push(FaviconBackfillError {
                        entry_id: Some(entry_id),
                        message: err.to_string(),
                    }),
                }
            }
            Ok(entries)
        }
        None => vault.list_provider_summaries(),
    }
}

pub(super) fn favicon_backfill_entry_is_skippable(entry: &EntrySummary) -> bool {
    entry.archived_at.is_some()
        || entry.deleted_at.is_some()
        || entry
            .favicon_url
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| value.starts_with("data:image/"))
}

pub(super) fn resolve_favicon_data_url(
    client: &HttpClient,
    entry: &EntrySummary,
) -> Option<String> {
    favicon_url_candidates(entry)
        .into_iter()
        .find_map(|candidate| download_favicon_data_url(client, &candidate))
}

pub(super) fn favicon_url_candidates(entry: &EntrySummary) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    if let Some(favicon_url) = entry.favicon_url.as_deref() {
        push_direct_favicon_candidate(&mut candidates, &mut seen, favicon_url);
    }

    if let Some(provider_id) = entry.provider_id.as_deref() {
        for provider in default_provider_definitions()
            .into_iter()
            .filter(|provider| provider.id == provider_id)
        {
            for (_, kind, url) in provider.endpoints {
                if kind == &EndpointKind::Console {
                    push_favicon_candidate(&mut candidates, &mut seen, url);
                }
            }
        }
    }

    for endpoint in &entry.endpoints {
        if endpoint.kind == EndpointKind::Console {
            if let Some(url) = endpoint.url.as_deref() {
                push_favicon_candidate(&mut candidates, &mut seen, url);
            }
        }
    }

    for domain in &entry.domains {
        push_favicon_candidate(&mut candidates, &mut seen, domain);
    }

    for endpoint in &entry.endpoints {
        if endpoint.kind == EndpointKind::Api {
            if let Some(url) = endpoint.url.as_deref() {
                push_favicon_candidate(&mut candidates, &mut seen, url);
            }
        }
    }

    candidates
}

pub(super) fn push_direct_favicon_candidate(
    candidates: &mut Vec<String>,
    seen: &mut HashSet<String>,
    value: &str,
) {
    let Ok(mut url) = Url::parse(value.trim()) else {
        return;
    };
    if !matches!(url.scheme(), "https" | "http")
        || !url.username().is_empty()
        || url.password().is_some()
        || favicon_host_is_blocked(&url)
    {
        return;
    }
    url.set_fragment(None);
    let candidate = url.to_string();
    if seen.insert(candidate.clone()) {
        candidates.push(candidate);
    }
}

pub(super) fn push_favicon_candidate(
    candidates: &mut Vec<String>,
    seen: &mut HashSet<String>,
    value: &str,
) {
    if let Some(candidate) = favicon_url_from_origin_candidate(value) {
        if seen.insert(candidate.clone()) {
            candidates.push(candidate);
        }
    }
}

pub(super) fn favicon_url_from_origin_candidate(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let candidate = if value.starts_with("https://") || value.starts_with("http://") {
        value.to_string()
    } else {
        format!("https://{value}")
    };
    let mut url = Url::parse(&candidate).ok()?;
    if !matches!(url.scheme(), "https" | "http") || favicon_host_is_blocked(&url) {
        return None;
    }
    url.set_path("/favicon.ico");
    url.set_query(None);
    url.set_fragment(None);
    Some(url.to_string())
}

pub(super) fn favicon_host_is_blocked(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return true;
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    host.trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
        .map(ip_addr_is_blocked_for_favicon)
        .unwrap_or(false)
}

pub(super) fn ip_addr_is_blocked_for_favicon(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_multicast()
        }
        IpAddr::V6(ip) => {
            let first = ip.segments()[0];
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
        }
    }
}

pub(super) fn download_favicon_data_url(client: &HttpClient, candidate: &str) -> Option<String> {
    if !favicon_candidate_resolves_publicly(candidate) {
        return None;
    }
    let response = client
        .get(candidate)
        .header(
            ACCEPT,
            "image/avif,image/webp,image/apng,image/png,image/jpeg,image/gif,image/x-icon,image/vnd.microsoft.icon,image/*;q=0.8",
        )
        .send()
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    if response
        .content_length()
        .is_some_and(|length| length == 0 || length > MAX_FAVICON_BYTES as u64)
    {
        return None;
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .map(str::to_ascii_lowercase);
    if content_type.as_deref().is_some_and(|value| {
        !favicon_content_type_is_image(value) && value != "application/octet-stream"
    }) {
        return None;
    }
    let mut bytes = Vec::new();
    response
        .take((MAX_FAVICON_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.is_empty() || bytes.len() > MAX_FAVICON_BYTES {
        return None;
    }
    let mime = favicon_image_mime(&bytes)?;
    Some(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

pub(super) fn favicon_candidate_resolves_publicly(candidate: &str) -> bool {
    let Ok(url) = Url::parse(candidate) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    if let Ok(ip) = host.parse::<IpAddr>() {
        return !ip_addr_is_blocked_for_favicon(ip);
    }
    let Some(port) = url.port_or_known_default() else {
        return false;
    };
    let Ok(addresses) = (host, port).to_socket_addrs() else {
        return false;
    };
    favicon_resolved_addresses_are_public(addresses.map(|address| address.ip()))
}

pub(super) fn favicon_resolved_addresses_are_public(
    addresses: impl IntoIterator<Item = IpAddr>,
) -> bool {
    let mut found = false;
    for address in addresses {
        found = true;
        if ip_addr_is_blocked_for_favicon(address) {
            return false;
        }
    }
    found
}

pub(super) fn favicon_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.starts_with(&[0, 0, 1, 0]) {
        return Some("image/x-icon");
    }
    if bytes.starts_with(b"BM") {
        return Some("image/bmp");
    }
    if bytes.len() >= 16
        && &bytes[4..8] == b"ftyp"
        && bytes[8..]
            .chunks(4)
            .take(5)
            .any(|brand| brand == b"avif" || brand == b"avis")
    {
        return Some("image/avif");
    }
    None
}

pub(super) fn favicon_content_type_is_image(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    value.starts_with("image/") || value.contains("svg") || value.contains("icon")
}
