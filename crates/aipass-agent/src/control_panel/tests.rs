use super::*;
use crate::session::{self, SessionState};
use aipass_agent_protocol::{AgentRequest, LockReason};
use aipass_crypto::SecretString;
use aipass_proxy::{Protocol, ProxyConfig, ProxyRouteConfig, ProxyTargetConfig};
use aipass_vault::Vault;
use reqwest::blocking::{Client, Response};
use serde_json::{json, Value};
use std::{net::TcpListener, time::Duration};
use uuid::Uuid;

struct Fixture {
    _temp: tempfile::TempDir,
    state: Arc<AgentState>,
    client: Client,
    url: String,
    code: String,
    route: Uuid,
    target: Uuid,
    vault_id: Uuid,
}

fn port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[test]
fn listener_authority_matches_browser_default_port_normalization() {
    for (address, https, expected) in [
        ("127.0.0.1:80", false, "127.0.0.1"),
        ("127.0.0.1:443", true, "127.0.0.1"),
        ("127.0.0.1:443", false, "127.0.0.1:443"),
        ("[::1]:443", true, "[::1]"),
        ("[::1]:8788", false, "[::1]:8788"),
    ] {
        assert_eq!(
            transport::authority(address.parse().unwrap(), https),
            expected
        );
    }
}

impl Fixture {
    fn new(https: bool) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("vault");
        let creation =
            Vault::create(&root, &SecretString::new("panel-test-vault-password")).unwrap();
        let vault_id = creation.vault.vault_id();
        let tools = temp.path().join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        crate::server::TOOL_HOME_OVERRIDES
            .lock()
            .unwrap()
            .insert(vault_id, tools);
        let provider = creation
            .vault
            .add_provider(crate::server::tests::sync_test_provider(
                "Test upstream",
                "fake-panel-provider-secret",
            ))
            .unwrap();
        let entry = creation.vault.get_provider_summary(provider).unwrap();
        let state = crate::server::tests::sync_test_state(root);
        session::set_session_vault(&state, creation.vault);
        let route = Uuid::new_v4();
        let target = Uuid::new_v4();
        session::with_vault(&state, false, |vault| {
            state.proxy.lock().unwrap().set_config(
                vault,
                ProxyConfig {
                    bind_addr: format!("127.0.0.1:{}", port()),
                    routes: vec![ProxyRouteConfig {
                        id: route,
                        name: "Default".into(),
                        token: "fake-panel-proxy-token".into(),
                        inbound_protocol: Protocol::OpenAiResponses,
                        upstream_protocol: Protocol::OpenAiResponses,
                        conversion_enabled: false,
                        strategy: Default::default(),
                        enabled: true,
                        retry: Default::default(),
                        targets: vec![ProxyTargetConfig {
                            id: target,
                            provider_entry_id: provider,
                            secret_id: entry.secret_refs[0].id.clone(),
                            label: "Upstream".into(),
                            base_url: "http://127.0.0.1:9/v1".into(),
                            auth_scheme: "bearer".into(),
                            headers: vec![("x-private".into(), "fake-panel-header-secret".into())],
                            group: None,
                            priority: 0,
                            weight: 1,
                            enabled: true,
                            protocol: None,
                            prefer_ws: false,
                        }],
                    }],
                    ..Default::default()
                },
            )?;
            Ok(())
        })
        .unwrap();
        let code = session::with_vault(&state, false, |vault| {
            ControlPanel::rotate_access_code(&state, vault, false)
        })
        .unwrap()
        .access_code
        .into_inner();
        let status = ControlPanel::configure(
            &state,
            vault_id,
            aipass_agent_protocol::ControlPanelSettings {
                enabled: true,
                address: "127.0.0.1".into(),
                port: port(),
                https,
            },
            None,
            false,
        )
        .unwrap();
        let mut client = Client::builder()
            .no_proxy()
            .local_address(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST))
            .timeout(Duration::from_secs(5));
        if let Some(pem) = status.certificate_pem {
            client = client
                .add_root_certificate(reqwest::Certificate::from_pem(pem.as_bytes()).unwrap());
        }
        Self {
            _temp: temp,
            state,
            client: client.build().unwrap(),
            url: status.url.unwrap(),
            code,
            route,
            target,
            vault_id,
        }
    }

    fn grant_remote_unlock(&mut self) {
        let response = crate::server::handle_request(
            &self.state,
            AgentRequest::ControlPanelRotateAccessCode {
                allow_remote_unlock: true,
            },
        );
        assert!(response.ok);
        self.code =
            serde_json::from_value::<aipass_agent_protocol::ControlPanelAccessCode>(response.data)
                .unwrap()
                .access_code
                .into_inner();
    }

    fn login(&self, code: &str) -> Response {
        self.client
            .post(format!("{}/api/login", self.url))
            .header("x-aipass-panel", "1")
            .header("origin", &self.url)
            .json(&json!({"accessCode":code}))
            .send()
            .unwrap()
    }

    fn credentials(&self) -> (String, String) {
        let response = self.login(&self.code);
        assert_eq!(response.status(), 200);
        let cookie = response.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let csrf = response.json::<Value>().unwrap()["csrf"]
            .as_str()
            .unwrap()
            .to_owned();
        (cookie, csrf)
    }

    fn get(&self, cookie: &str) -> Response {
        self.client
            .get(format!("{}/api/state", self.url))
            .header("x-aipass-panel", "1")
            .header("cookie", cookie)
            .send()
            .unwrap()
    }

    fn action(&self, cookie: &str, csrf: &str, action: Value) -> Response {
        self.client
            .post(format!("{}/api/action", self.url))
            .header("x-aipass-panel", "1")
            .header("cookie", cookie)
            .header("x-aipass-csrf", csrf)
            .header("origin", &self.url)
            .json(&action)
            .send()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.state
            .shutdown
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.state.sync_watcher.lock().unwrap().take();
        self.state.control_panel.shutdown();
        crate::server::TOOL_HOME_OVERRIDES
            .lock()
            .unwrap()
            .remove(&self.vault_id);
    }
}

