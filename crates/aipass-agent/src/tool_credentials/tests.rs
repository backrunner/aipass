use super::*;
use crate::session::{with_vault, SessionInfo, SessionState};
use aipass_agent_protocol::{
    AgentRequest, ToolConfigApplyResponse, ToolConfigMode, ToolConfigOutcome,
    ToolConfigPreviewResponse, ToolConfigRequest,
};
use aipass_crypto::SecretString;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::json;
use std::fs;

fn fixture() -> (tempfile::TempDir, Arc<AgentState>, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    fs::create_dir_all(&home).unwrap();
    crate::server::tests::install_test_tools(&home);
    let state = crate::server::tests::sync_test_state(dir.path().join("vault"));
    let vault = Vault::create(&state.vault_dir, &SecretString::new("fixture-password"))
        .unwrap()
        .vault;
    crate::server::TOOL_HOME_OVERRIDES
        .lock()
        .unwrap()
        .insert(vault.vault_id(), home.clone());
    let now = time::OffsetDateTime::now_utc();
    *state.session.lock().unwrap() = SessionState::Unlocked(Box::new(SessionInfo {
        id: Uuid::new_v4(),
        vault,
        unlocked_at: now,
        last_activity_at: now,
    }));
    (dir, state, home)
}
fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}
fn api(vault: &Vault, endpoint: &str, key: &str) -> Uuid {
    vault.add_provider(api_input(endpoint, key)).unwrap()
}
fn api_input(endpoint: &str, key: &str) -> ProviderEntryInput {
    ProviderEntryInput {
        title: "Selected API".into(),
        provider_kind: ProviderKind::Unknown,
        provider_id: None,
        credential_kind: CredentialKind::Api,
        account_identity: None,
        domains: vec![],
        favicon_url: None,
        endpoints: vec![ProviderEndpoint::api(endpoint)],
        interface_type: InterfaceType::OpenAiCompatible,
        auth_scheme: AuthScheme::Bearer,
        api_key: key.into(),
        secret_label: None,
        default_model: Some("test-model".into()),
        max_concurrent_requests: None,
        supports_websockets: None,
        model_aliases: vec![],
        headers: vec![],
        quota: None,
        subscription: None,
        gateway: None,
        tags: vec![],
        notes: None,
        secret_metadata: Default::default(),
    }
}
fn request(id: Uuid, tool: ToolConfigTool) -> ToolConfigRequest {
    ToolConfigRequest {
        tool,
        id,
        secret_id: None,
        mode: ToolConfigMode::Plaintext,
        codex_api_key_mode: None,
        preview_id: None,
    }
}

#[test]
fn signed_out_native_agents_can_preview_apply_and_rollback_managed_api_credentials() {
    for (tool, directory, auth_file, auth) in [
        (
            ToolConfigTool::Codex,
            ".codex",
            "auth.json",
            r#"{"tokens":null}"#,
        ),
        (
            ToolConfigTool::ClaudeCode,
            ".claude",
            ".credentials.json",
            r#"{"claudeAiOauth":null,"mcpOAuth":{}}"#,
        ),
    ] {
        let (_dir, state, home) = fixture();
        let path = home.join(directory).join(auth_file);
        write(&path, auth);
        let before = crate::tool_switch::status(&state, tool.clone()).unwrap();
        assert_eq!(before.state, "not_logged_in");
        assert!(before.entry_id.is_none());
        let id = with_vault(&state, false, |v| {
            let mut input = api_input("https://selected.test/v1", "private-selected");
            if tool == ToolConfigTool::ClaudeCode {
                input.interface_type = InterfaceType::AnthropicMessages;
                input.auth_scheme = AuthScheme::XApiKey;
            }
            v.add_provider(input).map_err(map_vault_error)
        })
        .unwrap();
        let mut selected = request(id, tool.clone());
        preview(&state, &mut selected);
        assert_eq!(fs::read_to_string(&path).unwrap(), auth);
        let result = apply(&state, selected).unwrap();
        assert_eq!(result.outcome, ToolConfigOutcome::Applied);
        let after = crate::tool_switch::status(&state, tool.clone()).unwrap();
        assert_eq!(after.state, "ready");
        assert_eq!(after.entry_id, Some(id));
        with_vault(&state, false, |v| {
            assert_eq!(v.list_provider_summaries().unwrap().len(), 1);
            Ok(())
        })
        .unwrap();
        crate::tool_switch::rollback(&state, result.operation_id).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), auth);
        assert_eq!(
            crate::tool_switch::status(&state, tool).unwrap().state,
            "not_logged_in"
        );
    }
}

