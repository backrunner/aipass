use super::*;
use crate::session::{SessionInfo, SessionState};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use std::fs;
fn jwt(value: Value) -> String {
    format!("e30.{}.fixture", URL_SAFE_NO_PAD.encode(value.to_string()))
}
fn codex_auth(user: &str, workspace: &str, token: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{"id_token":jwt(json!({"email":user})),"account_id":workspace,"access_token":jwt(json!({"exp":4000000000_i64,"token":token})),"refresh_token":format!("fake-refresh-{token}")}})).unwrap()
}
pub(super) fn fixture() -> (tempfile::TempDir, Arc<AgentState>, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    fs::create_dir_all(&home).unwrap();
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
fn register(state: &Arc<AgentState>, path: &Path) -> Uuid {
    with_vault(state, false, |vault| {
        let auth = crate::subscriptions::cli_accounts::reference("codex", path)
            .map_err(|e| safe_error(anyhow::anyhow!(e)))?;
        crate::community::register_cli(vault, "codex", auth)
    })
    .unwrap()
}
pub(super) fn request(id: Uuid, mode: ToolConfigMode) -> ToolConfigRequest {
    ToolConfigRequest {
        tool: ToolConfigTool::Codex,
        id,
        secret_id: None,
        mode,
        codex_api_key_mode: None,
        preview_id: None,
    }
}
fn switch(state: &Arc<AgentState>, r: &ToolConfigRequest) -> ToolConfigApplyResponse {
    with_vault(state, false, |vault| apply_inner(state, vault, r)).unwrap()
}
#[test]
fn codex_roundtrip_preserves_latest_tokens_exact_keys_and_encrypted_backups() {
    let (_dir, state, home) = fixture();
    let active = home.join(".codex");
    let b = home.join("source-b");
    fs::create_dir_all(&active).unwrap();
    fs::create_dir_all(&b).unwrap();
    fs::write(
        active.join("auth.json"),
        codex_auth("alice@example.test", "workspace-a", "a-original"),
    )
    .unwrap();
    fs::write(
        active.join("config.toml"),
        "model = \"gpt-fixture\"\nuser_preference = true\n",
    )
    .unwrap();
    fs::write(
        b.join("auth.json"),
        codex_auth("bob@example.test", "workspace-b", "b-original"),
    )
    .unwrap();
    let a_id = register(&state, &active);
    let b_id = register(&state, &b);
    switch(&state, &request(a_id, ToolConfigMode::Official));
    // Simulate the official CLI rotating both the access and refresh grants.
    fs::write(
        active.join("auth.json"),
        codex_auth("alice@example.test", "workspace-a", "a-renewed"),
    )
    .unwrap();
    let b_result = switch(&state, &request(b_id, ToolConfigMode::Official));
    let a_ref = with_vault(&state, false, |v| {
        reference(v, a_id, &ToolConfigTool::Codex).map_err(safe_error)
    })
    .unwrap();
    assert!(fs::read_to_string(
        PathBuf::from(a_ref["nativeHome"].as_str().unwrap()).join("auth.json")
    )
    .unwrap()
    .contains("fake-refresh-a-renewed"));
    assert!(fs::read_to_string(active.join("auth.json"))
        .unwrap()
        .contains("fake-refresh-b-original"));
    let api_id = with_vault(&state, false, |v| {
        v.add_provider(crate::server::tests::sync_test_provider(
            "API",
            "fake-api-primary",
        ))
        .map_err(map_vault_error)
    })
    .unwrap();
    let second = with_vault(&state, false, |v| {
        v.add_secret(api_id, "Selected", "fake-api-selected")
            .map_err(map_vault_error)
    })
    .unwrap();
    let mut api = request(api_id, ToolConfigMode::Plaintext);
    api.secret_id = Some(second.clone());
    switch(&state, &api);
    assert!(fs::read_to_string(active.join("auth.json"))
        .unwrap()
        .contains("fake-api-selected"));
    let result = switch(&state, &request(a_id, ToolConfigMode::Official));
    let actual = fs::read_to_string(active.join("auth.json")).unwrap();
    assert!(actual.contains("fake-refresh-a-renewed"));
    assert!(!actual.contains("fake-api-selected"));
    let current = status_snapshot(&state, ToolConfigTool::Codex).unwrap();
    assert_eq!(current.state, "ready");
    assert_eq!(current.entry_id, Some(a_id));
    fs::write(
        active.join("auth.json"),
        codex_auth("bob@example.test", "workspace-b", "external"),
    )
    .unwrap();
    assert_eq!(
        status_snapshot(&state, ToolConfigTool::Codex)
            .unwrap()
            .state,
        "conflict"
    );
    for item in fs::read_dir(root(&state)).unwrap() {
        let raw = fs::read(item.unwrap().path()).unwrap();
        let text = String::from_utf8_lossy(&raw);
        assert!(!text.contains("fake-refresh"));
        assert!(!text.contains("fake-api"));
    }
    assert!(Path::new(&b_result.backup_path).exists());
    assert!(Path::new(&result.backup_path).exists());
    assert!(fs::read_to_string(active.join("config.toml"))
        .unwrap()
        .contains("user_preference = true"));
}
#[test]
fn preview_is_bound_to_external_state_and_never_returns_secrets() {
    let (_dir, state, home) = fixture();
    let active = home.join(".codex");
    fs::create_dir_all(&active).unwrap();
    fs::write(
        active.join("auth.json"),
        codex_auth("alice@example.test", "workspace-a", "original"),
    )
    .unwrap();
    let id = register(&state, &active);
    let mut r = request(id, ToolConfigMode::Official);
    fs::write(
        active.join("config.toml"),
        "experimental_bearer_token = '''\nfake-multiline-private\n'''\nuser_preference = true\n",
    )
    .unwrap();
    let before = fs::read(active.join("auth.json")).unwrap();
    let preview = with_vault(&state, false, |v| {
        let (_, plan, content) = crate::server::build_tool_config_plan(v, &r, true)?;
        r.preview_id = Some(preview_id(v, &state, &r, &plan).unwrap());
        Ok(crate::server::tool_config_preview_files(&plan, &content))
    })
    .unwrap();
    assert_eq!(fs::read(active.join("auth.json")).unwrap(), before);
    assert!(!serde_json::to_string(&preview)
        .unwrap()
        .contains("fake-refresh"));
    assert!(!serde_json::to_string(&preview)
        .unwrap()
        .contains("fake-multiline-private"));
    fs::write(active.join("config.toml"), "user_preference = true\n").unwrap();
    let result = apply(&state, r).unwrap();
    assert_eq!(result.outcome, ToolConfigOutcome::Conflict);
    assert_eq!(fs::read(active.join("auth.json")).unwrap(), before);
    assert!(!root(&state).exists());
}

#[cfg(unix)]
#[test]
fn outgoing_archive_uses_a_canonical_home_for_keychain_account_hashes() {
    let (_dir, state, home) = fixture();
    let real = home.join("real");
    fs::create_dir_all(real.join(".codex")).unwrap();
    let alias = home.join("alias");
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    let active = alias.join(".codex");
    fs::write(
        active.join("auth.json"),
        codex_auth("alice@example.test", "a", "latest"),
    )
    .unwrap();
    let id = register(&state, &active);
    with_vault(&state, false, |v| {
        let store = NativeStore::codex(&active).unwrap();
        let mut changes = Vec::new();
        let mut refs = Vec::new();
        archive_outgoing(
            v,
            &request(Uuid::new_v4(), ToolConfigMode::Plaintext),
            &store,
            &home,
            &mut changes,
            &mut refs,
        )
        .unwrap();
        assert_eq!(refs[0].id, id);
        let archived = PathBuf::from(refs[0].after["nativeHome"].as_str().unwrap());
        assert_eq!(
            archived,
            real.canonicalize()
                .unwrap()
                .join(".codex/aipass-accounts")
                .join(id.to_string())
        );
        let tx = Transaction {
            operation_id: Uuid::new_v4(),
            committed: false,
            changes,
            metadata: json!([]),
        };
        tx.apply().unwrap();
        assert_eq!(archived.canonicalize().unwrap(), archived);
        Ok(())
    })
    .unwrap();
}

#[test]
fn recovery_waits_for_openai_alias_account_operations_without_touching_credentials() {
    let (_dir, state, home) = fixture();
    let active = home.join(".codex");
    fs::create_dir_all(&active).unwrap();
    let path = active.join("auth.json");
    fs::write(&path, codex_auth("alice@example.test", "a", "before")).unwrap();
    let id = register(&state, &active);
    with_vault(&state, false, |v| {
        let mut input: aipass_vault::ProviderEntryUpdateInput = serde_json::from_value(
            serde_json::to_value(v.get_provider_summary(id).unwrap()).unwrap(),
        )
        .unwrap();
        input.provider_id = Some("openai".into());
        v.update_provider(id, input).map_err(map_vault_error)?;
        Ok(())
    })
    .unwrap();
    let tx = Transaction {
        operation_id: Uuid::new_v4(),
        committed: false,
        changes: vec![
            Change::new(Resource::File(path.clone()), Some(b"interrupted".to_vec())).unwrap(),
        ],
        metadata: json!([]),
    };
    with_vault(&state, false, |v| {
        tx.save(&root(&state), &v.config_backup_key()).unwrap();
        Ok(())
    })
    .unwrap();
    tx.apply().unwrap();
    let locks = account_locks(&state).unwrap();
    assert!(!locks.is_empty());
    let guard = locks[0].lock().unwrap();
    assert_eq!(recover(&state).unwrap_err().code, AgentErrorCode::Conflict);
    assert_eq!(fs::read(&path).unwrap(), b"interrupted");
    drop(guard);
    recover(&state).unwrap();
    assert!(fs::read_to_string(path)
        .unwrap()
        .contains("fake-refresh-before"));
}
#[test]
fn interrupted_switch_recovery_restores_resources_and_bindings() {
    let (_dir, state, home) = fixture();
    let active = home.join(".codex");
    fs::create_dir_all(&active).unwrap();
    let path = active.join("auth.json");
    fs::write(&path, b"original").unwrap();
    let tx = Transaction {
        operation_id: Uuid::new_v4(),
        committed: false,
        changes: vec![Change::new(
            Resource::File(path.clone()),
            Some(b"fake-new-token".to_vec()),
        )
        .unwrap()],
        metadata: json!([]),
    };
    with_vault(&state, false, |v| {
        tx.save(&root(&state), &v.config_backup_key()).unwrap();
        Ok(())
    })
    .unwrap();
    tx.apply().unwrap();
    recover(&state).unwrap();
    assert_eq!(fs::read(path).unwrap(), b"original");
    assert!(!Transaction::path(&root(&state), tx.operation_id).exists());
}
#[test]
fn full_file_and_diff_redaction_cover_vendor_tokens_and_api_keys() {
    for raw in [
        r#"{"tokens":{"refresh_token":"fake-refresh","id_token":"fake-id","access_token":"fake-access"},"OPENAI_API_KEY":"fake-key"}"#,
        "- api_key = \"fake-key\"\n+ experimental_bearer_token = \"fake-token\"",
        r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"fake-token"},"apiKeyHelper":"echo fake-inline-key"}"#,
    ] {
        let redacted = redact_config(raw);
        assert!(!redacted.contains("fake-"), "{redacted}");
    }
}

