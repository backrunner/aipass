use super::quota::snapshot;
use super::*;
fn account() -> Account {
    Account{provider:"factory".into(),generation:Uuid::new_v4(),auth:SensitiveString::new(json!({"type":"oauth","access":"fake-private-community-access","refresh":"fake-private-community-refresh","accountId":"alice"}).to_string()),models:json!({"claude-sonnet-4-6":{"name":"Sonnet","api":{"npm":"@ai-sdk/anthropic","url":"https://api.factory.ai/api/llm/a/v1"}}}),revision:0,native_method:None,identity:"alice".into()}
}
#[test]
fn cli_login_retains_live_models_without_rotating_an_unchanged_binding() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::create(dir.path(), &SecretString::new("test password"))
        .unwrap()
        .vault;
    let auth = json!({"type":"oauth","accountId":"alice","nativeHome":"/private/cli-home","nativeDevice":"local-test-device","nativeProvider":"copilot"});
    let models = json!({"live-model":{"name":"Live","wire":"chat","api":{"id":"live-model","url":"https://api.githubcopilot.com"},"premiumMultiplier":1,"limit":{"context":128000,"output":16000}}});
    let id =
        register_cli_with_models(&vault, "copilot", auth.clone(), Some(models.clone())).unwrap();
    let marker = vault.reveal_secret(id).unwrap();
    let before = decode(read(&vault, id, &marker).unwrap().expose()).unwrap();
    assert_eq!(before.models, models);
    let mut newer = models;
    newer["live-model"]["limit"]["context"] = json!(200000);
    assert_eq!(
        register_cli_with_models(&vault, "copilot", auth, Some(newer.clone())).unwrap(),
        id
    );
    assert_eq!(vault.reveal_secret(id).unwrap(), marker);
    let after = decode(read(&vault, id, &marker).unwrap().expose()).unwrap();
    assert_eq!(after.models, newer);
    assert_eq!(after.revision, before.revision + 1);
    assert!(!after.auth.expose().contains("access"));
}
#[test]
fn account_rotation_is_encrypted_and_generation_bound() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::create(dir.path(), &SecretString::new("test password"))
        .unwrap()
        .vault;
    let id = persist(&vault, account()).unwrap();
    let marker = vault.reveal_secret(id).unwrap();
    let mut next = decode(read(&vault, id, &marker).unwrap().expose()).unwrap();
    assert!(
        !serde_json::to_string(&vault.get_provider_summary(id).unwrap())
            .unwrap()
            .contains("fake-private")
    );
    next.revision = 1;
    next.auth = SensitiveString::new(
        json!({"type":"oauth","access":"fake-private-community-new","accountId":"alice"})
            .to_string(),
    );
    let raw = serde_json::to_string(&next).unwrap();
    write(&vault, id, &marker, 0, &raw).unwrap();
    assert!(write(&vault, id, &marker, 0, &raw).is_err());
    next.revision = 2;
    next.auth = SensitiveString::new(
        json!({"type":"oauth","access":"other","accountId":"bob"}).to_string(),
    );
    assert!(write(
        &vault,
        id,
        &marker,
        1,
        &serde_json::to_string(&next).unwrap()
    )
    .is_err());
    assert!(read(&vault, id, "aipass:community:wrong").is_err());
    fn scan(path: &Path) {
        for entry in std::fs::read_dir(path).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                scan(&p);
            } else {
                let bytes = std::fs::read(p).unwrap();
                assert!(!String::from_utf8_lossy(&bytes).contains("fake-private-community"));
            }
        }
    }
    scan(dir.path());
    vault.archive_provider(id).unwrap();
    assert!(read(&vault, id, &marker).is_err());
}

#[cfg(unix)]
#[test]
fn a_failed_auth_write_replays_the_saved_pair_without_overwriting_newer_credentials() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::create(dir.path(), &SecretString::new("test password"))
        .unwrap()
        .vault;
    let before = account();
    let id = persist(&vault, before.clone()).unwrap();
    let marker = vault.reveal_secret(id).unwrap();
    let mut next = before.clone();
    next.revision += 1;
    next.auth = SensitiveString::new(json!({"type":"oauth","accountId":"alice","access":"rotated-access","refresh":"rotated-refresh"}).to_string());
    let mut recovery = AuthRecovery::default();
    recovery.pending.insert(id, (before.clone(), next.clone()));
    let objects = dir.path().join("objects");
    let permissions = std::fs::metadata(&objects).unwrap().permissions();
    std::fs::set_permissions(&objects, std::fs::Permissions::from_mode(0o500)).unwrap();
    let failed = write(
        &vault,
        id,
        &marker,
        0,
        &serde_json::to_string(&next).unwrap(),
    );
    std::fs::set_permissions(&objects, permissions).unwrap();
    assert!(failed.is_err());
    let current = decode(read(&vault, id, &marker).unwrap().expose()).unwrap();
    let replay = recovery.candidate(id, &current).unwrap();
    write(
        &vault,
        id,
        &marker,
        current.revision,
        &serde_json::to_string(&replay).unwrap(),
    )
    .unwrap();
    let saved = decode(read(&vault, id, &marker).unwrap().expose()).unwrap();
    assert!(recovery.candidate(id, &saved).is_none());
    assert!(saved.auth.expose().contains("rotated-refresh"));
    recovery.pending.insert(id, (before, next));
    let mut new_login = saved;
    new_login.generation = Uuid::new_v4();
    assert!(recovery.candidate(id, &new_login).is_none());
    let bridge = CommunityBridge::new(dir.path());
    bridge
        .inner
        .recovery
        .lock()
        .unwrap()
        .pending
        .insert(id, (new_login.clone(), new_login));
    bridge.revoke();
    assert!(bridge.inner.recovery.lock().unwrap().pending.is_empty());
}
#[test]
fn native_catalog_matches_rust_allowlist_and_has_no_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let bridge = CommunityBridge::new(dir.path());
    let value = bridge.catalog().unwrap();
    let ids: HashSet<&str> = value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, PROVIDERS.iter().copied().collect());
    assert!(value
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["methods"].as_array().is_some_and(|m| !m.is_empty())));
}
#[test]
fn scope_and_advisory_windows_never_exhaust_other_models() {
    let value = json!({"windows":[{"name":"All","used":40,"resetsAt":"2026-10-03T00:00:00Z"},{"name":"Core","used":100,"models":["core"]},{"name":"Total","used":100,"aside":true}]});
    let result = snapshot("factory", &value);
    assert_eq!(result.windows.len(), 3);
    assert!(result.windows[0].id.starts_with("community_account_"));
    assert!(result.windows[1].id.starts_with("community_scoped_"));
    assert!(result.windows[2].id.starts_with("community_scoped_"));
}
#[test]
fn cancellation_stops_a_waiting_native_adapter() {
    let mut worker = Adapter::start(None).unwrap();
    let p = worker.process.clone();
    let cancel = Cancel::default();
    cancel.attach(&p);
    cancel.cancel();
    assert!(p.canceled.load(Ordering::Acquire));
    assert!(p.send(&json!({"op":"catalog"})).is_err());
    assert!(worker.next().is_err());
}
