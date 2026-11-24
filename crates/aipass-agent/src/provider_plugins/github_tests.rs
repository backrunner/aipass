use super::*;
use aipass_agent_protocol::SubscriptionPluginAction as Action;
use serde_json::{json, Value};

fn manifest() -> Value {
    let mut m: Value = serde_json::from_str(include_str!(
        "../../../../examples/subscription-provider/manifest.json"
    ))
    .unwrap();
    m.as_object_mut().unwrap().remove("interpreter");
    m["runtime"] = json!("wasi");
    m["entry"] = json!("provider.wasm");
    m
}
fn release(m: &Value, bytes: &[u8], mutate: impl FnOnce(&mut Value)) -> Result<Preview> {
    let raw = serde_json::to_vec(m).unwrap();
    let asset = |id: u64, name: &str, data: &[u8]| json!({"id":id,"name":name,"size":data.len(),"digest":format!("sha256:{:x}",Sha256::digest(data)),"state":"uploaded"});
    let mut catalog = json!({"id":42,"tag_name":"v1","draft":false,"prerelease":false,"assets":[asset(1,MANIFEST_ASSET,&raw),asset(2,m["entry"].as_str().unwrap(),bytes)]});
    mutate(&mut catalog);
    prepare(
        "example/test".into(),
        &serde_json::to_vec(&catalog).unwrap(),
        false,
        |id, _| {
            Ok(match id {
                1 => raw.clone(),
                2 => bytes.to_vec(),
                _ => panic!("Unexpected asset"),
            })
        },
    )
}
// A valid command that exits immediately. It cannot pass the provider handshake.
const WASM: &[u8] = b"\0asm\x01\0\0\0\x01\x04\x01\x60\0\0\x03\x02\x01\0\x05\x03\x01\0\x01\x07\x13\x02\x06memory\x02\0\x06_start\0\0\x0a\x04\x01\x02\0\x0b";
#[test]
fn remote_source_scope_and_digest_are_required() {
    assert_eq!(
        repository("https://github.com/Owner/repo.git/").unwrap(),
        "Owner/repo"
    );
    for repo in [
        "file:///a/b",
        "https://example.org/o/r",
        "owner/../r",
        "owner/r?x",
        "../r",
    ] {
        assert!(repository(repo).is_err());
    }
    for origin in [
        "http://api.vendor.com",
        "https://localhost",
        "https://127.0.0.1",
        "https://api.vendor.com/path",
        "https://*.vendor.com",
    ] {
        assert!(!super::super::network::valid_origin(origin), "{origin}");
    }
    let mut m = manifest();
    assert!(release(&m, WASM, |r| r["assets"][1]["digest"] = json!(null)).is_err());
    assert!(release(&m, WASM, |r| r["assets"][0]["size"] = json!(1)).is_err());
    // Same size and correctly formatted SHA-256, for different bytes.
    assert!(release(&m, WASM, |r| r["assets"][1]["digest"] =
        json!(format!("sha256:{}", "00".repeat(32))))
    .is_err());
    assert!(release(&m, WASM, |r| {
        let duplicate = r["assets"][1].clone();
        r["assets"].as_array_mut().unwrap().push(duplicate);
    })
    .is_err());
    assert!(release(&m, b"wrong", |_| {}).is_err());
    m.as_object_mut().unwrap().remove("runtime");
    assert!(release(&m, WASM, |_| {}).is_err());
}
#[test]
fn release_review_pins_exact_bytes_and_revocation_clears_it() {
    let vault = tempfile::tempdir().unwrap();
    let mut registry = super::super::Registry::default();
    let preview = release(&manifest(), WASM, |_| {}).unwrap();
    let token = preview.token;
    let digest = preview.digest.clone();
    registry.preview(preview);
    let enable = |digest: String| Action::EnableGithub {
        review: token,
        digest,
    };
    assert!(registry
        .prepare_install(vault.path(), enable("wrong".into()))
        .is_err());
    let installation = registry
        .prepare_install(vault.path(), enable(digest.clone()))
        .unwrap();
    assert!(registry.catalog(vault.path()).unwrap().is_empty());
    assert!(installation.verify().is_err());
    assert!(registry.catalog(vault.path()).unwrap().is_empty());
    let packages = super::super::root(vault.path()).unwrap().join("packages");
    assert_eq!(std::fs::read_dir(packages).unwrap().count(), 0);
    assert!(registry
        .prepare_install(vault.path(), enable(digest))
        .is_ok());
    let preview = release(&manifest(), WASM, |_| {}).unwrap();
    let action = Action::EnableGithub {
        review: preview.token,
        digest: preview.digest.clone(),
    };
    registry.preview(preview);
    registry.cancel(None);
    assert!(registry.prepare_install(vault.path(), action).is_err());
}

