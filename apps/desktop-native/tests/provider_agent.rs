//! Real authenticated Agent roundtrip in disposable child processes. No user
//! vault, vendor profile, tray, LaunchAgent, network provider or system clipboard.
use aipass_agent::{run_server, ServerOptions};
use aipass_desktop_native::{
    catalog::{draft::Draft, Action, Catalog},
    session::{AgentTransport, Command, Reply, Transport},
};
use std::{
    fs,
    path::Path,
    process::{Child, Command as Process, Stdio},
    thread,
    time::{Duration, Instant},
};
#[path = "provider_agent/credentials.rs"]
mod credentials;
#[path = "provider_agent/integration.rs"]
mod integration;
#[path = "provider_agent/subscriptions.rs"]
mod subscriptions;
#[path = "provider_agent/tools.rs"]
mod tools;

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn child_command(role: &str) -> Process {
    let mut command = Process::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "provider_agent_roundtrip", "--nocapture"])
        .env("AIPASS_NATIVE_TEST_ROLE", role)
        .stdin(Stdio::null());
    command
}

fn snapshot(transport: &mut AgentTransport) -> Catalog {
    let Reply::Catalog {
        status, catalog, ..
    } = transport.execute(Command::Refresh).unwrap()
    else {
        panic!("catalog reply expected")
    };
    assert!(!status.locked);
    *catalog
}

fn applied(transport: &mut AgentTransport, action: Action) -> Option<uuid::Uuid> {
    let Reply::ProviderApplied { selected, .. } =
        transport.execute(Command::Provider(action)).unwrap()
    else {
        panic!("mutation acknowledgement expected")
    };
    selected
}

fn workflow(root: &Path) {
    let output = fs::File::create(root.join("agent-process.log")).unwrap();
    let mut server = ChildGuard(
        child_command("server")
            .stdout(output.try_clone().unwrap())
            .stderr(output)
            .spawn()
            .unwrap(),
    );
    let mut transport = AgentTransport::for_vault(root.join("data/vault")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if transport.execute(Command::Refresh).is_ok() {
            break;
        }
        assert!(
            server.0.try_wait().unwrap().is_none(),
            "isolated Agent exited"
        );
        assert!(Instant::now() < deadline, "isolated Agent did not bind");
        thread::sleep(Duration::from_millis(20));
    }
    let Reply::Created(_) = transport
        .execute(Command::Create {
            password: "synthetic-native-workflow-password".into(),
            local_only: true,
        })
        .unwrap()
    else {
        panic!("created reply expected")
    };
    assert!(snapshot(&mut transport).active.is_empty());
    let mut draft = Draft::new(None);
    draft.title = "Native IPC fixture".into();
    draft.api_key = "synthetic-native-provider-key".into();
    draft.endpoint = "https://provider.example.invalid/v1".into();
    draft.concurrency = "4".into();
    draft.websockets = false;
    draft.notes = "first line\nsecond line".into();
    let id = applied(&mut transport, draft.into_action().unwrap()).unwrap();
    let catalog = snapshot(&mut transport);
    assert_eq!(catalog.active.len(), 1);
    let original = catalog.active[0].clone();
    assert_eq!(original.id, id);
    assert_eq!(original.max_concurrent_requests, Some(4));
    assert_eq!(original.supports_websockets, Some(false));
    assert_eq!(original.secret_refs.len(), 1);
    let mut edit = Draft::new(Some(original.clone()));
    edit.title = "Renamed native fixture".into();
    edit.concurrency.clear();
    edit.concurrency_touched = true;
    edit.tags = "native, checked".into();
    applied(&mut transport, edit.into_action().unwrap());
    let edited = snapshot(&mut transport).active.remove(0);
    assert_eq!(edited.title, "Renamed native fixture");
    assert_eq!(edited.secret_refs, original.secret_refs);
    assert_eq!(edited.supports_websockets, Some(false));
    assert!(edited
        .max_concurrent_requests
        .is_none_or(|value| value == 0));
    assert_eq!(edited.tags, ["native", "checked"]);
    subscriptions::workflow(&mut transport, id);
    integration::workflow(&mut transport, root, id);
    let credential_id = credentials::workflow(&mut transport, id);
    applied(&mut transport, Action::Favorite(id, true));
    assert!(snapshot(&mut transport).active[0].favorite);
    applied(&mut transport, Action::Archive(id));
    let catalog = snapshot(&mut transport);
    assert!(catalog.active.is_empty());
    assert_eq!(catalog.archived[0].id, id);
    applied(&mut transport, Action::Restore(id));
    assert_eq!(snapshot(&mut transport).active[0].id, id);
    applied(&mut transport, Action::Trash(id));
    let catalog = snapshot(&mut transport);
    assert!(catalog.active.is_empty());
    assert_eq!(catalog.trash[0].id, id);
    applied(&mut transport, Action::Restore(id));
    assert_eq!(snapshot(&mut transport).active[0].id, id);
    transport.execute(Command::Lock).unwrap();
    credentials::locked(&mut transport, id, &credential_id);
    subscriptions::locked(&mut transport, id);
    let rejected = transport
        .execute(Command::Provider(Action::Favorite(id, false)))
        .err()
        .unwrap();
    assert_eq!(
        rejected.code,
        Some(aipass_agent_protocol::AgentErrorCode::Locked)
    );
    let Reply::Catalog { catalog, .. } = transport.execute(Command::Refresh).unwrap() else {
        panic!("locked catalog expected")
    };
    assert!(catalog.active.is_empty() && catalog.archived.is_empty() && catalog.trash.is_empty());
    transport
        .execute(Command::Unlock("synthetic-native-workflow-password".into()))
        .unwrap();
    let restored = snapshot(&mut transport);
    assert_eq!(restored.active[0].id, id);
    assert!(restored.active[0].favorite);
    credentials::persisted(&mut transport, id, &credential_id);
    tools::workflow(&mut transport);
}