fn claude_auth(token: &str, expiry: i64) -> Vec<u8> {
    serde_json::to_vec(&json!({"claudeAiOauth":{"accessToken":format!("fake-access-{token}"),"refreshToken":format!("fake-refresh-{token}"),"expiresAt":expiry*1000,"scopes":["user:inference"]}})).unwrap()
}
fn write_claude(path: &Path, user: &str, token: &str) {
    fs::create_dir_all(path).unwrap();
    fs::write(
        path.join(".credentials.json"),
        claude_auth(token, 4_000_000_000),
    )
    .unwrap();
    fs::write(
        path.join(".claude.json"),
        json!({"oauthAccount":{"emailAddress":user},"userPreference":true}).to_string(),
    )
    .unwrap();
}
fn claude_register(state: &Arc<AgentState>, path: &Path, user: &str) -> Uuid {
    with_vault(state, false, |v| {
        crate::official_accounts::persist_claude_login(
            v,
            &crate::claude_cli::NativeAccount {
                home: path.to_owned(),
                identity: user.into(),
                plan: None,
            },
        )
        .map_err(safe_error)
    })
    .unwrap()
}
#[test]
fn claude_roundtrip_archives_latest_grant_and_preserves_user_metadata() {
    let (_dir, state, home) = fixture();
    let active = home.join(".claude");
    let b = home.join("source-b");
    write_claude(&active, "alice@example.test", "a");
    // The default Claude profile is beside .claude, not inside it.
    fs::rename(active.join(".claude.json"), home.join(".claude.json")).unwrap();
    write_claude(&b, "bob@example.test", "b");
    fs::write(active.join("settings.json"), r#"{"permissions":{"allow":["Read"]},"env":{"ANTHROPIC_MODEL":"old-api-model","ANTHROPIC_BASE_URL":"https://old.invalid"}}"#).unwrap();
    let a = claude_register(&state, &active, "alice@example.test");
    let b = claude_register(&state, &b, "bob@example.test");
    let mut r = request(a, ToolConfigMode::Official);
    r.tool = ToolConfigTool::ClaudeCode;
    switch(&state, &r);
    fs::write(
        active.join(".credentials.json"),
        claude_auth("a-renewed", 4_000_000_000),
    )
    .unwrap();
    r.id = b;
    switch(&state, &r);
    assert_eq!(
        status_snapshot(&state, ToolConfigTool::ClaudeCode)
            .unwrap()
            .entry_id,
        Some(b)
    );
    let api = with_vault(&state, false, |v| {
        let mut input = crate::server::tests::sync_test_provider("Claude API", "fake-api-key");
        input.interface_type = aipass_provider_registry::InterfaceType::AnthropicMessages;
        input.auth_scheme = aipass_provider_registry::AuthScheme::XApiKey;
        input.default_model = Some("claude-fixture".into());
        v.add_provider(input).map_err(map_vault_error)
    })
    .unwrap();
    r.id = api;
    r.mode = ToolConfigMode::Plaintext;
    switch(&state, &r);
    assert!(fs::read_to_string(active.join("settings.json"))
        .unwrap()
        .contains("fake-api-key"));
    r.id = a;
    r.mode = ToolConfigMode::Official;
    switch(&state, &r);
    assert!(fs::read_to_string(active.join(".credentials.json"))
        .unwrap()
        .contains("fake-refresh-a-renewed"));
    let config = fs::read_to_string(active.join("settings.json")).unwrap();
    assert!(!config.contains("fake-api-key"));
    assert!(!config.contains("old-api-model"));
    assert!(config.contains("Read"));
    let profile: Value =
        serde_json::from_slice(&fs::read(home.join(".claude.json")).unwrap()).unwrap();
    assert_eq!(
        profile["oauthAccount"]["emailAddress"],
        "alice@example.test"
    );
    assert_eq!(profile["userPreference"], true);
}

#[test]
fn claude_quota_renewal_and_api_switch_share_the_account_lock() {
    let (_dir, state, home) = fixture();
    let active = home.join(".claude");
    write_claude(&active, "alice@example.test", "current");
    fs::rename(active.join(".claude.json"), home.join(".claude.json")).unwrap();
    let id = claude_register(&state, &active, "alice@example.test");
    let api = with_vault(&state, false, |v| {
        let mut input = crate::server::tests::sync_test_provider("API", "fake-api");
        input.interface_type = aipass_provider_registry::InterfaceType::AnthropicMessages;
        input.auth_scheme = aipass_provider_registry::AuthScheme::XApiKey;
        v.add_provider(input).map_err(map_vault_error)
    })
    .unwrap();
    let lock = native_account_lock(&state, id).unwrap();
    let guard = lock.lock().unwrap();
    assert_eq!(
        crate::official_accounts::refresh_claude(&state, id)
            .unwrap_err()
            .code,
        AgentErrorCode::Conflict
    );
    let mut r = request(api, ToolConfigMode::Plaintext);
    r.tool = ToolConfigTool::ClaudeCode;
    assert_eq!(
        apply(&state, r).unwrap().outcome,
        ToolConfigOutcome::Conflict
    );
    assert!(fs::read_to_string(active.join(".credentials.json"))
        .unwrap()
        .contains("fake-refresh-current"));
    drop(guard);
}
#[test]
fn backup_failure_and_repeated_confirmation_leave_current_credentials_untouched() {
    let (_dir, state, home) = fixture();
    let active = home.join(".codex");
    fs::create_dir_all(&active).unwrap();
    let bytes = codex_auth("alice@example.test", "a", "original");
    fs::write(active.join("auth.json"), &bytes).unwrap();
    let id = register(&state, &active);
    let r = request(id, ToolConfigMode::Official);
    fs::write(root(&state), b"blocked-backup-directory").unwrap();
    assert!(with_vault(&state, false, |v| apply_inner(&state, v, &r)).is_err());
    assert_eq!(fs::read(active.join("auth.json")).unwrap(), bytes);
    fs::remove_file(root(&state)).unwrap();
    let mut bound = r;
    bound.preview_id = Some(
        with_vault(&state, false, |v| {
            let (_, plan, _) = crate::server::build_tool_config_plan(v, &bound, true)?;
            preview_id(v, &state, &bound, &plan).map_err(safe_error)
        })
        .unwrap(),
    );
    switch(&state, &bound);
    assert_eq!(
        apply(&state, bound).unwrap().outcome,
        ToolConfigOutcome::Conflict
    );
}
#[test]
fn expired_unmanaged_history_is_never_replayed() {
    let (_dir, state, home) = fixture();
    let active = home.join(".codex");
    fs::create_dir_all(&active).unwrap();
    let mut auth: Value =
        serde_json::from_slice(&codex_auth("original@example.test", "original", "old")).unwrap();
    auth["tokens"]["access_token"] = json!(jwt(json!({"exp":1})));
    fs::write(active.join("auth.json"), auth.to_string()).unwrap();
    let id = with_vault(&state, false, |v| {
        v.add_provider(crate::server::tests::sync_test_provider(
            "API",
            "fake-api-key",
        ))
        .map_err(map_vault_error)
    })
    .unwrap();
    let result = switch(&state, &request(id, ToolConfigMode::Plaintext));
    let before = fs::read(active.join("auth.json")).unwrap();
    let error = rollback(&state, result.operation_id).unwrap_err();
    assert!(error.message.contains("expired"));
    assert_eq!(fs::read(active.join("auth.json")).unwrap(), before);
}

#[test]
fn reauthentication_rejects_other_accounts_and_keeps_the_selected_secret_id() {
    let (_dir, state, home) = fixture();
    let a = home.join("a");
    let signed_in = home.join("signed-in");
    write_claude(&a, "alice@example.test", "a");
    write_claude(&signed_in, "alice@example.test", "new");
    let id = claude_register(&state, &a, "alice@example.test");
    with_vault(&state, false, |v| {
        let before = reference(v, id, &ToolConfigTool::ClaudeCode).unwrap();
        let secret_ids = v
            .get_provider_summary(id)
            .unwrap()
            .secret_refs
            .iter()
            .map(|s| s.id.clone())
            .collect::<Vec<_>>();
        let wrong = crate::claude_cli::NativeAccount {
            home: signed_in.clone(),
            identity: "bob@example.test".into(),
            plan: None,
        };
        assert_eq!(
            login::bind_claude(&state, v, id, &wrong).unwrap_err().code,
            AgentErrorCode::Conflict
        );
        assert_eq!(
            reference(v, id, &ToolConfigTool::ClaudeCode).unwrap(),
            before
        );
        let correct = crate::claude_cli::NativeAccount {
            home: signed_in.clone(),
            identity: "alice@example.test".into(),
            plan: None,
        };
        assert_eq!(login::bind_claude(&state, v, id, &correct).unwrap(), id);
        assert_eq!(
            v.get_provider_summary(id)
                .unwrap()
                .secret_refs
                .iter()
                .map(|s| s.id.clone())
                .collect::<Vec<_>>(),
            secret_ids
        );
        assert_eq!(
            reference(v, id, &ToolConfigTool::ClaudeCode).unwrap()["nativeHome"],
            json!(signed_in)
        );
        Ok(())
    })
    .unwrap();
}
#[test]
fn api_probe_health_tracks_exact_key_and_does_not_block_a_key_update() {
    let (_dir, state, home) = fixture();
    fs::create_dir_all(home.join(".codex")).unwrap();
    let id = with_vault(&state, false, |v| {
        v.add_provider(crate::server::tests::sync_test_provider(
            "API",
            "fake-key-before",
        ))
        .map_err(map_vault_error)
    })
    .unwrap();
    switch(&state, &request(id, ToolConfigMode::Plaintext));
    let selected = with_vault(&state, false, |v| {
        Ok(v.get_provider_summary(id).unwrap().secret_refs[0].clone())
    })
    .unwrap();
    remember_probe(&state, id, Some(&selected), false, Some(401));
    assert_eq!(
        status_snapshot(&state, ToolConfigTool::Codex)
            .unwrap()
            .state,
        "api_key_invalid"
    );
    remember_probe(&state, id, Some(&selected), false, Some(429));
    assert_eq!(
        status_snapshot(&state, ToolConfigTool::Codex)
            .unwrap()
            .state,
        "quota_exhausted"
    );
    with_vault(&state, false, |v| {
        v.update_secret(
            id,
            &selected.id,
            &selected.label,
            Some("fake-key-after".into()),
        )
        .map_err(map_vault_error)
    })
    .unwrap();
    assert_eq!(
        status_snapshot(&state, ToolConfigTool::Codex)
            .unwrap()
            .state,
        "api_key_changed"
    );
    switch(&state, &request(id, ToolConfigMode::Plaintext));
    assert_eq!(
        status_snapshot(&state, ToolConfigTool::Codex)
            .unwrap()
            .state,
        "ready"
    );
}

#[test]
fn preview_rejects_a_changed_api_key_even_when_the_secret_id_is_stable() {
    let (_dir, state, _) = fixture();
    let id = with_vault(&state, false, |v| {
        v.add_provider(crate::server::tests::sync_test_provider(
            "API",
            "fake-before",
        ))
        .map_err(map_vault_error)
    })
    .unwrap();
    let mut r = request(id, ToolConfigMode::Plaintext);
    r.preview_id = Some(
        with_vault(&state, false, |v| {
            let (_, plan, _) = crate::server::build_tool_config_plan(v, &r, true)?;
            preview_id(v, &state, &r, &plan).map_err(safe_error)
        })
        .unwrap(),
    );
    with_vault(&state, false, |v| {
        let secret = v.get_provider_summary(id).unwrap().secret_refs[0].clone();
        v.update_secret(id, &secret.id, &secret.label, Some("fake-after".into()))
            .map_err(map_vault_error)
    })
    .unwrap();
    assert_eq!(
        apply(&state, r).unwrap().outcome,
        ToolConfigOutcome::Conflict
    );
}

#[test]
fn restoring_an_api_binding_uses_its_latest_key_and_rejects_a_stale_restore() {
    let (_dir, state, home) = fixture();
    let (a, b) = with_vault(&state, false, |v| {
        Ok((
            v.add_provider(crate::server::tests::sync_test_provider(
                "API A",
                "fake-a-old",
            ))
            .unwrap(),
            v.add_provider(crate::server::tests::sync_test_provider("API B", "fake-b"))
                .unwrap(),
        ))
    })
    .unwrap();
    switch(&state, &request(a, ToolConfigMode::Plaintext));
    let switched = switch(&state, &request(b, ToolConfigMode::Plaintext));
    with_vault(&state, false, |v| {
        let secret = v.get_provider_summary(a).unwrap().secret_refs[0].clone();
        v.update_secret(a, &secret.id, &secret.label, Some("fake-a-latest".into()))
            .unwrap();
        Ok(())
    })
    .unwrap();
    let restored = rollback(&state, switched.operation_id).unwrap();
    assert_eq!(restored.outcome, ToolConfigOutcome::Applied);
    assert_eq!(
        status_snapshot(&state, ToolConfigTool::Codex)
            .unwrap()
            .entry_id,
        Some(a)
    );
    let current = fs::read(home.join(".codex/auth.json")).unwrap();
    assert!(String::from_utf8_lossy(&current).contains("fake-a-latest"));
    assert_eq!(
        rollback(&state, switched.operation_id).unwrap_err().code,
        AgentErrorCode::Conflict
    );
    assert_eq!(fs::read(home.join(".codex/auth.json")).unwrap(), current);
}

#[test]
fn status_waits_for_the_switch_before_reading_its_binding() {
    let (_dir, state, _) = fixture();
    let (a, b) = with_vault(&state, false, |v| {
        Ok((
            v.add_provider(crate::server::tests::sync_test_provider("A", "fake-a"))
                .unwrap(),
            v.add_provider(crate::server::tests::sync_test_provider("B", "fake-b"))
                .unwrap(),
        ))
    })
    .unwrap();
    let guard = SWITCH_LOCK.lock().unwrap();
    switch(&state, &request(a, ToolConfigMode::Plaintext));
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (result_tx, result_rx) = std::sync::mpsc::channel();
    let reader_state = state.clone();
    let reader = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        result_tx
            .send(status(&reader_state, ToolConfigTool::Codex))
            .unwrap();
    });
    started_rx.recv().unwrap();
    let early = result_rx.recv_timeout(std::time::Duration::from_millis(100));
    switch(&state, &request(b, ToolConfigMode::Plaintext));
    drop(guard);
    let result = result_rx.recv_timeout(std::time::Duration::from_secs(5));
    reader.join().unwrap();
    assert!(matches!(
        early,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));
    assert_eq!(result.unwrap().unwrap().entry_id, Some(b));
}

