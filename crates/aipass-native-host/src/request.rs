use crate::config::NativeHostConfig;
use crate::preview::{detected_secret_preview, DetectedSecretFields};
use crate::protocol::{validate_extension_id, NativeRequest, NativeResponse};
use aipass_crypto::SecretString;
use aipass_provider_registry::{match_provider_by_domain, provider_kind_for_id};
use aipass_provider_registry::{AuthScheme, InterfaceType, ProviderEndpoint};
use aipass_vault::{ProviderEntryInput, TtlGrantSummary, Vault};
use anyhow::{bail, Context, Result};
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
        } => Ok(json!({ "protocolVersion": 1, "locked": config.master_password.is_none() })),
        NativeRequest::Ping { .. } => bail!("unsupported protocol version"),
        NativeRequest::UnlockRequest { reason, .. } => Ok(json!({
            "locked": config.master_password.is_none(),
            "reason": reason,
            "desktopRequired": config.master_password.is_none()
        })),
        NativeRequest::ContextLookup { origin, url, .. } => {
            let vault = open_vault(config)?;
            let mut entries = vault.lookup_by_origin(&origin)?;
            if entries.is_empty() {
                entries = vault.lookup_by_origin(&url)?;
            }
            let grants = entries
                .iter()
                .take(5)
                .map(|entry| {
                    vault.create_secret_grant(entry.id, "chrome.fill", 120, Some(origin.clone()))
                })
                .collect::<Result<Vec<TtlGrantSummary>, _>>()?;
            Ok(json!({ "entries": entries, "grants": grants }))
        }
        NativeRequest::SecretFill {
            entry_id, grant_id, ..
        } => {
            let vault = open_vault(config)?;
            let secret = vault.consume_secret_grant(grant_id)?;
            Ok(json!({ "entryId": entry_id, "field": "api_key", "secret": secret }))
        }
        NativeRequest::PreviewDetected {
            origin,
            url,
            title,
            endpoint,
            provider_id,
            interface_type,
            auth_scheme,
            api_key,
            environment,
            tags,
            ..
        } => {
            let vault = open_vault(config)?;
            let fields = DetectedSecretFields {
                origin,
                url,
                title,
                endpoint,
                provider_id,
                interface_type,
                auth_scheme,
                api_key,
                environment,
                tags,
            };
            Ok(serde_json::to_value(detected_secret_preview(
                &vault, &fields,
            ))?)
        }
        NativeRequest::SaveDetected {
            origin,
            title,
            endpoint,
            provider_id,
            interface_type,
            auth_scheme,
            api_key,
            environment,
            tags,
            ..
        } => {
            let vault = open_vault(config)?;
            let domain = crate::preview::host_from_origin(&origin);
            let provider_guess = provider_id.clone().or_else(|| {
                match_provider_by_domain(&domain).map(|provider| provider.id.to_string())
            });
            let provider_kind = provider_guess
                .as_deref()
                .map(|id| provider_kind_for_id(Some(id)))
                .unwrap_or(aipass_provider_registry::ProviderKind::Unknown);
            let interface_type = interface_type.unwrap_or_else(|| {
                endpoint
                    .as_deref()
                    .and_then(|endpoint| {
                        let endpoint = endpoint.to_lowercase();
                        if endpoint.contains("generativelanguage") || endpoint.contains("gemini") {
                            Some(InterfaceType::Gemini)
                        } else if endpoint.contains("anthropic") {
                            Some(InterfaceType::AnthropicMessages)
                        } else if endpoint.contains("openai")
                            || endpoint.contains("/v1")
                            || endpoint.contains("gateway")
                            || endpoint.contains("one-api")
                            || endpoint.contains("new-api")
                            || endpoint.contains("litellm")
                            || endpoint.contains("sub2api")
                        {
                            Some(InterfaceType::OpenAiCompatible)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(InterfaceType::CustomHttp)
            });
            let auth_scheme = auth_scheme.unwrap_or(match interface_type {
                InterfaceType::AnthropicMessages => AuthScheme::XApiKey,
                InterfaceType::Gemini => AuthScheme::GoogleApiKey,
                InterfaceType::AzureOpenAi => AuthScheme::AzureApiKey,
                InterfaceType::Bedrock => AuthScheme::AwsProfile,
                InterfaceType::OpenAiCompatible => AuthScheme::Bearer,
                InterfaceType::CustomHttp => AuthScheme::CustomHeader,
            });
            let title = title.unwrap_or_else(|| {
                provider_guess
                    .as_deref()
                    .unwrap_or("browser-provider")
                    .to_string()
            });
            let api_key = SecretString::new(api_key);
            let entry_id = vault.add_provider(ProviderEntryInput {
                title,
                provider_kind,
                provider_id,
                domains: vec![domain.clone()],
                favicon_url: None,
                endpoints: endpoint.into_iter().map(ProviderEndpoint::api).collect(),
                interface_type,
                auth_scheme,
                api_key: api_key.expose().to_string(),
                default_model: None,
                headers: Vec::new(),
                quota: None,
                tags,
                environment: environment.unwrap_or_else(|| "browser".to_string()),
                notes: Some(format!("Captured from {origin}")),
            })?;
            Ok(json!({ "entryId": entry_id }))
        }
    }
}

fn request_id(request: &NativeRequest) -> Uuid {
    match request {
        NativeRequest::Ping { id, .. }
        | NativeRequest::ContextLookup { id, .. }
        | NativeRequest::SecretFill { id, .. }
        | NativeRequest::SaveDetected { id, .. }
        | NativeRequest::PreviewDetected { id, .. }
        | NativeRequest::UnlockRequest { id, .. } => *id,
    }
}

fn request_extension_id(request: &NativeRequest) -> Option<&str> {
    match request {
        NativeRequest::Ping { extension_id, .. }
        | NativeRequest::ContextLookup { extension_id, .. }
        | NativeRequest::SecretFill { extension_id, .. }
        | NativeRequest::SaveDetected { extension_id, .. }
        | NativeRequest::PreviewDetected { extension_id, .. }
        | NativeRequest::UnlockRequest { extension_id, .. } => extension_id.as_deref(),
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
        ok,
        error,
        data,
    }
}

fn open_vault(config: &NativeHostConfig) -> Result<Vault> {
    let password = config.master_password.as_ref().context("vault is locked")?;
    Vault::open(&config.vault_dir, &SecretString::new(password)).map_err(Into::into)
}

fn redact_error(value: &str) -> String {
    if value.contains("sk-") || value.contains("AIza") {
        "[redacted]".to_string()
    } else {
        value.to_string()
    }
}
