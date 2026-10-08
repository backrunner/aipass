use super::*;

#[test]
fn websocket_capability_survives_lock_stop_retry_and_vault_reopen() {
    for mode in ["record_failure", "config_change", "audit_failure"] {
        let changed_endpoint = mode == "config_change";
        let temp = tempfile::tempdir().unwrap();
        let password = SecretString::new("capability-test-password");
        let vault = Vault::create(temp.path(), &password).unwrap().vault;
        let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = upstream.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            // Refuse WS and serve the HTTP fallback. Safe GET transport
            // retries may open extra sockets; assert protocol evidence,
            // not a fixed number of accepted TCP connections.
            let mut refusals = 0;
            let mut completed_http = false;
            for index in 0..8 {
                let (mut socket, _) = upstream.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    socket.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
                if request.contains("upgrade: websocket") {
                    refusals += 1;
                    socket.write_all(b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                } else {
                    assert!(
                        refusals >= 2,
                        "HTTP fallback arrived without repeated WS refusal"
                    );
                    assert!(
                        request.starts_with("post /v1/responses"),
                        "unexpected upstream request in {index}: {request}"
                    );
                    let length: usize = request
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length: "))
                        .unwrap()
                        .trim()
                        .parse()
                        .unwrap();
                    socket.read_exact(&mut vec![0; length]).unwrap();
                    let body = r#"{"id":"response-test","status":"completed","output":[]}"#;
                    write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                    completed_http = true;
                    break;
                }
            }
            assert!(completed_http, "HTTP fallback did not complete");
        });
        let mut input = provider_input("test-key", format!("http://{address}/v1"), "test-header");
        input.supports_websockets = Some(true);
        let id = vault.add_provider(input.clone()).unwrap();
        let summary = vault.get_provider_summary(id).unwrap();
        let mut service = ProxyService::new(temp.path()).unwrap();
        service.config = config_with_token("capability-local-token");
        let reserved = TcpListener::bind("127.0.0.1:0").unwrap();
        service.config.bind_addr = reserved.local_addr().unwrap().to_string();
        drop(reserved);
        service.config.upstream_proxy.mode = aipass_proxy::UpstreamProxyMode::Direct;
        service.config.routes[0].targets = vec![ProxyTargetConfig {
            id: Uuid::new_v4(),
            provider_entry_id: id,
            secret_id: summary.secret_refs[0].id.clone(),
            label: "primary".into(),
            base_url: format!("http://{address}/v1"),
            auth_scheme: "bearer".into(),
            headers: Vec::new(),
            group: None,
            priority: 0,
            weight: 1,
            enabled: true,
            protocol: None,
            prefer_ws: false,
        }];
        service.save_config(&vault).unwrap();
        let status = service.start(&vault).unwrap();
        service.lock_for_session();
        drop(vault);
        let response = reqwest::blocking::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .post(format!("http://{}/v1/responses", status.bind_addr))
            .bearer_auth("capability-local-token")
            .json(&serde_json::json!({"model":"test","input":"hello","stream":false}))
            .send()
            .unwrap();
        assert!(response.status().is_success());
        assert!(response.text().unwrap().contains("completed"));
        server.join().unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while service
            .handle
            .as_ref()
            .unwrap()
            .websocket_capability_events()
            .is_empty()
        {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        if mode != "audit_failure" {
            service.stop_while_locked().unwrap();
            assert_eq!(service.pending_ws_events.len(), 1);
        }
        let vault = Vault::open(temp.path(), &password).unwrap();
        assert_eq!(
            vault.get_provider_summary(id).unwrap().supports_websockets,
            Some(true)
        );
        if changed_endpoint {
            let mut edit: ProviderEntryUpdateInput =
                serde_json::from_value(serde_json::to_value(input).unwrap()).unwrap();
            edit.supports_websockets = None;
            edit.endpoints = vec![ProviderEndpoint::api("http://127.0.0.1:9/v1")];
            vault.update_provider(id, edit).unwrap();
        }
        #[cfg(unix)]
        if mode == "record_failure" {
            use std::os::unix::fs::PermissionsExt;
            let objects = temp.path().join("objects");
            let permissions = std::fs::metadata(&objects).unwrap().permissions();
            std::fs::set_permissions(&objects, std::fs::Permissions::from_mode(0o500)).unwrap();
            let result = service.persist_ws_capabilities(&vault);
            std::fs::set_permissions(&objects, permissions).unwrap();
            assert!(result.is_err());
            assert_eq!(service.pending_ws_events.len(), 1);
            assert_eq!(
                vault.get_provider_summary(id).unwrap().supports_websockets,
                Some(true)
            );
        }
        #[cfg(unix)]
        if mode == "audit_failure" {
            use std::os::unix::fs::PermissionsExt;
            let event = service
                .handle
                .as_ref()
                .unwrap()
                .websocket_capability_events()[0]
                .clone();
            let observation = service.begin_ws_probe(event.config_key);
            assert!(observation.is_some());
            let audit = temp.path().join("audit");
            let permissions = std::fs::metadata(&audit).unwrap().permissions();
            std::fs::set_permissions(&audit, std::fs::Permissions::from_mode(0o500)).unwrap();
            let result = service.persist_ws_capabilities(&vault);
            std::fs::set_permissions(&audit, permissions).unwrap();
            assert!(result.is_err());
            assert_eq!(
                vault.get_provider_summary(id).unwrap().supports_websockets,
                Some(false)
            );
            assert!(service.pending_ws_refresh.contains(&id));
            // A completion that arrives after the durable write can clear
            // live evidence, but it must not erase the pending refresh.
            service.confirm_ws_probe(observation);
            assert!(service
                .handle
                .as_ref()
                .unwrap()
                .websocket_capability_events()
                .is_empty());
            // A failed refresh must keep the retry without interrupting
            // the listener or republishing a stale capability event.
            let config_path = temp.path().join(CONFIG_FILE);
            let backup_path = temp.path().join("ws-test-config-backup");
            std::fs::rename(&config_path, &backup_path).unwrap();
            std::fs::create_dir(&config_path).unwrap();
            let result = service.persist_ws_capabilities(&vault);
            std::fs::remove_dir(&config_path).unwrap();
            std::fs::rename(&backup_path, &config_path).unwrap();
            assert!(result.is_err());
            assert!(service.status().running);
            assert!(service.pending_ws_refresh.contains(&id));
        }
        assert_eq!(
            service.persist_ws_capabilities(&vault).unwrap(),
            !changed_endpoint
        );
        assert!(!service.persist_ws_capabilities(&vault).unwrap()); // Duplicate tick is a no-op.
        assert!(service.pending_ws_refresh.is_empty());
        drop(vault);
        let vault = Vault::open(temp.path(), &password).unwrap();
        let saved = vault.get_provider_summary(id).unwrap();
        assert_eq!(saved.supports_websockets, Some(changed_endpoint));
        assert_eq!(saved.websocket_warning.is_some(), !changed_endpoint);
        assert_eq!(saved.title, summary.title);
        assert_eq!(vault.reveal_secret(id).unwrap(), "test-key");
    }
}

