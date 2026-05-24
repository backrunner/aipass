use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Official,
    ThirdParty,
    SelfHosted,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InterfaceType {
    OpenAiCompatible,
    AnthropicMessages,
    Gemini,
    AzureOpenAi,
    Bedrock,
    CustomHttp,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthScheme {
    Bearer,
    XApiKey,
    GoogleApiKey,
    AzureApiKey,
    AwsProfile,
    CustomHeader,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EndpointKind {
    Api,
    Console,
    Auth,
    Usage,
    Custom,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderEndpoint {
    pub id: String,
    pub kind: EndpointKind,
    pub url: Option<String>,
    pub region: Option<String>,
    pub deployment: Option<String>,
    pub api_version: Option<String>,
}

impl ProviderEndpoint {
    pub fn api(url: impl Into<String>) -> Self {
        Self {
            id: "api".to_string(),
            kind: EndpointKind::Api,
            url: Some(url.into()),
            region: None,
            deployment: None,
            api_version: None,
        }
    }

    pub fn console(url: impl Into<String>) -> Self {
        Self {
            id: "console".to_string(),
            kind: EndpointKind::Console,
            url: Some(url.into()),
            region: None,
            deployment: None,
            api_version: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SecretRef {
    pub id: String,
    pub label: String,
    pub masked: String,
    pub fingerprint: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QuotaInfo {
    pub label: Option<String>,
    pub limit: Option<String>,
    pub remaining: Option<String>,
    pub reset_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderEntry {
    pub id: Uuid,
    pub title: String,
    pub provider_kind: ProviderKind,
    pub provider_id: Option<String>,
    pub domains: Vec<String>,
    pub favicon_url: Option<String>,
    pub endpoints: Vec<ProviderEndpoint>,
    pub interface_type: InterfaceType,
    pub auth_scheme: AuthScheme,
    pub secret_refs: Vec<SecretRef>,
    pub default_model: Option<String>,
    pub model_aliases: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub quota: Option<QuotaInfo>,
    pub tags: Vec<String>,
    pub environment: String,
    pub notes: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub last_used_at: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub archived_at: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderDefinition {
    pub id: &'static str,
    pub display_name: &'static str,
    pub kind: ProviderKind,
    pub domains: &'static [&'static str],
    pub interfaces: &'static [InterfaceType],
    pub auth_schemes: &'static [AuthScheme],
    pub endpoints: &'static [(&'static str, EndpointKind, &'static str)],
    pub env_keys: &'static [&'static str],
}

pub fn default_provider_definitions() -> Vec<ProviderDefinition> {
    vec![
        ProviderDefinition {
            id: "openai",
            display_name: "OpenAI",
            kind: ProviderKind::Official,
            domains: &["platform.openai.com", "api.openai.com"],
            interfaces: &[InterfaceType::OpenAiCompatible],
            auth_schemes: &[AuthScheme::Bearer],
            endpoints: &[
                ("api", EndpointKind::Api, "https://api.openai.com/v1"),
                (
                    "console",
                    EndpointKind::Console,
                    "https://platform.openai.com",
                ),
            ],
            env_keys: &["OPENAI_API_KEY"],
        },
        ProviderDefinition {
            id: "anthropic",
            display_name: "Anthropic",
            kind: ProviderKind::Official,
            domains: &["console.anthropic.com", "api.anthropic.com"],
            interfaces: &[InterfaceType::AnthropicMessages],
            auth_schemes: &[AuthScheme::XApiKey],
            endpoints: &[
                ("api", EndpointKind::Api, "https://api.anthropic.com"),
                (
                    "console",
                    EndpointKind::Console,
                    "https://console.anthropic.com",
                ),
            ],
            env_keys: &["ANTHROPIC_API_KEY"],
        },
        ProviderDefinition {
            id: "gemini",
            display_name: "Google Gemini",
            kind: ProviderKind::Official,
            domains: &["aistudio.google.com", "generativelanguage.googleapis.com"],
            interfaces: &[InterfaceType::Gemini],
            auth_schemes: &[AuthScheme::GoogleApiKey],
            endpoints: &[
                (
                    "api",
                    EndpointKind::Api,
                    "https://generativelanguage.googleapis.com",
                ),
                (
                    "console",
                    EndpointKind::Console,
                    "https://aistudio.google.com",
                ),
            ],
            env_keys: &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
        },
        ProviderDefinition {
            id: "azure_openai",
            display_name: "Azure OpenAI",
            kind: ProviderKind::Official,
            domains: &["portal.azure.com", "openai.azure.com"],
            interfaces: &[InterfaceType::AzureOpenAi],
            auth_schemes: &[AuthScheme::AzureApiKey],
            endpoints: &[("console", EndpointKind::Console, "https://portal.azure.com")],
            env_keys: &["AZURE_OPENAI_API_KEY"],
        },
        ProviderDefinition {
            id: "bedrock",
            display_name: "AWS Bedrock",
            kind: ProviderKind::Official,
            domains: &["console.aws.amazon.com", "bedrock-runtime.amazonaws.com"],
            interfaces: &[InterfaceType::Bedrock],
            auth_schemes: &[AuthScheme::AwsProfile],
            endpoints: &[(
                "console",
                EndpointKind::Console,
                "https://console.aws.amazon.com/bedrock",
            )],
            env_keys: &["AWS_PROFILE", "AWS_REGION"],
        },
        ProviderDefinition {
            id: "openrouter",
            display_name: "OpenRouter",
            kind: ProviderKind::ThirdParty,
            domains: &["openrouter.ai"],
            interfaces: &[InterfaceType::OpenAiCompatible],
            auth_schemes: &[AuthScheme::Bearer],
            endpoints: &[
                ("api", EndpointKind::Api, "https://openrouter.ai/api/v1"),
                ("console", EndpointKind::Console, "https://openrouter.ai"),
            ],
            env_keys: &["OPENROUTER_API_KEY"],
        },
        ProviderDefinition {
            id: "deepseek",
            display_name: "DeepSeek",
            kind: ProviderKind::Official,
            domains: &["platform.deepseek.com", "api.deepseek.com"],
            interfaces: &[InterfaceType::OpenAiCompatible],
            auth_schemes: &[AuthScheme::Bearer],
            endpoints: &[
                ("api", EndpointKind::Api, "https://api.deepseek.com"),
                (
                    "console",
                    EndpointKind::Console,
                    "https://platform.deepseek.com",
                ),
            ],
            env_keys: &["DEEPSEEK_API_KEY"],
        },
        ProviderDefinition {
            id: "moonshot",
            display_name: "Moonshot AI",
            kind: ProviderKind::Official,
            domains: &["platform.moonshot.cn", "api.moonshot.cn"],
            interfaces: &[InterfaceType::OpenAiCompatible],
            auth_schemes: &[AuthScheme::Bearer],
            endpoints: &[
                ("api", EndpointKind::Api, "https://api.moonshot.cn/v1"),
                (
                    "console",
                    EndpointKind::Console,
                    "https://platform.moonshot.cn",
                ),
            ],
            env_keys: &["MOONSHOT_API_KEY"],
        },
        ProviderDefinition {
            id: "qwen",
            display_name: "Alibaba Qwen",
            kind: ProviderKind::Official,
            domains: &["dashscope.console.aliyun.com", "dashscope.aliyuncs.com"],
            interfaces: &[InterfaceType::OpenAiCompatible],
            auth_schemes: &[AuthScheme::Bearer],
            endpoints: &[
                (
                    "api",
                    EndpointKind::Api,
                    "https://dashscope.aliyuncs.com/compatible-mode/v1",
                ),
                (
                    "console",
                    EndpointKind::Console,
                    "https://dashscope.console.aliyun.com",
                ),
            ],
            env_keys: &["DASHSCOPE_API_KEY", "QWEN_API_KEY"],
        },
        ProviderDefinition {
            id: "zhipu",
            display_name: "Zhipu AI",
            kind: ProviderKind::Official,
            domains: &["bigmodel.cn", "open.bigmodel.cn"],
            interfaces: &[InterfaceType::OpenAiCompatible],
            auth_schemes: &[AuthScheme::Bearer],
            endpoints: &[
                (
                    "api",
                    EndpointKind::Api,
                    "https://open.bigmodel.cn/api/paas/v4",
                ),
                ("console", EndpointKind::Console, "https://bigmodel.cn"),
            ],
            env_keys: &["ZHIPUAI_API_KEY"],
        },
        ProviderDefinition {
            id: "volcengine",
            display_name: "Volcengine Ark",
            kind: ProviderKind::Official,
            domains: &["console.volcengine.com", "ark.cn-beijing.volces.com"],
            interfaces: &[InterfaceType::OpenAiCompatible],
            auth_schemes: &[AuthScheme::Bearer],
            endpoints: &[
                (
                    "api",
                    EndpointKind::Api,
                    "https://ark.cn-beijing.volces.com/api/v3",
                ),
                (
                    "console",
                    EndpointKind::Console,
                    "https://console.volcengine.com/ark",
                ),
            ],
            env_keys: &["ARK_API_KEY", "VOLCENGINE_API_KEY"],
        },
        ProviderDefinition {
            id: "together",
            display_name: "Together AI",
            kind: ProviderKind::ThirdParty,
            domains: &["api.together.xyz", "together.ai"],
            interfaces: &[InterfaceType::OpenAiCompatible],
            auth_schemes: &[AuthScheme::Bearer],
            endpoints: &[
                ("api", EndpointKind::Api, "https://api.together.xyz/v1"),
                ("console", EndpointKind::Console, "https://api.together.xyz"),
            ],
            env_keys: &["TOGETHER_API_KEY"],
        },
        ProviderDefinition {
            id: "fireworks",
            display_name: "Fireworks AI",
            kind: ProviderKind::ThirdParty,
            domains: &["fireworks.ai", "api.fireworks.ai"],
            interfaces: &[InterfaceType::OpenAiCompatible],
            auth_schemes: &[AuthScheme::Bearer],
            endpoints: &[
                (
                    "api",
                    EndpointKind::Api,
                    "https://api.fireworks.ai/inference/v1",
                ),
                ("console", EndpointKind::Console, "https://fireworks.ai"),
            ],
            env_keys: &["FIREWORKS_API_KEY"],
        },
        ProviderDefinition {
            id: "groq",
            display_name: "Groq",
            kind: ProviderKind::ThirdParty,
            domains: &["console.groq.com", "api.groq.com"],
            interfaces: &[InterfaceType::OpenAiCompatible],
            auth_schemes: &[AuthScheme::Bearer],
            endpoints: &[
                ("api", EndpointKind::Api, "https://api.groq.com/openai/v1"),
                ("console", EndpointKind::Console, "https://console.groq.com"),
            ],
            env_keys: &["GROQ_API_KEY"],
        },
        ProviderDefinition {
            id: "new_api",
            display_name: "New API",
            kind: ProviderKind::SelfHosted,
            domains: &[],
            interfaces: &[
                InterfaceType::OpenAiCompatible,
                InterfaceType::AnthropicMessages,
                InterfaceType::Gemini,
            ],
            auth_schemes: &[AuthScheme::Bearer],
            endpoints: &[],
            env_keys: &[],
        },
        ProviderDefinition {
            id: "one_api",
            display_name: "One API",
            kind: ProviderKind::SelfHosted,
            domains: &[],
            interfaces: &[InterfaceType::OpenAiCompatible],
            auth_schemes: &[AuthScheme::Bearer],
            endpoints: &[],
            env_keys: &[],
        },
        ProviderDefinition {
            id: "litellm",
            display_name: "LiteLLM",
            kind: ProviderKind::SelfHosted,
            domains: &[],
            interfaces: &[
                InterfaceType::OpenAiCompatible,
                InterfaceType::AnthropicMessages,
                InterfaceType::Gemini,
            ],
            auth_schemes: &[AuthScheme::Bearer, AuthScheme::XApiKey],
            endpoints: &[],
            env_keys: &[],
        },
        ProviderDefinition {
            id: "sub2api",
            display_name: "sub2api",
            kind: ProviderKind::SelfHosted,
            domains: &[],
            interfaces: &[
                InterfaceType::OpenAiCompatible,
                InterfaceType::AnthropicMessages,
            ],
            auth_schemes: &[AuthScheme::Bearer],
            endpoints: &[],
            env_keys: &[],
        },
        ProviderDefinition {
            id: "custom_openai_compatible",
            display_name: "Custom OpenAI-compatible",
            kind: ProviderKind::Unknown,
            domains: &[],
            interfaces: &[InterfaceType::OpenAiCompatible],
            auth_schemes: &[AuthScheme::Bearer],
            endpoints: &[],
            env_keys: &[],
        },
        ProviderDefinition {
            id: "custom_http",
            display_name: "Custom HTTP API",
            kind: ProviderKind::Unknown,
            domains: &[],
            interfaces: &[InterfaceType::CustomHttp],
            auth_schemes: &[AuthScheme::CustomHeader],
            endpoints: &[],
            env_keys: &[],
        },
    ]
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

pub fn provider_kind_for_id(provider_id: Option<&str>) -> ProviderKind {
    provider_id
        .and_then(|id| {
            default_provider_definitions()
                .into_iter()
                .find(|definition| definition.id == id)
                .map(|definition| definition.kind)
        })
        .unwrap_or(ProviderKind::Unknown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_first_class_non_openai_providers() {
        assert_eq!(
            match_provider_by_domain("console.anthropic.com")
                .unwrap()
                .id,
            "anthropic"
        );
        assert_eq!(
            match_provider_by_domain("aistudio.google.com").unwrap().id,
            "gemini"
        );
    }

    #[test]
    fn domain_matching_requires_label_boundary() {
        assert!(match_provider_by_domain("evilconsole.anthropic.com.attacker.test").is_none());
        assert!(match_provider_by_domain("notopenai.com").is_none());
        assert_eq!(
            match_provider_by_domain("https://team.console.anthropic.com/settings")
                .unwrap()
                .id,
            "anthropic"
        );
    }

    #[test]
    fn classifies_custom_providers_as_unknown() {
        assert_eq!(
            provider_kind_for_id(Some("custom_openai_compatible")),
            ProviderKind::Unknown
        );
        assert_eq!(
            provider_kind_for_id(Some("custom_http")),
            ProviderKind::Unknown
        );
        assert_eq!(provider_kind_for_id(None), ProviderKind::Unknown);
    }

    #[test]
    fn classifies_official_third_party_and_self_hosted_providers() {
        assert_eq!(
            provider_kind_for_id(Some("anthropic")),
            ProviderKind::Official
        );
        assert_eq!(
            provider_kind_for_id(Some("openrouter")),
            ProviderKind::ThirdParty
        );
        assert_eq!(
            provider_kind_for_id(Some("sub2api")),
            ProviderKind::SelfHosted
        );
        assert_eq!(
            provider_kind_for_id(Some("unknown_future_provider")),
            ProviderKind::Unknown
        );
    }
}