#[test]
fn backup_failure_returns_storage_unavailable_without_a_write() {
    let (_dir, state, home) = fixture();
    let id = with_vault(&state, false, |v| {
        v.add_provider(crate::server::tests::sync_test_provider("API", "fake-new"))
            .map_err(map_vault_error)
    })
    .unwrap();
    fs::create_dir_all(home.join(".codex")).unwrap();
    fs::write(
        home.join(".codex/auth.json"),
        br#"{"OPENAI_API_KEY":"fake-original"}"#,
    )
    .unwrap();
    fs::write(root(&state), b"blocked-backup-directory").unwrap();
    assert_eq!(
        apply(&state, request(id, ToolConfigMode::Plaintext))
            .unwrap()
            .outcome,
        ToolConfigOutcome::StorageUnavailable
    );
    assert_eq!(
        fs::read(home.join(".codex/auth.json")).unwrap(),
        br#"{"OPENAI_API_KEY":"fake-original"}"#
    );
}

#[test]
fn codex_reauthentication_refuses_another_workspace_and_preserves_ids() {
    let (_dir, state, home) = fixture();
    let active = home.join(".codex");
    let wrong = home.join("wrong-workspace");
    fs::create_dir_all(&active).unwrap();
    fs::create_dir_all(&wrong).unwrap();
    fs::write(
        active.join("auth.json"),
        codex_auth("alice@example.test", "workspace-a", "original"),
    )
    .unwrap();
    fs::write(
        wrong.join("auth.json"),
        codex_auth("alice@example.test", "workspace-b", "wrong"),
    )
    .unwrap();
    let id = register(&state, &active);
    with_vault(&state, false, |v| {
        let before = reference(v, id, &ToolConfigTool::Codex).unwrap();
        let ids = v.get_provider_summary(id).unwrap().secret_refs;
        let wrong_auth = crate::subscriptions::cli_accounts::reference("codex", &wrong).unwrap();
        assert!(login::bind_codex(v, id, wrong_auth).is_err());
        assert_eq!(reference(v, id, &ToolConfigTool::Codex).unwrap(), before);
        assert_eq!(v.get_provider_summary(id).unwrap().secret_refs, ids);
        Ok(())
    })
    .unwrap();
    assert!(fs::read_to_string(active.join("auth.json"))
        .unwrap()
        .contains("fake-refresh-original"));
}
