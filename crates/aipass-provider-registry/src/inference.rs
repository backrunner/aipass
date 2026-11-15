//! Endpoint evidence shared by the native UI and Agent.
use crate::{default_provider_definitions, InterfaceType, ProviderDefinition};

/// Keep provider selection aligned with `packages/schemas::inferProviderFromEndpoint`.
/// A custom AI relay defaults to OpenAI compatible; explicit UI protocol choices
/// remain authoritative and are applied by the caller.
pub fn infer_provider_from_endpoint(endpoint: &str) -> Option<ProviderDefinition> {
    let url = url::Url::parse(endpoint.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?;
    if let Some(provider) = match_provider_by_domain(host) {
        return Some(provider);
    }
    let definitions = default_provider_definitions();
    if let Some(provider) = definitions.iter().find(|provider| {
        provider.endpoints.iter().any(|(_, _, endpoint)| {
            url::Url::parse(endpoint)
                .ok()
                .and_then(|url| url.host_str().map(str::to_owned))
                .is_some_and(|known| host == known || host.ends_with(&format!(".{known}")))
        })
    }) {
        return Some(provider.clone());
    }
    let host: String = host
        .chars()
        .filter(|c| !matches!(c, '-' | '_' | '.'))
        .collect();
    for (evidence, id) in [
        ("sub2api", "sub2api"),
        ("litellm", "litellm"),
        ("oneapi", "one_api"),
        ("newapi", "new_api"),
        ("veloera", "veloera"),
        ("omniroute", "omniroute"),
        ("metapi", "metapi"),
        ("siliconflow", "siliconflow"),
        ("perplexity", "perplexity"),
        ("cerebras", "cerebras"),
        ("novita", "novita"),
        ("minimax", "minimax"),
    ] {
        if host.contains(evidence) {
            return definitions.into_iter().find(|provider| provider.id == id);
        }
    }
    let id = if infer_interface_from_endpoint(endpoint).is_some() {
        "custom_openai_compatible"
    } else {
        "custom_http"
    };
    definitions.into_iter().find(|provider| provider.id == id)
}

pub fn infer_interface_from_endpoint(endpoint: &str) -> Option<InterfaceType> {
    let endpoint = endpoint.to_lowercase();
    // Keep in sync with OPENAI_COMPATIBLE_ENDPOINT_PATTERN in
    // packages/schemas/src/index.ts. `/v\d` covers versioned API paths such
    // as /v2, /api/paas/v4, and /v1beta.
    const OPENAI_COMPATIBLE_EVIDENCE: &[&str] = &[
        "openai",
        "chat/completions",
        "messages",
        "gateway",
        "one-api",
        "one_api",
        "one api",
        "oneapi",
        "new-api",
        "new_api",
        "new api",
        "newapi",
        "litellm",
        "sub2api",
        "openrouter",
        "veloera",
        "omniroute",
        "metapi",
        "onehub",
        "donehub",
        "anyrouter",
        "siliconflow",
        "deepseek",
        "moonshot",
        "dashscope",
        "qwen",
        "bigmodel",
        "zhipu",
        "volcengine",
        "volces",
        "ark",
        "together",
        "fireworks",
        "groq",
        "x.ai",
        "mistral",
        "perplexity",
        "cerebras",
        "nvidia",
        "nim",
        "novita",
        "huggingface",
        "hugging face",
    ];
    let has_versioned_api_path = endpoint
        .as_bytes()
        .windows(3)
        .any(|window| window[0] == b'/' && window[1] == b'v' && window[2].is_ascii_digit());
    if endpoint.contains("generativelanguage") || endpoint.contains("gemini") {
        Some(InterfaceType::Gemini)
    } else if endpoint.contains("anthropic") || endpoint.contains("claude") {
        Some(InterfaceType::AnthropicMessages)
    } else if has_versioned_api_path
        || OPENAI_COMPATIBLE_EVIDENCE
            .iter()
            .any(|keyword| endpoint.contains(keyword))
    {
        Some(InterfaceType::OpenAiCompatible)
    } else {
        None
    }
}

pub fn match_provider_by_domain(domain: &str) -> Option<ProviderDefinition> {
    let normalized = domain
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or(domain)
        .split('@')
        .next_back()
        .unwrap_or(domain)
        .split(':')
        .next()
        .unwrap_or(domain)
        .to_lowercase();
    default_provider_definitions()
        .into_iter()
        .find(|definition| {
            definition
                .domains
                .iter()
                .any(|known| normalized == *known || normalized.ends_with(&format!(".{known}")))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_provider_inference_matches_shared_schema_cases() {
        for (endpoint, id) in [
            ("https://api.openai.com/v1/models", "openai"),
            ("https://api.anthropic.com/v1/messages", "anthropic"),
            (
                "https://generativelanguage.googleapis.com/v1beta/models",
                "gemini",
            ),
            ("https://api.minimaxi.com/v1", "minimax"),
            ("https://one-api.example.test/v1", "one_api"),
            (
                "https://llm.example.test/api/paas/v4",
                "custom_openai_compatible",
            ),
            (
                "https://claude-relay.example.test/v1/messages",
                "custom_openai_compatible",
            ),
            (
                "https://gemini-proxy.example.test/v1beta/models",
                "custom_openai_compatible",
            ),
            ("https://example.test/hooks/deploy", "custom_http"),
        ] {
            assert_eq!(
                infer_provider_from_endpoint(endpoint).unwrap().id,
                id,
                "{endpoint}"
            );
        }
        assert!(infer_provider_from_endpoint("").is_none());
        assert!(infer_provider_from_endpoint("file:///tmp/example").is_none());
    }
}
