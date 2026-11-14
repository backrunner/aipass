//! UI capability projection. Agent rechecks installation and protocol on writes.
use aipass_agent_protocol::{ToolConfigMode, ToolConfigTool as Tool};
use aipass_config_writers::ToolDetection;
use aipass_provider_registry::{
    AuthScheme as Auth, CredentialKind, InterfaceType as Interface, SecretRef,
};
use aipass_vault::EntrySummary;

pub const AUTOMATIC: [(Tool, &str); 16] = [
    (Tool::Codex, "Codex"),
    (Tool::ClaudeCode, "Claude Code"),
    (Tool::GeminiCli, "Gemini CLI"),
    (Tool::OpenCode, "OpenCode"),
    (Tool::Grok, "Grok Build"),
    (Tool::Pi, "Pi"),
    (Tool::Cursor, "Cursor Agent Local"),
    (Tool::KimiCode, "Kimi Code"),
    (Tool::Zed, "Zed"),
    (Tool::DeepseekHarness, "DeepSeek Harness"),
    (Tool::AntigravityCli, "Antigravity CLI"),
    (Tool::MuseCode, "Muse Code"),
    (Tool::HermesAgent, "Hermes Agent"),
    (Tool::CommandCode, "Command Code"),
    (Tool::Codebuddy, "CodeBuddy"),
    (Tool::Workbuddy, "WorkBuddy"),
];

pub fn name(tool: &Tool) -> &'static str {
    AUTOMATIC
        .iter()
        .find(|(id, _)| id == tool)
        .map(|(_, name)| *name)
        .unwrap_or("Agent")
}
pub fn tool_id(tool: &Tool) -> aipass_config_writers::ToolId {
    use aipass_config_writers::ToolId;
    match tool {
        Tool::Codex => ToolId::Codex,
        Tool::ClaudeCode => ToolId::ClaudeCode,
        Tool::GeminiCli => ToolId::GeminiCli,
        Tool::OpenCode => ToolId::OpenCode,
        Tool::Grok => ToolId::Grok,
        Tool::Pi => ToolId::Pi,
        Tool::Cursor => ToolId::Cursor,
        Tool::KimiCode => ToolId::KimiCode,
        Tool::Zed => ToolId::Zed,
        Tool::DeepseekHarness => ToolId::DeepseekHarness,
        Tool::AntigravityCli => ToolId::AntigravityCli,
        Tool::MuseCode => ToolId::MuseCode,
        Tool::HermesAgent => ToolId::HermesAgent,
        Tool::CommandCode => ToolId::CommandCode,
        Tool::Codebuddy => ToolId::Codebuddy,
        Tool::Workbuddy => ToolId::Workbuddy,
        Tool::VscodeChat => ToolId::VscodeChat,
        Tool::Cline => ToolId::Cline,
        Tool::Trae => ToolId::Trae,
        Tool::Qoder => ToolId::Qoder,
        Tool::Zcode => ToolId::Zcode,
        Tool::JetbrainsCopilot => ToolId::JetbrainsCopilot,
    }
}
pub fn installed(tool: &Tool, detections: &[ToolDetection]) -> bool {
    detections
        .iter()
        .any(|d| d.tool == tool_id(tool) && d.is_installed())
}
pub fn default_mode(tool: &Tool) -> ToolConfigMode {
    if matches!(tool, Tool::Codex | Tool::Codebuddy | Tool::Workbuddy) {
        ToolConfigMode::Plaintext
    } else {
        ToolConfigMode::Helper
    }
}
pub fn requires_model(tool: &Tool) -> bool {
    !matches!(
        tool,
        Tool::Codex
            | Tool::ClaudeCode
            | Tool::GeminiCli
            | Tool::OpenCode
            | Tool::Cursor
            | Tool::AntigravityCli
    )
}
pub fn compatible(tool: &Tool, entry: &EntrySummary, secret: &SecretRef) -> bool {
    if entry.credential_kind != CredentialKind::Api {
        return false;
    }
    let interface = secret
        .interface_type
        .as_ref()
        .unwrap_or(&entry.interface_type);
    let auth = secret.effective_auth(&entry.interface_type, &entry.auth_scheme);
    let openai = *interface == Interface::OpenAiCompatible && auth == Auth::Bearer;
    let anthropic = *interface == Interface::AnthropicMessages && auth == Auth::XApiKey;
    let gemini = *interface == Interface::Gemini && auth == Auth::GoogleApiKey;
    match tool {
        Tool::MuseCode => {
            openai
                && secret
                    .endpoint
                    .as_deref()
                    .or_else(|| {
                        entry
                            .endpoints
                            .iter()
                            .find(|e| e.kind == aipass_provider_registry::EndpointKind::Api)
                            .and_then(|e| e.url.as_deref())
                    })
                    .is_some_and(|url| url.trim_end_matches('/') == "https://api.meta.ai/v1")
        }
        Tool::Codex | Tool::Codebuddy | Tool::Workbuddy | Tool::Cursor | Tool::JetbrainsCopilot => {
            openai
        }
        Tool::ClaudeCode => {
            anthropic || (*interface == Interface::AnthropicMessages && auth == Auth::Bearer)
        }
        Tool::GeminiCli | Tool::AntigravityCli => gemini,
        Tool::OpenCode => !(*interface == Interface::AnthropicMessages && auth != Auth::XApiKey),
        Tool::KimiCode => gemini || openai || anthropic,
        _ => openai || anthropic,
    }
}

/// Native subscription credentials must never enter API-key config writers.
pub fn native_tool(entry: &EntrySummary) -> Option<Tool> {
    if entry.credential_kind != CredentialKind::OAuth
        || entry.provider_kind != aipass_provider_registry::ProviderKind::Official
    {
        return None;
    }
    match entry.provider_id.as_deref()? {
        "codex" | "openai" => Some(Tool::Codex),
        "anthropic" => Some(Tool::ClaudeCode),
        _ => None,
    }
}
pub fn ready(tool: &Tool, entry: &EntrySummary, secret: &SecretRef) -> bool {
    compatible(tool, entry, secret)
        && (!requires_model(tool)
            || secret
                .default_model
                .as_deref()
                .or(entry.default_model.as_deref())
                .is_some_and(|model| !model.trim().is_empty()))
}

pub fn icon(tool: &Tool) -> &'static str {
    match tool {
        Tool::Codex => "tool-codex",
        Tool::ClaudeCode => "tool-claude-code",
        Tool::GeminiCli => "tool-gemini-cli",
        Tool::OpenCode => "tool-opencode",
        Tool::Grok => "tool-grok",
        Tool::Pi => "tool-pi",
        Tool::Cursor => "tool-cursor",
        Tool::KimiCode => "tool-kimi-code",
        Tool::Zed => "tool-zed",
        Tool::DeepseekHarness => "tool-deepseek-harness",
        Tool::AntigravityCli => "tool-antigravity-cli",
        Tool::CommandCode => "tool-command-code",
        Tool::Workbuddy => "tool-workbuddy",
        Tool::Cline => "tool-cline",
        Tool::Trae => "tool-trae",
        Tool::Qoder => "tool-qoder",
        _ => "terminal",
    }
}
#[cfg(test)]
mod tests;
