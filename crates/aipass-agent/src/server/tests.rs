use super::*;
use aipass_agent_protocol::{SessionPolicy, SessionStatus};
use aipass_crypto::SecretString;
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use tempfile::tempdir;

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn explicit_local_creation_disables_cloud_and_keeps_the_typed_response_contract() {
    let temp = tempdir().unwrap();
    let state = sync_test_state(temp.path().join("vault"));
    let settings = crate::session::PersistedSyncSettings {
        mode: SyncMode::ICloud,
        ..Default::default()
    };
    aipass_storage::atomic_write_bytes(
        crate::session::sync_settings_path(&state.vault_dir),
        &serde_json::to_vec(&settings).unwrap(),
    )
    .unwrap();
    let response = handle_request(
        &state,
        AgentRequest::VaultCreate {
            password: "local-master-password".into(),
            local_only: true,
        },
    );
    assert!(response.ok);
    let created: VaultCreateResponse = serde_json::from_value(response.data).unwrap();
    assert!(created.session.exists && !created.session.locked);
    assert_eq!(
        load_sync_settings(&state.vault_dir).unwrap().mode,
        SyncMode::Local
    );
}

#[test]
fn proxy_endpoint_matches_each_client_sdk_contract() {
    let bind = "127.0.0.1:8787";
    assert_eq!(
        proxy_endpoint_for_tool(&ToolId::ClaudeCode, ProxyProtocol::AnthropicMessages, bind),
        "http://127.0.0.1:8787"
    );
    assert_eq!(
        proxy_endpoint_for_tool(&ToolId::OpenCode, ProxyProtocol::AnthropicMessages, bind),
        "http://127.0.0.1:8787/v1"
    );
    assert_eq!(
        proxy_endpoint_for_tool(&ToolId::OpenCode, ProxyProtocol::OpenAiResponses, bind),
        "http://127.0.0.1:8787/v1"
    );
    assert_eq!(
        proxy_endpoint_for_tool(&ToolId::Cursor, ProxyProtocol::OpenAiChatCompletions, bind),
        "http://127.0.0.1:8787/v1"
    );
    assert_eq!(
        proxy_endpoint_for_tool(&ToolId::Cursor, ProxyProtocol::AnthropicMessages, bind),
        "http://127.0.0.1:8787/v1/messages"
    );
}

#[test]
fn proxy_tool_capabilities_distinguish_endpoint_and_protocol_limits() {
    assert!(ensure_proxy_tool_protocol(&ToolId::OpenCode, ProxyProtocol::OpenAiResponses).is_ok());
    assert!(
        ensure_proxy_tool_protocol(&ToolId::OpenCode, ProxyProtocol::AnthropicMessages).is_ok()
    );
    let codex_error =
        ensure_proxy_tool_protocol(&ToolId::Codex, ProxyProtocol::OpenAiChatCompletions)
            .expect_err("Codex should require Responses");
    assert!(codex_error.message.contains("protocol"));
    assert!(
        ensure_proxy_tool_protocol(&ToolId::Cursor, ProxyProtocol::OpenAiChatCompletions).is_ok()
    );
    assert!(ensure_proxy_tool_protocol(&ToolId::Cursor, ProxyProtocol::AnthropicMessages).is_ok());
    assert!(ensure_proxy_tool_protocol(&ToolId::Cursor, ProxyProtocol::OpenAiResponses).is_ok());
}

struct EnvRestore {
    name: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvRestore {
    fn capture(name: &'static str) -> Self {
        Self {
            name,
            previous: std::env::var_os(name),
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.name, value),
            None => std::env::remove_var(self.name),
        }
    }
}

struct RunningAgent {
    root: PathBuf,
    vault_dir: PathBuf,
    client: crate::AgentClient,
    handle: Option<thread::JoinHandle<()>>,
}

