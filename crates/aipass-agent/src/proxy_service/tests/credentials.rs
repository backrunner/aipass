use super::*;

#[test]
fn claude_cli_inherits_global_proxy_and_respects_provider_override() {
    use aipass_agent_protocol::{ProviderProxyOptions, ProviderRuntimeOptions};
    use aipass_proxy::{UpstreamKind, UpstreamProxyConfig, UpstreamProxyMode};
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::create(temp.path(), &SecretString::new("test master"))
        .unwrap()
        .vault;
    let mut input = provider_input(
        "aipass:claude-cli:fixture",
        "https://api.anthropic.com".into(),
        "header",
    );
    input.provider_kind = ProviderKind::Official;
    input.provider_id = Some("anthropic".into());
    input.credential_kind = aipass_provider_registry::CredentialKind::OAuth;
    input.interface_type = InterfaceType::AnthropicMessages;
    input.supports_websockets = Some(true);
    let id = vault.add_provider(input).unwrap();
    let mut service = ProxyService::new(temp.path()).unwrap();
    service.config = config_with_token("claude-proxy-test");
    service.config.upstream_proxy = UpstreamProxyConfig {
        mode: UpstreamProxyMode::Custom,
        custom_url: Some("http://127.0.0.1:12345".into()),
    };
    service.config.routes[0].targets = vec![ProxyTargetConfig {
        id: Uuid::new_v4(),
        provider_entry_id: id,
        secret_id: vault.get_provider_summary(id).unwrap().secret_refs[0]
            .id
            .clone(),
        label: "Claude".into(),
        base_url: "https://api.anthropic.com".into(),
        auth_scheme: "bearer".into(),
        headers: Vec::new(),
        group: None,
        priority: 0,
        weight: 1,
        enabled: true,
        protocol: None,
        prefer_ws: true,
    }];
    let runtime = service.runtime_config(&vault).unwrap();
    let target = &runtime.routes[0].targets[0];
    assert_eq!(target.upstream_kind, UpstreamKind::ClaudeSubscription);
    assert_eq!(
        target.upstream_proxy.as_ref(),
        Some(&service.config.upstream_proxy)
    );
    assert!(!target.supports_websockets);
    crate::provider_runtime::save(
        &vault,
        id,
        ProviderRuntimeOptions {
            proxy: Some(ProviderProxyOptions {
                mode: UpstreamProxyMode::Direct,
                url: None,
                username: None,
                password: None,
                has_credentials: false,
                clear_credentials: false,
            }),
            ..Default::default()
        },
    )
    .unwrap();
    let runtime = service.runtime_config(&vault).unwrap();
    assert_eq!(
        runtime.routes[0].targets[0]
            .upstream_proxy
            .as_ref()
            .unwrap()
            .mode,
        UpstreamProxyMode::Direct
    );
}

#[test]
fn runtime_config_uses_configured_route_protocol_without_provider_inference() {
    let temp = tempfile::tempdir().expect("tempdir");
    let creation = Vault::create(
        temp.path(),
        &SecretString::new("correct horse battery staple"),
    )
    .expect("create vault");
    let provider_id = creation
        .vault
        .add_provider(provider_input(
            "upstream-key",
            "http://127.0.0.1:9/v1".into(),
            "header",
        ))
        .expect("add provider");
    let secret_id = creation
        .vault
        .get_provider_summary(provider_id)
        .expect("provider summary")
        .secret_refs[0]
        .id
        .clone();
    let mut service = ProxyService::new(temp.path()).expect("proxy service");
    service.config = config_with_token("aipass-protocol-drift");
    service.config.routes[0].targets = vec![ProxyTargetConfig {
        id: Uuid::new_v4(),
        provider_entry_id: provider_id,
        secret_id,
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
    }];
    assert!(service.runtime_config(&creation.vault).is_ok());

    creation
        .vault
        .update_provider(
            provider_id,
            ProviderEntryUpdateInput {
                max_concurrent_requests: None,
                supports_websockets: None,
                title: "Proxy upstream".into(),
                provider_kind: ProviderKind::Unknown,
                provider_id: None,
                credential_kind: Default::default(),
                account_identity: None,
                domains: Vec::new(),
                favicon_url: None,
                endpoints: vec![ProviderEndpoint::api("http://127.0.0.1:9/v1")],
                interface_type: InterfaceType::AnthropicMessages,
                auth_scheme: AuthScheme::XApiKey,
                api_key: None,
                secret_label: None,
                default_model: None,
                model_aliases: Vec::new(),
                headers: None,
                quota: None,
                subscription: None,
                gateway: None,
                tags: Vec::new(),
                notes: None,
                secret_metadata: Default::default(),
            },
        )
        .expect("update provider protocol");

    let runtime = service
        .runtime_config(&creation.vault)
        .expect("provider metadata must not override route protocol");
    assert_eq!(
        runtime.routes[0].targets[0].config.protocol,
        Some(aipass_proxy::Protocol::OpenAiResponses)
    );
}