#[test]
fn no_credentials_are_distinct_from_external_references_and_unknown_api_keys() {
    let (_dir, state, home) = fixture();
    for tool in [
        ToolConfigTool::Codex,
        ToolConfigTool::ClaudeCode,
        ToolConfigTool::OpenCode,
        ToolConfigTool::Pi,
        ToolConfigTool::Grok,
        ToolConfigTool::GeminiCli,
    ] {
        assert_eq!(status(&state, tool).unwrap().state, "not_logged_in");
    }
    let path = home.join(".claude/settings.json");
    for content in [
        r#"{"apiKeyHelper":"never-execute-this-command"}"#,
        r#"{"env":{"ANTHROPIC_API_KEY":"$EXTERNAL_KEY"}}"#,
        r#"{"env":{"ANTHROPIC_API_KEY":"unmanaged-key"}}"#,
    ] {
        write(&path, content);
        assert_eq!(
            status(&state, ToolConfigTool::ClaudeCode).unwrap().state,
            "unmanaged"
        );
    }
    let codex = home.join(".codex/config.toml");
    write(
        &codex,
        "model_provider = \"external\"\n[model_providers.external]\nenv_key = \"EXTERNAL_KEY\"\n",
    );
    assert_eq!(
        status(&state, ToolConfigTool::Codex).unwrap().state,
        "unmanaged"
    );
}
fn preview(state: &Arc<AgentState>, request: &mut ToolConfigRequest) {
    let response = crate::server::handle_request(
        state,
        AgentRequest::ToolConfigPreview {
            request: request.clone(),
        },
    );
    assert!(response.ok, "{:?}", response.message);
    let preview: ToolConfigPreviewResponse = serde_json::from_value(response.data).unwrap();
    assert!(!serde_json::to_string(&preview)
        .unwrap()
        .contains("private-original"));
    request.preview_id = preview.preview_id;
}
fn apply(
    state: &Arc<AgentState>,
    request: ToolConfigRequest,
) -> ServiceResult<ToolConfigApplyResponse> {
    let response =
        crate::server::handle_request(state, AgentRequest::ToolConfigManagedApply { request });
    if !response.ok {
        return Err(ServiceError::new(
            response.code.unwrap(),
            response.message.unwrap_or_default(),
        ));
    }
    Ok(serde_json::from_value(response.data).unwrap())
}

#[test]
fn imports_every_codex_key_without_rotating_or_duplicating_existing_keys() {
    let (_dir, state, home) = fixture();
    write(
        &home.join(".codex/auth.json"),
        r#"{"OPENAI_API_KEY":"private-original"}"#,
    );
    write(&home.join(".codex/config.toml"), "model_provider = \"custom\"\nmodel = \"test\"\n[model_providers.custom]\nbase_url = \"https://first.test/v1\"\n[model_providers.other]\nbase_url = \"https://second.test/v1\"\nexperimental_bearer_token = \"private-second\"\n");
    with_vault(&state, false, |vault| {
        let original = api(vault, "https://first.test/v1/", "private-original");
        preserve(vault, &state, &ToolConfigTool::Codex)?;
        preserve(vault, &state, &ToolConfigTool::Codex)?;
        let entries = vault.list_provider_summaries().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries.iter().find(|e| e.id == original).unwrap().title,
            "Selected API"
        );
        assert_eq!(vault.reveal_secret(original).unwrap(), "private-original");
        Ok(())
    })
    .unwrap();
}