#[test]
fn config_accepts_plaintext_token() {
    let config = config_with_token("matching-token");
    assert!(validate_config(&config).is_ok());
}

#[test]
fn config_accepts_every_resolvable_auth_scheme_and_rejects_unknown_schemes() {
    let mut config = config_with_token("matching-token");
    config.routes[0].targets.push(ProxyTargetConfig {
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
        protocol: None,
        prefer_ws: false,
    });
    for auth in [
        AuthScheme::Bearer,
        AuthScheme::CustomHeader,
        AuthScheme::XApiKey,
        AuthScheme::AzureApiKey,
        AuthScheme::GoogleApiKey,
    ] {
        let scheme = proxy_auth_scheme(&auth).unwrap();
        config.routes[0].targets[0].auth_scheme = scheme.into();
        assert!(validate_config(&config).is_ok(), "rejected {scheme}");
    }
    assert!(proxy_auth_scheme(&AuthScheme::AwsProfile).is_none());
    for unsupported in ["aws_profile", "unknown"] {
        config.routes[0].targets[0].auth_scheme = unsupported.into();
        assert!(validate_config(&config).is_err());
    }
}

#[test]
fn config_rejects_zero_hold_initial_delay() {
    let mut config = config_with_token("hold-token");
    config.routes[0].retry.hold_on_failure = true;
    config.routes[0].retry.hold_initial_delay_ms = 0;
    let err = validate_config(&config).expect_err("zero hold initial delay is rejected");
    assert_eq!(
        err.code,
        aipass_agent_protocol::AgentErrorCode::ValidationFailed
    );
    assert!(err.message.contains("hold initial delay"));
}