#[test]
fn key_bound_interface_syncs_target_protocol_on_refresh() {
    let temp = tempfile::tempdir().expect("tempdir");
    let creation = Vault::create(
        temp.path(),
        &SecretString::new("correct horse battery staple"),
    )
    .expect("create vault");
    let provider_id = creation
        .vault
        .add_provider(provider_input(
            "upstream-key",
            "http://127.0.0.1:9/v1".into(),
            "header",
        ))
        .expect("add provider");
    let primary_id = creation
        .vault
        .get_provider_summary(provider_id)
        .expect("provider summary")
        .secret_refs[0]
        .id
        .clone();
    // A second key bound to the Anthropic wire format: the same entry can
    // relay differently-shaped groups.
    let anthropic_key = creation
        .vault
        .add_secret_with_metadata(
            provider_id,
            "anthropic",
            "sk-ant-key",
            &SecretMetadataInput {
                interface_type: Some(InterfaceType::AnthropicMessages),
                endpoint: Some("http://127.0.0.1:19/anthropic".into()),
                group: Some("claude".into()),
                ..SecretMetadataInput::default()
            },
        )
        .expect("add anthropic key");

    let mut service = ProxyService::new(temp.path()).expect("proxy service");
    service.config = config_with_token("aipass-key-protocol");
    service.config.routes[0].targets = vec![ProxyTargetConfig {
        id: Uuid::new_v4(),
        provider_entry_id: provider_id,
        secret_id: anthropic_key.clone(),
        label: "anthropic".into(),
        base_url: "http://127.0.0.1:9/v1".into(),
        auth_scheme: "bearer".into(),
        headers: Vec::new(),
        group: None,
        priority: 0,
        weight: 1,
        enabled: true,
        protocol: None,
        prefer_ws: false,
    }];
    assert!(service
        .refresh_provider_credentials(&creation.vault, provider_id)
        .expect("refresh proxy"));

    let target = &service.config.routes[0].targets[0];
    assert_eq!(
        target.protocol,
        Some(aipass_proxy::Protocol::AnthropicMessages)
    );
    assert_eq!(target.group.as_deref(), Some("claude"));
    assert_eq!(target.base_url, "http://127.0.0.1:19/anthropic");
    assert_eq!(target.auth_scheme, "x_api_key");
    let runtime = service
        .runtime_config(&creation.vault)
        .expect("key overrides at runtime");
    assert_eq!(
        runtime.routes[0].targets[0].config.base_url,
        target.base_url
    );
    assert_eq!(runtime.routes[0].targets[0].config.auth_scheme, "x_api_key");

    // The unbound sibling keeps its route-level protocol untouched.
    service.config.routes[0].targets.push(ProxyTargetConfig {
        id: Uuid::new_v4(),
        provider_entry_id: provider_id,
        secret_id: primary_id,
        label: "primary".into(),
        base_url: "http://127.0.0.1:9/v1".into(),
        auth_scheme: "bearer".into(),
        headers: Vec::new(),
        group: None,
        priority: 1,
        weight: 1,
        enabled: true,
        protocol: None,
        prefer_ws: false,
    });
    service
        .save_config(&creation.vault)
        .expect("persist second target");
    assert!(service
        .refresh_provider_credentials(&creation.vault, provider_id)
        .expect("refresh proxy"));
    assert_eq!(service.config.routes[0].targets[1].protocol, None);
}

