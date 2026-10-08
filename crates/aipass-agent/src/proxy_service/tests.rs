use super::*;
use aipass_crypto::SecretString;
use aipass_provider_registry::{AuthScheme, InterfaceType, ProviderEndpoint, ProviderKind};
use aipass_proxy::{ProxyRouteConfig, ProxyTargetConfig, RetryPolicy, RouteStrategy};
use aipass_vault::{ProviderEntryInput, ProviderEntryUpdateInput, SecretMetadataInput};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

fn config_with_token(token: &str) -> ProxyConfig {
    ProxyConfig {
        routes: vec![ProxyRouteConfig {
            id: Uuid::new_v4(),
            name: "test".into(),
            token: token.into(),
            inbound_protocol: aipass_proxy::Protocol::OpenAiResponses,
            upstream_protocol: aipass_proxy::Protocol::OpenAiResponses,
            conversion_enabled: false,
            strategy: RouteStrategy::Fallback,
            targets: Vec::new(),
            retry: RetryPolicy::default(),
            enabled: true,
        }],
        ..ProxyConfig::default()
    }
}

fn provider_input(api_key: &str, endpoint: String, header: &str) -> ProviderEntryInput {
    ProviderEntryInput {
        // Credential-refresh fixtures serve HTTP generations only.
        max_concurrent_requests: None,
        supports_websockets: Some(false),
        title: "Proxy upstream".into(),
        provider_kind: ProviderKind::Unknown,
        // Matches the routes these tests build: an OpenAI-native entry
        // speaks the Responses API.
        provider_id: Some("openai".into()),
        credential_kind: Default::default(),
        account_identity: None,
        domains: Vec::new(),
        favicon_url: None,
        endpoints: vec![ProviderEndpoint::api(endpoint)],
        interface_type: InterfaceType::OpenAiCompatible,
        auth_scheme: AuthScheme::Bearer,
        api_key: api_key.into(),
        secret_label: None,
        default_model: None,
        model_aliases: Vec::new(),
        headers: vec![("x-provider-header".into(), header.into())],
        quota: None,
        subscription: None,
        gateway: None,
        tags: Vec::new(),
        notes: None,
        secret_metadata: SecretMetadataInput::default(),
    }
}

mod credentials;
mod lifecycle;
mod routing;
