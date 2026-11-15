use super::*;
use aipass_agent_protocol::ToolConfigOutcome;
use std::fs;

#[test]
fn quick_integration_generic_write_binds_preview_and_restores_encrypted_backup() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    fs::create_dir_all(&home).unwrap();
    crate::server::tests::install_test_tools(&home);
    let state = crate::server::tests::sync_test_state(dir.path().join("vault"));
    let vault = Vault::create(&state.vault_dir, &SecretString::new("synthetic-password"))
        .unwrap()
        .vault;
    crate::server::TOOL_HOME_OVERRIDES
        .lock()
        .unwrap()
        .insert(vault.vault_id(), home.clone());
    let id = vault
        .add_provider(crate::server::tests::sync_test_provider(
            "Native integration",
            "synthetic-integration-value",
        ))
        .unwrap();
    let secret = vault.get_provider_summary(id).unwrap().secret_refs[0]
        .id
        .clone();
    crate::session::set_session_vault(&state, vault);
    let mut request = ToolConfigRequest {
        tool: ToolConfigTool::OpenCode,
        id,
        secret_id: Some(secret),
        mode: ToolConfigMode::Plaintext,
        codex_api_key_mode: None,
        preview_id: None,
    };
    let preview = |request: &ToolConfigRequest| -> ToolConfigPreviewResponse {
        let response = handle(
            &state,
            AgentRequest::ToolConfigPreview {
                request: request.clone(),
            },
        )
        .unwrap();
        assert!(response.ok);
        serde_json::from_value(response.data).unwrap()
    };
    let first = preview(&request);
    let target = PathBuf::from(&first.target_path);
    assert!(target.starts_with(&home));
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, br#"{"theme":"system"}"#).unwrap();
    let shown = preview(&request);
    assert!(!serde_json::to_string(&shown)
        .unwrap()
        .contains("synthetic-integration-value"));
    request.preview_id = shown.preview_id;
    let external = br#"{"theme":"dark","username":"external"}"#;
    fs::write(&target, external).unwrap();
    let failure = handle(
        &state,
        AgentRequest::ToolConfigApply {
            request: request.clone(),
        },
    )
    .err()
    .unwrap();
    assert_eq!(failure.code, AgentErrorCode::Conflict);
    assert_eq!(fs::read(&target).unwrap(), external);
    let fresh = preview(&request);
    assert_ne!(request.preview_id, fresh.preview_id);
    request.preview_id = fresh.preview_id;
    let response = handle(
        &state,
        AgentRequest::ToolConfigApply {
            request: request.clone(),
        },
    )
    .unwrap();
    let applied: ToolConfigApplyResponse = serde_json::from_value(response.data).unwrap();
    assert_eq!(applied.outcome, ToolConfigOutcome::Applied);
    let content = fs::read_to_string(&target).unwrap();
    assert!(content.contains("synthetic-integration-value") && content.contains("external"));
    assert!(Path::new(&applied.backup_path).starts_with(&home));
    assert!(
        !String::from_utf8_lossy(&fs::read(&applied.backup_path).unwrap()).contains("external")
    );
    let restored = handle(
        &state,
        AgentRequest::ToolConfigRollback {
            operation_id: applied.operation_id,
        },
    )
    .unwrap();
    assert!(restored.ok);
    assert_eq!(fs::read(&target).unwrap(), external);
    // The bound credential ID is not enough: replacing its value invalidates it.
    request.preview_id = preview(&request).preview_id;
    with_vault(&state, false, |vault| {
        vault
            .update_secret(
                id,
                request.secret_id.as_deref().unwrap(),
                "API Key",
                Some("synthetic-rotated-value".into()),
            )
            .map_err(map_vault_error)
    })
    .unwrap();
    assert_eq!(
        handle(&state, AgentRequest::ToolConfigApply { request })
            .err()
            .unwrap()
            .code,
        AgentErrorCode::Conflict
    );
    assert_eq!(fs::read(&target).unwrap(), external);
}
