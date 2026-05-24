use aipass_crypto::mask_secret;
use aipass_provider_registry::{
    default_provider_definitions, match_provider_by_domain, AuthScheme, EndpointKind, InterfaceType,
};
use aipass_vault::Vault;
use serde::Serialize;

#[derive(Clone, Debug)]
pub(crate) struct DetectedSecretFields {
    pub(crate) origin: String,
    pub(crate) url: String,
    pub(crate) title: Option<String>,
    pub(crate) endpoint: Option<String>,
    pub(crate) provider_id: Option<String>,
    pub(crate) interface_type: Option<InterfaceType>,
    pub(crate) auth_scheme: Option<AuthScheme>,
    pub(crate) api_key: String,
    pub(crate) environment: Option<String>,
    pub(crate) tags: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DetectedSecretPreview {
    title: String,
    provider_id: Option<String>,
    endpoint: Option<String>,
    interface_type: InterfaceType,
    auth_scheme: AuthScheme,
    masked_secret: String,
    fingerprint: String,
    environment: String,
    tags: Vec<String>,
}

pub(crate) fn detected_secret_preview(
    vault: &Vault,
    fields: &DetectedSecretFields,
) -> DetectedSecretPreview {
    let domain = host_from_origin(&fields.origin);
    let provider_guess = fields
        .provider_id
        .clone()
        .or_else(|| match_provider_by_domain(&domain).map(|provider| provider.id.to_string()));
    let provider_definition = provider_guess.as_deref().and_then(|id| {
        default_provider_definitions()
            .into_iter()
            .find(|provider| provider.id == id)
    });
    let endpoint = fields
        .endpoint
        .clone()
        .or_else(|| {
            provider_definition.as_ref().and_then(|provider| {
                provider
                    .endpoints
                    .iter()
                    .find(|(_, kind, _)| *kind == EndpointKind::Api)
                    .map(|(_, _, url)| (*url).to_string())
            })
        })
        .or_else(|| Some(fields.url.clone()));
    let interface_type = fields.interface_type.clone().unwrap_or_else(|| {
        endpoint
            .as_deref()
            .and_then(infer_interface_from_endpoint)
            .or_else(|| {
                provider_definition
                    .as_ref()
                    .and_then(|provider| provider.interfaces.first().cloned())
            })
            .or_else(|| provider_guess_interface(&fields.origin))
            .unwrap_or(InterfaceType::CustomHttp)
    });
    let auth_scheme = fields.auth_scheme.clone().unwrap_or_else(|| {
        provider_definition
            .as_ref()
            .and_then(|provider| provider.auth_schemes.first().cloned())
            .unwrap_or_else(|| default_auth_for_interface(&interface_type))
    });
    let title = fields
        .title
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            provider_definition
                .as_ref()
                .map(|provider| provider.display_name.to_string())
        })
        .unwrap_or_else(|| "Browser Provider".to_string());
    let environment = fields
        .environment
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| "browser".to_string());
    let tags = if fields.tags.is_empty() {
        vec!["browser".to_string()]
    } else {
        fields.tags.clone()
    };

    DetectedSecretPreview {
        title,
        provider_id: provider_guess,
        endpoint,
        interface_type,
        auth_scheme,
        masked_secret: mask_secret(&fields.api_key),
        fingerprint: vault.fingerprint_secret(&fields.api_key),
        environment,
        tags,
    }
}

pub(crate) fn host_from_origin(value: &str) -> String {
    let trimmed = value.trim().to_lowercase();
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(&trimmed);
    without_scheme
        .split('/')
        .next()
        .unwrap_or(without_scheme)
        .split('@')
        .next_back()
        .unwrap_or(without_scheme)
        .split(':')
        .next()
        .unwrap_or(without_scheme)
        .to_string()
}

fn provider_guess_interface(origin: &str) -> Option<InterfaceType> {
    match_provider_by_domain(origin).and_then(|provider| provider.interfaces.first().cloned())
}

fn infer_interface_from_endpoint(endpoint: &str) -> Option<InterfaceType> {
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
}

fn default_auth_for_interface(interface_type: &InterfaceType) -> AuthScheme {
    match interface_type {
        InterfaceType::AnthropicMessages => AuthScheme::XApiKey,
        InterfaceType::Gemini => AuthScheme::GoogleApiKey,
        InterfaceType::AzureOpenAi => AuthScheme::AzureApiKey,
        InterfaceType::Bedrock => AuthScheme::AwsProfile,
        InterfaceType::OpenAiCompatible => AuthScheme::Bearer,
        InterfaceType::CustomHttp => AuthScheme::CustomHeader,
    }
}
