use super::*;
use aipass_agent_protocol::SubscriptionPluginAction as Action;

const SCRIPT: &str = r#"
import json, struct, sys
def read():
    size, = struct.unpack(">I", sys.stdin.buffer.read(4))
    return json.loads(sys.stdin.buffer.read(size))
def send(event):
    raw = json.dumps(event).encode()
    sys.stdout.buffer.write(struct.pack(">I", len(raw)) + raw)
    sys.stdout.buffer.flush()
hello = read()
assert set(hello) == {"op", "provider", "protocol", "proxyMode", "nonce"}
assert hello["op"] == "initialize" and hello["protocol"] == 1 and hello["proxyMode"] == "direct"
send({"type":"ready", "protocol":1, "nonce":hello["nonce"]})
assert read() == {"op":"shutdown"}
assert sys.stdin.buffer.read() == b""
"#;

struct Local {
    dir: tempfile::TempDir,
    path: PathBuf,
}
impl Local {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("manifest.json");
        fs::write(
            &path,
            include_bytes!("../../../../examples/subscription-provider/manifest.json"),
        )
        .unwrap();
        Self { dir, path }
    }

    fn review(&self, script: &str) -> SubscriptionPluginAction {
        fs::write(self.dir.path().join("provider.py"), script).unwrap();
        let (g, _) = inspect(&self.path).unwrap();
        Action::Enable {
            path: self.path.clone(),
            digest: g.digest,
        }
    }
}
#[test]
fn install_requires_protocol_and_a_clean_shutdown() {
    let pkg = Local::new();
    let vault = pkg.dir.path();
    let mut registry = Registry::default();
    for invalid in [
        "pass".to_string(),
        SCRIPT.replace(r#""nonce":hello["nonce"]"#, r#""nonce":"old""#),
        SCRIPT.replace(r#""protocol":1"#, r#""protocol":2"#),
        SCRIPT.replace(r#""type":"ready""#, r#""type":"http""#),
        SCRIPT.replace("send({", "sys.stdout.buffer.write(struct.pack('>I', 4097)); sys.stdout.buffer.flush(); sys.exit(0)
send({"),
        format!("{SCRIPT}\nsys.exit(1)"),
        format!("{SCRIPT}\nsend({{'type':'end'}})"),
    ] {
        let install = registry.prepare_install(vault, pkg.review(&invalid)).unwrap();
        assert!(install.verify().is_err());
        assert!(registry.catalog(vault).unwrap().is_empty());
    }
    let action = pkg.review(SCRIPT);
    let install = registry.prepare_install(vault, action).unwrap();
    let verified = install.verify().unwrap();
    assert!(registry.catalog(vault).unwrap().is_empty());
    registry.commit_install(verified).unwrap();
    assert_eq!(registry.catalog(vault).unwrap().len(), 1);
    assert!(registry.prepare_install(vault, pkg.review(SCRIPT)).is_err());
    registry
        .manage(
            vault,
            Action::Disable {
                id: "plugin.example.offline".into(),
            },
        )
        .unwrap();
}
#[test]
fn replacement_and_revocation_cannot_race_a_verified_install() {
    let pkg = Local::new();
    let vault = pkg.dir.path();
    let mut registry = Registry::default();
    let install = registry.prepare_install(vault, pkg.review(SCRIPT)).unwrap();
    let verified = install.verify().unwrap();
    let _new = pkg.review("pass");
    assert!(registry.commit_install(verified).is_err());
    for id in [Some("plugin.example.offline"), None] {
        let install = registry.prepare_install(vault, pkg.review(SCRIPT)).unwrap();
        let verified = install.verify().unwrap();
        registry.cancel(id);
        assert!(registry.commit_install(verified).is_err());
        assert!(registry.catalog(vault).unwrap().is_empty());
    }
}
#[test]
fn hung_check_expires_and_vault_lock_interrupts_verification() {
    let pkg = Local::new();
    let vault = pkg.dir.path();
    let mut registry = Registry::default();
    let script = "import time; time.sleep(60)";
    let install = registry.prepare_install(vault, pkg.review(script)).unwrap();
    assert!(install.verify().is_err());
    let install = registry.prepare_install(vault, pkg.review(script)).unwrap();
    std::thread::scope(|scope| {
        let running = scope.spawn(|| install.verify());
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(registry.prepare_install(vault, pkg.review(script)).is_err());
        registry.cancel(None);
        assert!(matches!(running.join().unwrap(), Err(e) if e == "Plugin operation revoked"));
    });
    assert!(registry.catalog(vault).unwrap().is_empty());
}

#[test]
fn both_installation_and_execution_directories_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let parent = tempfile::tempdir().unwrap();
    let packages = parent.path().join("registry").join("packages");
    create_registry_dir(&packages).unwrap();
    for dir in [
        private_stage(Some(&packages)).unwrap(),
        private_stage(None).unwrap(),
    ] {
        assert_eq!(
            fs::metadata(dir.path()).unwrap().permissions().mode() & 0o077,
            0
        );
    }
    assert_eq!(
        fs::metadata(packages).unwrap().permissions().mode() & 0o077,
        0
    );
}