#[test]
fn config_rejects_hold_max_delay_below_initial_delay() {
    let mut config = config_with_token("hold-token");
    config.routes[0].retry.hold_on_failure = true;
    config.routes[0].retry.hold_initial_delay_ms = 1_000;
    config.routes[0].retry.hold_max_delay_ms = 500;
    let err = validate_config(&config).expect_err("hold max delay below initial is rejected");
    assert_eq!(
        err.code,
        aipass_agent_protocol::AgentErrorCode::ValidationFailed
    );
    assert!(err.message.contains("hold max delay"));
}

#[test]
fn saving_an_enabled_route_generates_a_missing_token() {
    let temp = tempfile::tempdir().expect("tempdir");
    let creation = Vault::create(
        temp.path(),
        &SecretString::new("correct horse battery staple"),
    )
    .expect("create vault");
    let mut service = ProxyService::new(temp.path()).expect("proxy service");

    let saved = service
        .set_config(&creation.vault, config_with_token(""))
        .expect("save config");

    assert!(saved.routes[0].token.starts_with("sk-"));
    assert!(!saved.routes[0].token.trim().is_empty());
}

#[test]
fn loading_a_legacy_enabled_route_generates_and_persists_a_token() {
    let temp = tempfile::tempdir().expect("tempdir");
    let creation = Vault::create(
        temp.path(),
        &SecretString::new("correct horse battery staple"),
    )
    .expect("create vault");
    let mut service = ProxyService::new(temp.path()).expect("proxy service");
    service.config = config_with_token("");
    service
        .save_config(&creation.vault)
        .expect("save legacy config");

    let mut reloaded = ProxyService::new(temp.path()).expect("reloaded proxy service");
    let migrated = reloaded
        .load_config(&creation.vault)
        .expect("migrate legacy config");
    assert!(migrated.routes[0].token.starts_with("sk-"));

    let generated = migrated.routes[0].token.clone();
    let mut persisted = ProxyService::new(temp.path()).expect("persisted proxy service");
    assert_eq!(
        persisted
            .load_config(&creation.vault)
            .expect("load persisted config")
            .routes[0]
            .token,
        generated
    );
}

#[test]
fn client_config_returns_stored_plaintext_token() {
    let temp = tempfile::tempdir().expect("tempdir");
    let creation = Vault::create(
        temp.path(),
        &SecretString::new("correct horse battery staple"),
    )
    .expect("create vault");
    let mut service = ProxyService::new(temp.path()).expect("proxy service");
    let token = "matching-token";

    let saved_config = service
        .set_config(&creation.vault, config_with_token(token))
        .expect("save config");
    assert_eq!(saved_config.routes[0].token, token);

    let client_config = service
        .client_config(&creation.vault)
        .expect("load client config");
    assert_eq!(client_config.routes[0].token, token);
}

