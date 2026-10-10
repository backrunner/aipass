use super::{invalid, sessions::Sessions, unavailable};
use crate::session::{map_vault_error, with_vault, AgentState, ServiceError, ServiceResult};
use aipass_agent_protocol::{
    AgentErrorCode, AgentRequest, ControlPanelAction, ControlPanelToolSelection, LockReason,
    ProxyConfig,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Instant};
use uuid::Uuid;

pub(super) struct Preview {
    pub id: Uuid,
    pub created: Instant,
    selection: ControlPanelToolSelection,
    digest: [u8; 32],
}

pub(super) fn snapshot(state: &Arc<AgentState>) -> ServiceResult<Value> {
    with_vault(state, false, |vault| {
        let mut entries = vault.list_provider_summaries().map_err(map_vault_error)?;
        entries.extend(
            vault
                .list_archived_provider_summaries()
                .map_err(map_vault_error)?,
        );
        entries.extend(
            vault
                .list_trash_provider_summaries()
                .map_err(map_vault_error)?,
        );
        let mut proxy = state.proxy.lock().map_err(|_| unavailable())?;
        let config = proxy.config(vault)?;
        let status = proxy.status();
        let providers: Vec<_> = entries.iter()
            .map(|entry| Ok(json!({"id":entry.id,"title":entry.title,"providerId":entry.provider_id,
                "credentialKind":entry.credential_kind,"interfaceType":entry.interface_type,"authScheme":entry.auth_scheme,"providerKind":entry.provider_kind,
                "favorite":entry.favorite,"tags":entry.tags,"lastUsedAt":panel_timestamp(entry.last_used_at)?,
                "archivedAt":panel_timestamp(entry.archived_at)?,"deletedAt":panel_timestamp(entry.deleted_at)?,
                "secrets":entry.secret_refs.iter().map(|s| json!({"id":s.id,"label":s.label,"masked":s.masked,"interfaceType":s.interface_type.as_ref().unwrap_or(&entry.interface_type),
                    "proxyEligible":entry.archived_at.is_none() && entry.deleted_at.is_none()
                        && crate::proxy_service::key_upstream_protocol(s.interface_type.as_ref().unwrap_or(&entry.interface_type),entry).is_some()
                        && crate::proxy_service::proxy_auth_scheme(&s.effective_auth(&entry.interface_type, &entry.auth_scheme)).is_some()
                        && (s.endpoint.is_some() || aipass_agent_protocol::endpoint_url(&entry.endpoints).is_some())})).collect::<Vec<_>>() })))
            .collect::<ServiceResult<Vec<_>>>()?;
        let routes: Vec<_> = config.routes.iter().map(|route| json!({
            "id":route.id,"name":route.name,"enabled":route.enabled,"strategy":route.strategy,
            "protocol":route.inbound_protocol,"inboundProtocol":route.inbound_protocol,
            "upstreamProtocol":route.upstream_protocol,"conversionEnabled":route.conversion_enabled,"retry":route.retry,
            "targets":route.targets.iter().map(|target| json!({
                "id":target.id,"label":target.label,"providerEntryId":target.provider_entry_id,
                "secretId":target.secret_id,"enabled":target.enabled,"priority":target.priority,
                "weight":target.weight,"preferWs":target.prefer_ws,"protocol":target.protocol,"model":target.model,
            })).collect::<Vec<_>>()
        })).collect();
        let mut logs =
            serde_json::to_value(proxy.logs()?.into_iter().rev().take(50).collect::<Vec<_>>())
                .map_err(|_| unavailable())?;
        redact(vault, &config, &mut logs)?;
        Ok(
            json!({"providers":providers,"routes":routes,"revision":revision(&config)?,
            "proxy":{"running":status.running,"bindAddr":status.bind_addr,"requests":status.requests,
                "failures":status.failures,"recentRequests":status.recent_requests,"recentTokens":status.recent_tokens,
                "inFlightRequests":status.in_flight_requests,"availableChannels":status.available_channels,
                "totalChannels":status.total_channels,"successRateBps":status.success_rate_bps,
                "averageFirstTokenMs":status.average_first_token_ms,"degraded":status.degraded,
                "degradedTargetIds":status.degraded_target_ids},"logs":logs}),
        )
    })
}