/// A key deleted elsewhere — synced from a peer device, for example —
/// leaves a dangling route target. Reconciliation must drop it durably so
/// the next cold start does not fail on the missing credential, while
/// untouched keys stay in place.
#[test]
fn reconcile_prunes_targets_whose_credential_was_removed() {
    let temp = tempfile::tempdir().expect("tempdir");
    let creation = Vault::create(
        temp.path(),
        &SecretString::new("correct horse battery staple"),
    )
    .expect("create vault");
    let provider_id = creation
        .vault
        .add_provider(provider_input(
            "upstream-key",
            "http://127.0.0.1:9/v1".into(),
            "header",
        ))
        .expect("add provider");
    let primary_id = creation
        .vault
        .get_provider_summary(provider_id)
        .expect("provider summary")
        .secret_refs[0]
        .id
        .clone();
    let second_key = creation
        .vault
        .add_secret(provider_id, "secondary", "sk-second")
        .expect("add second key");

    let target_for = |secret_id: &str, priority: u16| ProxyTargetConfig {
        id: Uuid::new_v4(),
        provider_entry_id: provider_id,
        secret_id: secret_id.to_string(),
        label: secret_id.to_string(),
        base_url: "http://127.0.0.1:9/v1".into(),
        auth_scheme: "bearer".into(),
        headers: Vec::new(),
        group: None,
        priority,
        weight: 1,
        enabled: true,
        protocol: None,
        prefer_ws: false,
    };
    let mut service = ProxyService::new(temp.path()).expect("proxy service");
    service.config = config_with_token("aipass-reconcile");
    service.config.routes[0].targets = vec![target_for(&primary_id, 0), target_for(&second_key, 1)];
    service
        .set_pricing_assignment(&creation.vault, provider_id, second_key.clone(), None, 1.0)
        .expect("pricing assignment");
    service
        .save_config(&creation.vault)
        .expect("save proxy config");

    // The vault-side delete bypasses the handler cleanup, mirroring a
    // synced record where the key is simply gone.
    creation
        .vault
        .remove_secret(provider_id, &second_key)
        .expect("remove second key");
    assert!(service
        .reconcile_missing_credentials(&creation.vault)
        .expect("reconcile"));

    let route = &service.config.routes[0];
    assert_eq!(route.targets.len(), 1);
    assert_eq!(route.targets[0].secret_id, primary_id);
    assert!(service
        .pricing_config(&creation.vault)
        .expect("pricing config")
        .assignments
        .iter()
        .all(|assignment| assignment.secret_id != second_key));

    // Trashing the entry removes its remaining target and the emptied
    // route, and a fresh start no longer trips over stale references.
    creation
        .vault
        .trash_provider(provider_id)
        .expect("trash provider");
    assert!(service
        .reconcile_missing_credentials(&creation.vault)
        .expect("reconcile after trash"));
    assert!(service.config.routes.is_empty());
    let error = service.start(&creation.vault).expect_err("no routes left");
    assert!(error
        .message
        .contains("enable at least one proxy route group"));
}

