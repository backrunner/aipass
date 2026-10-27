//! Every path is inside a TempDir; no vendor CLI, vault or keychain is opened.
use super::*;
use serde_json::{json, Value};

struct Fixture {
    temp: tempfile::TempDir,
    root: PathBuf,
    owner: PathBuf,
    vault: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        Self {
            root: temp.path().join("recovery"),
            owner: temp
                .path()
                .join("AIPass.app/Contents/Resources/aipass-agent"),
            vault: temp.path().join("vault"),
            temp,
        }
    }
    fn open(&self) -> Recovery {
        Recovery::open(&self.root, |_| Ok(Zeroizing::new([7; 32]))).unwrap()
    }
    fn file(&self, name: &str) -> PathBuf {
        self.temp.path().join(name)
    }
    fn record(&self, recovery: &mut Recovery, path: &Path, content: &[u8]) -> Change {
        let change = Change::new(Resource::File(path.to_owned()), Some(content.to_vec())).unwrap();
        recovery
            .record(
                &self.owner,
                &self.vault,
                ToolId::Codex,
                vec![Change {
                    resource: change.resource.clone(),
                    before: change.before.clone(),
                    after: change.after.clone(),
                }],
            )
            .unwrap();
        change
    }
    fn apply(&self, recovery: &mut Recovery, path: &Path, content: &[u8]) {
        let change = self.record(recovery, path, content);
        change.resource.write(change.after.as_deref()).unwrap();
        recovery.commit().unwrap();
    }
}
fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn repeated_proxy_switches_restore_first_original_and_preserve_later_user_fields() {
    let f = Fixture::new();
    let file = f.file("settings.json");
    fs::write(&file, br#"{"env":{"ANTHROPIC_BASE_URL":"https://original.invalid","ANTHROPIC_AUTH_TOKEN":"original-secret"},"theme":"old"}"#).unwrap();
    let mut r = f.open();
    f.apply(&mut r, &file, br#"{"env":{"ANTHROPIC_BASE_URL":"http://127.0.0.1:1234","ANTHROPIC_AUTH_TOKEN":"route-a"},"theme":"old"}"#);
    fs::write(&file, br#"{"env":{"ANTHROPIC_BASE_URL":"http://127.0.0.1:1234","ANTHROPIC_AUTH_TOKEN":"route-a"},"theme":"user"}"#).unwrap();
    f.apply(&mut r, &file, br#"{"env":{"ANTHROPIC_BASE_URL":"http://127.0.0.1:5678","ANTHROPIC_AUTH_TOKEN":"route-b"},"theme":"user"}"#);
    assert_eq!(r.restore(&f.owner).unwrap(), 1);
    assert_eq!(
        read_json(&file),
        json!({"env":{"ANTHROPIC_BASE_URL":"https://original.invalid","ANTHROPIC_AUTH_TOKEN":"original-secret"},"theme":"user"})
    );
    assert_eq!(r.restore(&f.owner).unwrap(), 0);
}

#[test]
fn repeated_route_writes_do_not_accumulate_full_file_history() {
    let f = Fixture::new();
    let mut r = f.open();
    let file = f.file("settings.json");
    fs::write(&file, br#"{"url":"original"}"#).unwrap();
    for index in 0..200 {
        f.apply(
            &mut r,
            &file,
            serde_json::to_string(&json!({"url":format!("proxy-{index}")}))
                .unwrap()
                .as_bytes(),
        );
    }
    assert_eq!(r.receipt.steps.len(), 1);
    assert!(
        fs::metadata(f.root.join("receipt.aiprecovery"))
            .unwrap()
            .len()
            < 4096
    );
    r.restore(&f.owner).unwrap();
    assert_eq!(read_json(&file), json!({"url":"original"}));
}

#[test]
fn receipt_is_encrypted_and_wrong_key_or_tampering_changes_no_files() {
    let f = Fixture::new();
    let file = f.file("settings.json");
    fs::write(&file, br#"{"token":"original-private-token"}"#).unwrap();
    {
        let mut r = f.open();
        f.apply(&mut r, &file, br#"{"token":"proxy-private-token"}"#);
    }
    let bytes = fs::read(f.root.join("receipt.aiprecovery")).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    for secret in [
        "original-private-token",
        "proxy-private-token",
        "settings.json",
    ] {
        assert!(!text.contains(secret));
    }
    assert!(Recovery::open(&f.root, |_| Ok(Zeroizing::new([8; 32]))).is_err());
    let mut envelope: Value = serde_json::from_slice(&bytes).unwrap();
    envelope["ciphertext_b64"] = json!("invalid");
    fs::write(
        f.root.join("receipt.aiprecovery"),
        serde_json::to_vec(&envelope).unwrap(),
    )
    .unwrap();
    assert!(Recovery::open(&f.root, |_| Ok(Zeroizing::new([7; 32]))).is_err());
    assert_eq!(read_json(&file), json!({"token":"proxy-private-token"}));
}

#[test]
fn conflict_in_one_file_prevents_restoration_of_every_other_file() {
    let f = Fixture::new();
    let a = f.file("a.json");
    let b = f.file("b.json");
    let mut r = f.open();
    f.apply(&mut r, &a, br#"{"url":"proxy"}"#);
    f.apply(&mut r, &b, br#"{"url":"proxy"}"#);
    fs::write(&a, br#"{"url":"user-owned"}"#).unwrap();
    assert!(r.restore(&f.owner).is_err());
    assert_eq!(read_json(&a), json!({"url":"user-owned"}));
    assert_eq!(read_json(&b), json!({"url":"proxy"}));
    assert!(f.root.join("receipt.aiprecovery").is_file());
}

#[test]
fn absent_original_files_are_removed_but_new_user_fields_survive() {
    let f = Fixture::new();
    let mut r = f.open();
    let a = f.file("absent.json");
    let b = f.file("user.json");
    f.apply(&mut r, &a, br#"{"provider":{"url":"proxy"}}"#);
    f.apply(&mut r, &b, br#"{"provider":{"url":"proxy"}}"#);
    fs::write(&b, br#"{"provider":{"url":"proxy","user":true},"new":42}"#).unwrap();
    r.restore(&f.owner).unwrap();
    assert!(!a.exists());
    assert_eq!(read_json(&b), json!({"provider":{"user":true},"new":42}));
}

#[test]
fn interrupted_proxy_install_restores_each_written_resource_and_leaves_unwritten_ones() {
    let f = Fixture::new();
    let mut r = f.open();
    let a = f.file("a.json");
    let b = f.file("b.json");
    fs::write(&a, br#"{"url":"original"}"#).unwrap();
    let changes = vec![
        Change::new(
            Resource::File(a.clone()),
            Some(br#"{"url":"proxy"}"#.to_vec()),
        )
        .unwrap(),
        Change::new(
            Resource::File(b.clone()),
            Some(br#"{"token":"fake-local-token"}"#.to_vec()),
        )
        .unwrap(),
    ];
    r.record(&f.owner, &f.vault, ToolId::Codex, changes)
        .unwrap();
    fs::write(&a, br#"{"url":"proxy"}"#).unwrap();
    drop(r);
    f.open().restore(&f.owner).unwrap();
    assert_eq!(read_json(&a), json!({"url":"original"}));
    assert!(!b.exists());
}

#[test]
fn failed_native_reapply_preserves_the_binding_preimage_and_uninstall_removes_ownership() {
    let f = Fixture::new();
    let mut r = f.open();
    let config = f.file("config.json");
    let binding = f.file("codex.aipbinding");
    fs::write(&config, br#"{"url":"original"}"#).unwrap();
    fs::write(&binding, b"direct-binding").unwrap();
    for (url, binding_bytes, apply) in [
        (
            br#"{"url":"proxy-a"}"#.as_slice(),
            b"proxy-a-binding".as_slice(),
            true,
        ),
        (
            br#"{"url":"proxy-b"}"#.as_slice(),
            b"proxy-b-binding".as_slice(),
            false,
        ),
    ] {
        let changes = vec![
            Change::new(Resource::File(config.clone()), Some(url.to_vec())).unwrap(),
            Change::new(
                Resource::File(binding.clone()),
                Some(binding_bytes.to_vec()),
            )
            .unwrap(),
        ];
        if apply {
            for c in &changes {
                c.resource.write(c.after.as_deref()).unwrap();
            }
        }
        r.record_native(&f.owner, &f.vault, ToolId::Codex, changes, vec![])
            .unwrap();
    }
    r.restore(&f.owner).unwrap();
    assert_eq!(read_json(&config), json!({"url":"original"}));
    assert!(!binding.exists());
}

#[test]
fn interrupted_uninstall_resumes_without_replaying_already_restored_files() {
    let f = Fixture::new();
    let mut r = f.open();
    let a = f.file("a.json");
    let b = f.file("b.json");
    f.apply(&mut r, &a, br#"{"url":"proxy-a"}"#);
    f.apply(&mut r, &a, br#"{"url":"proxy-b"}"#);
    f.apply(&mut r, &b, br#"{"url":"proxy-b"}"#);
    let tx = Transaction {
        operation_id: Uuid::new_v4(),
        committed: false,
        changes: vec![
            Change::new(Resource::File(a.clone()), None).unwrap(),
            Change::new(Resource::File(b.clone()), None).unwrap(),
        ],
        metadata: json!({"owner": f.owner}),
    };
    tx.save(&f.root, &r.key).unwrap();
    Resource::File(r.pending_path())
        .write(Some(
            &serde_json::to_vec(&json!({"operation_id":tx.operation_id})).unwrap(),
        ))
        .unwrap();
    fs::remove_file(&a).unwrap();
    assert!(r.ensure_writable().is_err());
    drop(r);
    assert_eq!(f.open().restore(&f.owner).unwrap(), 1);
    assert!(!a.exists());
    assert!(!b.exists());
}

#[test]
fn toml_restores_owned_tables_and_preserves_user_changes_and_comments() {
    let f = Fixture::new();
    let mut r = f.open();
    let file = f.file("config.toml");
    fs::write(&file, "# my config\nmodel = \"original\"\n").unwrap();
    f.apply(&mut r, &file, b"# my config\nmodel = \"proxy\"\n[model_providers.aipass]\nbase_url = \"http://127.0.0.1:1\"\n");
    fs::write(&file, "# my config\nmodel = \"proxy\"\n# user preference\nverbose = true\n[model_providers.aipass]\nbase_url = \"http://127.0.0.1:1\"\nuser_option = 9\n").unwrap();
    r.restore(&f.owner).unwrap();
    let text = fs::read_to_string(file).unwrap();
    assert!(text.contains("model = \"original\""));
    assert!(text.contains("# user preference"));
    assert!(text.contains("user_option = 9"));
    assert!(!text.contains("base_url"));
}

#[test]
fn intentional_direct_switch_retires_the_old_proxy_baseline() {
    let f = Fixture::new();
    let mut r = f.open();
    let file = f.file("config.json");
    f.apply(&mut r, &file, br#"{"url":"proxy"}"#);
    fs::write(&file, br#"{"url":"new-direct-provider"}"#).unwrap();
    r.retire(&f.owner, &ToolId::Codex, &[Resource::File(file.clone())])
        .unwrap();
    r.restore(&f.owner).unwrap();
    assert_eq!(read_json(&file), json!({"url":"new-direct-provider"}));
}

#[test]
fn direct_switch_retires_only_written_resources_and_preserves_other_profiles() {
    let f = Fixture::new();
    let mut r = f.open();
    let a = f.file("profile-a.json");
    let b = f.file("profile-b.json");
    let changes = vec![
        Change::new(
            Resource::File(a.clone()),
            Some(br#"{"url":"proxy-a"}"#.to_vec()),
        )
        .unwrap(),
        Change::new(
            Resource::File(b.clone()),
            Some(br#"{"url":"proxy-b"}"#.to_vec()),
        )
        .unwrap(),
    ];
    r.record(&f.owner, &f.vault, ToolId::Codex, changes)
        .unwrap();
    fs::write(&a, br#"{"url":"new-direct"}"#).unwrap();
    fs::write(&b, br#"{"url":"proxy-b"}"#).unwrap();
    r.retire(&f.owner, &ToolId::Codex, &[Resource::File(a.clone())])
        .unwrap();
    r.restore(&f.owner).unwrap();
    assert_eq!(read_json(&a), json!({"url":"new-direct"}));
    assert!(!b.exists());
}

#[test]
fn installation_and_vault_ownership_survive_restart_and_service_cleanup_retry() {
    let f = Fixture::new();
    let file = f.file("config.json");
    {
        let mut r = f.open();
        f.apply(&mut r, &file, br#"{"url":"proxy"}"#);
    }
    let mut r = f.open();
    let other = f.file("Other.app/Contents/Resources/aipass-agent");
    assert!(r
        .record(
            &other,
            &f.vault,
            ToolId::Codex,
            vec![Change::new(
                Resource::File(file.clone()),
                Some(br#"{"url":"other"}"#.to_vec())
            )
            .unwrap()]
        )
        .is_err());
    assert_eq!(r.restore(&other).unwrap(), 0);
    assert_eq!(r.vaults(&f.owner), vec![f.vault.clone()]);
    r.restore(&f.owner).unwrap();
    assert_eq!(r.vaults(&f.owner), vec![f.vault.clone()]);
    r.finish(&f.owner).unwrap();
    assert!(r.vaults(&f.owner).is_empty());
}

#[test]
fn global_lock_prevents_a_second_writer_and_uninstaller() {
    let f = Fixture::new();
    let first = f.open();
    assert!(Recovery::open(&f.root, |_| panic!("key access before lock")).is_err());
    drop(first);
    f.open();
}

#[test]
fn detached_installations_cannot_write_again_until_an_explicit_session_audit() {
    let f = Fixture::new();
    let mut r = f.open();
    let file = f.file("config.json");
    f.apply(&mut r, &file, br#"{"url":"proxy"}"#);
    r.restore(&f.owner).unwrap();
    assert!(r.ensure_attached(&f.owner).is_err());
    assert!(r.record(&f.owner, &f.vault, ToolId::Codex, vec![]).is_err());
    r.audit(&f.owner, &f.vault, vec![]).unwrap();
    r.ensure_attached(&f.owner).unwrap();
}

#[test]
fn legacy_proxy_without_a_verified_baseline_blocks_uninstall_and_proxy_reapply() {
    let f = Fixture::new();
    let mut r = f.open();
    let resource = Resource::File(f.file("config.json"));
    r.audit(
        &f.owner,
        &f.vault,
        vec![(ToolId::Codex, vec![resource.clone()])],
    )
    .unwrap();
    assert!(r.restore(&f.owner).is_err());
    assert!(r
        .record(
            &f.owner,
            &f.vault,
            ToolId::Codex,
            vec![Change::unchanged(resource.clone()).unwrap()]
        )
        .is_err());
    // An intentional direct configuration is the only way to retire that
    // unknown history. No localhost heuristic is used to invent an original.
    r.retire(&f.owner, &ToolId::Codex, &[resource]).unwrap();
    r.restore(&f.owner).unwrap();
}

#[test]
fn auditing_a_different_profile_never_discards_unresolved_legacy_history() {
    let f = Fixture::new();
    let mut r = f.open();
    let a = Resource::File(f.file("legacy-a.json"));
    let b = Resource::File(f.file("legacy-b.json"));
    r.audit(&f.owner, &f.vault, vec![(ToolId::Codex, vec![a.clone()])])
        .unwrap();
    r.audit(&f.owner, &f.vault, vec![]).unwrap();
    assert!(r.restore(&f.owner).is_err());
    for _ in 0..2 {
        r.audit(&f.owner, &f.vault, vec![(ToolId::Codex, vec![b.clone()])])
            .unwrap();
    }
    assert_eq!(r.receipt.steps.len(), 1);
    assert_eq!(r.receipt.steps[0].changes.len(), 2);
    r.retire(&f.owner, &ToolId::Codex, &[a]).unwrap();
    assert!(r.restore(&f.owner).is_err());
    r.retire(&f.owner, &ToolId::Codex, &[b]).unwrap();
    r.restore(&f.owner).unwrap();
}

fn codex_grant(email: &str, refresh: &str) -> Vec<u8> {
    use base64::Engine;
    let claims = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&json!({"email":email})).unwrap());
    serde_json::to_vec(&json!({"tokens":{"id_token":format!("header.{claims}.signature"),"account_id":"fixture-workspace","refresh_token":refresh}})).unwrap()
}

#[test]
fn native_original_uses_the_latest_archived_tokens_and_checks_account_identity() {
    use crate::native_auth::NativeStore;
    let f = Fixture::new();
    let mut r = f.open();
    let original_home = f.file("codex");
    let archived_home = f.file("archived");
    fs::create_dir(&original_home).unwrap();
    fs::create_dir(&archived_home).unwrap();
    let file = original_home.join("auth.json");
    let original = codex_grant("fixture@example.invalid", "old-refresh");
    fs::write(&file, &original).unwrap();
    fs::write(archived_home.join("auth.json"), &original).unwrap();
    let active = NativeStore::codex(&original_home).unwrap();
    let archived = NativeStore::codex(&archived_home).unwrap();
    let identity = active.identity().unwrap().unwrap();
    let proxy = br#"{"OPENAI_API_KEY":"synthetic-proxy-token"}"#;
    r.record_native(
        &f.owner,
        &f.vault,
        ToolId::Codex,
        vec![Change::new(Resource::File(file.clone()), Some(proxy.to_vec())).unwrap()],
        vec![NativeSource {
            active,
            archived,
            identity,
        }],
    )
    .unwrap();
    fs::write(&file, proxy).unwrap();
    let next_proxy = br#"{"OPENAI_API_KEY":"second-synthetic-proxy-token"}"#;
    r.record(
        &f.owner,
        &f.vault,
        ToolId::Codex,
        vec![Change::new(Resource::File(file.clone()), Some(next_proxy.to_vec())).unwrap()],
    )
    .unwrap();
    fs::write(&file, next_proxy).unwrap();
    r.commit().unwrap();
    assert_eq!(r.receipt.steps.len(), 1);
    // Background renewal must not be undone by replaying the old grant.
    let newest = codex_grant("fixture@example.invalid", "rotated-refresh");
    fs::write(
        archived_home.join("auth.json"),
        codex_grant("different@example.invalid", "different-refresh"),
    )
    .unwrap();
    assert!(r.restore(&f.owner).is_err());
    assert_eq!(fs::read(&file).unwrap(), next_proxy);
    fs::write(archived_home.join("auth.json"), &newest).unwrap();
    r.restore(&f.owner).unwrap();
    assert_eq!(fs::read(file).unwrap(), newest);
}

#[test]
fn opaque_launcher_edits_and_array_edits_are_conflicts() {
    let f = Fixture::new();
    let mut r = f.open();
    let file = f.file("launcher.ps1");
    f.apply(&mut r, &file, b"synthetic proxy launcher");
    fs::write(&file, b"user script").unwrap();
    assert!(r.restore(&f.owner).is_err());
    assert_eq!(fs::read(file).unwrap(), b"user script");
    assert!(merge::undo(
        Some(Path::new("a.json")),
        Some(br#"{"models":["old"]}"#),
        Some(br#"{"models":["proxy"]}"#),
        Some(br#"{"models":["user"]}"#)
    )
    .is_err());
}

#[test]
fn commented_json_edits_are_preserved_instead_of_being_reserialized_without_comments() {
    let f = Fixture::new();
    let mut r = f.open();
    let file = f.file("settings.jsonc");
    f.apply(&mut r, &file, br#"{"url":"proxy"}"#);
    let current = b"{\n// user documentation\n\"url\":\"proxy\",\"option\":true\n}";
    fs::write(&file, current).unwrap();
    assert!(r.restore(&f.owner).is_err());
    assert_eq!(fs::read(file).unwrap(), current);
}

#[cfg(unix)]
#[test]
fn symbolic_link_targets_are_never_followed() {
    let f = Fixture::new();
    let mut r = f.open();
    let file = f.file("config.json");
    let other = f.file("user.json");
    f.apply(&mut r, &file, br#"{"url":"proxy"}"#);
    fs::write(&other, b"user-owned").unwrap();
    fs::remove_file(&file).unwrap();
    std::os::unix::fs::symlink(&other, &file).unwrap();
    assert!(r.restore(&f.owner).is_err());
    assert_eq!(fs::read(other).unwrap(), b"user-owned");
}
