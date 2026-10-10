use super::*;
use aipass_crypto::SecretString;
use aipass_provider_registry::{AuthScheme, InterfaceType, ProviderEndpoint, ProviderKind};
use aipass_proxy::{ProxyRouteConfig, ProxyTargetConfig, RetryPolicy, RouteStrategy};
use aipass_vault::{ProviderEntryInput, ProviderEntryUpdateInput, SecretMetadataInput};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

fn read_http_request(reader: &mut impl Read) -> std::io::Result<Vec<u8>> {
    let mut request = Vec::new();
    let mut byte = [0];
    while !request.ends_with(b"\r\n\r\n") {
        if request.len() >= 64 * 1024 {
            return Err(std::io::Error::other("fixture request headers too large"));
        }
        reader.read_exact(&mut byte)?;
        request.push(byte[0]);
    }
    let headers = String::from_utf8_lossy(&request).to_ascii_lowercase();
    assert!(
        !headers.contains("transfer-encoding:"),
        "fixture expects a fixed-length request"
    );
    let length = headers
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .map(|v| v.trim().parse::<usize>())
        .transpose()
        .map_err(std::io::Error::other)?
        .unwrap_or(0);
    assert!(length <= 1024 * 1024, "fixture request body too large");
    let offset = request.len();
    request.resize(offset + length, 0);
    reader.read_exact(&mut request[offset..])?;
    Ok(request)
}

#[test]
fn fixture_consumes_headers_and_body_across_separate_reads() {
    let headers = b"POST /v1/responses HTTP/1.1\r\nContent-Length: 2\r\n\r\n";
    let body = b"{}";
    let mut reader = headers.as_slice().chain(body.as_slice());
    let request = read_http_request(&mut reader).unwrap();
    assert_eq!(request, [headers.as_slice(), body.as_slice()].concat());
    assert_eq!(reader.read(&mut [0]).unwrap(), 0);
}

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