fn panel_timestamp(value: Option<time::OffsetDateTime>) -> ServiceResult<Option<String>> {
    value
        .map(|value| {
            value
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|_| unavailable())
        })
        .transpose()
}

pub(super) fn action(
    state: &Arc<AgentState>,
    sessions: &Sessions,
    token: &str,
    action: ControlPanelAction,
) -> ServiceResult<Value> {
    let event = match &action {
        ControlPanelAction::ProxyStart => "control_panel.proxy.start",
        ControlPanelAction::ProxyStop => "control_panel.proxy.stop",
        ControlPanelAction::RouteEnabled { .. } => "control_panel.route.enabled",
        ControlPanelAction::RouteSave { .. } => "control_panel.route.save",
        ControlPanelAction::RouteDelete { .. } => "control_panel.route.delete",
        ControlPanelAction::TargetUpdate { .. } => "control_panel.target.update",
        ControlPanelAction::ToolPreview { .. } => "control_panel.tool.preview",
        ControlPanelAction::ToolApply { .. } => "control_panel.tool.apply",
        ControlPanelAction::VaultLock => "control_panel.vault.lock",
    };
    let _scope = crate::logging::RequestScope::new(Uuid::new_v4());
    let result = perform(state, sessions, token, action);
    crate::logging::write_component_log(
        crate::logging::AGENT_LOG,
        if result.is_ok() { "INFO" } else { "WARN" },
        &format!(
            "event={event} outcome={}",
            if result.is_ok() {
                "completed"
            } else {
                "failed"
            }
        ),
    );
    if result.is_ok() {
        crate::session::touch_session(state);
    }
    result
}