#[test]
fn start_if_enabled_restores_persisted_proxy_runtime() {
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
    let probe = TcpListener::bind("127.0.0.1:0").expect("reserve proxy address");
    let bind_addr = probe.local_addr().expect("proxy address").to_string();
    drop(probe);

    let mut config = config_with_token("restore-token");
    config.bind_addr = bind_addr;
    config.routes[0].targets = vec![ProxyTargetConfig {
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
    config.enabled = true;

    let mut persisted = ProxyService::new(temp.path()).expect("proxy service");
    persisted.config = config;
    persisted
        .save_config(&creation.vault)
        .expect("save enabled config");

    let mut restored = ProxyService::new(temp.path()).expect("restored proxy service");
    let status = restored
        .start_if_enabled(&creation.vault)
        .expect("restore proxy")
        .expect("enabled proxy should start");
    assert!(status.running);
    assert_eq!(status.bind_addr, persisted.config.bind_addr);
}

#[test]
fn start_if_enabled_skips_explicitly_stopped_proxy() {
    let temp = tempfile::tempdir().expect("tempdir");
    let creation = Vault::create(
        temp.path(),
        &SecretString::new("correct horse battery staple"),
    )
    .expect("create vault");
    let mut persisted = ProxyService::new(temp.path()).expect("proxy service");
    persisted.config = config_with_token("restore-token");
    persisted.config.enabled = false;
    persisted
        .save_config(&creation.vault)
        .expect("save disabled config");

    let mut restored = ProxyService::new(temp.path()).expect("restored proxy service");
    assert!(restored
        .start_if_enabled(&creation.vault)
        .expect("check persisted state")
        .is_none());
    assert!(!restored.status().running);
}

#[test]
fn locking_session_keeps_runtime_credentials_available_to_proxy() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut service = ProxyService::new(temp.path()).expect("proxy service");

    let upstream = TcpListener::bind("127.0.0.1:0").expect("bind upstream");
    let upstream_addr = upstream.local_addr().expect("upstream address");
    let (request_tx, request_rx) = std::sync::mpsc::channel();
    let upstream_thread = std::thread::spawn(move || {
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
    });

    let proxy_probe = TcpListener::bind("127.0.0.1:0").expect("reserve proxy address");
    let proxy_addr = proxy_probe.local_addr().expect("proxy address");
    drop(proxy_probe);
    let local_token = "aipass-local-token";
    let upstream_api_key = "upstream-secret";
    service.config = config_with_token(local_token);
    let runtime_route = ResolvedRoute {
        config: service.config.routes[0].clone(),
        local_token: local_token.into(),
        targets: vec![ResolvedTarget {
            upstream_proxy: None,
            upstream_kind: aipass_proxy::UpstreamKind::Standard,
            quota: Vec::new(),
            model_override: None,
            profile: aipass_proxy::ProviderProfile::Generic,
            max_concurrent_requests: None,
            supports_websockets: false,
            config: ProxyTargetConfig {
                id: Uuid::new_v4(),
                provider_entry_id: Uuid::new_v4(),
                secret_id: "primary".into(),
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
            },
            api_key: upstream_api_key.into(),
        }],
    };
    service.handle = Some(
        ProxyHandle::start(
            RuntimeConfig::from_routes(proxy_addr.to_string(), vec![runtime_route]),
            service.usage.clone(),
        )
        .expect("start proxy"),
    );

    service.lock_for_session();

    assert!(service.status().running);
    assert!(service.config.routes[0].token.is_empty());

    let response = reqwest::blocking::Client::new()
        .post(format!("http://{proxy_addr}/v1/responses"))
        .bearer_auth(local_token)
        .json(&serde_json::json!({"model": "gpt-test", "input": "hello"}))
        .send()
        .expect("request through locked proxy");
    assert!(response.status().is_success());
    let upstream_request = request_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("upstream request");
    assert!(upstream_request
        .to_ascii_lowercase()
        .contains("authorization: bearer upstream-secret"));
    upstream_thread.join().expect("upstream thread");
}

#[test]
fn inactive_targets_do_not_interrupt_live_refresh_or_capability_persistence() {
    for capability in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let vault = Vault::create(temp.path(), &SecretString::new("test master"))
            .unwrap()
            .vault;
        let mut service = ProxyService::new(temp.path()).unwrap();
        service.config = config_with_token("refresh-test");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        service.config.bind_addr = listener.local_addr().unwrap().to_string();
        drop(listener);
        let mut providers = Vec::new();
        for secret in ["first-key", "second-key"] {
            let mut input = provider_input(secret, "http://127.0.0.1:9/v1".into(), "header");
            input.supports_websockets = Some(true);
            let id = vault.add_provider(input).unwrap();
            providers.push(id);
            service.config.routes[0].targets.push(ProxyTargetConfig {
                id: Uuid::new_v4(),
                provider_entry_id: id,
                secret_id: vault.get_provider_summary(id).unwrap().secret_refs[0]
                    .id
                    .clone(),
                label: secret.into(),
                base_url: "http://127.0.0.1:9/v1".into(),
                auth_scheme: "bearer".into(),
                headers: Vec::new(),
                group: None,
                priority: 0,
                weight: 1,
                enabled: true,
                protocol: None,
                prefer_ws: false,
            });
        }
        service.save_config(&vault).unwrap();
        service.start(&vault).unwrap();
        let bind = service.status().bind_addr;
        let runtime = service.runtime_config(&vault).unwrap();
        let event = aipass_proxy::WebsocketCapabilityEvent {
            id: Uuid::new_v4(),
            provider_entry_id: providers[1],
            config_key: aipass_proxy::websocket_config_key(
                &runtime.routes[0].targets[1],
                &runtime.upstream_proxy,
            ),
            status: 405,
            detected_at: 1,
        };
        vault.archive_provider(providers[0]).unwrap();
        if capability {
            // A sync can leave stored references to an unavailable provider.
            service.reload_if_running(&vault).unwrap();
            service
                .handle
                .as_ref()
                .unwrap()
                .restore_websocket_capability_event(&event);
            assert!(service.persist_ws_capabilities(&vault).unwrap());
            assert_eq!(
                vault
                    .get_provider_summary(providers[1])
                    .unwrap()
                    .supports_websockets,
                Some(false)
            );
        } else {
            assert!(service
                .refresh_provider_credentials(&vault, providers[0])
                .unwrap());
            // Editing another provider must also tolerate the retained reference.
            assert!(service
                .refresh_provider_credentials(&vault, providers[1])
                .unwrap());
        }
        assert!(service.status().running);
        assert!(service.config.enabled);
        assert_eq!(service.status().bind_addr, bind);
        assert_eq!(service.status().channels.len(), 1);
        assert_eq!(service.status().channels[0].provider_entry_id, providers[1]);
        assert_eq!(service.config.routes[0].targets.len(), 2);
        vault.restore_provider(providers[0]).unwrap();
        service
            .refresh_provider_credentials(&vault, providers[0])
            .unwrap();
        assert_eq!(service.status().channels.len(), 2);
    }
}

