use super::*;

#[test]
fn config_allows_multiple_enabled_routes_with_independent_tokens() {
    let mut config = config_with_token("first-token");
    let mut second = config.routes[0].clone();
    second.id = Uuid::new_v4();
    second.token = "second-token".into();
    config.routes.push(second);
    assert!(validate_config(&config).is_ok());
}

#[test]
fn config_rejects_duplicate_route_group_tokens() {
    let mut config = config_with_token("same-token");
    let mut second = config.routes[0].clone();
    second.id = Uuid::new_v4();
    config.routes.push(second);
    assert!(validate_config(&config).is_err());
}

#[test]
fn loading_legacy_route_without_token_generates_one() {
    let mut config = config_with_token("first-token");
    let mut second = config.routes[0].clone();
    second.id = Uuid::new_v4();
    second.token.clear();
    second.enabled = false;
    config.routes.push(second);

    assert!(ensure_route_tokens(&mut config));
    assert!(config.routes[0].enabled);
    assert!(!config.routes[1].enabled);
    assert!(config.routes[1].token.starts_with("sk-"));
    assert!(validate_config(&config).is_ok());
}

#[test]
fn config_accepts_supported_protocol_conversion() {
    let mut config = config_with_token("matching-token");
    config.routes[0].inbound_protocol = aipass_proxy::Protocol::AnthropicMessages;
    config.routes[0].upstream_protocol = aipass_proxy::Protocol::OpenAiChatCompletions;
    config.routes[0].conversion_enabled = true;
    assert!(validate_config(&config).is_ok());

    let mut config = config_with_token("matching-token");
    config.routes[0].inbound_protocol = aipass_proxy::Protocol::AnthropicMessages;
    config.routes[0].upstream_protocol = aipass_proxy::Protocol::OpenAiResponses;
    config.routes[0].conversion_enabled = true;
    assert!(validate_config(&config).is_ok());

    // Same-protocol routes with the flag on are harmless.
    let mut config = config_with_token("matching-token");
    config.routes[0].conversion_enabled = true;
    assert!(validate_config(&config).is_ok());
}

#[test]
fn config_accepts_direct_openai_protocol_conversion() {
    let mut config = config_with_token("matching-token");
    config.routes[0].inbound_protocol = aipass_proxy::Protocol::OpenAiChatCompletions;
    config.routes[0].upstream_protocol = aipass_proxy::Protocol::OpenAiResponses;
    config.routes[0].conversion_enabled = true;
    assert!(validate_config(&config).is_ok());
    config.routes[0].inbound_protocol = aipass_proxy::Protocol::OpenAiResponses;
    config.routes[0].upstream_protocol = aipass_proxy::Protocol::OpenAiChatCompletions;
    assert!(validate_config(&config).is_ok());
}

#[test]
fn subscription_adaptation_requires_official_oauth_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::create(
        temp.path(),
        &SecretString::new("correct horse battery staple"),
    )
    .unwrap()
    .vault;
    let input = provider_input(
        "fake-token",
        "https://chatgpt.com/backend-api/codex".into(),
        "header",
    );
    let id = vault.add_provider(input).unwrap();
    let mut entry = vault.get_provider_summary(id).unwrap();
    assert_eq!(upstream_kind(&entry), aipass_proxy::UpstreamKind::Standard);
    entry.provider_kind = ProviderKind::Official;
    assert_eq!(upstream_kind(&entry), aipass_proxy::UpstreamKind::Standard);
    entry.credential_kind = CredentialKind::OAuth;
    assert_eq!(
        upstream_kind(&entry),
        aipass_proxy::UpstreamKind::CodexSubscription
    );
    entry.provider_id = Some("codex".into());
    assert_eq!(
        upstream_kind(&entry),
        aipass_proxy::UpstreamKind::CodexSubscription
    );
    entry.provider_kind = ProviderKind::ThirdParty;
    assert_eq!(upstream_kind(&entry), aipass_proxy::UpstreamKind::Standard);
}

