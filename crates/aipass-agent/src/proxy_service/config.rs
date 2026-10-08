//! Route configuration validation and token normalization.
use super::*;

pub(super) fn normalize_versions(versions: &mut Vec<aipass_agent_protocol::GroupPriceVersion>) {
    versions.sort_by_key(|version| version.effective_from);
    let mut deduped: Vec<aipass_agent_protocol::GroupPriceVersion> =
        Vec::with_capacity(versions.len());
    for version in versions.drain(..) {
        match deduped.last_mut() {
            Some(last) if last.effective_from == version.effective_from => *last = version,
            _ => deduped.push(version),
        }
    }
    *versions = deduped;
}

pub(super) fn ensure_route_tokens(config: &mut ProxyConfig) -> bool {
    let mut changed = false;
    for route in &mut config.routes {
        if route.token.trim().is_empty() {
            route.token = generate_local_token();
            changed = true;
        }
    }
    changed
}

pub(super) fn generate_local_token() -> String {
    format!("sk-{}", Uuid::new_v4().simple())
}

/// Fixed-model members always use the credential's trusted native protocol.
/// All clients share this normalization, including CLI and remote-panel edits.
pub(super) fn resolve_model_bindings(vault: &Vault, config: &mut ProxyConfig) -> ServiceResult<()> {
    for route in &mut config.routes {
        for target in route
            .targets
            .iter_mut()
            .filter(|target| target.enabled && target.model.is_some())
        {
            let entry = vault
                .get_provider_summary(target.provider_entry_id)
                .map_err(map_vault_error)?;
            let secret = entry
                .secret_refs
                .iter()
                .find(|secret| secret.id == target.secret_id)
                .ok_or_else(|| {
                    ServiceError::new(
                        aipass_agent_protocol::AgentErrorCode::NotFound,
                        "group member credential no longer exists",
                    )
                })?;
            target.protocol = key_upstream_protocol(
                secret
                    .interface_type
                    .as_ref()
                    .unwrap_or(&entry.interface_type),
                &entry,
            );
            route.conversion_enabled |= target
                .protocol
                .is_some_and(|protocol| protocol != route.inbound_protocol);
        }
    }
    Ok(())
}

pub(super) fn validate_config(config: &ProxyConfig) -> ServiceResult<()> {
    let bind_addr = config
        .bind_addr
        .parse::<std::net::SocketAddr>()
        .map_err(|_| {
            ServiceError::new(
                aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                "proxy bind address must be host:port",
            )
        })?;
    if bind_addr.port() == 0 {
        return Err(ServiceError::new(
            aipass_agent_protocol::AgentErrorCode::ValidationFailed,
            "proxy bind port must be greater than zero",
        ));
    }
    if config.upstream_proxy.mode == aipass_proxy::UpstreamProxyMode::Custom {
        let url = config
            .upstream_proxy
            .custom_url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .ok_or_else(|| {
                ServiceError::new(
                    aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                    "custom upstream proxy mode requires a proxy URL",
                )
            })?;
        reqwest::Proxy::all(url).map_err(|err| {
            ServiceError::new(
                aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                format!("custom upstream proxy URL is invalid: {err}"),
            )
        })?;
    }
    for route in &config.routes {
        let bound = route
            .targets
            .iter()
            .any(|target| target.enabled && target.model.is_some());
        for target in &route.targets {
            if let Some(model) = &target.model {
                if model.trim().is_empty()
                    || model != model.trim()
                    || model.len() > 256
                    || model.chars().any(char::is_control)
                {
                    return Err(ServiceError::new(
                        aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                        "upstream model must be nonempty, trimmed and at most 256 bytes",
                    ));
                }
            }
            if bound && target.enabled && target.model.is_none() {
                return Err(ServiceError::new(
                    aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                    "every enabled group member must bind an upstream model",
                ));
            }
        }
        // `upstream_protocol` is the legacy route-level fallback; targets
        // with an explicit protocol validate against their own value.
        let target_protocols = std::iter::once(route.upstream_protocol).chain(
            route
                .targets
                .iter()
                .filter(|target| target.enabled)
                .filter_map(|target| target.protocol),
        );
        for target_protocol in target_protocols {
            if route.inbound_protocol != target_protocol && !route.conversion_enabled {
                return Err(ServiceError::new(
                    aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                    format!(
                        "proxy route {} mixes protocols without enabling conversion",
                        route.name
                    ),
                ));
            }
            if !aipass_proxy::supports(route.inbound_protocol, target_protocol) {
                return Err(ServiceError::new(
                    aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                    format!(
                        "proxy route {} has no conversion path between its inbound and upstream protocols",
                        route.name
                    ),
                ));
            }
        }
    }
    if config
        .routes
        .iter()
        .any(|route| route.token.trim().is_empty())
    {
        return Err(ServiceError::new(
            aipass_agent_protocol::AgentErrorCode::ValidationFailed,
            "every proxy route group needs a local token",
        ));
    }
    let mut route_ids = HashSet::new();
    let mut route_tokens = HashSet::new();
    let mut target_ids = HashSet::new();
    for route in &config.routes {
        if !route_ids.insert(route.id) {
            return Err(ServiceError::new(
                aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                "proxy route ids must be unique",
            ));
        }
        if !route_tokens.insert(route.token.as_str()) {
            return Err(ServiceError::new(
                aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                "proxy route group tokens must be unique",
            ));
        }
        if route.retry.hold_initial_delay_ms == 0 {
            return Err(ServiceError::new(
                aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                format!(
                    "proxy route {} hold initial delay must be greater than zero",
                    route.name
                ),
            ));
        }
        if route.retry.hold_max_delay_ms < route.retry.hold_initial_delay_ms {
            return Err(ServiceError::new(
                aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                format!(
                    "proxy route {} hold max delay must be at least its initial delay",
                    route.name
                ),
            ));
        }
        for target in &route.targets {
            if !target_ids.insert(target.id) {
                return Err(ServiceError::new(
                    aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                    "proxy target ids must be unique",
                ));
            }
            if !target.enabled {
                continue;
            }
            let url = reqwest::Url::parse(&target.base_url).map_err(|_| {
                ServiceError::new(
                    aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                    format!("proxy target {} has an invalid base URL", target.label),
                )
            })?;
            if !matches!(url.scheme(), "http" | "https") {
                return Err(ServiceError::new(
                    aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                    format!(
                        "proxy target {} must use an HTTP or HTTPS base URL",
                        target.label
                    ),
                ));
            }
            if !matches!(
                target.auth_scheme.as_str(),
                "bearer" | "custom_header" | "x_api_key" | "azure_api_key" | "google_api_key"
            ) {
                return Err(ServiceError::new(
                    aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                    format!(
                        "proxy target {} uses an unsupported authentication scheme",
                        target.label
                    ),
                ));
            }
        }
    }
    Ok(())
}