#[test]
fn managed_switch_imports_unmanaged_codex_subscription_and_archives_its_native_home() {
    let (_dir, state, home) = fixture();
    let jwt = format!(
        "e30.{}.fixture",
        URL_SAFE_NO_PAD.encode(json!({"email":"original@example.test"}).to_string())
    );
    let native = json!({"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{"id_token":jwt,"account_id":"workspace-original","access_token":"private-original","refresh_token":"private-refresh"}});
    write(&home.join(".codex/auth.json"), &native.to_string());
    write(&home.join(".codex/config.toml"), "model = \"test\"\n");
    let id = with_vault(&state, false, |v| {
        Ok(api(v, "https://new.test/v1", "private-new"))
    })
    .unwrap();
    let mut r = request(id, ToolConfigTool::Codex);
    preview(&state, &mut r);
    let result = apply(&state, r).unwrap();
    assert_eq!(result.outcome, ToolConfigOutcome::Applied);
    assert!(
        !String::from_utf8_lossy(&fs::read(&result.backup_path).unwrap())
            .contains("private-original")
    );
    with_vault(&state, false, |vault| {
        let original = vault
            .list_provider_summaries()
            .unwrap()
            .into_iter()
            .find(|e| e.credential_kind == CredentialKind::OAuth)
            .unwrap();
        assert_eq!(
            original.account_identity.as_deref(),
            Some("original@example.test:workspace-original")
        );
        let reference = vault
            .provider_runtime_extension(original.id, "community_account_v1")
            .unwrap()
            .unwrap();
        let bundle: serde_json::Value = serde_json::from_str(reference.expose()).unwrap();
        let reference: serde_json::Value =
            serde_json::from_str(bundle["auth"].as_str().unwrap()).unwrap();
        let archived = PathBuf::from(reference["nativeHome"].as_str().unwrap());
        assert_ne!(archived, home.join(".codex"));
        let auth: serde_json::Value =
            serde_json::from_slice(&fs::read(archived.join("auth.json")).unwrap()).unwrap();
        assert_eq!(auth, native);
        Ok(())
    })
    .unwrap();
}

#[test]
fn managed_generic_switch_imports_before_write_and_keeps_encrypted_backup() {
    let (_dir, state, home) = fixture();
    let path = home.join(".config/opencode/opencode.json");
    let original = r#"{"model":"custom/old","provider":{"custom":{"npm":"@ai-sdk/openai-compatible","options":{"apiKey":"private-original","baseURL":"https://original.test/v1"}}}}"#;
    write(&path, original);
    let id = with_vault(&state, false, |v| {
        Ok(api(v, "https://new.test/v1", "private-new"))
    })
    .unwrap();
    let mut r = request(id, ToolConfigTool::OpenCode);
    preview(&state, &mut r);
    let result = apply(&state, r).unwrap();
    assert_eq!(result.outcome, ToolConfigOutcome::Applied);
    assert!(
        !String::from_utf8_lossy(&fs::read(&result.backup_path).unwrap())
            .contains("private-original")
    );
    with_vault(&state, false, |vault| {
        let entries = vault.list_provider_summaries().unwrap();
        assert_eq!(entries.len(), 2);
        let imported = entries.iter().find(|e| e.id != id).unwrap();
        assert_eq!(
            vault.reveal_secret(imported.id).unwrap(),
            "private-original"
        );
        assert_eq!(imported.default_model.as_deref(), Some("old"));
        Ok(())
    })
    .unwrap();
}

#[test]
fn unsupported_existing_credential_stops_switch_without_changing_configuration() {
    let (_dir, state, home) = fixture();
    let path = home.join(".config/opencode/opencode.json");
    let original = r#"{"provider":{"unknown":{"options":{"apiKey":"private-original"}}}}"#;
    write(&path, original);
    let id = with_vault(&state, false, |v| {
        Ok(api(v, "https://new.test/v1", "private-new"))
    })
    .unwrap();
    let mut r = request(id, ToolConfigTool::OpenCode);
    preview(&state, &mut r);
    let error = apply(&state, r).unwrap_err();
    assert_eq!(error.code, AgentErrorCode::ValidationFailed);
    assert!(!error.message.contains("private-original"));
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    with_vault(&state, false, |v| {
        assert_eq!(v.list_provider_summaries().unwrap().len(), 1);
        Ok(())
    })
    .unwrap();
}

#[test]
fn stale_preview_and_missing_preview_cannot_import_or_switch() {
    let (_dir, state, home) = fixture();
    let path = home.join(".config/opencode/opencode.json");
    write(&path, r#"{"provider":{}}"#);
    let id = with_vault(&state, false, |v| {
        Ok(api(v, "https://new.test/v1", "private-new"))
    })
    .unwrap();
    let mut r = request(id, ToolConfigTool::OpenCode);
    assert_eq!(
        apply(&state, r.clone()).unwrap_err().code,
        AgentErrorCode::Conflict
    );
    preview(&state, &mut r);
    let changed = r#"{"provider":{"custom":{"options":{"apiKey":"private-original","baseURL":"https://original.test/v1"}}}}"#;
    write(&path, changed);
    assert_eq!(apply(&state, r).unwrap_err().code, AgentErrorCode::Conflict);
    assert_eq!(fs::read_to_string(&path).unwrap(), changed);
    with_vault(&state, false, |v| {
        assert_eq!(v.list_provider_summaries().unwrap().len(), 1);
        Ok(())
    })
    .unwrap();
}

#[test]
fn config_readers_preserve_auth_semantics_and_never_execute_helpers() {
    let (_dir, state, home) = fixture();
    write(
        &home.join(".claude/settings.json"),
        r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"private-token","ANTHROPIC_API_KEY":"private-api","ANTHROPIC_BASE_URL":"https://claude.test"}}"#,
    );
    let keys = reader::read(&home, &ToolConfigTool::ClaudeCode).unwrap();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0].auth, AuthScheme::Bearer);
    assert_eq!(keys[1].auth, AuthScheme::XApiKey);
    write(
        &home.join(".gemini/.env"),
        "GEMINI_API_KEY=\"private-gemini\"\nGOOGLE_GEMINI_BASE_URL='https://gemini.test'\n",
    );
    let keys = reader::read(&home, &ToolConfigTool::GeminiCli).unwrap();
    assert_eq!(keys[0].interface, InterfaceType::Gemini);
    assert_eq!(keys[0].auth, AuthScheme::GoogleApiKey);
    write(
        &home.join(".pi/agent/models.json"),
        r#"{"providers":{"custom":{"api":"anthropic-messages","baseUrl":"https://pi.test","apiKey":"$AIPASS_KEY_TEST"}}}"#,
    );
    assert!(reader::read(&home, &ToolConfigTool::Pi).unwrap().is_empty());
    write(&home.join(".grok/config.toml"), "[model.custom]\napi_key = \"private-grok\"\nbase_url = \"https://grok.test\"\napi_backend = \"messages\"\n");
    let keys = reader::read(&home, &ToolConfigTool::Grok).unwrap();
    assert_eq!(keys[0].interface, InterfaceType::AnthropicMessages);
    // Status only returns metadata and stable IDs, never imported secrets.
    with_vault(&state, false, |v| {
        preserve(v, &state, &ToolConfigTool::Grok)?;
        Ok(())
    })
    .unwrap();
    let result = status(&state, ToolConfigTool::Grok).unwrap();
    assert_eq!(result.state, "ready");
    assert!(!serde_json::to_string(&result)
        .unwrap()
        .contains("private-grok"));
}