impl RunningAgent {
    fn start() -> Self {
        let dir = tempdir().expect("tempdir");
        let vault_dir = dir.path().join("vault");
        aipass_vault::Vault::create(
            &vault_dir,
            &SecretString::new("correct horse battery staple"),
        )
        .expect("create vault");
        // Pin sync to a temp folder: macOS defaults to iCloud when no
        // sync settings file exists, and startup sync must never write
        // into a real cloud directory on the host running the tests.
        let settings = crate::session::PersistedSyncSettings {
            mode: SyncMode::Local,
            sync_folder: Some(dir.path().join("sync")),
            ..Default::default()
        };
        let settings_path = crate::session::sync_settings_path(&vault_dir);
        fs::create_dir_all(settings_path.parent().expect("settings parent"))
            .expect("create settings dir");
        atomic_write_bytes(
            &settings_path,
            &serde_json::to_vec_pretty(&settings).expect("encode settings"),
        )
        .expect("write sync settings");
        let root = dir.keep();
        let server_vault_dir = vault_dir.clone();
        let handle = thread::spawn(move || {
            run_server(ServerOptions::without_desktop_tray(server_vault_dir)).expect("server");
        });
        let client = crate::AgentClient::for_vault(vault_dir.clone()).expect("client");
        for _ in 0..50 {
            if client
                .request::<SessionStatus>(&AgentRequest::SessionStatus)
                .is_ok()
            {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        Self {
            root,
            vault_dir,
            client,
            handle: Some(handle),
        }
    }
}

impl Drop for RunningAgent {
    fn drop(&mut self) {
        let _ = self.client.shutdown();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn sequential_status_requests_do_not_pay_an_accept_poll_delay() {
    let agent = RunningAgent::start();
    let started = Instant::now();
    for _ in 0..12 {
        agent
            .client
            .request::<SessionStatus>(&AgentRequest::SessionStatus)
            .expect("session status");
    }
    let elapsed = started.elapsed();
    eprintln!("12 sequential status requests: {elapsed:?}");
    assert!(elapsed < Duration::from_secs(1), "elapsed {elapsed:?}");
}

#[test]
fn constant_time_eq_checks_full_input() {
    assert!(constant_time_eq(b"same", b"same"));
    assert!(!constant_time_eq(b"same", b"some"));
    assert!(!constant_time_eq(b"same", b"same-longer"));
    assert!(!constant_time_eq(b"same-longer", b"same"));
}

#[test]
fn endpoint_interface_inference_follows_shared_ai_evidence() {
    assert_eq!(
        infer_interface_from_endpoint("https://llm.example.test/api/paas/v4"),
        Some(InterfaceType::OpenAiCompatible)
    );
    assert_eq!(
        infer_interface_from_endpoint("https://llm.example.test/v1beta/models"),
        Some(InterfaceType::OpenAiCompatible)
    );
    assert_eq!(
        infer_interface_from_endpoint("https://claude-relay.example.test/v1/messages"),
        Some(InterfaceType::AnthropicMessages)
    );
    assert_eq!(
        infer_interface_from_endpoint("https://gemini-proxy.example.test/v1beta/models"),
        Some(InterfaceType::Gemini)
    );
    assert_eq!(
        infer_interface_from_endpoint("https://api.minimaxi.com/v1"),
        Some(InterfaceType::OpenAiCompatible)
    );
    assert_eq!(
        infer_interface_from_endpoint("https://example.test/hooks/deploy"),
        None
    );
    assert_eq!(
        infer_interface_from_endpoint("https://api.replicate.com"),
        None
    );
}

#[test]
fn current_process_options_respect_tray_suppression_env() {
    let _guard = env_lock().lock().unwrap();
    let _restore = EnvRestore::capture(crate::desktop::SUPPRESS_TRAY_ENV);
    let vault_dir = PathBuf::from("/tmp/aipass-test-vault");

    std::env::remove_var(crate::desktop::SUPPRESS_TRAY_ENV);
    assert!(ServerOptions::for_current_process(vault_dir.clone()).launch_desktop_tray);

    std::env::set_var(crate::desktop::SUPPRESS_TRAY_ENV, "1");
    assert!(!ServerOptions::for_current_process(vault_dir.clone()).launch_desktop_tray);

    std::env::set_var(crate::desktop::SUPPRESS_TRAY_ENV, "0");
    assert!(ServerOptions::for_current_process(vault_dir).launch_desktop_tray);
}

#[test]
fn auth_json_preview_includes_plaintext_credentials() {
    let dir = tempdir().expect("tempdir");
    let target = dir.path().join("auth.json");
    fs::write(
        &target,
        r#"{"OPENAI_API_KEY":"sk-old-secret","other":true}"#,
    )
    .expect("write old config");
    let content = "{\n  \"OPENAI_API_KEY\": \"sk-new-secret\",\n  \"other\": true\n}";
    let plan = ConfigPlan {
        operation_id: Uuid::new_v4(),
        tool: ToolId::Codex,
        target_path: target.clone(),
        backup_path: target.with_extension("backup"),
        summary: "preview".to_string(),
        preview: aipass_config_writers::diff_preview_for_path(&target, content),
        extra_writes: Vec::new(),
        codex_session_migration: None,
        codex_provider_migration: None,
    };

    let files = tool_config_preview_files(&plan, content);
    assert_eq!(files.len(), 1);
    assert!(files[0].content.contains("sk-new-secret"));
    assert!(files[0].diff.contains("sk-old-secret"));
    assert!(files[0].diff.contains("sk-new-secret"));
    assert_eq!(combined_tool_config_preview(&files), files[0].diff);
}

#[test]
fn native_subscription_references_cannot_be_written_as_tool_credentials() {
    let dir = tempdir().unwrap();
    let creation = Vault::create(
        dir.path().join("vault"),
        &SecretString::new("fixture-password"),
    )
    .unwrap();
    for provider in [
        "anthropic",
        "codex",
        "openai",
        "grok",
        "xai",
        "copilot",
        "gemini-cli",
    ] {
        let mut input = sync_test_provider(provider, "fixture-routing-reference");
        input.provider_kind = ProviderKind::Official;
        input.credential_kind = CredentialKind::OAuth;
        input.provider_id = Some(provider.into());
        let id = creation.vault.add_provider(input).unwrap();
        for mode in [
            ToolConfigMode::Plaintext,
            ToolConfigMode::Helper,
            ToolConfigMode::Official,
        ] {
            let request = ToolConfigRequest {
                id,
                secret_id: None,
                tool: ToolConfigTool::Codex,
                mode,
                codex_api_key_mode: None,
            };
            for preview in [true, false] {
                let error = match build_tool_config_plan(&creation.vault, &request, preview) {
                    Err(error) => error,
                    Ok(_) => panic!("subscription reference must not become a tool credential"),
                };
                assert_eq!(error.code, AgentErrorCode::ValidationFailed);
                assert!(error.message.contains("local proxy route"));
            }
        }
    }
}

#[test]
fn tool_configuration_binds_the_selected_key_and_its_overrides() {
    let dir = tempdir().unwrap();
    let creation = Vault::create(
        dir.path().join("vault"),
        &SecretString::new("fixture-password"),
    )
    .unwrap();
    let vault = &creation.vault;
    TOOL_HOME_OVERRIDES
        .lock()
        .unwrap()
        .insert(vault.vault_id(), dir.path().join("home"));
    let id = vault
        .add_provider(sync_test_provider("Mixed gateway", "fixture-openai-key"))
        .unwrap();
    let second = vault
        .add_secret_with_metadata(
            id,
            "Claude",
            "fixture-anthropic-key",
            &aipass_vault::SecretMetadataInput {
                interface_type: Some(InterfaceType::AnthropicMessages),
                endpoint: Some("https://claude.example.test/v1".into()),
                default_model: Some("fixture-claude".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let mut request = ToolConfigRequest {
        id,
        secret_id: None,
        tool: ToolConfigTool::ClaudeCode,
        mode: ToolConfigMode::Plaintext,
        codex_api_key_mode: None,
    };
    assert!(
        build_tool_config_plan(vault, &request, true).is_err(),
        "multiple keys require selection"
    );
    request.secret_id = Some(second.clone());
    let (entry, _, content) = build_tool_config_plan(vault, &request, true).unwrap();
    assert_eq!(entry.auth_scheme, AuthScheme::XApiKey);
    assert!(content.contains("fixture-anthropic-key"));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&content).unwrap()["env"]["ANTHROPIC_BASE_URL"],
        "https://claude.example.test"
    );
    assert!(!content.contains("fixture-openai-key"));

    // The detail pane's default Claude entry point is helper mode. Both
    // preview views and the actual writer must use this exact key.
    request.mode = ToolConfigMode::Helper;
    let (_, preview_plan, preview_content) = build_tool_config_plan(vault, &request, true).unwrap();
    let files = tool_config_preview_files(&preview_plan, &preview_content);
    let command = format!("aipass get {id} --secret-id '{second}' --reveal");
    let config: serde_json::Value = serde_json::from_str(&files[0].content).unwrap();
    assert_eq!(config["apiKeyHelper"], command);
    assert_eq!(config["env"]["ANTHROPIC_MODEL"], "fixture-claude");
    assert!(files[0].diff.contains(&second));
    assert!(!files[0].content.contains("fixture-anthropic-key"));
    assert!(!preview_plan.target_path.exists());
    let (_, apply_plan, apply_content) = build_tool_config_plan(vault, &request, false).unwrap();
    assert_eq!(apply_plan.target_path, preview_plan.target_path);
    assert_eq!(apply_content, files[0].content);
    let applied =
        apply_plan_encrypted(&apply_plan, &apply_content, &vault.config_backup_key()).unwrap();
    assert_eq!(
        fs::read_to_string(&applied.target_path).unwrap(),
        files[0].content
    );
    rollback_encrypted(&applied.backup_path, &vault.config_backup_key()).unwrap();
    assert!(!applied.target_path.exists());

    // Editing metadata changes the generated base/model without rebinding
    // the helper; clearing overrides inherits the provider's versioned base.
    vault
        .set_secret_metadata(
            id,
            &second,
            &SecretMetadataInput {
                interface_type: Some(InterfaceType::AnthropicMessages),
                endpoint: Some(String::new()),
                default_model: Some(String::new()),
                ..Default::default()
            },
        )
        .unwrap();
    let (_, _, inherited) = build_tool_config_plan(vault, &request, true).unwrap();
    let inherited: serde_json::Value = serde_json::from_str(&inherited).unwrap();
    assert_eq!(inherited["apiKeyHelper"], command);
    assert_eq!(inherited["env"]["ANTHROPIC_BASE_URL"], "http://127.0.0.1:9");
    assert!(inherited["env"].get("ANTHROPIC_MODEL").is_none());
    vault
        .set_secret_metadata(
            id,
            &second,
            &SecretMetadataInput {
                interface_type: Some(InterfaceType::AnthropicMessages),
                default_model: Some("fixture-claude".into()),
                ..Default::default()
            },
        )
        .unwrap();
    request.tool = ToolConfigTool::Codex;
    assert!(
        build_tool_config_plan(vault, &request, true).is_err(),
        "incompatible selected format"
    );
    request.tool = ToolConfigTool::Grok;
    request.mode = ToolConfigMode::Helper;
    let (_, plan, content) = build_tool_config_plan(vault, &request, true).unwrap();
    assert!(content.contains("fixture-claude"));
    let helper = &plan.extra_writes.last().unwrap().content;
    assert!(helper.contains(&format!("--secret-id '{second}' --reveal")));
    assert!(!helper.contains("--field api_key"));
    assert!(!helper.contains("fixture-anthropic-key"));
    vault.remove_secret(id, &second).unwrap();
    assert!(
        build_tool_config_plan(vault, &request, true).is_err(),
        "deleted key cannot fall back"
    );
    request.tool = ToolConfigTool::ClaudeCode;
    assert!(build_tool_config_plan(vault, &request, false).is_err());
    request.secret_id = Some(
        vault.get_provider_summary(id).unwrap().secret_refs[0]
            .id
            .clone(),
    );
    assert!(
        build_tool_config_plan(vault, &request, true).is_err(),
        "Claude rejects the OpenAI key"
    );
    request.tool = ToolConfigTool::Codex;
    request.mode = ToolConfigMode::Plaintext;
    let (_, plan, content) = build_tool_config_plan(vault, &request, true).unwrap();
    assert!(content.contains("http://127.0.0.1:9/v1"));
    assert!(plan
        .extra_writes
        .iter()
        .any(|write| write.content.contains("fixture-openai-key")));
    TOOL_HOME_OVERRIDES
        .lock()
        .unwrap()
        .remove(&vault.vault_id());
}

#[test]
fn agent_starts_locked_even_when_the_vault_already_exists() {
    let agent = RunningAgent::start();
    let status = agent
        .client
        .request::<SessionStatus>(&AgentRequest::SessionStatus)
        .expect("status response");

    assert!(status.exists);
    assert!(status.locked);
    assert_eq!(status.last_lock_reason, Some(LockReason::AgentRestart));
}

#[test]
fn initial_sync_runs_at_startup_and_clears_the_pending_flag() {
    let agent = RunningAgent::start();

    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let status = agent
            .client
            .request::<SessionStatus>(&AgentRequest::SessionStatus)
            .expect("status response");
        if !status.initial_sync_pending {
            break;
        }
        assert!(Instant::now() < deadline, "initial sync stuck pending");
        thread::sleep(Duration::from_millis(50));
    }

    // A locked vault has no root key with which to authenticate a new
    // snapshot. Startup remains responsive and publication waits for unlock.
    assert!(!agent.root.join("sync/snapshots").exists());
}

pub(crate) fn sync_test_state(vault_dir: PathBuf) -> Arc<AgentState> {
    atomic_write_bytes(
        crate::session::sync_settings_path(&vault_dir),
        &serde_json::to_vec(&crate::session::PersistedSyncSettings::default()).unwrap(),
    )
    .unwrap();
    Arc::new(AgentState {
        control_panel: Default::default(),
        policy: Mutex::new(SessionPolicy::default()),
        vault_dir: vault_dir.clone(),
        namespace: "test".to_string(),
        auth_token: SensitiveString::from("token"),
        session: Mutex::new(SessionState::Locked),
        session_changed: Condvar::new(),
        last_lock_reason: Mutex::new(None),
        proxy: Mutex::new(
            crate::proxy_service::ProxyService::new(&vault_dir).expect("proxy service"),
        ),
        favicon_backfill: Mutex::new(()),
        sync_lock: Mutex::new(()),
        cloudkit: Default::default(),
        webdav_transport: Mutex::new(None),
        sync_wake: std::sync::atomic::AtomicU64::new(0),
        initial_sync: Mutex::new(InitialSyncState::Done),
        sync_revision: std::sync::atomic::AtomicU64::new(0),
        sync_status: Mutex::new(None),
        sync_watcher: Mutex::new(None),
        shutdown: AtomicBool::new(false),
    })
}

pub(crate) fn sync_test_provider(title: &str, api_key: &str) -> ProviderEntryInput {
    ProviderEntryInput {
        max_concurrent_requests: None,
        supports_websockets: None,
        title: title.to_string(),
        provider_kind: ProviderKind::Unknown,
        provider_id: Some("openai".to_string()),
        credential_kind: Default::default(),
        account_identity: None,
        domains: Vec::new(),
        favicon_url: None,
        endpoints: vec![ProviderEndpoint::api("http://127.0.0.1:9/v1")],
        interface_type: InterfaceType::OpenAiCompatible,
        auth_scheme: AuthScheme::Bearer,
        api_key: api_key.to_string(),
        secret_label: None,
        default_model: None,
        model_aliases: Vec::new(),
        headers: Vec::new(),
        quota: None,
        subscription: None,
        gateway: None,
        tags: Vec::new(),
        notes: None,
        secret_metadata: Default::default(),
    }
}

#[test]
fn provider_operations_are_correlated_persistent_and_secret_free() {
    let temp = tempdir().unwrap();
    let vault_dir = temp.path().join("vault");
    let creation = Vault::create(&vault_dir, &SecretString::new("test-master-password")).unwrap();
    let state = sync_test_state(vault_dir);
    crate::session::set_session_vault(&state, creation.vault);
    let log_dir = temp.path().join("logs");
    let request_id = Uuid::new_v4();
    let provider_id = crate::logging::with_test_log_dir(&log_dir, || {
        let _scope = crate::logging::RequestScope::new(request_id);
        let input = sync_test_provider("private-provider-title", "fake-private-api-key");
        let response = handle_request(
            &state,
            AgentRequest::ProviderAdd {
                input: input.clone(),
            },
        );
        assert!(response.ok);
        let id: Uuid = serde_json::from_value(response.data).unwrap();
        let update = serde_json::from_value(serde_json::to_value(input).unwrap()).unwrap();
        for request in [
            AgentRequest::ProviderUpdate { id, input: update },
            AgentRequest::SecretAdd {
                id,
                label: "private-label".into(),
                secret: "fake-second-key".into(),
                metadata: None,
            },
            AgentRequest::ProviderArchive { id },
            AgentRequest::ProviderRestore { id },
            AgentRequest::ProviderTrash { id },
            AgentRequest::ProviderDelete { id },
        ] {
            assert!(handle_request(&state, request).ok);
        }
        assert!(!handle_request(&state, AgentRequest::ProviderGet { id }).ok);
        assert!(handle_request(&state, AgentRequest::SessionStatus).ok);
        id
    });
    let logs = fs::read_to_string(log_dir.join("agent.log")).unwrap();
    for event in [
        "provider.add",
        "provider.update",
        "secret.add",
        "provider.archive",
        "provider.restore",
        "provider.trash",
        "provider.delete",
    ] {
        assert!(logs.contains(&format!("event={event} outcome=started")));
        assert!(logs.contains(&format!(
            "event={event} outcome=completed resource_id={provider_id}"
        )));
    }
    assert!(logs.contains("event=provider.get outcome=failed"));
    assert!(logs
        .lines()
        .all(|line| line.contains(&format!("request_id={request_id}"))));
    for forbidden in [
        "private-provider-title",
        "fake-private-api-key",
        "fake-second-key",
        "private-label",
        "test-master-password",
        "127.0.0.1:9",
        "event=session.status",
    ] {
        assert!(
            !logs.contains(forbidden),
            "unexpected log content: {forbidden}"
        );
    }
}

#[test]
fn sync_download_reloads_the_unlocked_vault_and_keeps_the_proxy_serving() {
    let temp = tempdir().expect("tempdir");
    let vault_dir = temp.path().join("vault");
    let sync_dir = temp.path().join("sync");
    let password = SecretString::new("correct horse battery staple");
    let creation = Vault::create(&vault_dir, &password).expect("create vault");
    let state = sync_test_state(vault_dir.clone());
    crate::session::set_session_vault(&state, creation.vault);

    // Provider A is wired into a running proxy route group.
    let provider_a = with_vault(&state, false, |vault| {
        vault
            .add_provider(sync_test_provider("Upstream A", "key-a"))
            .map_err(map_vault_error)
    })
    .expect("add provider A");
    let secret_a = with_vault(&state, true, |vault| {
        vault
            .get_provider_summary(provider_a)
            .map_err(map_vault_error)
    })
    .expect("provider A summary")
    .secret_refs[0]
        .id
        .clone();
    let proxy_probe = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve proxy address");
    let proxy_addr = proxy_probe.local_addr().expect("proxy address").to_string();
    drop(proxy_probe);
    with_vault(&state, false, |vault| {
        let mut proxy = state
            .proxy
            .lock()
            .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "proxy lock poisoned"))?;
        proxy.set_config(
            vault,
            aipass_proxy::ProxyConfig {
                bind_addr: proxy_addr,
                routes: vec![aipass_proxy::ProxyRouteConfig {
                    id: Uuid::new_v4(),
                    name: "default".into(),
                    token: "route-token".into(),
                    inbound_protocol: aipass_proxy::Protocol::OpenAiResponses,
                    upstream_protocol: aipass_proxy::Protocol::OpenAiResponses,
                    conversion_enabled: false,
                    strategy: aipass_proxy::RouteStrategy::Fallback,
                    targets: vec![aipass_proxy::ProxyTargetConfig {
                        id: Uuid::new_v4(),
                        provider_entry_id: provider_a,
                        secret_id: secret_a,
                        label: "primary".into(),
                        base_url: "http://127.0.0.1:9/v1".into(),
                        auth_scheme: "bearer".into(),
                        headers: Vec::new(),
                        group: None,
                        priority: 0,
                        weight: 1,
                        enabled: true,
                        protocol: None,
                        prefer_ws: false,
                    }],
                    retry: aipass_proxy::RetryPolicy::default(),
                    enabled: true,
                }],
                ..Default::default()
            },
        )?;
        proxy.start(vault)?;
        Ok(())
    })
    .expect("start proxy");

    let first = run_sync_local(&state, &sync_dir).expect("initial sync");
    assert_eq!(first.downloaded, 0);

    // A second device restores the complete encrypted snapshot, changes
    // it, and publishes a descendant while the first proxy stays running.
    let other_dir = temp.path().join("other/vault");
    fs::create_dir_all(&other_dir).unwrap();
    let other = sync_test_state(other_dir.clone());
    run_sync_local(&other, &sync_dir).unwrap();
    let other_vault = Vault::open(&other_dir, &password).unwrap();
    crate::session::set_session_vault(&other, other_vault);
    // Establish the imported revision before mutating it.
    run_sync_local(&other, &sync_dir).unwrap();
    with_vault(&other, false, |vault| {
        vault
            .add_provider(sync_test_provider("Upstream B", "key-b"))
            .map_err(map_vault_error)
    })
    .unwrap();
    run_sync_local(&other, &sync_dir).unwrap();

    let activity_before = match &*state.session.lock().unwrap() {
        SessionState::Unlocked(info) => info.last_activity_at,
        _ => panic!("session must remain unlocked"),
    };
    let second = run_sync_local(&state, &sync_dir).expect("sync with download");
    assert_eq!(second.downloaded, 1);
    assert!(
        matches!(&*state.session.lock().unwrap(), SessionState::Unlocked(info) if info.last_activity_at == activity_before)
    );

    // The in-memory session vault sees the downloaded record, and the
    // running proxy rebuilt its snapshot without going down.
    let titles = with_vault(&state, true, |vault| {
        vault.list_provider_summaries().map_err(map_vault_error)
    })
    .expect("list providers")
    .into_iter()
    .map(|entry| entry.title)
    .collect::<Vec<_>>();
    assert!(titles.iter().any(|title| title == "Upstream B"));
    assert!(state.proxy.lock().expect("proxy lock").status().running);
    // Removing a synced credential must not disable the listening service.
    with_vault(&other, false, |vault| {
        vault
            .delete_provider_permanently(provider_a)
            .map_err(map_vault_error)
    })
    .unwrap();
    run_sync_local(&other, &sync_dir).unwrap();
    run_sync_local(&state, &sync_dir).unwrap();
    assert!(state.proxy.lock().unwrap().status().running);
    assert!(!session_status(&state).unwrap().locked);
}

#[test]
fn incomplete_connection_does_not_block_subsequent_requests() {
    let agent = RunningAgent::start();
    let mut stuck = crate::ipc::connect(&agent.vault_dir).expect("connect stuck client");
    stuck.write_all(&8_u32.to_le_bytes()).expect("write length");

    let started = Instant::now();
    let status = agent
        .client
        .request::<SessionStatus>(&AgentRequest::SessionStatus)
        .expect("status response");

    assert!(status.exists);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "status request was blocked behind incomplete connection"
    );
}

fn favicon_test_entry() -> EntrySummary {
    EntrySummary {
        max_concurrent_requests: None,
        supports_websockets: None,
        websocket_warning: None,
        id: Uuid::new_v4(),
        title: "Example".to_string(),
        favorite: false,
        provider_id: None,
        provider_kind: aipass_provider_registry::ProviderKind::Unknown,
        credential_kind: Default::default(),
        account_identity: None,
        domains: vec!["example.com".to_string()],
        favicon_url: None,
        endpoints: vec![ProviderEndpoint::api("https://api.example.com/v1")],
        interface_type: InterfaceType::OpenAiCompatible,
        auth_scheme: AuthScheme::Bearer,
        masked_secret: "****".to_string(),
        fingerprint: "fp".to_string(),
        secret_refs: Vec::new(),
        default_model: None,
        model_aliases: Vec::new(),
        quota: None,
        subscription: None,
        gateway: None,
        usage_source: None,
        tags: Vec::new(),
        notes: None,
        header_names: Vec::new(),
        created_at: OffsetDateTime::now_utc(),
        updated_at: OffsetDateTime::now_utc(),
        last_used_at: None,
        archived_at: None,
        deleted_at: None,
    }
}

#[test]
fn favicon_candidates_follow_expected_source_order() {
    let mut entry = favicon_test_entry();
    entry.provider_id = Some("anthropic".to_string());
    entry.domains = vec!["domain.example".to_string()];
    entry.endpoints = vec![
        ProviderEndpoint::console("https://portal.example/settings"),
        ProviderEndpoint::api("https://api.example/v1"),
    ];

    let candidates = favicon_url_candidates(&entry);

    assert_eq!(
        candidates,
        vec![
            "https://console.anthropic.com/favicon.ico".to_string(),
            "https://portal.example/favicon.ico".to_string(),
            "https://domain.example/favicon.ico".to_string(),
            "https://api.example/favicon.ico".to_string(),
        ]
    );
}

#[test]
fn favicon_candidates_skip_localhost_and_private_ip_literals() {
    for value in [
        "localhost",
        "http://localhost:3000/app",
        "127.0.0.1",
        "10.0.0.1",
        "169.254.1.1",
        "http://[::1]/",
        "http://[fe80::1]/",
        "http://[fc00::1]/",
    ] {
        assert_eq!(favicon_url_from_origin_candidate(value), None, "{value}");
    }
    assert_eq!(
        favicon_url_from_origin_candidate("example.com/path").as_deref(),
        Some("https://example.com/favicon.ico")
    );
}

#[test]
fn favicon_dns_results_reject_any_private_address() {
    assert!(favicon_resolved_addresses_are_public([
        "1.1.1.1".parse().unwrap(),
        "2606:4700:4700::1111".parse().unwrap(),
    ]));
    assert!(!favicon_resolved_addresses_are_public([
        "1.1.1.1".parse().unwrap(),
        "127.0.0.1".parse().unwrap(),
    ]));
    assert!(!favicon_resolved_addresses_are_public(["10.0.0.4"
        .parse()
        .unwrap(),]));
    assert!(!favicon_resolved_addresses_are_public([]));
}

#[test]
fn favicon_magic_detection_accepts_bounded_bitmap_formats() {
    assert_eq!(
        favicon_image_mime(b"\x89PNG\r\n\x1a\nrest"),
        Some("image/png")
    );
    assert_eq!(
        favicon_image_mime(&[0xff, 0xd8, 0xff, 0xe0]),
        Some("image/jpeg")
    );
    assert_eq!(favicon_image_mime(b"GIF89arest"), Some("image/gif"));
    assert_eq!(
        favicon_image_mime(b"RIFF\x04\x00\x00\x00WEBPrest"),
        Some("image/webp")
    );
    assert_eq!(
        favicon_image_mime(&[0, 0, 1, 0, 1, 0]),
        Some("image/x-icon")
    );
    assert_eq!(favicon_image_mime(b"<svg></svg>"), None);
    assert_eq!(favicon_image_mime(b"not an image"), None);
}

#[test]
fn favicon_backfill_migrates_remote_urls_and_skips_cached_data() {
    let mut entry = favicon_test_entry();
    entry.favicon_url = Some("https://example.com/favicon.ico".to_string());
    assert!(!favicon_backfill_entry_is_skippable(&entry));
    assert_eq!(
        favicon_url_candidates(&entry).first().map(String::as_str),
        Some("https://example.com/favicon.ico")
    );

    entry.favicon_url = Some("data:image/png;base64,iVBORw0KGgo=".to_string());
    assert!(favicon_backfill_entry_is_skippable(&entry));
}
#[test]
fn provider_probe_shares_responses_endpoint_and_credentials_with_the_proxy() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for ws in [false, true] {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let count = socket.read(&mut chunk).unwrap();
                assert!(count > 0);
                request.extend_from_slice(&chunk[..count]);
            }
            let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
            assert!(request.contains("authorization: bearer probe-test-key"));
            assert!(request.contains("x-provider-test: required"));
            assert!(request.starts_with(if ws {
                "get /v1/responses "
            } else {
                "get /v1/models "
            }));
            let response = if ws {
                assert!(request.contains("upgrade: websocket"));
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_owned()
            } else {
                let body = r#"{"data":[{"id":"test-model"}]}"#;
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
            };
            socket.write_all(response.as_bytes()).unwrap();
        }
    });
    let mut entry = favicon_test_entry();
    entry.interface_type = InterfaceType::OpenAiCompatible;
    entry.auth_scheme = AuthScheme::Bearer;
    entry.provider_kind = ProviderKind::Unknown;
    entry.endpoints = vec![ProviderEndpoint {
        kind: EndpointKind::Api,
        url: Some(format!("http://{address}")),
        id: "api".into(),
        region: None,
        deployment: None,
        api_version: None,
    }];
    let result = probe_entry(
        entry,
        "probe-test-key".into(),
        3,
        vec![("x-provider-test".into(), "required".into())],
        aipass_proxy::UpstreamProxyConfig {
            mode: aipass_proxy::UpstreamProxyMode::Direct,
            custom_url: None,
        },
        None,
    );
    assert!(result.ok);
    assert_eq!(result.model_count, Some(1));
    let websocket = result.websocket.unwrap();
    assert_eq!(websocket.supported, Some(false));
    assert_eq!(websocket.status, Some(404));
    server.join().unwrap();
}