fn perform(
    state: &Arc<AgentState>,
    sessions: &Sessions,
    token: &str,
    action: ControlPanelAction,
) -> ServiceResult<Value> {
    match action {
        ControlPanelAction::ProxyStart => with_vault(state, false, |vault| {
            state
                .proxy
                .lock()
                .map_err(|_| unavailable())?
                .start(vault)?;
            Ok(json!({"ok":true}))
        }),
        ControlPanelAction::ProxyStop => with_vault(state, false, |vault| {
            state
                .proxy
                .lock()
                .map_err(|_| unavailable())?
                .stop_and_save(vault)?;
            Ok(json!({"ok":true}))
        }),
        ControlPanelAction::RouteEnabled { route_id, enabled } => {
            with_vault(state, false, |vault| {
                state
                    .proxy
                    .lock()
                    .map_err(|_| unavailable())?
                    .set_route_enabled(vault, route_id, enabled)?;
                Ok(json!({"ok":true}))
            })
        }
        ControlPanelAction::RouteSave {
            revision: expected,
            route,
        } => with_vault(state, false, |vault| {
            let mut proxy = state.proxy.lock().map_err(|_| unavailable())?;
            let mut config = proxy.config(vault)?;
            require_revision(&config, &expected)?;
            let previous = config.routes.iter().find(|item| item.id == route.id);
            if route.name.trim().is_empty()
                || route.name.len() > 256
                || route.targets.is_empty()
                || route.targets.len() > 1024
            {
                return Err(invalid(
                    "A group needs a name and at least one upstream credential.",
                ));
            }
            let mut ids = std::collections::HashSet::new();
            let targets = route
                .targets
                .iter()
                .map(|draft| {
                    if !ids.insert(draft.id)
                        || config
                            .routes
                            .iter()
                            .filter(|item| item.id != route.id)
                            .any(|item| item.targets.iter().any(|target| target.id == draft.id))
                    {
                        return Err(invalid("Duplicate upstream identifier."));
                    }
                    if !(1..=100_000).contains(&draft.weight) {
                        return Err(invalid("Weight must be between 1 and 100000."));
                    }
                    let existing = previous
                        .and_then(|item| item.targets.iter().find(|target| target.id == draft.id))
                        .filter(|target| {
                            target.provider_entry_id == draft.provider_entry_id
                                && target.secret_id == draft.secret_id
                        });
                    let mut target = if let Some(existing) = existing {
                        existing.clone()
                    } else {
                        let entry = vault
                            .get_provider_summary(draft.provider_entry_id)
                            .map_err(map_vault_error)?;
                        vault
                            .runtime_provider_credentials(draft.provider_entry_id, &draft.secret_id)
                            .map_err(map_vault_error)?;
                        let secret = entry
                            .secret_refs
                            .iter()
                            .find(|secret| secret.id == draft.secret_id)
                            .ok_or_else(|| invalid("Credential no longer exists."))?;
                        crate::proxy_service::key_upstream_protocol(
                            secret
                                .interface_type
                                .as_ref()
                                .unwrap_or(&entry.interface_type),
                            &entry,
                        )
                        .ok_or_else(|| {
                            invalid("This credential does not support the proxy's protocols.")
                        })?;
                        aipass_proxy::ProxyTargetConfig {
                            id: draft.id,
                            provider_entry_id: draft.provider_entry_id,
                            secret_id: draft.secret_id.clone(),
                            label: entry.title.clone(),
                            base_url: secret
                                .endpoint
                                .clone()
                                .or_else(|| aipass_agent_protocol::endpoint_url(&entry.endpoints))
                                .ok_or_else(|| invalid("Provider needs an API endpoint."))?,
                            auth_scheme: crate::proxy_service::proxy_auth_scheme(
                                &secret.effective_auth(&entry.interface_type, &entry.auth_scheme),
                            )
                            .ok_or_else(|| invalid("Unsupported authentication scheme."))?
                            .to_owned(),
                            headers: Vec::new(),
                            group: secret.group.clone(),
                            priority: draft.priority,
                            weight: draft.weight,
                            enabled: draft.enabled,
                            protocol: None,
                            prefer_ws: false,
                            model: None,
                        }
                    };
                    target.priority = draft.priority;
                    target.weight = draft.weight;
                    target.enabled = draft.enabled;
                    if let Some(model) = &draft.model {
                        target.model.clone_from(model);
                    }
                    Ok(target)
                })
                .collect::<ServiceResult<Vec<_>>>()?;
            let next = aipass_proxy::ProxyRouteConfig {
                id: route.id,
                name: route.name.trim().to_owned(),
                enabled: route.enabled,
                token: previous.map(|item| item.token.clone()).unwrap_or_default(),
                strategy: route.strategy,
                inbound_protocol: route.inbound_protocol,
                upstream_protocol: previous
                    .filter(|item| item.conversion_enabled)
                    .map(|item| item.upstream_protocol)
                    .unwrap_or(route.inbound_protocol),
                conversion_enabled: previous.is_some_and(|item| item.conversion_enabled),
                retry: route.retry,
                targets,
            };
            if let Some(item) = config.routes.iter_mut().find(|item| item.id == next.id) {
                *item = next;
            } else {
                config.routes.push(next);
            }
            proxy.set_config(vault, config)?;
            Ok(json!({"ok":true}))
        }),
        ControlPanelAction::RouteDelete {
            revision: expected,
            route_id,
        } => with_vault(state, false, |vault| {
            let mut proxy = state.proxy.lock().map_err(|_| unavailable())?;
            let mut config = proxy.config(vault)?;
            require_revision(&config, &expected)?;
            let before = config.routes.len();
            config.routes.retain(|route| route.id != route_id);
            if config.routes.len() == before {
                return Err(invalid("Group no longer exists."));
            }
            proxy.set_config(vault, config)?;
            Ok(json!({"ok":true}))
        }),
        ControlPanelAction::TargetUpdate {
            route_id,
            target_id,
            revision: expected,
            provider_entry_id,
            secret_id,
            enabled,
            priority,
            weight,
            prefer_ws,
        } => with_vault(state, false, |vault| {
            if !(1..=100_000).contains(&weight) {
                return Err(invalid("Weight must be between 1 and 100000."));
            }
            let mut proxy = state.proxy.lock().map_err(|_| unavailable())?;
            let mut config = proxy.config(vault)?;
            if revision(&config)? != expected {
                return Err(ServiceError::new(
                    AgentErrorCode::Conflict,
                    "Configuration changed.",
                ));
            }
            let target = config
                .routes
                .iter_mut()
                .find(|r| r.id == route_id)
                .and_then(|r| r.targets.iter_mut().find(|t| t.id == target_id))
                .ok_or_else(|| invalid("Target no longer exists."))?;
            if target.provider_entry_id != provider_entry_id || target.secret_id != secret_id {
                let entry = vault
                    .get_provider_summary(provider_entry_id)
                    .map_err(map_vault_error)?;
                let secret = entry
                    .secret_refs
                    .iter()
                    .find(|s| s.id == secret_id)
                    .ok_or_else(|| invalid("Credential no longer exists."))?;
                // Also rejects archived/deleted providers and unavailable keys.
                vault
                    .runtime_provider_credentials(provider_entry_id, &secret_id)
                    .map_err(map_vault_error)?;
                let protocol = crate::proxy_service::key_upstream_protocol(
                    secret
                        .interface_type
                        .as_ref()
                        .unwrap_or(&entry.interface_type),
                    &entry,
                )
                .ok_or_else(|| {
                    invalid("This credential does not support the proxy's protocols.")
                })?;
                target.base_url = secret
                    .endpoint
                    .clone()
                    .or_else(|| aipass_agent_protocol::endpoint_url(&entry.endpoints))
                    .ok_or_else(|| invalid("Provider needs an API endpoint."))?;
                target.auth_scheme = crate::proxy_service::proxy_auth_scheme(
                    &secret.effective_auth(&entry.interface_type, &entry.auth_scheme),
                )
                .ok_or_else(|| invalid("Unsupported authentication scheme."))?
                .to_owned();
                target.provider_entry_id = provider_entry_id;
                target.secret_id = secret_id;
                target.label = entry.title;
                target.protocol = Some(protocol);
                target.headers.clear();
            }
            target.enabled = enabled;
            target.priority = priority;
            target.weight = weight;
            target.prefer_ws = prefer_ws;
            proxy.set_config(vault, config)?;
            Ok(json!({"ok":true}))
        }),
        ControlPanelAction::ToolPreview { selection } => {
            let (data, digest) = tool_preview(state, &selection)?;
            let preview = Preview {
                id: Uuid::new_v4(),
                created: Instant::now(),
                selection,
                digest,
            };
            let result = json!({"previewId":preview.id,"tool":data["tool"],"mode":data["mode"],
                "entryTitle":data["entryTitle"],"targetPath":data["targetPath"],"preview":data["preview"]});
            sessions.save_preview(token, preview)?;
            Ok(result)
        }
        ControlPanelAction::ToolApply { preview_id } => {
            let preview = sessions.take_preview(token, preview_id)?;
            let (fresh, digest) = tool_preview(state, &preview.selection)?;
            if digest != preview.digest {
                return Err(ServiceError::new(
                    AgentErrorCode::Conflict,
                    "Preview changed.",
                ));
            }
            let request = match preview.selection {
                ControlPanelToolSelection::Credential { mut request } => {
                    request.preview_id = fresh["previewId"].as_str().map(str::to_owned);
                    AgentRequest::ToolConfigApply { request }
                }
                ControlPanelToolSelection::Proxy { request } => {
                    AgentRequest::ToolConfigProxyApply { request }
                }
            };
            let data = dispatch(state, request)?;
            if let Some(outcome) = data["outcome"].as_str().filter(|v| *v != "applied") {
                return Err(ServiceError::new(
                    if outcome == "conflict" {
                        AgentErrorCode::Conflict
                    } else {
                        AgentErrorCode::ValidationFailed
                    },
                    data["message"]
                        .as_str()
                        .unwrap_or("Tool switch was not applied."),
                ));
            }
            Ok(json!({"ok":true,"targetPath":data["targetPath"]}))
        }
        ControlPanelAction::VaultLock => {
            super::sessions::validate(state)?;
            sessions.revoke_all();
            crate::session::lock_session(state, LockReason::Manual);
            Ok(json!({"ok":true}))
        }
    }
}