#[test]
fn managed_switch_wire_contract_retains_a_long_timeout() {
    let wire = json!({"type":"tool_config.managed_apply","request":{"tool":"opencode","id":Uuid::new_v4(),"mode":"plaintext","previewId":"safe-preview"}});
    let r: AgentRequest = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(r.event_name(), "tool_config.managed_apply");
    assert_eq!(r.response_timeout(), std::time::Duration::from_secs(300));
    assert_eq!(serde_json::to_value(r).unwrap()["type"], wire["type"]);
    // Preserve the established Agent wire spelling; desktop also accepts opencode.
    assert_eq!(
        serde_json::to_value(ToolConfigTool::OpenCode).unwrap(),
        "open-code"
    );
    assert_eq!(
        serde_json::from_value::<ToolConfigTool>(json!("opencode")).unwrap(),
        ToolConfigTool::OpenCode
    );
}

#[test]
fn gemini_api_switch_preserves_google_login_in_an_independent_cli_home() {
    let (_dir, state, home) = fixture();
    let dir = home.join(".gemini");
    write(
        &dir.join("oauth_creds.json"),
        r#"{"access_token":"private-google","refresh_token":"private-refresh","expiry_date":4000000000000}"#,
    );
    write(
        &dir.join("google_accounts.json"),
        r#"{"active":"google@example.test"}"#,
    );
    write(
        &dir.join("settings.json"),
        r#"{"security":{"auth":{"selectedType":"oauth-personal"}},"theme":"Dark"}"#,
    );
    write(&dir.join(".env"), "GEMINI_API_KEY='private-dormant'\n");
    let current = status(&state, ToolConfigTool::GeminiCli).unwrap();
    assert_eq!(
        current.account_identity.as_deref(),
        Some("google@example.test")
    );
    assert_eq!(current.state, "unmanaged");
    let id = with_vault(&state, false, |v| {
        let mut input = api_input("https://gemini.test", "private-gemini");
        input.interface_type = InterfaceType::Gemini;
        input.auth_scheme = AuthScheme::GoogleApiKey;
        v.add_provider(input).map_err(map_vault_error)
    })
    .unwrap();
    let mut r = request(id, ToolConfigTool::GeminiCli);
    preview(&state, &mut r);
    assert_eq!(
        apply(&state, r).unwrap().outcome,
        ToolConfigOutcome::Applied
    );
    let settings: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("settings.json")).unwrap()).unwrap();
    assert_eq!(
        settings["security"]["auth"]["selectedType"],
        "gemini-api-key"
    );
    assert_eq!(settings["theme"], "Dark");
    with_vault(&state, false, |vault| {
        let original = vault
            .list_provider_summaries()
            .unwrap()
            .into_iter()
            .find(|e| e.provider_id.as_deref() == Some("gemini-cli"))
            .unwrap();
        let raw = vault
            .provider_runtime_extension(original.id, "community_account_v1")
            .unwrap()
            .unwrap();
        let bundle: serde_json::Value = serde_json::from_str(raw.expose()).unwrap();
        let reference: serde_json::Value =
            serde_json::from_str(bundle["auth"].as_str().unwrap()).unwrap();
        let archived = PathBuf::from(reference["nativeHome"].as_str().unwrap()).join(".gemini");
        assert_ne!(archived, dir);
        let settings: serde_json::Value =
            serde_json::from_slice(&fs::read(archived.join("settings.json")).unwrap()).unwrap();
        assert_eq!(
            settings["security"]["auth"]["selectedType"],
            "oauth-personal"
        );
        assert!(fs::read_to_string(archived.join("oauth_creds.json"))
            .unwrap()
            .contains("private-google"));
        Ok(())
    })
    .unwrap();
}

