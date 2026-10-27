use super::reader::{add, document, field, paths};
use super::*;
use aipass_config_writers::native_auth::PrivateJson;
use anyhow::Result;
use serde_json::json;

pub(super) fn read(home: &Path, tool: &ToolConfigTool) -> Result<Vec<DetectedCredential>> {
    let paths = paths(home, tool);
    let mut out = Vec::new();
    match tool {
        ToolConfigTool::MuseCode => {
            let settings = document(&paths[0])?;
            let auth = document(&paths[1])?;
            add(
                &mut out,
                field(&auth.0["providers"]["meta"], "api_key"),
                field(&settings.0["endpoint_transport"], "base_url")
                    .or(Some("https://api.meta.ai/v1")),
                InterfaceType::OpenAiCompatible,
                AuthScheme::Bearer,
                field(&settings.0, "model"),
            )?;
        }
        ToolConfigTool::HermesAgent => {
            let root = PrivateJson(aipass_config_writers::read_yaml(&paths[0], json!({}))?);
            let config = &root.0["model"];
            let anthropic = field(config, "api_mode") == Some("anthropic_messages");
            add(
                &mut out,
                field(config, "api_key"),
                field(config, "base_url"),
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
                field(config, "default"),
            )?;
        }
        ToolConfigTool::CommandCode => {
            let providers = document(&paths[0])?;
            // Keep the original auth store intact and in the encrypted snapshot.
            // Its OAuth/native sign-ins are not exported or executed as helpers.
            let _auth = document(&paths[2])?;
            if let Some(providers) = providers.0["provider"].as_object() {
                for provider in providers.values() {
                    if let Some(key) = field(provider, "apiKey") {
                        anyhow::ensure!(
                            key.starts_with(['$', '!', '{']) || key.is_empty(),
                            "unsupported raw Command Code key"
                        );
                    }
                }
            }
        }
        ToolConfigTool::Codebuddy | ToolConfigTool::Workbuddy => {
            let root = match std::fs::read_to_string(&paths[0]) {
                Ok(raw) => PrivateJson(
                    json5::from_str(&Zeroizing::new(raw))
                        .map_err(|_| anyhow::anyhow!("invalid editor configuration"))?,
                ),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
                Err(_) => anyhow::bail!("editor configuration unavailable"),
            };
            let models = root
                .0
                .as_array()
                .or_else(|| root.0["models"].as_array())
                .ok_or_else(|| anyhow::anyhow!("invalid editor model list"))?;
            for model in models {
                let endpoint = field(model, "url").map(|url| {
                    url.trim_end_matches('/')
                        .trim_end_matches("/chat/completions")
                });
                add(
                    &mut out,
                    field(model, "apiKey"),
                    endpoint,
                    InterfaceType::OpenAiCompatible,
                    AuthScheme::Bearer,
                    field(model, "id"),
                )?;
            }
        }
        _ => anyhow::bail!("unsupported BYOK credential reader"),
    }
    Ok(out)
}
