use crate::config::NativeHostConfig;
use crate::protocol::{
    validate_extension_id, NativeRequest, NativeResponse, NATIVE_PROTOCOL_VERSION,
};
use aipass_agent::{AgentClient, AgentClientConfig, AgentCommandError};
use aipass_agent_protocol::{
    AgentErrorCode, AgentRequest, BrowserContextLookupData, BrowserDetectedSecretFields,
    BrowserDetectedSecretPreview, BrowserFillResult, BrowserIgnoreOriginResult,
    BrowserIgnoredStatus, FaviconBackfillRequest, FaviconBackfillResponse, SaveDetectedResult,
    SessionStatus, SessionUnlockMode, UsageProbeResult,
};
use aipass_provider_registry::{provider_kind_for_id, ProviderEndpoint};
use aipass_vault::{ProviderEntryInput, ProviderEntryUpdateInput};
use anyhow::{bail, Result};
use serde::de::DeserializeOwned;
use serde_json::json;
use uuid::Uuid;

pub fn handle_request(request: NativeRequest) -> NativeResponse {
    match NativeHostConfig::from_env() {
        Ok(config) => handle_request_with_config(request, &config),
        Err(err) => response(
            request_id(&request),
            false,
            Some(err.to_string()),
            serde_json::json!({}),
        ),
    }
}

pub fn handle_request_with_config(
    request: NativeRequest,
    config: &NativeHostConfig,
) -> NativeResponse {
    let id = request_id(&request);
    if let Err(err) = validate_request_extension(&request, config) {
        return response(
            id,
            false,
            Some(redact_error(&err.to_string())),
            serde_json::json!({}),
        );
    }
    match handle_request_inner(request, config) {
        Ok(data) => response(id, true, None, data),
        Err(err) => response(
            id,
            false,
            Some(redact_error(&err.to_string())),
            serde_json::json!({}),
        ),
    }
}

