//! Tool configuration plans and proxy-route integration.
use super::*;

pub(crate) fn build_tool_config_plan(
    vault: &Vault,
    request: &ToolConfigRequest,
    preview: bool,
) -> ServiceResult<(EntrySummary, ConfigPlan, String)> {
    if request.codex_api_key_mode.is_some()
        && (!matches!(request.tool, ToolConfigTool::Codex)
            || !matches!(request.mode, ToolConfigMode::Plaintext))
    {
        return Err(ServiceError::new(
            AgentErrorCode::ValidationFailed,
            "codex_api_key_mode requires tool=codex and mode=plaintext",
        ));
    }
    let mut entry = vault
        .get_provider_summary(request.id)
        .map_err(map_vault_error)?;
    if !matches!(request.mode, ToolConfigMode::Official)
        && (crate::community::is_account(vault, entry.id)
            || (entry.provider_kind == ProviderKind::Official
                && entry.credential_kind == CredentialKind::OAuth
                && matches!(
                    entry.provider_id.as_deref(),
                    Some(
                        "anthropic"
                            | "codex"
                            | "openai"
                            | "grok"
                            | "xai"
                            | "copilot"
                            | "gemini-cli"
                    )
                )))
    {
        return Err(ServiceError::new(
            AgentErrorCode::ValidationFailed,
            "CLI and community subscriptions must be used through a local proxy route",
        ));
    }
    if request.mode == ToolConfigMode::Official {
        crate::tool_switch::validate_reference(vault, entry.id, &request.tool)?;
    }
    let secret = match request.secret_id.as_deref() {
        Some(id) => entry
            .secret_refs
            .iter()
            .find(|secret| secret.id == id)
            .ok_or_else(|| {
                ServiceError::new(
                    AgentErrorCode::NotFound,
                    "selected credential no longer exists",
                )
            })?,
        None if entry.secret_refs.len() == 1 => &entry.secret_refs[0],
        _ => {
            return Err(ServiceError::new(
                AgentErrorCode::ValidationFailed,
                "select a credential by secret_id before configuring this tool",
            ))
        }
    }
    .clone();
    entry.auth_scheme = secret.effective_auth(&entry.interface_type, &entry.auth_scheme);
    entry.interface_type = secret
        .interface_type
        .clone()
        .unwrap_or(entry.interface_type);
    entry.title = format!("{} · {}", entry.title, secret.label);
    if !matches!(request.mode, ToolConfigMode::Official) {
        ensure_tool_credential(&request.tool, &entry)?;
    }
    if matches!(request.mode, ToolConfigMode::Official) {
        // Official mode reuses the tool's own CLI credential store, so the
        // vault entry must be that provider's official OAuth account.
        let expected_provider = match request.tool {
            ToolConfigTool::Codex => "openai",
            ToolConfigTool::ClaudeCode => "anthropic",
            _ => "",
        };
        if expected_provider.is_empty()
            || !matches!(&entry.provider_kind, ProviderKind::Official)
            || !matches!(&entry.credential_kind, CredentialKind::OAuth)
            || !(entry.provider_id.as_deref() == Some(expected_provider)
                || (expected_provider == "openai" && entry.provider_id.as_deref() == Some("codex")))
        {
            return Err(ServiceError::new(
                AgentErrorCode::ValidationFailed,
                "official mode requires the tool's own official OAuth account",
            ));
        }
    }
    if matches!(request.mode, ToolConfigMode::Plaintext)
        && matches!(
            request.tool,
            ToolConfigTool::Codex | ToolConfigTool::ClaudeCode
        )
        && !matches!(&entry.credential_kind, CredentialKind::Api)
    {
        return Err(ServiceError::new(
            AgentErrorCode::ValidationFailed,
            "API key mode requires an API credential",
        ));
    }
    let home = home_dir(vault)?;
    let mut tool_entry = ToolEntry {
        supports_websockets: entry.supports_websockets,
        id: entry.id,
        secret_id: Some(secret.id.clone()),
        title: entry.title.clone(),
        provider_id: entry.provider_id.clone(),
        endpoint: secret
            .endpoint
            .clone()
            .or_else(|| endpoint_url(&entry.endpoints)),
        interface_type: entry.interface_type.clone(),
        auth_scheme: entry.auth_scheme.clone(),
        env_key: match &request.tool {
            ToolConfigTool::ClaudeCode if matches!(&entry.auth_scheme, AuthScheme::Bearer) => {
                "ANTHROPIC_AUTH_TOKEN".to_string()
            }
            ToolConfigTool::ClaudeCode => "ANTHROPIC_API_KEY".to_string(),
            ToolConfigTool::GeminiCli => "GEMINI_API_KEY".to_string(),
            // Tools with configurable env names must not share one provider
            // variable across independently selectable key configurations.
            _ if matches!(request.mode, ToolConfigMode::Helper) => format!(
                "AIPASS_KEY_{}_{}",
                entry.id.simple(),
                secret
                    .id
                    .chars()
                    .map(|ch| if ch.is_ascii_alphanumeric() {
                        ch.to_ascii_uppercase()
                    } else {
                        '_'
                    })
                    .collect::<String>()
            ),
            _ => env_key_for_entry(&entry),
        },
        default_model: secret
            .default_model
            .clone()
            .or_else(|| entry.default_model.clone()),
        api_key: None,
    };
    if matches!(request.mode, ToolConfigMode::Official)
        && matches!(request.tool, ToolConfigTool::Codex)
    {
        tool_entry.provider_id = Some("openai".into());
        tool_entry.endpoint = Some("https://chatgpt.com/backend-api/codex".into());
    }
    if matches!(request.mode, ToolConfigMode::Plaintext) {
        tool_entry.api_key = Some(
            vault
                .reveal_secret_by_id(entry.id, &secret.id)
                .map_err(map_vault_error)?,
        );
    }
    let (mut plan, content) = match (&request.tool, &request.mode) {
        (ToolConfigTool::Codex, ToolConfigMode::Official) => (if preview {
            aipass_config_writers::preview_codex_official
        } else {
            plan_codex_official
        })(&home, &tool_entry)
        .map_err(ServiceError::internal)?,
        (ToolConfigTool::Codex, ToolConfigMode::Helper) => (if preview {
            aipass_config_writers::preview_codex
        } else {
            plan_codex
        })(&home, &tool_entry)
        .map_err(ServiceError::internal)?,
        (ToolConfigTool::Codex, ToolConfigMode::Env) => {
            plan_tool_env_helper(&home, ToolConfigTool::Codex, &tool_entry)?
        }
        (ToolConfigTool::Codex, ToolConfigMode::Plaintext) => {
            let mode = request
                .codex_api_key_mode
                .as_ref()
                .map(|mode| match mode {
                    CodexApiKeyMode::ExperimentalBearerToken => {
                        WriterCodexApiKeyMode::ExperimentalBearerToken
                    }
                    CodexApiKeyMode::AuthJson => WriterCodexApiKeyMode::AuthJson,
                })
                .unwrap_or(WriterCodexApiKeyMode::AuthJson);
            (if preview {
                aipass_config_writers::preview_codex_plaintext_with_mode
            } else {
                plan_codex_plaintext_with_mode
            })(&home, &tool_entry, mode)
            .map_err(ServiceError::internal)?
        }
        (ToolConfigTool::ClaudeCode, ToolConfigMode::Helper) => {
            plan_claude_code(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (ToolConfigTool::ClaudeCode, ToolConfigMode::Official) => {
            plan_claude_code_official(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (ToolConfigTool::ClaudeCode, ToolConfigMode::Env) => {
            plan_tool_env_helper(&home, ToolConfigTool::ClaudeCode, &tool_entry)?
        }
        (ToolConfigTool::ClaudeCode, ToolConfigMode::Plaintext) => {
            plan_claude_code_plaintext(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (ToolConfigTool::GeminiCli, ToolConfigMode::Helper) => {
            plan_gemini_cli(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (ToolConfigTool::GeminiCli, ToolConfigMode::Env) => {
            plan_tool_env_helper(&home, ToolConfigTool::GeminiCli, &tool_entry)?
        }
        (ToolConfigTool::GeminiCli, ToolConfigMode::Plaintext) => {
            plan_gemini_cli_plaintext(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (ToolConfigTool::OpenCode, ToolConfigMode::Helper) => {
            plan_opencode(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (ToolConfigTool::OpenCode, ToolConfigMode::Env) => {
            plan_tool_env_helper(&home, ToolConfigTool::OpenCode, &tool_entry)?
        }
        (ToolConfigTool::OpenCode, ToolConfigMode::Plaintext) => {
            plan_opencode_plaintext(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (ToolConfigTool::Grok, ToolConfigMode::Helper) => {
            plan_grok(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (ToolConfigTool::Grok, ToolConfigMode::Env) => {
            plan_tool_env_helper(&home, ToolConfigTool::Grok, &tool_entry)?
        }
        (ToolConfigTool::Grok, ToolConfigMode::Plaintext) => {
            plan_grok_plaintext(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (ToolConfigTool::Pi, ToolConfigMode::Helper) => {
            plan_pi(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (ToolConfigTool::Pi, ToolConfigMode::Env) => {
            plan_tool_env_helper(&home, ToolConfigTool::Pi, &tool_entry)?
        }
        (ToolConfigTool::Pi, ToolConfigMode::Plaintext) => {
            plan_pi_plaintext(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (
            ToolConfigTool::KimiCode
            | ToolConfigTool::Zed
            | ToolConfigTool::DeepseekHarness
            | ToolConfigTool::AntigravityCli
            | ToolConfigTool::MuseCode
            | ToolConfigTool::HermesAgent
            | ToolConfigTool::CommandCode
            | ToolConfigTool::Codebuddy
            | ToolConfigTool::Workbuddy
            | ToolConfigTool::VscodeChat
            | ToolConfigTool::Cline
            | ToolConfigTool::Trae
            | ToolConfigTool::Qoder
            | ToolConfigTool::Zcode
            | ToolConfigTool::JetbrainsCopilot,
            ToolConfigMode::Helper | ToolConfigMode::Plaintext,
        ) => aipass_config_writers::plan_additional_tool(
            &home,
            tool_id_for(&request.tool),
            &tool_entry,
            request.mode == ToolConfigMode::Plaintext,
            None,
        )
        .map_err(ServiceError::internal)?,
        (
            ToolConfigTool::KimiCode
            | ToolConfigTool::Zed
            | ToolConfigTool::DeepseekHarness
            | ToolConfigTool::AntigravityCli
            | ToolConfigTool::MuseCode
            | ToolConfigTool::HermesAgent
            | ToolConfigTool::CommandCode
            | ToolConfigTool::Codebuddy
            | ToolConfigTool::Workbuddy
            | ToolConfigTool::VscodeChat
            | ToolConfigTool::Cline
            | ToolConfigTool::Trae
            | ToolConfigTool::Qoder
            | ToolConfigTool::Zcode
            | ToolConfigTool::JetbrainsCopilot,
            ToolConfigMode::Env,
        ) => {
            return Err(ServiceError::new(
                AgentErrorCode::ValidationFailed,
                "use helper mode to configure this Agent and its environment together",
            ));
        }
        (ToolConfigTool::Cursor, ToolConfigMode::Plaintext) => {
            plan_cursor_local_plaintext(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (ToolConfigTool::Cursor, ToolConfigMode::Helper | ToolConfigMode::Env) => {
            plan_cursor_local(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        _ => {
            return Err(ServiceError::new(
                AgentErrorCode::ValidationFailed,
                "official mode is only supported for Codex and Claude Code",
            ))
        }
    };
    if matches!(request.mode, ToolConfigMode::Helper)
        && matches!(
            request.tool,
            ToolConfigTool::Codex
                | ToolConfigTool::OpenCode
                | ToolConfigTool::Grok
                | ToolConfigTool::Pi
        )
    {
        let (env_plan, env_content) =
            plan_tool_env_helper(&home, request.tool.clone(), &tool_entry)?;
        plan.extra_writes.push(aipass_config_writers::PlannedWrite {
            target_path: env_plan.target_path,
            backup_path: env_plan.backup_path,
            content: env_content,
        });
    }
    let content = if request.tool == ToolConfigTool::Codex && request.mode != ToolConfigMode::Env {
        aipass_config_writers::prepare_native_codex_plan(&mut plan, &content)
            .map_err(ServiceError::internal)?
    } else {
        content
    };
    Ok((entry, plan, content))
}

pub(super) fn ensure_tool_credential(
    tool: &ToolConfigTool,
    entry: &EntrySummary,
) -> ServiceResult<()> {
    let openai = entry.interface_type == InterfaceType::OpenAiCompatible
        && entry.auth_scheme == AuthScheme::Bearer;
    let anthropic = entry.interface_type == InterfaceType::AnthropicMessages
        && matches!(entry.auth_scheme, AuthScheme::Bearer | AuthScheme::XApiKey);
    if anthropic && entry.auth_scheme == AuthScheme::Bearer && *tool != ToolConfigTool::ClaudeCode {
        return Err(ServiceError::new(AgentErrorCode::ValidationFailed, "use an AIPass proxy route for bearer-authenticated Anthropic credentials with this tool"));
    }
    let compatible = match tool {
        ToolConfigTool::Codex => openai,
        ToolConfigTool::ClaudeCode => anthropic,
        ToolConfigTool::GeminiCli | ToolConfigTool::AntigravityCli => {
            entry.interface_type == InterfaceType::Gemini
                && entry.auth_scheme == AuthScheme::GoogleApiKey
        }
        ToolConfigTool::Cursor => openai,
        ToolConfigTool::Grok
        | ToolConfigTool::Pi
        | ToolConfigTool::Zed
        | ToolConfigTool::DeepseekHarness => openai || anthropic,
        ToolConfigTool::KimiCode => {
            openai
                || anthropic
                || (entry.interface_type == InterfaceType::Gemini
                    && entry.auth_scheme == AuthScheme::GoogleApiKey)
        }
        ToolConfigTool::MuseCode | ToolConfigTool::Codebuddy | ToolConfigTool::Workbuddy => openai,
        ToolConfigTool::HermesAgent
        | ToolConfigTool::CommandCode
        | ToolConfigTool::VscodeChat
        | ToolConfigTool::Cline
        | ToolConfigTool::Trae
        | ToolConfigTool::Qoder
        | ToolConfigTool::Zcode => openai || anthropic,
        ToolConfigTool::JetbrainsCopilot => openai,
        ToolConfigTool::OpenCode => true,
    };
    if !compatible {
        return Err(ServiceError::new(
            AgentErrorCode::ValidationFailed,
            "selected credential's API format is incompatible with this tool",
        ));
    }
    Ok(())
}

pub(crate) fn build_tool_config_proxy_plan(
    vault: &Vault,
    state: &Arc<AgentState>,
    request: &ToolConfigProxyRequest,
    preview: bool,
) -> ServiceResult<(ToolEntry, ConfigPlan, String)> {
    let (bind_addr, route) = {
        let mut proxy = state
            .proxy
            .lock()
            .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "proxy lock poisoned"))?;
        let config = proxy.config(vault)?;
        let route = config
            .routes
            .iter()
            .find(|route| route.id == request.route_id)
            .cloned()
            .ok_or_else(|| ServiceError::new(AgentErrorCode::NotFound, "proxy route not found"))?;
        (config.bind_addr.clone(), route)
    };
    if route.token.is_empty() {
        return Err(ServiceError::new(
            AgentErrorCode::ValidationFailed,
            "proxy route has no local token; rotate the route token first",
        ));
    }
    if !route.enabled {
        return Err(ServiceError::new(
            AgentErrorCode::ValidationFailed,
            "cannot configure a disabled proxy route",
        ));
    }
    ensure_proxy_tool_protocol(&request.tool, route.inbound_protocol)?;
    let tool_bind_addr = advertised_bind_addr(&bind_addr);
    let endpoint = proxy_endpoint_for_tool(&request.tool, route.inbound_protocol, &tool_bind_addr);
    let anthropic = route.inbound_protocol == ProxyProtocol::AnthropicMessages;
    let default_model = route.group_model_id().or_else(|| {
        route
            .targets
            .iter()
            .filter(|target| target.enabled)
            .find_map(|target| {
                let entry = vault.get_provider_summary(target.provider_entry_id).ok()?;
                let secret = entry
                    .secret_refs
                    .iter()
                    .find(|secret| secret.id == target.secret_id)?;
                secret
                    .default_model
                    .clone()
                    .or(entry.default_model)
                    .filter(|model| !model.trim().is_empty())
            })
    });
    if matches!(
        request.tool,
        ToolId::Grok
            | ToolId::Pi
            | ToolId::KimiCode
            | ToolId::Zed
            | ToolId::DeepseekHarness
            | ToolId::MuseCode
            | ToolId::HermesAgent
            | ToolId::CommandCode
            | ToolId::Codebuddy
            | ToolId::Workbuddy
            | ToolId::VscodeChat
            | ToolId::Cline
            | ToolId::Trae
            | ToolId::Qoder
            | ToolId::Zcode
            | ToolId::JetbrainsCopilot
    ) && default_model.is_none()
    {
        return Err(ServiceError::new(
            AgentErrorCode::ValidationFailed,
            "Agent integration requires a default model on a route credential",
        ));
    }
    let tool_entry = ToolEntry {
        supports_websockets: Some(true),
        id: route.id,
        secret_id: None,
        title: route.name.clone(),
        provider_id: None,
        endpoint: Some(endpoint),
        interface_type: if anthropic {
            InterfaceType::AnthropicMessages
        } else {
            InterfaceType::OpenAiCompatible
        },
        auth_scheme: if anthropic {
            AuthScheme::XApiKey
        } else {
            AuthScheme::Bearer
        },
        env_key: "AIPASS_PROXY_TOKEN".to_string(),
        default_model,
        api_key: Some(route.token.clone()),
    };
    let home = home_dir(vault)?;
    let (mut plan, content) = match request.tool {
        ToolId::Codex => {
            (if preview { aipass_config_writers::preview_codex_plaintext_with_mode } else { plan_codex_plaintext_with_mode })(&home, &tool_entry, WriterCodexApiKeyMode::AuthJson).map_err(ServiceError::internal)?
        }
        ToolId::ClaudeCode => {
            plan_claude_code_plaintext(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        ToolId::GeminiCli | ToolId::AntigravityCli => return Err(ServiceError::new(
            AgentErrorCode::ValidationFailed,
            "Gemini CLI requires a Gemini-native model endpoint; the AIPass local proxy currently exposes OpenAI Responses, OpenAI Chat Completions, and Anthropic Messages",
        )),
        ToolId::OpenCode => {
            let api = match route.inbound_protocol {
                ProxyProtocol::OpenAiResponses => OpenCodeApi::OpenAiResponses,
                ProxyProtocol::OpenAiChatCompletions => OpenCodeApi::OpenAiChatCompletions,
                ProxyProtocol::AnthropicMessages => OpenCodeApi::AnthropicMessages,
            };
            plan_opencode_plaintext_with_api(&home, &tool_entry, api)
                .map_err(ServiceError::internal)?
        }
        ToolId::Grok => {
            let backend = match route.inbound_protocol {
                ProxyProtocol::OpenAiResponses => GrokApiBackend::Responses,
                ProxyProtocol::OpenAiChatCompletions => GrokApiBackend::ChatCompletions,
                ProxyProtocol::AnthropicMessages => GrokApiBackend::Messages,
            };
            plan_grok_plaintext_with_backend(&home, &tool_entry, backend)
                .map_err(ServiceError::internal)?
        }
        ToolId::Pi => {
            let api = match route.inbound_protocol {
                ProxyProtocol::OpenAiResponses => PiApi::OpenAiResponses,
                ProxyProtocol::OpenAiChatCompletions => PiApi::OpenAiCompletions,
                ProxyProtocol::AnthropicMessages => PiApi::AnthropicMessages,
            };
            plan_pi_plaintext_with_api(&home, &tool_entry, api)
                .map_err(ServiceError::internal)?
        }
        ToolId::KimiCode | ToolId::Zed | ToolId::DeepseekHarness | ToolId::MuseCode | ToolId::HermesAgent | ToolId::CommandCode | ToolId::Codebuddy | ToolId::Workbuddy | ToolId::VscodeChat | ToolId::Cline | ToolId::Trae | ToolId::Qoder | ToolId::Zcode | ToolId::JetbrainsCopilot => {
            let api = match route.inbound_protocol {
                ProxyProtocol::OpenAiResponses => aipass_config_writers::ModelApi::Responses,
                ProxyProtocol::OpenAiChatCompletions => aipass_config_writers::ModelApi::ChatCompletions,
                ProxyProtocol::AnthropicMessages => aipass_config_writers::ModelApi::Messages,
            };
            aipass_config_writers::plan_additional_tool(&home, request.tool.clone(), &tool_entry, true, Some(api)).map_err(ServiceError::internal)?
        }
        ToolId::Cursor => {
            aipass_config_writers::plan_cursor_local_proxy_plaintext(&home, &tool_entry)
                .map_err(ServiceError::internal)?
        }
    };
    let content = if request.tool == ToolId::Codex {
        aipass_config_writers::prepare_native_codex_plan(&mut plan, &content)
            .map_err(ServiceError::internal)?
    } else {
        content
    };
    Ok((tool_entry, plan, content))
}

pub(super) fn ensure_proxy_tool_protocol(
    tool: &ToolId,
    protocol: ProxyProtocol,
) -> ServiceResult<()> {
    let supported = match tool {
        ToolId::Codex => protocol == ProxyProtocol::OpenAiResponses,
        ToolId::ClaudeCode => protocol == ProxyProtocol::AnthropicMessages,
        ToolId::OpenCode
        | ToolId::Grok
        | ToolId::Pi
        | ToolId::KimiCode
        | ToolId::Zed
        | ToolId::DeepseekHarness => true,
        ToolId::GeminiCli | ToolId::AntigravityCli => {
            return Err(ServiceError::new(
                AgentErrorCode::ValidationFailed,
                "Gemini CLI requires a Gemini-native model endpoint; the AIPass local proxy currently exposes OpenAI Responses, OpenAI Chat Completions, and Anthropic Messages",
            ));
        }
        ToolId::MuseCode => {
            return Err(ServiceError::new(AgentErrorCode::ValidationFailed, "Muse Code requires a Meta model catalog; ordinary Responses routes are insufficient"));
        }
        ToolId::Codebuddy | ToolId::Workbuddy => protocol == ProxyProtocol::OpenAiChatCompletions,
        ToolId::HermesAgent | ToolId::CommandCode | ToolId::VscodeChat | ToolId::Cline => true,
        ToolId::Trae => protocol != ProxyProtocol::OpenAiResponses,
        ToolId::Qoder => true,
        ToolId::Zcode => protocol != ProxyProtocol::OpenAiResponses,
        ToolId::JetbrainsCopilot => protocol == ProxyProtocol::OpenAiChatCompletions,
        ToolId::Cursor => true,
    };
    if supported {
        Ok(())
    } else {
        Err(ServiceError::new(
            AgentErrorCode::ValidationFailed,
            "the selected tool cannot speak the route's inbound model API protocol",
        ))
    }
}

/// SDKs disagree about whether an Anthropic base URL includes `/v1`.
/// Claude Code appends the versioned path itself, while the AI SDK providers
/// used by OpenCode and Grok expect the versioned base URL. Other SDK writers
/// normalize their Anthropic base for both direct credentials and proxy routes.
pub(super) fn proxy_endpoint_for_tool(
    tool: &ToolId,
    protocol: ProxyProtocol,
    bind_addr: &str,
) -> String {
    let origin = format!("http://{bind_addr}");
    match (tool, protocol) {
        (ToolId::ClaudeCode, ProxyProtocol::AnthropicMessages) => origin,
        _ => format!("{origin}/v1"),
    }
}

pub(crate) fn tool_config_preview_files(
    plan: &ConfigPlan,
    content: &str,
) -> Vec<ToolConfigPreviewFile> {
    std::iter::once((&plan.target_path, content))
        .chain(
            plan.extra_writes
                .iter()
                .map(|w| (&w.target_path, w.content.as_str())),
        )
        .map(|(path, content)| {
            // Redact complete structured documents before constructing line diffs.
            let before = zeroize::Zeroizing::new(std::fs::read_to_string(path).unwrap_or_default());
            let before = crate::tool_switch::redact_config(&before);
            let content = crate::tool_switch::redact_config(content);
            ToolConfigPreviewFile {
                path: path.display().to_string(),
                diff: aipass_config_writers::diff_preview_from(&before, &content),
                content,
            }
        })
        .collect()
}

pub(crate) fn combined_tool_config_preview(files: &[ToolConfigPreviewFile]) -> String {
    if files.len() == 1 {
        return files[0].diff.clone();
    }
    files
        .iter()
        .map(|file| format!("--- {}\n{}", file.path, file.diff))
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(crate) fn advertised_bind_addr(bind_addr: &str) -> String {
    bind_addr
        .parse::<std::net::SocketAddr>()
        .ok()
        .and_then(|addr| {
            addr.ip().is_unspecified().then(|| match addr.ip() {
                IpAddr::V4(_) => format!("127.0.0.1:{}", addr.port()),
                IpAddr::V6(_) => format!("[::1]:{}", addr.port()),
            })
        })
        .unwrap_or_else(|| bind_addr.to_string())
}

pub(crate) fn tool_config_tool_for(tool: &ToolId) -> ToolConfigTool {
    match tool {
        ToolId::Codex => ToolConfigTool::Codex,
        ToolId::ClaudeCode => ToolConfigTool::ClaudeCode,
        ToolId::GeminiCli => ToolConfigTool::GeminiCli,
        ToolId::OpenCode => ToolConfigTool::OpenCode,
        ToolId::Grok => ToolConfigTool::Grok,
        ToolId::Pi => ToolConfigTool::Pi,
        ToolId::Cursor => ToolConfigTool::Cursor,
        ToolId::KimiCode => ToolConfigTool::KimiCode,
        ToolId::Zed => ToolConfigTool::Zed,
        ToolId::DeepseekHarness => ToolConfigTool::DeepseekHarness,
        ToolId::AntigravityCli => ToolConfigTool::AntigravityCli,
        ToolId::MuseCode => ToolConfigTool::MuseCode,
        ToolId::HermesAgent => ToolConfigTool::HermesAgent,
        ToolId::CommandCode => ToolConfigTool::CommandCode,
        ToolId::Codebuddy => ToolConfigTool::Codebuddy,
        ToolId::Workbuddy => ToolConfigTool::Workbuddy,
        ToolId::VscodeChat => ToolConfigTool::VscodeChat,
        ToolId::Cline => ToolConfigTool::Cline,
        ToolId::Trae => ToolConfigTool::Trae,
        ToolId::Qoder => ToolConfigTool::Qoder,
        ToolId::Zcode => ToolConfigTool::Zcode,
        ToolId::JetbrainsCopilot => ToolConfigTool::JetbrainsCopilot,
    }
}

pub(crate) fn tool_apply_response(
    request: ToolConfigRequest,
    entry: EntrySummary,
    plan: ConfigPlan,
    result: ApplyResult,
) -> ToolConfigApplyResponse {
    ToolConfigApplyResponse {
        outcome: aipass_agent_protocol::ToolConfigOutcome::Applied,
        message: None,
        tool: request.tool,
        mode: request.mode,
        entry_id: entry.id,
        entry_title: entry.title,
        operation_id: result.operation_id,
        target_path: result.target_path.display().to_string(),
        backup_path: result.backup_path.display().to_string(),
        summary: plan.summary,
    }
}

pub(super) fn env_key_for_entry(item: &EntrySummary) -> String {
    match item.provider_id.as_deref() {
        Some("anthropic") => "ANTHROPIC_API_KEY".to_string(),
        Some("gemini") => "GEMINI_API_KEY".to_string(),
        Some("openrouter") => "OPENROUTER_API_KEY".to_string(),
        Some("deepseek") => "DEEPSEEK_API_KEY".to_string(),
        Some("moonshot") => "MOONSHOT_API_KEY".to_string(),
        Some("qwen") => "DASHSCOPE_API_KEY".to_string(),
        Some("zhipu") => "ZHIPUAI_API_KEY".to_string(),
        Some("volcengine") => "ARK_API_KEY".to_string(),
        Some("groq") => "GROQ_API_KEY".to_string(),
        Some("replicate") => "REPLICATE_API_TOKEN".to_string(),
        Some("together") => "TOGETHER_API_KEY".to_string(),
        Some("fireworks") => "FIREWORKS_API_KEY".to_string(),
        _ => match item.auth_scheme {
            AuthScheme::GoogleApiKey => "GEMINI_API_KEY".to_string(),
            AuthScheme::AzureApiKey => "AZURE_OPENAI_API_KEY".to_string(),
            _ => "AIPASS_API_KEY".to_string(),
        },
    }
}

pub(super) fn plan_tool_env_helper(
    home: &Path,
    tool: ToolConfigTool,
    entry: &ToolEntry,
) -> ServiceResult<(ConfigPlan, String)> {
    let tool_id = match tool {
        ToolConfigTool::Codex => aipass_config_writers::ToolId::Codex,
        ToolConfigTool::ClaudeCode => aipass_config_writers::ToolId::ClaudeCode,
        ToolConfigTool::GeminiCli => aipass_config_writers::ToolId::GeminiCli,
        ToolConfigTool::OpenCode => aipass_config_writers::ToolId::OpenCode,
        ToolConfigTool::Grok => aipass_config_writers::ToolId::Grok,
        ToolConfigTool::Pi => aipass_config_writers::ToolId::Pi,
        ToolConfigTool::Cursor => aipass_config_writers::ToolId::Cursor,
        ToolConfigTool::KimiCode => aipass_config_writers::ToolId::KimiCode,
        ToolConfigTool::Zed => aipass_config_writers::ToolId::Zed,
        ToolConfigTool::DeepseekHarness => aipass_config_writers::ToolId::DeepseekHarness,
        ToolConfigTool::AntigravityCli => aipass_config_writers::ToolId::AntigravityCli,
        ToolConfigTool::MuseCode => aipass_config_writers::ToolId::MuseCode,
        ToolConfigTool::HermesAgent => aipass_config_writers::ToolId::HermesAgent,
        ToolConfigTool::CommandCode => aipass_config_writers::ToolId::CommandCode,
        ToolConfigTool::Codebuddy => aipass_config_writers::ToolId::Codebuddy,
        ToolConfigTool::Workbuddy => aipass_config_writers::ToolId::Workbuddy,
        ToolConfigTool::VscodeChat => aipass_config_writers::ToolId::VscodeChat,
        ToolConfigTool::Cline => aipass_config_writers::ToolId::Cline,
        ToolConfigTool::Trae => aipass_config_writers::ToolId::Trae,
        ToolConfigTool::Qoder => aipass_config_writers::ToolId::Qoder,
        ToolConfigTool::Zcode => aipass_config_writers::ToolId::Zcode,
        ToolConfigTool::JetbrainsCopilot => aipass_config_writers::ToolId::JetbrainsCopilot,
    };
    let tool_name = match tool {
        ToolConfigTool::Codex => "codex",
        ToolConfigTool::ClaudeCode => "claude-code",
        ToolConfigTool::GeminiCli => "gemini-cli",
        ToolConfigTool::OpenCode => "opencode",
        ToolConfigTool::Grok => "grok",
        ToolConfigTool::Pi => "pi",
        ToolConfigTool::Cursor => "cursor",
        ToolConfigTool::KimiCode => "kimi-code",
        ToolConfigTool::Zed => "zed",
        ToolConfigTool::DeepseekHarness => "deepseek-harness",
        ToolConfigTool::AntigravityCli => "antigravity-cli",
        ToolConfigTool::MuseCode => "muse-code",
        ToolConfigTool::HermesAgent => "hermes-agent",
        ToolConfigTool::CommandCode => "command-code",
        ToolConfigTool::Codebuddy => "codebuddy",
        ToolConfigTool::Workbuddy => "workbuddy",
        ToolConfigTool::VscodeChat => "vscode-chat",
        ToolConfigTool::Cline => "cline",
        ToolConfigTool::Trae => "trae",
        ToolConfigTool::Qoder => "qoder",
        ToolConfigTool::Zcode => "zcode",
        ToolConfigTool::JetbrainsCopilot => "jetbrains-copilot",
    };
    let target = aipass_config_writers::helper_path(home, tool_name);
    let operation_id = Uuid::new_v4();
    let backup_path = config_backup_path(&target);
    let env_key = match tool {
        ToolConfigTool::ClaudeCode if matches!(&entry.auth_scheme, AuthScheme::Bearer) => {
            "ANTHROPIC_AUTH_TOKEN"
        }
        ToolConfigTool::ClaudeCode => "ANTHROPIC_API_KEY",
        ToolConfigTool::GeminiCli => "GEMINI_API_KEY",
        _ => entry.env_key.as_str(),
    };
    let mut content = aipass_config_writers::shell_env(entry, env_key, None);
    if let Some(endpoint) = &entry.endpoint {
        let endpoint_key = match tool {
            ToolConfigTool::ClaudeCode => "ANTHROPIC_BASE_URL",
            ToolConfigTool::GeminiCli => "GOOGLE_GEMINI_BASE_URL",
            _ => "AIPASS_BASE_URL",
        };
        content.push_str(&aipass_config_writers::env_literal(endpoint_key, endpoint));
    }
    if let Some(model) = &entry.default_model {
        let model_key = match tool {
            ToolConfigTool::ClaudeCode => Some("ANTHROPIC_MODEL"),
            ToolConfigTool::GeminiCli => Some("GEMINI_MODEL"),
            _ => None,
        };
        if let Some(model_key) = model_key {
            content.push_str(&aipass_config_writers::env_literal(model_key, model));
        }
    }
    let plan = ConfigPlan {
        operation_id,
        tool: tool_id,
        target_path: target.clone(),
        backup_path,
        summary: format!("Configure {tool_name} env helper for {}", entry.title),
        preview: redacted_diff_preview(&diff_preview_for_path(&target, &content), &[]),
        extra_writes: Vec::new(),
        codex_session_migration: None,
        codex_provider_migration: None,
    };
    Ok((plan, content))
}

pub(crate) fn tool_id_for(tool: &ToolConfigTool) -> ToolId {
    match tool {
        ToolConfigTool::Codex => ToolId::Codex,
        ToolConfigTool::ClaudeCode => ToolId::ClaudeCode,
        ToolConfigTool::GeminiCli => ToolId::GeminiCli,
        ToolConfigTool::OpenCode => ToolId::OpenCode,
        ToolConfigTool::Grok => ToolId::Grok,
        ToolConfigTool::Pi => ToolId::Pi,
        ToolConfigTool::Cursor => ToolId::Cursor,
        ToolConfigTool::KimiCode => ToolId::KimiCode,
        ToolConfigTool::Zed => ToolId::Zed,
        ToolConfigTool::DeepseekHarness => ToolId::DeepseekHarness,
        ToolConfigTool::AntigravityCli => ToolId::AntigravityCli,
        ToolConfigTool::MuseCode => ToolId::MuseCode,
        ToolConfigTool::HermesAgent => ToolId::HermesAgent,
        ToolConfigTool::CommandCode => ToolId::CommandCode,
        ToolConfigTool::Codebuddy => ToolId::Codebuddy,
        ToolConfigTool::Workbuddy => ToolId::Workbuddy,
        ToolConfigTool::VscodeChat => ToolId::VscodeChat,
        ToolConfigTool::Cline => ToolId::Cline,
        ToolConfigTool::Trae => ToolId::Trae,
        ToolConfigTool::Qoder => ToolId::Qoder,
        ToolConfigTool::Zcode => ToolId::Zcode,
        ToolConfigTool::JetbrainsCopilot => ToolId::JetbrainsCopilot,
    }
}