#[test]
fn deleting_first_and_last_keys_cleans_live_routes_and_pricing_by_id() {
    let temp = tempfile::tempdir().expect("tempdir");
    let creation = Vault::create(temp.path(), &SecretString::new("test password")).unwrap();
    let vault = &creation.vault;
    let provider_id = vault
        .add_provider(provider_input(
            "first-key",
            "http://127.0.0.1:9/v1".into(),
            "header",
        ))
        .unwrap();
    let first_id = vault.get_provider_summary(provider_id).unwrap().secret_refs[0]
        .id
        .clone();
    let second_id = vault
        .add_secret(provider_id, "second", "second-key")
        .unwrap();
    let target_for = |secret_id: &str| ProxyTargetConfig {
        id: Uuid::new_v4(),
        provider_entry_id: provider_id,
        secret_id: secret_id.into(),
        label: secret_id.into(),
        base_url: "http://127.0.0.1:9/v1".into(),
        auth_scheme: "bearer".into(),
        headers: Vec::new(),
        group: None,
        priority: 0,
        weight: 1,
        enabled: true,
        protocol: None,
        prefer_ws: false,
    };
    let mut service = ProxyService::new(temp.path()).unwrap();
    service.config = config_with_token("aipass-delete-keys");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    service.config.bind_addr = listener.local_addr().unwrap().to_string();
    drop(listener);
    service.config.routes[0].targets = vec![target_for(&first_id), target_for(&second_id)];
    let surviving_target = service.config.routes[0].targets[1].id;
    for id in [&first_id, &second_id] {
        service
            .set_pricing_assignment(vault, provider_id, id.clone(), None, 2.0)
            .unwrap();
    }
    service.save_config(vault).unwrap();
    service.start(vault).unwrap();

    vault.remove_secret(provider_id, &first_id).unwrap();
    service
        .remove_provider_references(vault, provider_id, Some(&first_id))
        .unwrap();
    assert!(service.status().running);
    assert_eq!(service.status().total_channels, 1);
    let remaining = &service.config.routes[0].targets;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, surviving_target);
    assert_eq!(remaining[0].secret_id, second_id);
    let pricing = service.pricing_config(vault).unwrap();
    assert_eq!(pricing.assignments.len(), 1);
    assert_eq!(pricing.assignments[0].secret_id, second_id);
    assert_eq!(vault.reveal_secret(provider_id).unwrap(), "second-key");

    vault.remove_secret(provider_id, &second_id).unwrap();
    service
        .remove_provider_references(vault, provider_id, Some(&second_id))
        .unwrap();
    assert!(service.status().running);
    assert_eq!(service.status().total_channels, 0);
    assert!(service.config.routes.is_empty());
    assert!(service
        .pricing_config(vault)
        .unwrap()
        .assignments
        .is_empty());
    let mut reopened = ProxyService::new(temp.path()).unwrap();
    assert!(reopened.load_config(vault).unwrap().routes.is_empty());
    service.stop().unwrap();
}

#[test]
fn stop_while_locked_is_persisted_after_the_next_unlock() {
    let temp = tempfile::tempdir().expect("tempdir");
    let creation = aipass_vault::Vault::create(
        temp.path(),
        &SecretString::new("correct horse battery staple"),
    )
    .expect("create vault");
    let mut service = ProxyService::new(temp.path()).expect("proxy service");
    service.config = config_with_token("local-token");
    service.config.enabled = true;
    service
        .save_config(&creation.vault)
        .expect("save enabled config");

    let stopped = service.stop_while_locked().expect("stop while locked");
    assert!(!stopped.running);
    assert!(!stopped.enabled);
    assert!(service.pending_disabled_persist);

    service
        .load_config(&creation.vault)
        .expect("reconcile after unlock");
    assert!(!service.config.enabled);
    assert!(!service.pending_disabled_persist);

    let mut reloaded = ProxyService::new(temp.path()).expect("reloaded service");
    assert!(
        !reloaded
            .load_config(&creation.vault)
            .expect("load persisted config")
            .enabled
    );
}