#[test]
fn opencode_auth_store_imports_selected_builtin_key_and_rejects_unsupported_subscription() {
    let (_dir, state, home) = fixture();
    let path = home.join(".config/opencode/opencode.json");
    let auth = home.join(".local/share/opencode/auth.json");
    write(&path, r#"{"model":"anthropic/old"}"#);
    write(
        &auth,
        r#"{"anthropic":{"type":"api","key":"private-original"}}"#,
    );
    let keys = reader::read(&home, &ToolConfigTool::OpenCode).unwrap();
    assert_eq!(keys[0].interface, InterfaceType::AnthropicMessages);
    with_vault(&state, false, |v| {
        preserve(v, &state, &ToolConfigTool::OpenCode)?;
        assert_eq!(v.list_provider_summaries().unwrap().len(), 1);
        Ok(())
    })
    .unwrap();
    write(
        &auth,
        r#"{"anthropic":{"type":"oauth","refresh":"private-refresh"}}"#,
    );
    let id = with_vault(&state, false, |v| {
        Ok(api(v, "https://new.test/v1", "private-new"))
    })
    .unwrap();
    let mut r = request(id, ToolConfigTool::OpenCode);
    preview(&state, &mut r);
    assert_eq!(
        apply(&state, r).unwrap_err().code,
        AgentErrorCode::ValidationFailed
    );
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        r#"{"model":"anthropic/old"}"#
    );
    assert!(fs::read_to_string(auth)
        .unwrap()
        .contains("private-refresh"));
}