#[test]
fn provider_update_refreshes_running_credentials_and_headers() {
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
    let local_token = "aipass-provider-refresh";
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

    let revision = creation.vault.sync_revision().unwrap();
    for _ in 0..3 {
        service.reload_if_running(&creation.vault).unwrap();
    }
    assert_eq!(
        creation.vault.sync_revision().unwrap(),
        revision,
        "runtime reconciliation must not rewrite provider records or create audit entries"
    );

    let request = || {
        reqwest::blocking::Client::new()
            .post(format!("http://{proxy_addr}/v1/responses"))
            .bearer_auth(local_token)
            .body("{}")
            .send()
            .expect("proxy request")
            .error_for_status()
            .expect("proxy status")
            .text()
            .expect("proxy body");
    };
    request();

    creation
        .vault
        .update_provider(
            provider_id,
            ProviderEntryUpdateInput {
                max_concurrent_requests: Some(3),
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
    assert!(service
        .refresh_provider_credentials(&creation.vault, provider_id)
        .expect("refresh proxy"));
    assert_eq!(
        service.runtime_config(&creation.vault).unwrap().routes[0].targets[0]
            .max_concurrent_requests,
        Some(3)
    );
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
    assert!(first.contains("x-provider-header: old-header"));
    assert!(second.contains("authorization: bearer new-upstream-key"));
    assert!(second.contains("x-provider-header: new-header"));
    upstream_thread.join().expect("upstream thread");
}

#[test]
fn failed_provider_refresh_stops_the_stale_runtime() {
    let temp = tempfile::tempdir().expect("tempdir");
    let creation = Vault::create(
        temp.path(),
        &SecretString::new("correct horse battery staple"),
    )
    .expect("create vault");
    let provider_id = creation
        .vault
        .add_provider(provider_input(
            "old-upstream-key",
            "http://127.0.0.1:9/v1".into(),
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
    let mut service = ProxyService::new(temp.path()).expect("proxy service");
    service.config = config_with_token("aipass-stale-refresh");
    service.config.bind_addr = proxy_addr.to_string();
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
    service
        .save_config(&creation.vault)
        .expect("save proxy config");
    service.start(&creation.vault).expect("start proxy");
    assert!(service.status().running);

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
                interface_type: InterfaceType::OpenAiCompatible,
                auth_scheme: AuthScheme::AwsProfile,
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
        .expect("update provider");

    assert!(service
        .refresh_provider_credentials(&creation.vault, provider_id)
        .is_err());
    assert!(!service.status().running);
    assert!(!service.config.enabled);

    let mut reloaded = ProxyService::new(temp.path()).expect("reloaded proxy service");
    assert!(
        !reloaded
            .load_config(&creation.vault)
            .expect("load disabled config")
            .enabled
    );
}