#[test]
fn provider_agent_roundtrip() {
    if let Ok(role) = std::env::var("AIPASS_NATIVE_TEST_ROLE") {
        let root = std::env::current_dir().unwrap();
        assert!(root.join("disposable-native-profile").is_file());
        if role == "server" {
            run_server(ServerOptions::without_desktop_tray(root.join("data/vault"))).unwrap();
        } else {
            assert_eq!(role, "client");
            workflow(&root);
        }
        return;
    }
    let temp = if cfg!(target_os = "macos") {
        tempfile::Builder::new()
            .prefix("ap-native-")
            .tempdir_in("/tmp")
            .unwrap()
    } else {
        tempfile::tempdir().unwrap()
    };
    let root = temp.path();
    let home = root.join("home");
    for path in [&home, &root.join("r"), &root.join("data/agent")] {
        fs::create_dir_all(path).unwrap();
    }
    fs::write(root.join("disposable-native-profile"), b"synthetic").unwrap();
    fs::create_dir_all(home.join("bin")).unwrap();
    fs::write(home.join("bin/opencode"), b"#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(home.join("bin/opencode"), fs::Permissions::from_mode(0o700)).unwrap();
    }
    fs::write(
        root.join("data/agent/sync-settings.json"),
        br#"{"mode":"local","cloudkitMigrationComplete":true}"#,
    )
    .unwrap();
    let mut command = child_command("client");
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("AIPASS_") {
            command.env_remove(key);
        }
    }
    for key in [
        "CODEX_HOME",
        "CLAUDE_CONFIG_DIR",
        "GEMINI_CLI_HOME",
        "GROK_HOME",
        "GROK_BIN_DIR",
        "PI_CODING_AGENT_DIR",
        "KIMI_CODE_HOME",
        "DSH_HOME",
    ] {
        command.env_remove(key);
    }
    command
        .env("AIPASS_NATIVE_TEST_ROLE", "client")
        .current_dir(root)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env(
            "PATH",
            format!(
                "{}:/usr/bin:/bin:/usr/sbin:/sbin",
                home.join("bin").display()
            ),
        )
        .env("APPDATA", home.join("AppData/Roaming"))
        .env("LOCALAPPDATA", home.join("AppData/Local"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_RUNTIME_DIR", root.join("r"))
        .env("AIPASS_AGENT_RUNTIME_DIR", root.join("r"))
        .env("AIPASS_LOG_DIR", root.join("logs"))
        .env("AIPASS_AGENT_SUPPRESS_TRAY", "1");
    let mut child = ChildGuard(command.spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success(), "disposable provider workflow failed");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "disposable provider workflow timed out"
        );
        thread::sleep(Duration::from_millis(50));
    }
}