#[test]
fn config_rejects_cross_protocol_passthrough() {
    let mut config = config_with_token("matching-token");
    config.routes[0].upstream_protocol = aipass_proxy::Protocol::AnthropicMessages;
    assert!(validate_config(&config).is_err());
}

#[test]
fn config_rejects_target_protocol_mismatch_without_conversion() {
    let mut config = config_with_token("matching-token");
    config.routes[0].targets = vec![ProxyTargetConfig {
        id: Uuid::new_v4(),
        provider_entry_id: Uuid::new_v4(),
        secret_id: "primary".into(),
        label: "primary".into(),
        base_url: "http://127.0.0.1:9/v1".into(),
        auth_scheme: "bearer".into(),
        headers: Vec::new(),
        group: None,
        priority: 0,
        weight: 1,
        enabled: true,
        protocol: Some(aipass_proxy::Protocol::AnthropicMessages),
        prefer_ws: false,
    }];
    assert!(validate_config(&config).is_err());
    config.routes[0].conversion_enabled = true;
    assert!(validate_config(&config).is_ok());
}

#[test]
fn runtime_config_populates_target_protocols() {
    let temp = tempfile::tempdir().expect("tempdir");
    let creation = Vault::create(
        temp.path(),
        &SecretString::new("correct horse battery staple"),
    )
    .expect("create vault");
    let mut input = provider_input("upstream-key", "http://127.0.0.1:9/v1".into(), "header");
    input.provider_id = Some("openai".into());
    let provider_id = creation.vault.add_provider(input).expect("add provider");
    let secret_id = creation
        .vault
        .get_provider_summary(provider_id)
        .expect("provider summary")
        .secret_refs[0]
        .id
        .clone();

    let mut service = ProxyService::new(temp.path()).expect("proxy service");
    service.config = config_with_token("matching-token");
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
    let runtime = service
        .runtime_config(&creation.vault)
        .expect("runtime config");
    assert_eq!(
        runtime.routes[0].targets[0].config.protocol,
        Some(aipass_proxy::Protocol::OpenAiResponses)
    );
}

#[test]
fn official_oauth_anthropic_endpoint_is_pinned() {
    assert_eq!(
        pinned_official_oauth_endpoint(
            &ProviderKind::Official,
            &CredentialKind::OAuth,
            Some("anthropic"),
        ),
        Some("https://api.anthropic.com")
    );
}

#[test]
fn legacy_managed_oauth_cannot_bypass_cli_ownership() {
    use aipass_provider_registry::OAuthProvider;
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::create(temp.path(), &SecretString::new("test password"))
        .unwrap()
        .vault;
    let mut input = provider_input(
        "stale-mirror",
        "https://chatgpt.com/backend-api/codex".into(),
        "header",
    );
    input.provider_kind = ProviderKind::Official;
    input.credential_kind = CredentialKind::OAuth;
    input.provider_id = Some("openai".into());
    let id = vault.add_provider(input).unwrap();
    let entry = vault.get_provider_summary(id).unwrap();
    let primary = &entry.secret_refs[0].id;
    let mut account = aipass_vault::ManagedOAuthAccount {
        id: Uuid::new_v4(),
        provider: OAuthProvider::Codex,
        entry_id: Some(id),
        access_token: "durable-rotated-access".into(),
        refresh_token: "durable-refresh".into(),
        expires_at_ms: crate::oauth::now_ms() + 3600000,
        last_refresh_ms: 42,
        id_token: None,
        chatgpt_account_id: None,
        account_identity: None,
        requires_reauth: false,
        is_default: false,
        authenticated_at: OffsetDateTime::now_utc(),
    };
    vault.add_oauth_account(account.clone()).unwrap();
    assert!(managed_oauth_token(&vault, &entry, primary).is_err());
    assert!(managed_oauth_token(&vault, &entry, "separate-api-key")
        .unwrap()
        .is_none());
    assert_eq!(vault.reveal_secret(id).unwrap(), "stale-mirror");
    account.requires_reauth = true;
    vault.update_oauth_account(account.clone()).unwrap();
    assert!(managed_oauth_token(&vault, &entry, primary).is_err());
    account.requires_reauth = false;
    account.expires_at_ms = 1;
    vault.update_oauth_account(account).unwrap();
    assert!(managed_oauth_token(&vault, &entry, primary).is_err());
}