fn tool_preview(
    state: &Arc<AgentState>,
    selection: &ControlPanelToolSelection,
) -> ServiceResult<(Value, [u8; 32])> {
    let request = match selection {
        ControlPanelToolSelection::Credential { request } => AgentRequest::ToolConfigPreview {
            request: request.clone(),
        },
        ControlPanelToolSelection::Proxy { request } => AgentRequest::ToolConfigProxyPreview {
            request: request.clone(),
        },
    };
    let mut data = dispatch(state, request)?;
    let digest = hash(&data)?;
    with_vault(state, false, |vault| {
        let config = state
            .proxy
            .lock()
            .map_err(|_| unavailable())?
            .config(vault)?;
        redact(vault, &config, &mut data)
    })?;
    if let Some(diff) = data["preview"].as_str() {
        data["preview"] = json!(safe_diff(diff));
    }
    Ok((data, digest))
}

fn safe_diff(diff: &str) -> String {
    aipass_config_writers::redacted_diff_preview(diff, &[])
        .lines()
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            if [
                "token",
                "password",
                "authorization",
                "api_key",
                "apikey",
                "secret",
                "bearer ",
                "\"key\"",
            ]
            .iter()
            .any(|key| lower.contains(key))
            {
                format!(
                    "{}[redacted]",
                    if line.starts_with("+ ") {
                        "+ "
                    } else if line.starts_with("- ") {
                        "- "
                    } else {
                        "  "
                    }
                )
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn dispatch(state: &Arc<AgentState>, request: AgentRequest) -> ServiceResult<Value> {
    let response = crate::server::handle_request(state, request);
    if !response.ok {
        return Err(ServiceError::new(response.code.unwrap_or(AgentErrorCode::Internal),
            "This configuration cannot be applied. Check the provider's endpoint, credential and tool mode."));
    }
    Ok(response.data)
}

fn hash(value: &impl serde::Serialize) -> ServiceResult<[u8; 32]> {
    let bytes = zeroize::Zeroizing::new(serde_json::to_vec(value).map_err(|_| unavailable())?);
    Ok(Sha256::digest(&*bytes).into())
}
fn revision(config: &ProxyConfig) -> ServiceResult<String> {
    Ok(hash(config)?
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn require_revision(config: &ProxyConfig, expected: &str) -> ServiceResult<()> {
    if revision(config)? != expected {
        return Err(ServiceError::new(
            AgentErrorCode::Conflict,
            "Configuration changed. Reopen the editor and try again.",
        ));
    }
    Ok(())
}

// Proxy diagnostics already redact known wire fields. Remove exact current
// credentials too before allowing logs or configuration diffs across the LAN.
fn redact(
    vault: &aipass_vault::Vault,
    config: &ProxyConfig,
    value: &mut Value,
) -> ServiceResult<()> {
    fn remove(value: &mut Value, secret: &str) {
        if secret.is_empty() {
            return;
        }
        match value {
            Value::String(text) => *text = text.replace(secret, "[redacted]"),
            Value::Array(values) => values.iter_mut().for_each(|value| remove(value, secret)),
            Value::Object(values) => values.values_mut().for_each(|value| remove(value, secret)),
            _ => {}
        }
    }
    for route in &config.routes {
        remove(value, &route.token);
        for target in &route.targets {
            for (_, secret) in &target.headers {
                remove(value, secret);
            }
        }
    }
    for entry in vault.list_provider_summaries().map_err(map_vault_error)? {
        for secret in &entry.secret_refs {
            if let Ok(credentials) = vault.runtime_provider_credentials(entry.id, &secret.id) {
                remove(value, credentials.secret.expose());
                for (_, secret) in credentials.headers.iter() {
                    remove(value, secret);
                }
            }
        }
    }
    Ok(())
}
