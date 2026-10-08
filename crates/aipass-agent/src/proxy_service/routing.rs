//! Provider wire, quota and credential routing contracts.
use super::*;

pub(super) fn provider_profile(
    entry: &aipass_vault::EntrySummary,
) -> aipass_proxy::ProviderProfile {
    use aipass_proxy::ProviderProfile::*;
    if entry.provider_kind != ProviderKind::Official {
        return Generic;
    }
    match entry.provider_id.as_deref() {
        Some("openai") => OpenAi,
        Some("deepseek") => DeepSeek,
        Some("moonshot" | "kimi") => Kimi,
        Some("mistral") => Mistral,
        Some("gemini") => GeminiCompatible,
        _ => Generic,
    }
}

pub(super) fn account_quota(entry: &aipass_vault::EntrySummary) -> Vec<aipass_proxy::QuotaWindow> {
    if entry.provider_kind != ProviderKind::Official
        || entry.credential_kind != CredentialKind::OAuth
    {
        return Vec::new();
    }
    let Some(snapshot) = entry
        .subscription
        .as_ref()
        .filter(|s| !s.stale && s.error.is_none())
    else {
        return Vec::new();
    };
    let timestamp = |value: &str| {
        OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
            .ok()
            .and_then(|t| u64::try_from(t.unix_timestamp()).ok())
    };
    let Some(observed_at) = timestamp(&snapshot.observed_at) else {
        return Vec::new();
    };
    snapshot
        .windows
        .iter()
        .filter_map(|window| {
            // Model-specific limits must not disable every model on the account.
            if !window.id.starts_with("community_account_")
                && !matches!(
                    window.id.as_str(),
                    "primary" | "secondary" | "five_hour" | "seven_day"
                )
            {
                return None;
            }
            let used = window.used_percent.filter(|v| v.is_finite() && *v >= 0.0)?;
            Some(aipass_proxy::QuotaWindow {
                models: None,
                not_models: Vec::new(),
                used_basis_points: (used.min(100.0) * 100.0).round() as u16,
                observed_at,
                resets_at: window.resets_at.as_deref().and_then(timestamp),
            })
        })
        .collect()
}

/// Legacy managed grants cannot bypass CLI ownership. Only the primary
/// subscription credential is retired; extra API keys retain their selection.
pub(super) fn managed_oauth_token(
    vault: &Vault,
    entry: &aipass_vault::EntrySummary,
    secret_id: &str,
) -> ServiceResult<Option<String>> {
    if entry.provider_kind == ProviderKind::Official
        && entry.credential_kind == CredentialKind::OAuth
        && aipass_provider_registry::primary_secret_ref(&entry.secret_refs)
            .is_some_and(|secret| secret.id == secret_id)
        && matches!(
            entry.provider_id.as_deref(),
            Some("openai" | "codex" | "xai" | "grok" | "copilot" | "gemini-cli")
        )
        && !crate::community::is_account(vault, entry.id)
    {
        return Err(ServiceError::new(
            aipass_agent_protocol::AgentErrorCode::ValidationFailed,
            "Reconnect this subscription with its official CLI; legacy managed OAuth is retired",
        ));
    }
    Ok(None)
}

pub(crate) fn upstream_kind(entry: &aipass_vault::EntrySummary) -> aipass_proxy::UpstreamKind {
    if entry.provider_kind == ProviderKind::Official
        && entry.credential_kind == CredentialKind::OAuth
        && entry.provider_id.as_deref() == Some("anthropic")
    {
        return aipass_proxy::UpstreamKind::ClaudeSubscription;
    }
    if entry.provider_kind == ProviderKind::Official
        && entry.provider_id.as_deref() == Some("copilot")
    {
        return aipass_proxy::UpstreamKind::Copilot;
    }
    if entry.provider_kind == ProviderKind::Official
        && entry.credential_kind == CredentialKind::OAuth
        && matches!(entry.provider_id.as_deref(), Some("openai" | "codex"))
    {
        aipass_proxy::UpstreamKind::CodexSubscription
    } else {
        aipass_proxy::UpstreamKind::Standard
    }
}

pub(crate) fn pinned_official_oauth_endpoint(
    provider_kind: &ProviderKind,
    credential_kind: &CredentialKind,
    provider_id: Option<&str>,
) -> Option<&'static str> {
    if provider_kind != &ProviderKind::Official || credential_kind != &CredentialKind::OAuth {
        return None;
    }
    match provider_id {
        Some("anthropic") => Some("https://api.anthropic.com"),
        Some("openai" | "codex") => Some("https://chatgpt.com/backend-api/codex"),
        Some("xai") => Some("https://cli-chat-proxy.grok.com/v1"),
        Some("copilot") => Some("https://api.githubcopilot.com"),
        _ => None,
    }
}

pub(crate) fn proxy_auth_scheme(auth_scheme: &AuthScheme) -> Option<&'static str> {
    match auth_scheme {
        AuthScheme::Bearer => Some("bearer"),
        AuthScheme::CustomHeader => Some("custom_header"),
        AuthScheme::XApiKey => Some("x_api_key"),
        AuthScheme::AzureApiKey => Some("azure_api_key"),
        AuthScheme::GoogleApiKey => Some("google_api_key"),
        AuthScheme::AwsProfile => None,
    }
}

/// Upstream wire protocol for a key's bound interface, mirroring the desktop
/// `nativeProtocolForEntry` mapping: first-party OpenAI (and Codex OAuth) speak
/// the Responses API, every other OpenAI-compatible endpoint speaks Chat
/// Completions. Interfaces without a proxy protocol return `None`.
pub(crate) fn key_upstream_protocol(
    interface: &InterfaceType,
    entry: &aipass_vault::EntrySummary,
) -> Option<aipass_proxy::Protocol> {
    match interface {
        InterfaceType::AnthropicMessages => Some(aipass_proxy::Protocol::AnthropicMessages),
        InterfaceType::Gemini => Some(aipass_proxy::Protocol::OpenAiChatCompletions),
        InterfaceType::OpenAiCompatible | InterfaceType::AzureOpenAi => {
            if entry.provider_id.as_deref() == Some("openai")
                || (entry.provider_id.as_deref() == Some("codex")
                    && entry.credential_kind == CredentialKind::OAuth)
            {
                Some(aipass_proxy::Protocol::OpenAiResponses)
            } else {
                Some(aipass_proxy::Protocol::OpenAiChatCompletions)
            }
        }
        _ => None,
    }
}
