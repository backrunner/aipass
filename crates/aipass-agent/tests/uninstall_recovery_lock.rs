//! Exercise the production library in a separate Agent process. The recovery
//! lock is held throughout, so this fixture never opens the OS credential store.
#![cfg(any(target_os = "macos", windows))]

use aipass_agent_protocol::{
    AgentRequest, AgentResponse, AuthenticatedAgentRequest, SensitiveString, SessionUnlockMode,
    AGENT_PROTOCOL_VERSION,
};
use aipass_config_writers::{
    native_auth::Resource,
    proxy_recovery::Recovery,
    transaction::{Change, Transaction},
};
use aipass_crypto::SecretString;
use aipass_vault::Vault;
use interprocess::local_socket::{prelude::*, Stream};
use std::{
    fs,
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use uuid::Uuid;
use zeroize::Zeroizing;

struct Agent(Child);
impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn request(
    runtime: &Path,
    namespace: &str,
    request: AgentRequest,
) -> anyhow::Result<AgentResponse> {
    let token = Zeroizing::new(fs::read_to_string(
        runtime.join(format!("{namespace}.token")),
    )?);
    #[cfg(target_os = "macos")]
    let mut stream = {
        let path = runtime.join(format!("{namespace}.sock"));
        Stream::connect(path.to_fs_name::<interprocess::local_socket::GenericFilePath>()?)?
    };
    #[cfg(windows)]
    let mut stream = {
        let name = format!("dev.aipass.agent.{namespace}");
        Stream::connect(name.to_ns_name::<interprocess::local_socket::GenericNamespaced>()?)?
    };
    let payload = AuthenticatedAgentRequest {
        protocol_version: AGENT_PROTOCOL_VERSION,
        auth_token: SensitiveString::new(token.trim().to_owned()),
        request_id: Some(Uuid::new_v4()),
        request,
    };
    aipass_agent::ipc::framing::write_frame_with_deadline(
        &mut stream,
        &payload,
        Duration::from_secs(20),
    )?;
    aipass_agent::ipc::framing::read_frame_with_deadline(&mut stream, Duration::from_secs(20))
}

#[test]
fn unlock_cannot_replay_pending_config_writes_while_uninstall_holds_the_lock() {
    #[cfg(target_os = "macos")]
    let temp = tempfile::Builder::new()
        .prefix("aipass-lk-")
        .tempdir_in("/tmp")
        .unwrap();
    #[cfg(windows)]
    let temp = tempfile::Builder::new()
        .prefix("aipass-lk-")
        .tempdir()
        .unwrap();
    let root = temp.path();
    let home = root.join("home");
    let runtime = root.join("r");
    let vault_dir = root.join("data/vault");
    let settings = root.join("data/agent");
    for path in [&home, &runtime, &settings] {
        fs::create_dir_all(path).unwrap();
    }
    fs::write(settings.join("sync-settings.json"), serde_json::json!({"mode":"local","syncFolder":root.join("sync"),"cloudkitMigrationComplete":true}).to_string()).unwrap();
    let password = SecretString::new("independent-recovery-fixture");
    let creation = Vault::create(&vault_dir, &password).unwrap();
    let file = home.join("vendor.json");
    fs::write(&file, br#"{"url":"original"}"#).unwrap();
    let tx = Transaction {
        operation_id: Uuid::new_v4(),
        committed: false,
        changes: vec![Change::new(
            Resource::File(file.clone()),
            Some(br#"{"url":"proxy"}"#.to_vec()),
        )
        .unwrap()],
        metadata: serde_json::json!([]),
    };
    let pending = tx
        .save(
            &vault_dir.join("local-tool-transactions"),
            &creation.vault.config_backup_key(),
        )
        .unwrap();
    tx.apply().unwrap();
    drop(creation);
    // The Agent must fail at the file lock before any device-key lookup.
    let _recovery = Recovery::open(&home.join(".aipass/proxy-recovery"), |_| {
        Ok(Zeroizing::new([7; 32]))
    })
    .unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_aipass-agent"));
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
        .args(["--vault"])
        .arg(&vault_dir)
        .current_dir(root)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("APPDATA", home.join("AppData/Roaming"))
        .env("LOCALAPPDATA", home.join("AppData/Local"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("AIPASS_AGENT_RUNTIME_DIR", &runtime)
        .env("AIPASS_LOG_DIR", root.join("logs"))
        .env("AIPASS_AGENT_SUPPRESS_TRAY", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Declared after the recovery guard: child shutdown always precedes unlock.
    let mut agent = Agent(command.spawn().unwrap());
    let namespace = aipass_agent::namespace_for_vault_dir(&vault_dir).unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if request(&runtime, &namespace, AgentRequest::SessionStatus).is_ok() {
            break;
        }
        assert!(
            agent.0.try_wait().unwrap().is_none(),
            "Fixture Agent exited"
        );
        assert!(Instant::now() < deadline, "Fixture Agent did not bind");
        std::thread::sleep(Duration::from_millis(20));
    }
    let response = request(
        &runtime,
        &namespace,
        AgentRequest::SessionUnlock {
            mode: SessionUnlockMode::Password {
                password: "independent-recovery-fixture".into(),
            },
        },
    )
    .unwrap();
    assert!(response.ok, "Unlock failed: {:?}", response.code);
    assert_eq!(fs::read(file).unwrap(), br#"{"url":"proxy"}"#);
    assert!(
        pending.exists(),
        "Pending replay discarded its recovery journal"
    );
}
