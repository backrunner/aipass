//! Import only literal credentials, without executing vendor helpers.
use super::reader::{add, document, env, field, paths};
use super::*;
use anyhow::{Context, Result};
use serde_json::{json, Value};

pub(super) fn read(home: &Path, tool: &ToolConfigTool) -> Result<Vec<DetectedCredential>> {
    if matches!(
        tool,
        ToolConfigTool::VscodeChat
            | ToolConfigTool::Cline
            | ToolConfigTool::Trae
            | ToolConfigTool::Qoder
            | ToolConfigTool::Zcode
            | ToolConfigTool::JetbrainsCopilot
    ) {
        return Ok(vec![]);
    }
    let files = paths(home, tool);
    let mut out = Vec::new();
    match tool {
        ToolConfigTool::MuseCode
        | ToolConfigTool::HermesAgent
        | ToolConfigTool::CommandCode
        | ToolConfigTool::Codebuddy
        | ToolConfigTool::Workbuddy => return super::byok::read(home, tool),
        ToolConfigTool::KimiCode => {
            let doc = document(&files[0])?;
            let selected = field(&doc.0, "default_model").and_then(|id| doc.0["models"].get(id));
            let selected_provider = selected.and_then(|m| field(m, "provider"));
            if let Some(providers) = doc.0["providers"].as_object() {
                let mut ordered = providers.iter().collect::<Vec<_>>();
                ordered.sort_by_key(|(id, _)| selected_provider != Some(id.as_str()));
                for (id, provider) in ordered {
                    let kind = field(provider, "type").unwrap_or("kimi");
                    let interface = match kind {
                        "anthropic" => InterfaceType::AnthropicMessages,
                        "google-genai" => InterfaceType::Gemini,
                        _ => InterfaceType::OpenAiCompatible,
                    };
                    let auth = match kind {
                        "anthropic" => AuthScheme::XApiKey,
                        "google-genai" => AuthScheme::GoogleApiKey,
                        _ => AuthScheme::Bearer,
                    };
                    let model = (selected_provider == Some(id.as_str()))
                        .then(|| selected.and_then(|m| field(m, "model")))
                        .flatten();
                    add(
                        &mut out,
                        field(provider, "api_key"),
                        field(provider, "base_url"),
                        interface,
                        auth,
                        model,
                    )?;
                }
            }
        }
        ToolConfigTool::Zed => {
            let doc = document(&files[0])?;
            let values = env(&files[1])?;
            let model = field(&doc.0["agent"]["default_model"], "model");
            for (group, interface, auth) in [
                (
                    "openai_compatible",
                    InterfaceType::OpenAiCompatible,
                    AuthScheme::Bearer,
                ),
                (
                    "anthropic_compatible",
                    InterfaceType::AnthropicMessages,
                    AuthScheme::XApiKey,
                ),
            ] {
                if let Some(provider) = doc.0["language_models"][group].get("aipass") {
                    add(
                        &mut out,
                        field(&values.0, "AIPASS_API_KEY"),
                        field(provider, "api_url"),
                        interface,
                        auth,
                        model,
                    )?;
                }
            }
        }
        ToolConfigTool::AntigravityCli => {
            let _settings = document(&files[0])?;
            let values = env(&files[1])?;
            add(
                &mut out,
                field(&values.0, "GEMINI_API_KEY"),
                field(&values.0, "GOOGLE_GEMINI_BASE_URL")
                    .or(Some("https://generativelanguage.googleapis.com")),
                InterfaceType::Gemini,
                AuthScheme::GoogleApiKey,
                None,
            )?;
        }
        ToolConfigTool::DeepseekHarness => {
            let patch = aipass_config_writers::native_auth::PrivateJson(
                aipass_config_writers::read_yaml(&files[0], json!([]))?,
            );
            let credentials = aipass_config_writers::native_auth::PrivateJson(
                aipass_config_writers::read_yaml(&files[1], json!({"version":1}))?,
            );
            anyhow::ensure!(
                credentials.0["version"] == 1,
                "unsupported Harness credential format"
            );
            let plugins = patch.0.as_array().context("invalid Harness profile")?;
            for plugin in plugins.iter().filter(|p| p["id"] == "llm-pi-ai") {
                if let Some(providers) = plugin["config"]["providers"].as_object() {
                    for provider in providers.values() {
                        let Some(reference) = field(provider, "apiKeyEnv") else {
                            continue;
                        };
                        let key = credentials.0["refs"].get(reference).and_then(Value::as_str);
                        let anthropic = field(provider, "api") == Some("anthropic-messages");
                        add(
                            &mut out,
                            key,
                            field(provider, "baseURL"),
                            if anthropic {
                                InterfaceType::AnthropicMessages
                            } else {
                                InterfaceType::OpenAiCompatible
                            },
                            if anthropic {
                                AuthScheme::XApiKey
                            } else {
                                AuthScheme::Bearer
                            },
                            provider["models"]
                                .as_array()
                                .and_then(|m| m.first())
                                .and_then(|m| field(m, "id")),
                        )?;
                    }
                }
            }
        }
        _ => anyhow::bail!("unsupported Agent credential reader"),
    }
    Ok(out)
}