#[test]
fn selected_provider_is_reported_before_dormant_keys() {
    let (_dir, _state, home) = fixture();
    write(
        &home.join(".config/opencode/opencode.json"),
        r#"{"model":"z/selected","provider":{"a":{"options":{"apiKey":"dormant","baseURL":"https://a.test"}},"z":{"options":{"apiKey":"selected","baseURL":"https://z.test"}}}}"#,
    );
    let keys = reader::read(&home, &ToolConfigTool::OpenCode).unwrap();
    assert_eq!(keys[0].key.as_str(), "selected");
    assert_eq!(keys.len(), 2);
    write(
        &home.join(".config/opencode/opencode.json"),
        r#"{"model":"google/gemini","provider":{"google":{"npm":"@ai-sdk/google","options":{"apiKey":"private-google"}}}}"#,
    );
    let keys = reader::read(&home, &ToolConfigTool::OpenCode).unwrap();
    assert_eq!(keys[0].interface, InterfaceType::Gemini);
    assert_eq!(keys[0].auth, AuthScheme::GoogleApiKey);
    let helper = aipass_config_writers::helper_path(&home, "cursor-agent-local");
    write(
        &helper,
        &format!(
            "{}{}",
            aipass_config_writers::env_literal("CURSOR_LOCAL_AGENT_API_KEY", "private-cursor"),
            aipass_config_writers::env_literal(
                "CURSOR_LOCAL_AGENT_BASE_URL",
                "https://cursor.test/v1/messages"
            )
        ),
    );
    let keys = reader::read(&home, &ToolConfigTool::Cursor).unwrap();
    assert_eq!(keys[0].interface, InterfaceType::AnthropicMessages);
    assert_eq!(keys[0].auth, AuthScheme::XApiKey);
    // The current runtime takes a versioned base, so retain AIPass protocol
    // metadata when importing a previously configured local proxy credential.
    let mut cursor = aipass_config_writers::ToolEntry {
        supports_websockets: None,
        id: Uuid::new_v4(),
        secret_id: None,
        title: "Cursor route".into(),
        provider_id: None,
        endpoint: Some("http://127.0.0.1:8787/v1".into()),
        interface_type: InterfaceType::AnthropicMessages,
        auth_scheme: AuthScheme::XApiKey,
        env_key: "AIPASS_PROXY_TOKEN".into(),
        default_model: Some("fixture-model".into()),
        api_key: Some("private-cursor-route".into()),
    };
    let (_, content) =
        aipass_config_writers::plan_cursor_local_proxy_plaintext(&home, &cursor).unwrap();
    write(&helper, &content);
    let keys = reader::read(&home, &ToolConfigTool::Cursor).unwrap();
    assert_eq!(keys[0].interface, InterfaceType::AnthropicMessages);
    assert_eq!(keys[0].endpoint, "http://127.0.0.1:8787/v1");
    assert_eq!(keys[0].key.as_str(), "private-cursor-route");
    cursor.interface_type = InterfaceType::OpenAiCompatible;
    cursor.auth_scheme = AuthScheme::Bearer;
    let (_, content) = aipass_config_writers::plan_cursor_local_plaintext(&home, &cursor).unwrap();
    write(&helper, &content);
    let keys = reader::read(&home, &ToolConfigTool::Cursor).unwrap();
    assert_eq!(keys[0].interface, InterfaceType::OpenAiCompatible);
}

