//! Authenticated lifecycle roundtrip in child processes with a disposable vault.
#![cfg(unix)]
use aipass_agent::{AgentClient, ServerOptions};
use aipass_agent_protocol::{AgentRequest, SessionStatus};
use aipass_desktop_native::runtime::{agent::AgentServices, Gate, Runtime, Services};
use std::{fs, io::Write, path::Path, process::Command, sync::Arc};

fn last_pid(root: &Path) -> String {
    fs::read_to_string(root.join("pids"))
        .unwrap()
        .lines()
        .last()
        .unwrap()
        .to_owned()
}

fn workflow(root: &Path) {
    let client = AgentClient::for_vault(root.join("data/vault")).unwrap();
    let services = Arc::new(
        AgentServices::new(
            client,
            root.join("agent-launcher"),
            std::env::current_exe().unwrap(),
            None,
            false,
        )
        .unwrap(),
    );
    struct Shutdown(Arc<AgentServices>);
    impl Drop for Shutdown {
        fn drop(&mut self) {
            let _ = self.0.stop();
        }
    }
    let _cleanup = Shutdown(services.clone());
    services.ensure_ready().unwrap();
    let first_pid = last_pid(root);
    let gate = Arc::new(Gate::default());
    let runtime = Runtime::new(gate.clone(), services.clone());
    let mut replacement_pid = String::new();
    let error = runtime
        .install(false, || {
            assert!(aipass_agent::ipc::connect(root.join("data/vault")).is_err());
            assert!(gate.enter().is_err());
            // Simulate a candidate that launches its sidecar then fails readiness.
            services.ensure_ready()?;
            replacement_pid = last_pid(root);
            assert_ne!(replacement_pid, first_pid);
            anyhow::bail!("candidate readiness failed")
        })
        .unwrap_err();
    assert!(error.to_string().contains("candidate readiness failed"));
    let restored_pid = last_pid(root);
    assert_ne!(restored_pid, first_pid);
    assert_ne!(
        restored_pid, replacement_pid,
        "recovery must retire the candidate sidecar"
    );
    assert!(gate.enter().is_ok());
    let status: SessionStatus = services
        .client
        .request(&AgentRequest::SessionStatus)
        .unwrap();
    assert!(status.locked);
    assert!(runtime
        .install(false, || {
            assert!(aipass_agent::ipc::connect(root.join("data/vault")).is_err());
            Ok(())
        })
        .unwrap());
    assert!(gate.enter().is_err());
}

#[test]
fn managed_agent_is_stopped_and_replaced_after_failed_update() {
    if let Ok(role) = std::env::var("AIPASS_RUNTIME_TEST_ROLE") {
        let root = std::env::current_dir().unwrap();
        assert!(root.join("fixture-marker").is_file());
        if role == "server" {
            let mut log = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(root.join("pids"))
                .unwrap();
            writeln!(log, "{}", std::process::id()).unwrap();
            aipass_agent::run_server(ServerOptions::without_desktop_tray(root.join("data/vault")))
                .unwrap();
        } else {
            assert_eq!(role, "client");
            workflow(&root);
        }
        return;
    }
    let temp = tempfile::Builder::new()
        .prefix("ap-up-")
        .tempdir_in("/tmp")
        .unwrap();
    let root = temp.path().canonicalize().unwrap();
    for part in ["home", "r", "data/agent"] {
        fs::create_dir_all(root.join(part)).unwrap();
    }
    fs::write(root.join("fixture-marker"), b"synthetic").unwrap();
    fs::write(
        root.join("data/agent/sync-settings.json"),
        br#"{"mode":"local","cloudkitMigrationComplete":true}"#,
    )
    .unwrap();
    fs::write(root.join("agent-launcher"), b"#!/bin/sh\nAIPASS_RUNTIME_TEST_ROLE=server exec \"$AIPASS_RUNTIME_TEST_EXE\" --exact managed_agent_is_stopped_and_replaced_after_failed_update --nocapture\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(
        root.join("agent-launcher"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    let exe = std::env::current_exe().unwrap();
    let mut command = Command::new(&exe);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("AIPASS_") {
            command.env_remove(key);
        }
    }
    let output = command
        .args([
            "--exact",
            "managed_agent_is_stopped_and_replaced_after_failed_update",
            "--nocapture",
        ])
        .current_dir(&root)
        .env("AIPASS_RUNTIME_TEST_ROLE", "client")
        .env("AIPASS_RUNTIME_TEST_EXE", exe)
        .env("AIPASS_AGENT_BINARY", root.join("agent-launcher"))
        .env("AIPASS_AGENT_SUPPRESS_TRAY", "1")
        .env("AIPASS_AGENT_RUNTIME_DIR", root.join("r"))
        .env("AIPASS_LOG_DIR", root.join("logs"))
        .env("HOME", root.join("home"))
        .env("USERPROFILE", root.join("home"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