fn handle_request_inner(
    request: NativeRequest,
    config: &NativeHostConfig,
) -> Result<serde_json::Value> {
    match request {
        NativeRequest::Ping {
            protocol_version: 1,
            ..
        } => {
            let status = session_status(config)?;
            Ok(json!({
                "protocolVersion": 1,
                "locked": status.locked,
                "exists": status.exists,
                "vaultNamespace": status.vault_namespace,
            }))
        }
        NativeRequest::Ping { .. } => bail!("unsupported protocol version"),
        NativeRequest::UnlockRequest { reason, .. } => {
            let _: SessionStatus = request_agent(
                config,
                &AgentRequest::SessionUnlock {
                    mode: SessionUnlockMode::NativeWindow,
                },
            )?;
            let status = session_status(config)?;
            Ok(json!({
                "locked": status.locked,
                "reason": reason,
                "desktopRequired": true
            }))
        }
        NativeRequest::SessionUnlock {
            interactive,
            wait,
            timeout_ms,
            password,
            ..
        } => {
            let status: SessionStatus = if let Some(password) = password {
                request_agent(
                    config,
                    &AgentRequest::SessionUnlock {
                        mode: SessionUnlockMode::Password { password },
                    },
                )?
            } else if interactive.as_deref() == Some("native_window") {
                let mode = if wait {
                    SessionUnlockMode::NativeWindowWait {
                        timeout_ms: timeout_ms.unwrap_or(30_000),
                    }
                } else {
                    SessionUnlockMode::NativeWindow
                };
                request_agent(config, &AgentRequest::SessionUnlock { mode })?
            } else {
                bail!("interactive unlock via desktop window is required")
            };
            Ok(json!({
                "locked": status.locked,
                "exists": status.exists,
                "policy": status.policy,
                "vaultNamespace": status.vault_namespace,
            }))
        }
        NativeRequest::UiOpenMain { .. } => {
            let _: serde_json::Value = request_agent(config, &AgentRequest::UiOpenMain)?;
            Ok(json!({ "opened": true }))
        }
        NativeRequest::IsOriginIgnored { origin, .. } => {
            let result: BrowserIgnoredStatus =
                request_agent(config, &AgentRequest::BrowserIsOriginIgnored { origin })?;
            Ok(serde_json::to_value(result)?)
        }
        NativeRequest::IgnoreOrigin { origin, .. } => {
            let result: BrowserIgnoreOriginResult =
                request_agent(config, &AgentRequest::BrowserIgnoreOrigin { origin })?;
            Ok(serde_json::to_value(result)?)
        }
        NativeRequest::ContextLookup { origin, url, .. } => {
            let result: BrowserContextLookupData =
                request_agent(config, &AgentRequest::BrowserContextLookup { origin, url })?;
            Ok(serde_json::to_value(result)?)
        }
        NativeRequest::EntriesList { .. } => {
            let entries: Vec<aipass_vault::EntrySummary> =
                request_agent(config, &AgentRequest::EntriesList { archived: false })?;
            Ok(json!({ "entries": entries, "grants": [] }))
        }
        NativeRequest::EntriesSearch { origin, query, .. } => {
            let result: BrowserContextLookupData = request_agent(
                config,
                &AgentRequest::BrowserEntriesSearch { origin, query },
            )?;
            Ok(serde_json::to_value(result)?)
        }
        NativeRequest::SecretFill {
            entry_id, grant_id, ..
        } => {
            let result: BrowserFillResult = request_agent(
                config,
                &AgentRequest::BrowserSecretFill {
                    entry_id: Some(entry_id),
                    grant_id,
                },
            )?;
            Ok(serde_json::to_value(result)?)
        }
        NativeRequest::PreviewDetected {
            origin,
            url,
            title,
            favicon_url,
            secret_label,
            endpoint,
            provider_id,
            interface_type,
            auth_scheme,
            api_key,
            tags,
            gateway,
            ..
        } => {
            let result: BrowserDetectedSecretPreview = request_agent(
                config,
                &AgentRequest::BrowserPreviewDetected {
                    fields: BrowserDetectedSecretFields {
                        origin,
                        url,
                        title,
                        favicon_url,
                        secret_label,
                        endpoint,
                        provider_id,
                        interface_type,
                        auth_scheme,
                        api_key,
                        tags,
                        gateway,
                    },
                },
            )?;
            Ok(serde_json::to_value(result)?)
        }
        NativeRequest::SaveDetected {
            origin,
            url,
            title,
            favicon_url,
            secret_label,
            endpoint,
            provider_id,
            interface_type,
            auth_scheme,
            api_key,
            tags,
            gateway,
            ..
        } => {
            let result: SaveDetectedResult = request_agent(
                config,
                &AgentRequest::BrowserSaveDetected {
                    fields: BrowserDetectedSecretFields {
                        origin,
                        url,
                        title,
                        favicon_url,
                        secret_label,
                        endpoint,
                        provider_id,
                        interface_type,
                        auth_scheme,
                        api_key,
                        tags,
                        gateway,
                    },
                },
            )?;
            Ok(serde_json::to_value(result)?)
        }
        NativeRequest::ProviderAdd {
            title,
            provider_id,
            domain,
            favicon_url,
            endpoint,
            endpoints,
            console_endpoints,
            interface_type,
            auth_scheme,
            api_key,
            default_model,
            model_aliases,
            headers,
            quota,
            gateway,
            tags,
            notes,
            ..
        } => {
            let mut api_endpoints: Vec<ProviderEndpoint> = endpoints
                .into_iter()
                .chain(endpoint)
                .filter_map(non_empty)
                .map(ProviderEndpoint::api)
                .collect();
            api_endpoints.extend(
                console_endpoints
                    .into_iter()
                    .filter_map(non_empty)
                    .map(ProviderEndpoint::console),
            );
            let input = ProviderEntryInput {
                title: non_empty(title).unwrap_or_else(|| "Custom Provider".to_string()),
                provider_kind: provider_kind_for_id(provider_id.as_deref()),
                provider_id,
                domains: domain.into_iter().filter_map(non_empty).collect(),
                favicon_url: favicon_url.and_then(non_empty),
                endpoints: api_endpoints,
                interface_type,
                auth_scheme,
                api_key: api_key.into_inner(),
                secret_label: None,
                default_model: default_model.and_then(non_empty),
                model_aliases: model_aliases
                    .into_iter()
                    .filter_map(|(a, m)| Some((non_empty(a)?, non_empty(m)?)))
                    .collect(),
                headers,
                quota,
                gateway,
                tags: tags.into_iter().filter_map(non_empty).collect(),
                notes: notes.and_then(non_empty),
            };
            let entry_id: Uuid = request_agent(config, &AgentRequest::ProviderAdd { input })?;
            Ok(json!({ "entryId": entry_id }))
        }
        NativeRequest::ProviderUpdate {
            entry_id,
            title,
            provider_id,
            domain,
            favicon_url,
            endpoint,
            endpoints,
            console_endpoints,
            interface_type,
            auth_scheme,
            api_key,
            default_model,
            model_aliases,
            headers,
            quota,
            gateway,
            tags,
            notes,
            ..
        } => {
            let mut api_endpoints: Vec<ProviderEndpoint> = endpoints
                .into_iter()
                .chain(endpoint)
                .filter_map(non_empty)
                .map(ProviderEndpoint::api)
                .collect();
            api_endpoints.extend(
                console_endpoints
                    .into_iter()
                    .filter_map(non_empty)
                    .map(ProviderEndpoint::console),
            );
            let input = ProviderEntryUpdateInput {
                title: non_empty(title).unwrap_or_else(|| "Custom Provider".to_string()),
                provider_kind: provider_kind_for_id(provider_id.as_deref()),
                provider_id,
                domains: domain.into_iter().filter_map(non_empty).collect(),
                favicon_url: favicon_url.and_then(non_empty),
                endpoints: api_endpoints,
                interface_type,
                auth_scheme,
                api_key: api_key.map(|value| value.into_inner()).and_then(non_empty),
                default_model: default_model.and_then(non_empty),
                model_aliases: model_aliases
                    .into_iter()
                    .filter_map(|(a, m)| Some((non_empty(a)?, non_empty(m)?)))
                    .collect(),
                headers,
                quota,
                gateway,
                tags: tags.into_iter().filter_map(non_empty).collect(),
                notes: notes.and_then(non_empty),
            };
            let _: serde_json::Value = request_agent(
                config,
                &AgentRequest::ProviderUpdate {
                    id: entry_id,
                    input,
                },
            )?;
            Ok(json!({ "entryId": entry_id }))
        }
        NativeRequest::ProviderUsageProbe {
            entry_id,
            mode,
            timeout_seconds,
            ..
        } => {
            let result: UsageProbeResult = request_agent(
                config,
                &AgentRequest::ProviderUsageProbe {
                    id: entry_id,
                    mode,
                    timeout_seconds: timeout_seconds.max(1),
                    base_url: None,
                    access_token: None,
                    user_id: None,
                },
            )?;
            Ok(serde_json::to_value(result)?)
        }
        NativeRequest::ProviderUsageApply {
            entry_id,
            quota,
            gateway,
            ..
        } => {
            let _: serde_json::Value = request_agent(
                config,
                &AgentRequest::ProviderUsageApply {
                    id: entry_id,
                    quota,
                    gateway,
                },
            )?;
            Ok(json!({ "entryId": entry_id }))
        }
        NativeRequest::ProviderFaviconBackfill {
            entry_ids, limit, ..
        } => {
            let result: FaviconBackfillResponse = request_agent(
                config,
                &AgentRequest::ProviderFaviconBackfill {
                    request: FaviconBackfillRequest { entry_ids, limit },
                },
            )?;
            Ok(serde_json::to_value(result)?)
        }
        NativeRequest::ProviderDelete { entry_id, .. } => {
            let _: serde_json::Value =
                request_agent(config, &AgentRequest::ProviderDelete { id: entry_id })?;
            Ok(json!({ "entryId": entry_id, "deleted": true }))
        }
    }
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn request_id(request: &NativeRequest) -> Uuid {
    match request {
        NativeRequest::Ping { id, .. }
        | NativeRequest::ContextLookup { id, .. }
        | NativeRequest::EntriesList { id, .. }
        | NativeRequest::EntriesSearch { id, .. }
        | NativeRequest::IsOriginIgnored { id, .. }
        | NativeRequest::IgnoreOrigin { id, .. }
        | NativeRequest::SecretFill { id, .. }
        | NativeRequest::SaveDetected { id, .. }
        | NativeRequest::PreviewDetected { id, .. }
        | NativeRequest::ProviderAdd { id, .. }
        | NativeRequest::ProviderUpdate { id, .. }
        | NativeRequest::ProviderUsageProbe { id, .. }
        | NativeRequest::ProviderUsageApply { id, .. }
        | NativeRequest::ProviderFaviconBackfill { id, .. }
        | NativeRequest::ProviderDelete { id, .. }
        | NativeRequest::UnlockRequest { id, .. }
        | NativeRequest::SessionUnlock { id, .. }
        | NativeRequest::UiOpenMain { id, .. } => *id,
    }
}

fn request_extension_id(request: &NativeRequest) -> Option<&str> {
    match request {
        NativeRequest::Ping { extension_id, .. }
        | NativeRequest::ContextLookup { extension_id, .. }
        | NativeRequest::EntriesList { extension_id, .. }
        | NativeRequest::EntriesSearch { extension_id, .. }
        | NativeRequest::IsOriginIgnored { extension_id, .. }
        | NativeRequest::IgnoreOrigin { extension_id, .. }
        | NativeRequest::SecretFill { extension_id, .. }
        | NativeRequest::SaveDetected { extension_id, .. }
        | NativeRequest::PreviewDetected { extension_id, .. }
        | NativeRequest::ProviderAdd { extension_id, .. }
        | NativeRequest::ProviderUpdate { extension_id, .. }
        | NativeRequest::ProviderUsageProbe { extension_id, .. }
        | NativeRequest::ProviderUsageApply { extension_id, .. }
        | NativeRequest::ProviderFaviconBackfill { extension_id, .. }
        | NativeRequest::ProviderDelete { extension_id, .. }
        | NativeRequest::UnlockRequest { extension_id, .. }
        | NativeRequest::SessionUnlock { extension_id, .. }
        | NativeRequest::UiOpenMain { extension_id, .. } => extension_id.as_deref(),
    }
}

fn validate_request_extension(request: &NativeRequest, config: &NativeHostConfig) -> Result<()> {
    let Some(extension_id) = request_extension_id(request) else {
        if config.allowed_extension_ids.is_empty() {
            return Ok(());
        }
        bail!("extension id missing");
    };
    validate_extension_id(extension_id, &config.allowed_extension_ids)
}

fn response(id: Uuid, ok: bool, error: Option<String>, data: serde_json::Value) -> NativeResponse {
    NativeResponse {
        id,
        protocol_version: NATIVE_PROTOCOL_VERSION,
        ok,
        error,
        data,
    }
}

fn session_status(config: &NativeHostConfig) -> Result<SessionStatus> {
    request_agent(config, &AgentRequest::SessionStatus)
}

fn request_agent<T: DeserializeOwned>(
    config: &NativeHostConfig,
    request: &AgentRequest,
) -> Result<T> {
    let client = AgentClient::new(AgentClientConfig::for_vault(config.vault_dir.clone())?);
    match client.request::<T>(request) {
        Ok(value) => Ok(value),
        Err(err) if matches!(err.code, Some(AgentErrorCode::ServiceUnavailable)) => {
            client.ensure_running_for_desktop_companion()?;
            client.request::<T>(request).map_err(agent_error_to_anyhow)
        }
        Err(err) => Err(agent_error_to_anyhow(err)),
    }
}

fn agent_error_to_anyhow(err: AgentCommandError) -> anyhow::Error {
    let message = match err.code {
        Some(code) => format!(
            "{}: {}",
            aipass_agent_protocol::error_code_name(&code),
            err.message
        ),
        None => err.message,
    };
    anyhow::anyhow!(message)
}

fn redact_error(value: &str) -> String {
    if value.contains("sk-")
        || value.contains("AIza")
        || value.contains("key=")
        || value.to_lowercase().contains("authorization")
        || value.to_lowercase().contains("api-key")
    {
        "[redacted]".to_string()
    } else {
        value.to_string()
    }
}