#[test]
fn additional_agents_import_outgoing_keys_and_reject_stale_previews() {
    for tool in [ToolConfigTool::KimiCode, ToolConfigTool::DeepseekHarness] {
        let (_dir, state, home) = fixture();
        let (config, original) = match tool {
            ToolConfigTool::KimiCode => (home.join(".kimi-code/config.toml"),
                "default_model = 'original'\n[models.original]\nprovider = 'old'\nmodel = 'old-model'\n[providers.old]\ntype = 'openai'\nbase_url = 'https://old.test/v1'\napi_key = 'old-fixture-key'\n"),
            _ => (home.join(".dsh/profiles/web/cordis.patch.yml"),
                "- id: llm-pi-ai\n  config:\n    providers:\n      original:\n        api: openai-completions\n        apiKeyEnv: ORIGINAL_KEY\n        baseURL: https://old.test/v1\n        models:\n          - id: old-model\n"),
        };
        assert!(reader::paths(&home, &tool)
            .iter()
            .all(|path| path.starts_with(&home)));
        write(&config, original);
        if tool == ToolConfigTool::DeepseekHarness {
            write(
                &home.join(".dsh/.credentials.yaml"),
                "version: 1\nrefs:\n  ORIGINAL_KEY: old-fixture-key\n",
            );
        }
        let id = with_vault(&state, false, |v| {
            Ok(api(v, "https://new.test/v1", "new-fixture-key"))
        })
        .unwrap();
        let mut r = request(id, tool.clone());
        preview(&state, &mut r);
        write(&config, &format!("{original}\n# external edit\n"));
        let failed = crate::server::handle_request(
            &state,
            AgentRequest::ToolConfigManagedApply { request: r.clone() },
        );
        assert!(!failed.ok);
        assert!(fs::read_to_string(&config)
            .unwrap()
            .contains("# external edit"));
        preview(&state, &mut r);
        let applied = crate::server::handle_request(
            &state,
            AgentRequest::ToolConfigManagedApply { request: r },
        );
        assert!(applied.ok, "{:?}", applied.message);
        with_vault(&state, false, |v| {
            let imported = v
                .list_provider_summaries()
                .unwrap()
                .into_iter()
                .find(|e| e.tags.iter().any(|t| t == "agent-import"))
                .unwrap();
            assert_eq!(
                v.reveal_secret_by_id(imported.id, &imported.secret_refs[0].id)
                    .unwrap(),
                "old-fixture-key"
            );
            Ok(())
        })
        .unwrap();
        assert!(fs::read_to_string(&config)
            .unwrap()
            .contains("https://new.test/v1"));
    }
}

#[test]
fn zed_import_reads_shell_quoted_keys_without_executing_them() {
    let (_dir, _state, home) = fixture();
    let config = aipass_config_writers::zed_config_path(&home);
    assert!(
        config.starts_with(&home),
        "fixture must remain in its own home"
    );
    write(
        &config,
        r#"{"language_models":{"openai_compatible":{"aipass":{"api_url":"https://fixture.test/v1"}}}}"#,
    );
    write(
        &aipass_config_writers::helper_path(&home, "zed"),
        &aipass_config_writers::env_literal("AIPASS_API_KEY", "fixture'$(echo literal)"),
    );
    let keys = reader::read(&home, &ToolConfigTool::Zed).unwrap();
    assert_eq!(keys[0].key.as_str(), "fixture'$(echo literal)");
}

