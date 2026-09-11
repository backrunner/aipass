use crate::cli_types::*;
use aipass_agent_protocol::{CodexApiKeyMode, ToolConfigMode, ToolConfigTool};
use aipass_config_writers::ToolId;
use aipass_provider_registry::{AuthScheme, CredentialKind, InterfaceType};
use aipass_proxy_conversion::ProxyProtocol;

impl From<InterfaceArg> for InterfaceType {
    fn from(value: InterfaceArg) -> Self {
        match value {
            InterfaceArg::OpenaiCompatible => InterfaceType::OpenAiCompatible,
            InterfaceArg::AnthropicMessages => InterfaceType::AnthropicMessages,
            InterfaceArg::Gemini => InterfaceType::Gemini,
            InterfaceArg::AzureOpenai => InterfaceType::AzureOpenAi,
            InterfaceArg::Bedrock => InterfaceType::Bedrock,
            InterfaceArg::CustomHttp => InterfaceType::CustomHttp,
        }
    }
}

impl From<CredentialKindArg> for CredentialKind {
    fn from(value: CredentialKindArg) -> Self {
        match value {
            CredentialKindArg::Api => Self::Api,
            CredentialKindArg::Oauth => Self::OAuth,
        }
    }
}

impl From<AuthArg> for AuthScheme {
    fn from(value: AuthArg) -> Self {
        match value {
            AuthArg::Bearer => AuthScheme::Bearer,
            AuthArg::XApiKey => AuthScheme::XApiKey,
            AuthArg::GoogleApiKey => AuthScheme::GoogleApiKey,
            AuthArg::AzureApiKey => AuthScheme::AzureApiKey,
            AuthArg::AwsProfile => AuthScheme::AwsProfile,
            AuthArg::CustomHeader => AuthScheme::CustomHeader,
        }
    }
}

impl From<ToolArg> for ToolConfigTool {
    fn from(value: ToolArg) -> Self {
        match value {
            ToolArg::Codex => Self::Codex,
            ToolArg::ClaudeCode => Self::ClaudeCode,
            ToolArg::GeminiCli => Self::GeminiCli,
            ToolArg::OpenCode => Self::OpenCode,
            ToolArg::Grok => Self::Grok,
            ToolArg::Pi => Self::Pi,
            ToolArg::Cursor => Self::Cursor,
        }
    }
}

impl From<ConfigureMode> for ToolConfigMode {
    fn from(value: ConfigureMode) -> Self {
        match value {
            ConfigureMode::Official => Self::Official,
            ConfigureMode::Helper => Self::Helper,
            ConfigureMode::Env => Self::Env,
            ConfigureMode::Plaintext => Self::Plaintext,
        }
    }
}

impl From<CodexApiKeyModeArg> for CodexApiKeyMode {
    fn from(value: CodexApiKeyModeArg) -> Self {
        match value {
            CodexApiKeyModeArg::ExperimentalBearerToken => Self::ExperimentalBearerToken,
            CodexApiKeyModeArg::AuthJson => Self::AuthJson,
        }
    }
}

impl From<ProxyProtocolArg> for ProxyProtocol {
    fn from(value: ProxyProtocolArg) -> Self {
        match value {
            ProxyProtocolArg::AnthropicMessages => ProxyProtocol::AnthropicMessages,
            ProxyProtocolArg::OpenAiResponses => ProxyProtocol::OpenAiResponses,
            ProxyProtocolArg::OpenAiChatCompletions => ProxyProtocol::OpenAiChatCompletions,
        }
    }
}

impl From<ToolArg> for ToolId {
    fn from(value: ToolArg) -> Self {
        match value {
            ToolArg::Codex => ToolId::Codex,
            ToolArg::ClaudeCode => ToolId::ClaudeCode,
            ToolArg::GeminiCli => ToolId::GeminiCli,
            ToolArg::OpenCode => ToolId::OpenCode,
            ToolArg::Grok => ToolId::Grok,
            ToolArg::Pi => ToolId::Pi,
            ToolArg::Cursor => ToolId::Cursor,
        }
    }
}