#[test]
fn http_management_authentication_revocation_and_proxy_continuity() {
    let mut fixture = Fixture::new(false);
    let page = fixture.client.get(&fixture.url).send().unwrap();
    assert_eq!(page.status(), 200);
    assert!(page.headers()["content-security-policy"]
        .to_str()
        .unwrap()
        .contains("frame-ancestors 'none'"));
    assert!(page.text().unwrap().contains("panel.js"));
    assert_eq!(fixture.get("").status(), 401);
    let (cookie, csrf) = fixture.credentials();
    let activity = match &*fixture.state.session.lock().unwrap() {
        SessionState::Unlocked(info) => info.last_activity_at,
        _ => unreachable!(),
    };
    let snapshot = fixture.get(&cookie).text().unwrap();
    for forbidden in [
        "fake-panel-provider-secret",
        "fake-panel-proxy-token",
        "fake-panel-header-secret",
        &fixture.code,
    ] {
        assert!(!snapshot.contains(forbidden));
    }
    match &*fixture.state.session.lock().unwrap() {
        SessionState::Unlocked(info) => assert_eq!(info.last_activity_at, activity),
        _ => unreachable!(),
    }
    assert_eq!(
        fixture
            .action(&cookie, "wrong", json!({"type":"proxy_start"}))
            .status(),
        401
    );
    let cross_origin = fixture
        .client
        .post(format!("{}/api/action", fixture.url))
        .header("x-aipass-panel", "1")
        .header("origin", "http://evil.test")
        .header("cookie", &cookie)
        .header("x-aipass-csrf", &csrf)
        .json(&json!({"type":"proxy_start"}))
        .send()
        .unwrap();
    assert_eq!(cross_origin.status(), 403);
    assert_eq!(
        fixture
            .client
            .get(&fixture.url)
            .header("host", "evil.test")
            .send()
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        fixture
            .action(&cookie, &csrf, json!({"type":"vault.reset"}))
            .status(),
        400
    );
    assert_eq!(
        fixture
            .action(&cookie, &csrf, json!({"type":"proxy_start"}))
            .status(),
        200
    );
    assert!(fixture.state.proxy.lock().unwrap().status().running);

    let before: Value = fixture.get(&cookie).json().unwrap();
    let target = &before["routes"][0]["targets"][0];
    let patch = json!({"type":"target_update","routeId":fixture.route,"targetId":fixture.target,
        "revision":before["revision"],"providerEntryId":target["providerEntryId"],"secretId":target["secretId"],
        "enabled":true,"priority":7,"weight":3,"preferWs":true});
    assert_eq!(fixture.action(&cookie, &csrf, patch.clone()).status(), 200);
    assert_eq!(fixture.action(&cookie, &csrf, patch).status(), 409); // stale browser cannot overwrite another edit
    let after: Value = fixture.get(&cookie).json().unwrap();
    assert_eq!(after["routes"][0]["targets"][0]["priority"], 7);
    assert!(fixture.state.proxy.lock().unwrap().status().running);

    let logout = fixture
        .client
        .post(format!("{}/api/logout", fixture.url))
        .header("x-aipass-panel", "1")
        .header("origin", &fixture.url)
        .header("cookie", &cookie)
        .header("x-aipass-csrf", &csrf)
        .json(&json!({}))
        .send()
        .unwrap();
    assert_eq!(logout.status(), 200);
    assert_eq!(fixture.get(&cookie).status(), 401);
    assert!(!session::session_status(&fixture.state).unwrap().locked);
    let (cookie, _) = fixture.credentials();
    session::lock_session(&fixture.state, LockReason::Manual);
    assert_eq!(fixture.get(&cookie).status(), 401);
    assert_eq!(fixture.login(&fixture.code).status(), 423);
    assert!(fixture.state.proxy.lock().unwrap().status().running);
    // A new local session does not revive the old remote session.
    let vault = Vault::open(
        &fixture.state.vault_dir,
        &SecretString::new("panel-test-vault-password"),
    )
    .unwrap();
    session::set_session_vault(&fixture.state, vault);
    assert_eq!(fixture.get(&cookie).status(), 401);
    let (cookie, _) = fixture.credentials();
    fixture.code = session::with_vault(&fixture.state, false, |vault| {
        ControlPanel::rotate_access_code(&fixture.state, vault, false)
    })
    .unwrap()
    .access_code
    .into_inner();
    assert_eq!(fixture.get(&cookie).status(), 401);
    let persisted = std::fs::read_to_string(settings_path(&fixture.state)).unwrap();
    assert!(!persisted.contains(&fixture.code));
    assert!(!persisted.contains("panel-test-vault-password"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(settings_path(&fixture.state))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn optional_https_uses_a_verified_certificate_and_secure_cookie() {
    let fixture = Fixture::new(true);
    let response = fixture.login(&fixture.code);
    assert_eq!(response.status(), 200);
    let cookie = response.headers()["set-cookie"].to_str().unwrap();
    assert!(cookie.starts_with("__Host-aipass-panel-"));
    assert!(cookie.contains("; Secure;"));
    // A normal untrusted client rejects the self-signed certificate.
    let error = Client::builder()
        .no_proxy()
        .local_address(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST))
        .tls_certs_only([])
        .build()
        .unwrap()
        .get(&fixture.url)
        .send()
        .unwrap_err();
    assert!(format!("{error:?}").contains("UnknownIssuer"), "{error:?}");
    let identity = tls::Identity::generate("127.0.0.1".parse().unwrap()).unwrap();
    assert!(tls::server_config(&identity, "127.0.0.2".parse().unwrap()).is_err());
}

#[test]
fn switching_credentials_clears_old_headers_and_survives_listener_restart() {
    let fixture = Fixture::new(false);
    let (cookie, csrf) = fixture.credentials();
    let replacement = session::with_vault(&fixture.state, false, |vault| {
        let id = vault
            .add_provider(crate::server::tests::sync_test_provider(
                "Replacement upstream",
                "replacement-panel-secret",
            ))
            .unwrap();
        Ok(vault.get_provider_summary(id).unwrap())
    })
    .unwrap();
    let before: Value = fixture.get(&cookie).json().unwrap();
    let response = fixture.action(
        &cookie,
        &csrf,
        json!({
            "type":"target_update", "routeId":fixture.route, "targetId":fixture.target,
            "revision":before["revision"], "providerEntryId":replacement.id,
            "secretId":replacement.secret_refs[0].id, "enabled":true,
            "priority":0, "weight":1, "preferWs":false
        }),
    );
    assert_eq!(response.status(), 200);
    session::with_vault(&fixture.state, false, |vault| {
        let config = fixture.state.proxy.lock().unwrap().config(vault)?;
        let target = &config.routes[0].targets[0];
        assert_eq!(target.provider_entry_id, replacement.id);
        assert_eq!(target.secret_id, replacement.secret_refs[0].id);
        assert!(target.headers.is_empty());
        Ok(())
    })
    .unwrap();
    assert!(!fixture
        .get(&cookie)
        .text()
        .unwrap()
        .contains("replacement-panel-secret"));

    fixture.state.control_panel.shutdown();
    ControlPanel::restore(&fixture.state);
    assert!(fixture.state.control_panel.status().unwrap().running);
    assert_eq!(fixture.get(&cookie).status(), 401);
    assert_eq!(fixture.login(&fixture.code).status(), 200);
    session::lock_session(&fixture.state, LockReason::Manual);
    assert!(crate::server::handle_request(&fixture.state, AgentRequest::ControlPanelStop).ok);
    ControlPanel::restore(&fixture.state);
    let status = fixture.state.control_panel.status().unwrap();
    assert!(!status.running);
    assert!(!status.settings.enabled);
}

#[test]
fn password_change_and_inflight_lock_revoke_authorization_at_vault_access() {
    let fixture = Fixture::new(false);
    let (cookie, _) = fixture.credentials();
    let response = crate::server::handle_request(
        &fixture.state,
        AgentRequest::VaultChangePassword {
            new_password: "different-panel-vault-password".into(),
        },
    );
    assert!(response.ok);
    assert_eq!(fixture.get(&cookie).status(), 401);
    // Check revocation again at the actual storage boundary, not only in HTTP middleware.
    let sessions = sessions::Sessions::default();
    let binding = match &*fixture.state.session.lock().unwrap() {
        SessionState::Unlocked(info) => sessions::Binding::from_session(info),
        _ => unreachable!(),
    };
    let (token, csrf) = sessions.issue(binding, sessions.generation()).unwrap();
    let result = sessions.authorized(token.expose(), Some(&csrf), || {
        session::lock_session(&fixture.state, LockReason::Manual);
        let vault = Vault::open(
            &fixture.state.vault_dir,
            &SecretString::new("different-panel-vault-password"),
        )
        .unwrap();
        session::set_session_vault(&fixture.state, vault);
        session::with_vault(&fixture.state, false, |_| Ok(()))
    });
    assert!(result.is_err());
}

#[test]
fn login_is_bounded_and_does_not_accept_unknown_fields() {
    let fixture = Fixture::new(false);
    let master_password_payload = fixture
        .client
        .post(format!("{}/api/login", fixture.url))
        .header("x-aipass-panel", "1")
        .header("origin", &fixture.url)
        .json(&json!({"password":"panel-test-vault-password"}))
        .send()
        .unwrap();
    assert_eq!(master_password_payload.status(), 400);
    for _ in 0..4 {
        assert_eq!(fixture.login("wrong").status(), 401);
    }
    assert_eq!(fixture.login("wrong").status(), 429);
    assert!(!ControlPanelSettings::default().enabled);
}

#[test]
fn replacing_the_vault_requires_a_new_locally_generated_access_code() {
    let mut fixture = Fixture::new(false);
    let (cookie, _) = fixture.credentials();
    let replacement = Vault::create(
        fixture._temp.path().join("replacement"),
        &SecretString::new("replacement-password"),
    )
    .unwrap()
    .vault;
    session::set_session_vault(&fixture.state, replacement);
    assert_eq!(fixture.get(&cookie).status(), 401);
    assert_eq!(fixture.login(&fixture.code).status(), 401);
    fixture.code = session::with_vault(&fixture.state, false, |vault| {
        ControlPanel::rotate_access_code(&fixture.state, vault, false)
    })
    .unwrap()
    .access_code
    .into_inner();
    assert_eq!(fixture.login(&fixture.code).status(), 200);
}

#[test]
fn tool_preview_is_redacted_bound_to_confirmation_and_writes_only_the_test_home() {
    let fixture = Fixture::new(false);
    let (cookie, csrf) = fixture.credentials();
    let state: Value = fixture.get(&cookie).json().unwrap();
    let tool_home = fixture._temp.path().join("tools");
    std::fs::create_dir_all(tool_home.join(".codex")).unwrap();
    let auth_file = tool_home.join(".codex/auth.json");
    std::fs::write(
        &auth_file,
        r#"{"OPENAI_API_KEY":"old-credential-not-in-vault"}"#,
    )
    .unwrap();
    let selection = json!({"type":"tool_preview","selection":{"source":"credential","request":{
        "tool":"codex","id":state["providers"][0]["id"],"mode":"plaintext"}}});
    let response = fixture.action(&cookie, &csrf, selection.clone());
    assert_eq!(response.status(), 200, "{}", response.text().unwrap());
    let preview: Value = response.json().unwrap();
    let serialized = preview.to_string();
    for secret in ["fake-panel-provider-secret", "old-credential-not-in-vault"] {
        assert!(!serialized.contains(secret), "preview leaked a credential");
    }
    assert!(std::fs::read_to_string(&auth_file)
        .unwrap()
        .contains("old-credential-not-in-vault"));
    std::fs::write(&auth_file, r#"{"OPENAI_API_KEY":"concurrent-edit"}"#).unwrap();
    assert_eq!(
        fixture
            .action(
                &cookie,
                &csrf,
                json!({"type":"tool_apply","previewId":preview["previewId"]})
            )
            .status(),
        409
    );
    let preview: Value = fixture.action(&cookie, &csrf, selection).json().unwrap();
    let apply = json!({"type":"tool_apply","previewId":preview["previewId"]});
    assert_eq!(fixture.action(&cookie, &csrf, apply.clone()).status(), 200);
    assert!(std::fs::read_to_string(&auth_file)
        .unwrap()
        .contains("fake-panel-provider-secret"));
    assert_eq!(fixture.action(&cookie, &csrf, apply).status(), 400); // one-shot confirmation
    let proxy_preview = fixture.action(&cookie, &csrf, json!({"type":"tool_preview","selection":{"source":"proxy","request":{"tool":"claude_code","routeId":fixture.route}}}));
    // The typed snake_case tool is accepted; incompatible proxy protocol is rejected by the shared core.
    assert_eq!(proxy_preview.status(), 400);
}

#[test]
#[ignore = "interactive browser fixture; set AIPASS_PANEL_FIXTURE_OUTPUT to a temporary file"]
fn browser_fixture() {
    let path = std::path::PathBuf::from(
        std::env::var_os("AIPASS_PANEL_FIXTURE_OUTPUT").expect("fixture output path"),
    );
    let mut fixture = Fixture::new(false);
    // Representative, entirely synthetic data for visual QA. All upstream URLs
    // remain loopback test addresses; this fixture never contacts a provider.
    session::with_vault(&fixture.state, false, |vault| {
        let mut proxy = fixture.state.proxy.lock().unwrap();
        let mut config = proxy.config(vault)?;
        let primary = config.routes[0].targets[0].provider_entry_id;
        let mut input =
            crate::server::tests::sync_test_provider("OpenAI Workspace", "sk-preview-primary-7a42");
        input.secret_label = Some("Development".into());
        vault
            .update_provider(
                primary,
                serde_json::from_value(serde_json::to_value(input).unwrap()).unwrap(),
            )
            .unwrap();
        let mut backup_input =
            crate::server::tests::sync_test_provider("OpenRouter Backup", "sk-preview-backup-9f31");
        backup_input.provider_id = Some("openrouter".into());
        backup_input.secret_label = Some("Fallback".into());
        let backup = vault.add_provider(backup_input).unwrap();
        let mut automation_input = crate::server::tests::sync_test_provider(
            "DeepSeek Automation",
            "sk-preview-automation-2c86",
        );
        automation_input.provider_id = Some("deepseek".into());
        automation_input.secret_label = Some("Automation".into());
        let automation = vault.add_provider(automation_input).unwrap();
        config.routes[0].name = "日常开发".into();
        config.routes[0].targets[0].label = "OpenAI Workspace".into();
        config.routes[0].targets[0].prefer_ws = true;
        let mut fallback = config.routes[0].targets[0].clone();
        fallback.id = Uuid::new_v4();
        fallback.label = "OpenRouter Backup".into();
        fallback.provider_entry_id = backup;
        fallback.secret_id = vault.get_provider_summary(backup).unwrap().secret_refs[0]
            .id
            .clone();
        fallback.priority = 1;
        fallback.prefer_ws = false;
        config.routes[0].targets.push(fallback);
        let mut automation_route = config.routes[0].clone();
        automation_route.id = Uuid::new_v4();
        automation_route.name = "自动化任务".into();
        automation_route.token = "fake-panel-automation-token".into();
        automation_route.targets.truncate(1);
        automation_route.targets[0].id = Uuid::new_v4();
        automation_route.targets[0].label = "DeepSeek Automation".into();
        automation_route.targets[0].provider_entry_id = automation;
        automation_route.targets[0].secret_id =
            vault.get_provider_summary(automation).unwrap().secret_refs[0]
                .id
                .clone();
        automation_route.targets[0].prefer_ws = false;
        config.routes.push(automation_route);
        proxy.set_config(vault, config)?;
        Ok(())
    })
    .unwrap();
    fixture.grant_remote_unlock();
    session::lock_session(&fixture.state, LockReason::Manual);
    atomic_write_bytes(
        &path,
        &serde_json::to_vec(&json!({"url":fixture.url,"accessCode":fixture.code})).unwrap(),
    )
    .unwrap();
    let started = std::time::Instant::now();
    while path.exists() && started.elapsed() < Duration::from_secs(1800) {
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn remote_code_unlocks_after_agent_restart_and_restores_proxy() {
    let mut fixture = Fixture::new(false);
    fixture.grant_remote_unlock();
    let (cookie, csrf) = fixture.credentials();
    assert_eq!(
        fixture
            .action(&cookie, &csrf, json!({"type":"proxy_start"}))
            .status(),
        200
    );
    assert_eq!(
        fixture
            .action(&cookie, &csrf, json!({"type":"vault_lock"}))
            .status(),
        200
    );
    assert!(session::session_status(&fixture.state).unwrap().locked);
    assert_eq!(fixture.get(&cookie).status(), 401);
    assert_eq!(fixture.login(&fixture.code).status(), 200);
    assert!(!session::session_status(&fixture.state).unwrap().locked);
    assert!(fixture.state.last_lock_reason.lock().unwrap().is_none());

    let root = fixture.state.vault_dir.clone();
    fixture.state.control_panel.shutdown();
    session::lock_session(&fixture.state, LockReason::Manual);
    // Persisted proxy enablement survives a fresh Agent, just like the panel grant.
    fixture.state = crate::server::tests::sync_test_state(root);
    let sync_folder = fixture._temp.path().join("remote-unlock-sync");
    std::fs::create_dir_all(&sync_folder).unwrap();
    atomic_write_bytes(
        session::sync_settings_path(&fixture.state.vault_dir),
        &serde_json::to_vec(&session::PersistedSyncSettings {
            sync_folder: Some(sync_folder),
            ..Default::default()
        })
        .unwrap(),
    )
    .unwrap();
    assert!(session::session_status(&fixture.state).unwrap().locked);
    assert!(!fixture.state.proxy.lock().unwrap().status().running);
    ControlPanel::restore(&fixture.state);
    assert_eq!(fixture.get(&cookie).status(), 401);
    assert_eq!(fixture.login(&fixture.code).status(), 200);
    assert!(!session::session_status(&fixture.state).unwrap().locked);
    assert!(fixture.state.proxy.lock().unwrap().status().running);
    assert!(fixture.state.sync_watcher.lock().unwrap().is_some());
    let persisted = std::fs::read_to_string(settings_path(&fixture.state)).unwrap();
    for secret in [
        &fixture.code,
        "panel-test-vault-password",
        "fake-panel-provider-secret",
        "fake-panel-proxy-token",
    ] {
        assert!(!persisted.contains(secret));
    }
}

#[test]
fn remote_unlock_rejects_wrong_rotated_password_changed_and_revoked_codes() {
    let mut fixture = Fixture::new(false);
    let ordinary = fixture.code.clone();
    fixture.grant_remote_unlock();
    let old_remote = fixture.code.clone();
    fixture.grant_remote_unlock();
    session::lock_session(&fixture.state, LockReason::Manual);
    assert_eq!(fixture.login(&ordinary).status(), 401);
    assert_eq!(fixture.login(&old_remote).status(), 401);
    assert_eq!(fixture.login(&sessions::random_token()).status(), 401);
    assert!(session::session_status(&fixture.state).unwrap().locked);
    let (cookie, _) = fixture.credentials();
    assert!(
        crate::server::handle_request(
            &fixture.state,
            AgentRequest::VaultChangePassword {
                new_password: "new-remote-test-password".into(),
            }
        )
        .ok
    );
    assert_eq!(fixture.get(&cookie).status(), 401);
    assert_eq!(fixture.login(&fixture.code).status(), 401);
    session::lock_session(&fixture.state, LockReason::Manual);
    // Avoid the HTTP attempt limit to separately verify the locked path.
    let listener_sessions = fixture
        .state
        .control_panel
        .inner
        .lock()
        .unwrap()
        .listener
        .as_ref()
        .unwrap()
        .sessions
        .clone();
    assert!(ControlPanel::login(
        &fixture.state,
        &listener_sessions,
        &fixture.code,
        listener_sessions.generation()
    )
    .is_err());
    assert!(session::session_status(&fixture.state).unwrap().locked);
    // Re-grant against the new password revision, then revoke a still-valid code while locked.
    session::unlock_with_password(&fixture.state, "new-remote-test-password".into()).unwrap();
    fixture.grant_remote_unlock();
    session::lock_session(&fixture.state, LockReason::Manual);
    assert!(ControlPanel::login(
        &fixture.state,
        &listener_sessions,
        &fixture.code,
        listener_sessions.generation()
    )
    .is_ok());
    session::lock_session(&fixture.state, LockReason::Manual);
    assert!(
        crate::server::handle_request(
            &fixture.state,
            AgentRequest::ControlPanelDisableRemoteUnlock
        )
        .ok
    );
    ControlPanel::restore(&fixture.state);
    let status = fixture.state.control_panel.status().unwrap();
    assert!(!status.running && !status.remote_unlock_enabled && !status.has_access_code);
    assert!(ControlPanel::login(
        &fixture.state,
        &listener_sessions,
        &fixture.code,
        listener_sessions.generation()
    )
    .is_err());
    assert!(session::session_status(&fixture.state).unwrap().locked);
    let stored: Stored =
        serde_json::from_slice(&std::fs::read(settings_path(&fixture.state)).unwrap()).unwrap();
    assert!(stored.remote_unlock.is_none() && stored.access_code_hash.is_none());
}

#[test]
fn queued_login_from_old_listener_or_generation_cannot_unlock() {
    let mut fixture = Fixture::new(false);
    fixture.grant_remote_unlock();
    let sessions = fixture
        .state
        .control_panel
        .inner
        .lock()
        .unwrap()
        .listener
        .as_ref()
        .unwrap()
        .sessions
        .clone();
    let generation = sessions.generation();
    fixture.grant_remote_unlock();
    session::lock_session(&fixture.state, LockReason::Manual);
    assert!(ControlPanel::login(&fixture.state, &sessions, &fixture.code, generation).is_err());
    assert!(session::session_status(&fixture.state).unwrap().locked);
    fixture.state.control_panel.shutdown();
    ControlPanel::restore(&fixture.state);
    assert!(ControlPanel::login(
        &fixture.state,
        &sessions,
        &fixture.code,
        sessions.generation()
    )
    .is_err());
    assert!(session::session_status(&fixture.state).unwrap().locked);
    assert_eq!(fixture.login(&fixture.code).status(), 200);
}