#[test]
fn official_oauth_openai_endpoint_is_pinned() {
    assert_eq!(
        pinned_official_oauth_endpoint(
            &ProviderKind::Official,
            &CredentialKind::OAuth,
            Some("openai"),
        ),
        Some("https://chatgpt.com/backend-api/codex")
    );
}

#[test]
fn official_oauth_xai_endpoint_is_pinned() {
    assert_eq!(
        pinned_official_oauth_endpoint(
            &ProviderKind::Official,
            &CredentialKind::OAuth,
            Some("xai"),
        ),
        Some("https://cli-chat-proxy.grok.com/v1")
    );
}

#[test]
fn official_oauth_unknown_provider_keeps_entry_endpoint() {
    assert_eq!(
        pinned_official_oauth_endpoint(&ProviderKind::Official, &CredentialKind::OAuth, None),
        None
    );
    assert_eq!(
        pinned_official_oauth_endpoint(
            &ProviderKind::Official,
            &CredentialKind::OAuth,
            Some("gemini"),
        ),
        None
    );
}

#[test]
fn non_oauth_official_entries_keep_entry_endpoint() {
    assert_eq!(
        pinned_official_oauth_endpoint(
            &ProviderKind::Official,
            &CredentialKind::Api,
            Some("anthropic"),
        ),
        None
    );
}

#[test]
fn third_party_oauth_entries_keep_entry_endpoint() {
    assert_eq!(
        pinned_official_oauth_endpoint(
            &ProviderKind::ThirdParty,
            &CredentialKind::OAuth,
            Some("anthropic"),
        ),
        None
    );
}

#[test]
fn start_rejects_config_without_any_enabled_route_group() {
    let temp = tempfile::tempdir().expect("tempdir");
    let creation = Vault::create(
        temp.path(),
        &SecretString::new("correct horse battery staple"),
    )
    .expect("create vault");
    let mut service = ProxyService::new(temp.path()).expect("proxy service");
    let mut config = config_with_token("zero-group-token");
    config.routes[0].enabled = false;
    service
        .set_config(&creation.vault, config)
        .expect("save config");

    let error = service
        .start(&creation.vault)
        .expect_err("start must fail without an enabled route group");
    assert_eq!(
        error.code,
        aipass_agent_protocol::AgentErrorCode::ValidationFailed
    );
    assert!(!service.status().running);
}

#[test]
fn reload_if_running_is_a_noop_when_stopped() {
    let temp = tempfile::tempdir().expect("tempdir");
    let creation = Vault::create(
        temp.path(),
        &SecretString::new("correct horse battery staple"),
    )
    .expect("create vault");
    let mut service = ProxyService::new(temp.path()).expect("proxy service");
    service.config = config_with_token("stopped-token");
    service.save_config(&creation.vault).expect("save config");

    service
        .reload_if_running(&creation.vault)
        .expect("reload on a stopped proxy is a no-op");
    assert!(!service.status().running);
}