#[test]
fn tencent_byok_switch_imports_the_outgoing_model_key_before_replacing_it() {
    let (_dir, state, home) = fixture();
    write(
        &home.join(".codebuddy/models.json"),
        r#"{"models":[{"id":"test-model","apiKey":"outgoing-editor-key","url":"https://old.test/v1/chat/completions"}]}"#,
    );
    let id = with_vault(&state, false, |v| {
        Ok(api(v, "https://new.test/v1", "selected-editor-key"))
    })
    .unwrap();
    let mut r = request(id, ToolConfigTool::Workbuddy);
    preview(&state, &mut r);
    let result =
        crate::server::handle_request(&state, AgentRequest::ToolConfigManagedApply { request: r });
    assert!(result.ok, "{:?}", result.message);
    with_vault(&state, false, |v| {
        let imported = v
            .list_provider_summaries()
            .unwrap()
            .into_iter()
            .find(|e| e.tags.iter().any(|t| t == "agent-import"))
            .unwrap();
        assert_eq!(
            v.reveal_secret_by_id(imported.id, &imported.secret_refs[0].id)
                .unwrap(),
            "outgoing-editor-key"
        );
        Ok(())
    })
    .unwrap();
    let models = fs::read_to_string(home.join(".codebuddy/models.json")).unwrap();
    assert!(models.contains("selected-editor-key"));
    assert!(!models.contains("outgoing-editor-key"));
}

#[test]
fn guarded_shell_credential_references_are_skipped_without_execution() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join(".aipass/tools/cursor-agent-local.env");
    write(&path, "unset CURSOR_LOCAL_AGENT_API_KEY\nCURSOR_LOCAL_AGENT_API_KEY=\"$(exit 73)\" || return $?\nexport CURSOR_LOCAL_AGENT_API_KEY\nexport CURSOR_LOCAL_AGENT_BASE_URL='http://127.0.0.1:8787/v1'\n# AIPASS_CURSOR_API_TYPE=anthropic_messages\n");
    assert!(reader::read(home.path(), &ToolConfigTool::Cursor)
        .unwrap()
        .is_empty());
}

#[test]
fn powershell_reader_imports_only_literal_values_without_executing_expressions() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("zed.ps1");
    write(
        &path,
        "\u{feff}# PowerShell helper\r\n$env:AIPASS_API_KEY = 'prefix O''Brien 中文 `$()'\r\n",
    );
    let values = reader::env(&path).unwrap();
    assert_eq!(values.0["AIPASS_API_KEY"], "prefix O'Brien 中文 `$()");
    write(&path, "$env:AIPASS_API_KEY = [string]$aipassValue\r\n");
    assert!(reader::env(&path).unwrap().0["AIPASS_API_KEY"].is_null());
}

#[test]
fn removing_an_installed_agent_after_preview_rejects_the_write() {
    let (_dir, state, home) = fixture();
    let id = with_vault(&state, false, |v| {
        Ok(api(v, "https://fixture.test/v1", "fixture-key"))
    })
    .unwrap();
    let mut r = request(id, ToolConfigTool::KimiCode);
    preview(&state, &mut r);
    let binary = home
        .join(".local/bin")
        .join(if cfg!(windows) { "kimi.exe" } else { "kimi" });
    fs::remove_file(binary).unwrap();
    let err = apply(&state, r).unwrap_err();
    assert_eq!(err.code, AgentErrorCode::ValidationFailed);
    assert!(err.message.contains("installed"));
    assert!(!home.join(".kimi-code/config.toml").exists());
}