#[test]
fn stopping_after_unlock_does_not_persist_lock_scrubbed_tokens() {
    let temp = tempfile::tempdir().expect("tempdir");
    let creation = aipass_vault::Vault::create(
        temp.path(),
        &SecretString::new("correct horse battery staple"),
    )
    .expect("create vault");
    let mut service = ProxyService::new(temp.path()).expect("proxy service");
    service.config = config_with_token("local-token");
    service.config.enabled = true;
    service
        .save_config(&creation.vault)
        .expect("save enabled config");

    service.lock_for_session();
    assert!(service.config.routes[0].token.is_empty());
    service
        .stop_and_save(&creation.vault)
        .expect("stop after unlock");

    let mut reloaded = ProxyService::new(temp.path()).expect("reloaded service");
    let config = reloaded
        .load_config(&creation.vault)
        .expect("load stopped config");
    assert!(!config.enabled);
    assert_eq!(config.routes[0].token, "local-token");
}

#[test]
fn stopping_after_the_last_enabled_route_persists_disabled_state() {
    let temp = tempfile::tempdir().expect("tempdir");
    let creation = Vault::create(
        temp.path(),
        &SecretString::new("correct horse battery staple"),
    )
    .expect("create vault");
    let mut service = ProxyService::new(temp.path()).expect("proxy service");
    service.config.enabled = true;
    service.save_config(&creation.vault).expect("save config");
    service.handle = Some(
        ProxyHandle::start(
            RuntimeConfig::from_routes("127.0.0.1:0", Vec::new()),
            service.usage.clone(),
        )
        .expect("start proxy"),
    );

    service
        .apply_runtime_config(&creation.vault)
        .expect("stop proxy");

    assert!(!service.status().running);
    assert!(!service.status().enabled);
    let stored = service.config(&creation.vault).expect("load stored config");
    assert!(!stored.enabled);
}

#[test]
fn set_route_enabled_toggles_only_the_target_route_and_persists() {
    let temp = tempfile::tempdir().expect("tempdir");
    let creation = Vault::create(
        temp.path(),
        &SecretString::new("correct horse battery staple"),
    )
    .expect("create vault");
    let mut service = ProxyService::new(temp.path()).expect("proxy service");
    let mut config = config_with_token("first-token");
    let mut second = config.routes[0].clone();
    second.id = Uuid::new_v4();
    second.token = "second-token".into();
    second.enabled = false;
    config.routes.push(second);
    let first_id = config.routes[0].id;
    let second_id = config.routes[1].id;
    service
        .set_config(&creation.vault, config)
        .expect("save config");

    let updated = service
        .set_route_enabled(&creation.vault, second_id, true)
        .expect("enable second route");
    assert!(updated.enabled);
    assert!(
        updated
            .routes
            .iter()
            .find(|route| route.id == first_id)
            .expect("first route")
            .enabled
    );
    assert!(
        updated
            .routes
            .iter()
            .find(|route| route.id == second_id)
            .expect("second route")
            .enabled
    );

    let updated = service
        .set_route_enabled(&creation.vault, first_id, false)
        .expect("disable first route");
    assert!(
        !updated
            .routes
            .iter()
            .find(|route| route.id == first_id)
            .expect("first route")
            .enabled
    );
    assert!(
        updated
            .routes
            .iter()
            .find(|route| route.id == second_id)
            .expect("second route")
            .enabled
    );

    let mut reloaded = ProxyService::new(temp.path()).expect("reloaded proxy service");
    let persisted = reloaded
        .load_config(&creation.vault)
        .expect("load persisted config");
    assert!(
        !persisted
            .routes
            .iter()
            .find(|route| route.id == first_id)
            .expect("first route")
            .enabled
    );
    assert!(
        persisted
            .routes
            .iter()
            .find(|route| route.id == second_id)
            .expect("second route")
            .enabled
    );

    let error = service
        .set_route_enabled(&creation.vault, Uuid::new_v4(), true)
        .expect_err("unknown route must be rejected");
    assert_eq!(error.code, aipass_agent_protocol::AgentErrorCode::NotFound);
}
