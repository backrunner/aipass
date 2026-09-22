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
        let entries = vault.list_provider_summaries().map_err(map_vault_error)?;
        let mut proxy = state.proxy.lock().map_err(|_| unavailable())?;
        let config = proxy.config(vault)?;
        let status = proxy.status();
        let providers: Vec<_> = entries.iter().filter(|e| e.archived_at.is_none() && e.deleted_at.is_none())
            .map(|entry| json!({"id":entry.id,"title":entry.title,"providerId":entry.provider_id,
                "credentialKind":entry.credential_kind,"interfaceType":entry.interface_type,
                "secrets":entry.secret_refs.iter().map(|s| json!({"id":s.id,"label":s.label,"masked":s.masked})).collect::<Vec<_>>() }))
            .collect();
        let routes: Vec<_> = config.routes.iter().map(|route| json!({
            "id":route.id,"name":route.name,"enabled":route.enabled,"strategy":route.strategy,
            "protocol":route.inbound_protocol,
            "targets":route.targets.iter().map(|target| json!({
                "id":target.id,"label":target.label,"providerEntryId":target.provider_entry_id,
                "secretId":target.secret_id,"enabled":target.enabled,"priority":target.priority,
                "weight":target.weight,"preferWs":target.prefer_ws,
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
                "totalChannels":status.total_channels},"logs":logs}),
        )
    })
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
                target.base_url = aipass_agent_protocol::endpoint_url(&entry.endpoints)
                    .ok_or_else(|| invalid("Provider needs an API endpoint."))?;
                target.auth_scheme = serde_json::to_value(&entry.auth_scheme)
                    .map_err(|_| unavailable())?
                    .as_str()
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
            if tool_preview(state, &preview.selection)?.1 != preview.digest {
                return Err(ServiceError::new(
                    AgentErrorCode::Conflict,
                    "Preview changed.",
                ));
            }
            let request = match preview.selection {
                ControlPanelToolSelection::Credential { request } => {
                    AgentRequest::ToolConfigApply { request }
                }
                ControlPanelToolSelection::Proxy { request } => {
                    AgentRequest::ToolConfigProxyApply { request }
                }
            };
            let data = dispatch(state, request)?;
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