#[test]
fn dual_release_defaults_to_wasi_and_native_selection_requires_a_new_review() {
    let platform = format!(
        "aipass-plugin-{}-{}.json",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let first = release(&manifest(), WASM, |r| {
        let mut native = r["assets"][0].clone();
        native["name"] = json!(platform);
        native["id"] = json!(3);
        r["assets"].as_array_mut().unwrap().push(native);
    })
    .unwrap();
    assert!(matches!(
        first.alternate,
        Some(super::super::Runtime::NativeSandbox)
    ));
    assert_eq!(first.summary()["runtime"], "wasi");
    let data = json!({"id":1,"tag_name":"v1","draft":false,"prerelease":false,"assets":[{"id":3,"name":platform,"state":"uploaded","size":1,"digest":null}]});
    let mut fetched = Vec::new();
    assert!(prepare(
        "owner/repo".into(),
        &serde_json::to_vec(&data).unwrap(),
        true,
        |id, _| {
            fetched.push(id);
            Ok(vec![0])
        }
    )
    .is_err());
    assert_eq!(fetched, vec![3]);
}

#[test]
fn preview_rejects_header_only_truncated_and_invalid_command_bodies() {
    let m = manifest();
    let mut bad_instruction = WASM.to_vec();
    *bad_instruction.last_mut().unwrap() = 0xff;
    let mut wrong_export = WASM.to_vec();
    let name = wrong_export
        .windows(6)
        .position(|b| b == b"_start")
        .unwrap();
    wrong_export[name] = b'x';
    for bytes in [
        &WASM[..8],
        &WASM[..WASM.len() - 1],
        &bad_instruction,
        &wrong_export,
    ] {
        assert!(release(&m, bytes, |_| {}).is_err());
    }
    // A valid function taking an i32 cannot serve as a WASI command.
    let mut arg = WASM.to_vec();
    arg[9] += 1;
    arg[12] = 1;
    arg.insert(13, 0x7f);
    assert!(release(&m, &arg, |_| {}).is_err());
    let mut wrong_slot = m.clone();
    wrong_slot["runtime"] = json!("native-sandbox");
    wrong_slot["target"] = json!(format!(
        "{}-{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    assert!(release(&wrong_slot, WASM, |_| {}).is_err());
    let mut disguised = m;
    disguised["name"] = json!("Example\u{202e}renwo");
    assert!(release(&disguised, WASM, |_| {}).is_err());
}

#[test]
#[ignore = "release.py supplies the real Agent and a compiled release package"]
fn verify_compiled_release_installation() {
    let path = std::path::PathBuf::from(std::env::var_os("AIPASS_EXAMPLE_RELEASE").unwrap());
    let m: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let entry = m["entry"].as_str().unwrap();
    let program = std::fs::read(path.with_file_name(entry)).unwrap();
    let preview = release(&m, &program, |r| {
        r["assets"][0]["name"] = json!(path.file_name().unwrap().to_str().unwrap())
    })
    .unwrap();
    let digest = preview.digest.clone();
    let review = preview.token;
    let vault = tempfile::tempdir().unwrap();
    let mut registry = super::super::Registry::default();
    registry.preview(preview);
    let enable = || Action::EnableGithub {
        review,
        digest: digest.clone(),
    };
    let verified = registry
        .prepare_install(vault.path(), enable())
        .unwrap()
        .verify()
        .unwrap();
    let saved = registry.commit_install(verified).unwrap();
    assert_eq!(saved["origin"]["repository"], "example/test");
    let id = m["id"].as_str().unwrap();
    assert!(registry.launch(vault.path(), id).is_ok());
    assert!(registry.prepare_install(vault.path(), enable()).is_err());
    let installed = std::path::Path::new(saved["path"].as_str().unwrap());
    std::fs::write(installed.with_file_name(entry), b"changed").unwrap();
    assert!(registry.launch(vault.path(), id).is_err());
    registry
        .manage(vault.path(), Action::Disable { id: id.to_owned() })
        .unwrap();
    assert!(!installed.exists());
    assert!(registry.catalog(vault.path()).unwrap().is_empty());
}

#[cfg(target_os = "macos")]
#[test]
fn an_ordinary_native_executable_cannot_be_registered_as_a_provider() {
    let mut m = manifest();
    let target = format!("macos-{}", std::env::consts::ARCH);
    m["runtime"] = json!("native-sandbox");
    m["target"] = json!(target);
    m["entry"] = json!("provider");
    let command = std::fs::read("/usr/bin/true").unwrap();
    let preview = release(&m, &command, |r| {
        r["assets"][0]["name"] = json!(format!("aipass-plugin-{target}.json"))
    })
    .unwrap();
    let vault = tempfile::tempdir().unwrap();
    let mut registry = super::super::Registry::default();
    let action = Action::EnableGithub {
        review: preview.token,
        digest: preview.digest.clone(),
    };
    registry.preview(preview);
    assert!(registry
        .prepare_install(vault.path(), action)
        .unwrap()
        .verify()
        .is_err());
    assert!(registry.catalog(vault.path()).unwrap().is_empty());
}