#[test]
fn reload_if_running_rebuilds_credentials_after_the_vault_changed() {
    // Simulates the sync-download path: the vault changed underneath a
    // running proxy, and reload_if_running must rebuild the runtime
    // snapshot so the next request uses the new credential.
    let temp = tempfile::tempdir().expect("tempdir");
    let creation = Vault::create(
        temp.path(),
        &SecretString::new("correct horse battery staple"),
    )
    .expect("create vault");
    let upstream = TcpListener::bind("127.0.0.1:0").expect("bind upstream");
    let upstream_addr = upstream.local_addr().expect("upstream address");
    let (request_tx, request_rx) = std::sync::mpsc::channel();
    let upstream_thread = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = upstream.accept().expect("accept proxy request");
            let mut request = vec![0_u8; 8192];
            let count = stream.read(&mut request).expect("read proxy request");
            request.truncate(count);
            request_tx
                .send(String::from_utf8_lossy(&request).to_string())
                .expect("capture proxy request");
            let body = r#"{"id":"response-test","status":"completed","output":[]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("write upstream response");
        }
    });

    let provider_id = creation
        .vault
        .add_provider(provider_input(
            "old-upstream-key",
            format!("http://{upstream_addr}/v1"),
            "old-header",
        ))
        .expect("add provider");
    let secret_id = creation
        .vault
        .get_provider_summary(provider_id)
        .expect("provider summary")
        .secret_refs[0]
        .id
        .clone();
    let proxy_probe = TcpListener::bind("127.0.0.1:0").expect("reserve proxy address");
    let proxy_addr = proxy_probe.local_addr().expect("proxy address");
    drop(proxy_probe);
    let local_token = "aipass-reload-if-running";
    let mut service = ProxyService::new(temp.path()).expect("proxy service");
    service.config = config_with_token(local_token);
    service.config.bind_addr = proxy_addr.to_string();
    service.config.routes[0].targets = vec![ProxyTargetConfig {
        id: Uuid::new_v4(),
        provider_entry_id: provider_id,
        secret_id,
        label: "primary".into(),
        base_url: format!("http://{upstream_addr}/v1"),
        auth_scheme: "bearer".into(),
        headers: Vec::new(),
        group: None,
        priority: 0,
        weight: 1,
        enabled: true,
        protocol: None,
        prefer_ws: false,
    }];
    service
        .save_config(&creation.vault)
        .expect("save proxy config");
    service.start(&creation.vault).expect("start proxy");

    let request = || {
        reqwest::blocking::Client::new()
            .post(format!("http://{proxy_addr}/v1/responses"))
            .bearer_auth(local_token)
            .body("{}")
            .send()
            .expect("proxy request")
            .error_for_status()
            .expect("proxy status");
    };
    request();

    // The vault write lands without any proxy call, as it would when a
    // sync download refreshed the records on disk.
    creation
        .vault
        .update_provider(
            provider_id,
            ProviderEntryUpdateInput {
                max_concurrent_requests: None,
                supports_websockets: None,
                title: "Proxy upstream".into(),
                provider_kind: ProviderKind::Unknown,
                provider_id: Some("openai".into()),
                credential_kind: Default::default(),
                account_identity: None,
                domains: Vec::new(),
                favicon_url: None,
                endpoints: vec![ProviderEndpoint::api(format!("http://{upstream_addr}/v1"))],
                interface_type: InterfaceType::OpenAiCompatible,
                auth_scheme: AuthScheme::Bearer,
                api_key: Some("new-upstream-key".into()),
                secret_label: None,
                default_model: None,
                model_aliases: Vec::new(),
                headers: Some(vec![("x-provider-header".into(), "new-header".into())]),
                quota: None,
                subscription: None,
                gateway: None,
                tags: Vec::new(),
                notes: None,
                secret_metadata: Default::default(),
            },
        )
        .expect("update provider");
    service
        .reload_if_running(&creation.vault)
        .expect("reload running proxy");
    assert!(service.status().running);
    request();

    let first = request_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("first upstream request")
        .to_ascii_lowercase();
    let second = request_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("second upstream request")
        .to_ascii_lowercase();
    assert!(first.contains("authorization: bearer old-upstream-key"));
    assert!(second.contains("authorization: bearer new-upstream-key"));
    assert!(second.contains("x-provider-header: new-header"));
    upstream_thread.join().expect("upstream thread");
}
