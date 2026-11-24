use crate::NativeHostConfig;
use aipass_agent::{run_server, AgentClient, ServerOptions};
use aipass_agent_protocol::{AgentRequest, SessionStatus, SessionUnlockMode};
use aipass_crypto::SecretString;
use std::{
    path::PathBuf,
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

const FIXTURE_VAULT: &str = "AIPASS_NATIVE_HOST_TEST_VAULT";

#[test]
#[ignore = "started by each native host fixture in an isolated home"]
fn serve() {
    let vault = PathBuf::from(std::env::var_os(FIXTURE_VAULT).expect("missing fixture vault"));
    run_server(ServerOptions::without_desktop_tray(vault)).unwrap();
}

pub(super) struct RunningAgent {
    pub dir: TempDir,
    pub password: String,
    pub client: AgentClient,
    child: Child,
}

impl RunningAgent {
    pub fn start() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("aipass-native-host-")
            .tempdir()
            .unwrap();
        let home = dir.path().join("home");
        let vault = dir.path().join("vault");
        std::fs::create_dir(&home).unwrap();
        let password = "correct horse battery staple".to_owned();
        aipass_vault::Vault::create(&vault, &SecretString::new(&password)).unwrap();
        let settings = aipass_agent::session::PersistedSyncSettings {
            mode: aipass_agent_protocol::SyncMode::Local,
            sync_folder: Some(dir.path().join("sync")),
            ..Default::default()
        };
        let settings_path = aipass_agent::session::sync_settings_path(&vault);
        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        std::fs::write(settings_path, serde_json::to_vec(&settings).unwrap()).unwrap();
        // The client and server share the transport directory and a unique vault
        // namespace. The server's startup, tool discovery and unlock audit use
        // disposable paths, including when the user's macOS keychain is locked.
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "test_agent::serve", "--ignored"])
            .env(FIXTURE_VAULT, &vault)
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("CFFIXED_USER_HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("APPDATA", home.join("AppData/Roaming"))
            .env("LOCALAPPDATA", home.join("AppData/Local"))
            .env("AIPASS_LOG_DIR", dir.path().join("logs"))
            .env(
                "AIPASS_PROXY_RECOVERY_KEYCHAIN",
                home.join(".aipass/proxy-recovery/fixture.keychain-db"),
            );
        #[cfg(unix)]
        command.env(
            "AIPASS_AGENT_RUNTIME_DIR",
            aipass_agent::paths::agent_runtime_dir().unwrap(),
        );
        let client = AgentClient::for_vault(vault).unwrap();
        let mut fixture = Self {
            dir,
            password,
            client,
            child: command.spawn().unwrap(),
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while fixture
            .client
            .request::<SessionStatus>(&AgentRequest::SessionStatus)
            .is_err()
        {
            assert!(
                fixture.child.try_wait().unwrap().is_none(),
                "fixture exited during startup"
            );
            assert!(Instant::now() < deadline, "fixture failed to become ready");
            thread::sleep(Duration::from_millis(50));
        }
        fixture
    }

    pub fn config(&self) -> NativeHostConfig {
        NativeHostConfig {
            vault_dir: self.client.config.vault_dir.clone(),
            allowed_extension_ids: vec![],
        }
    }

    pub fn config_with_allowed_extension(&self, extension_id: &str) -> NativeHostConfig {
        NativeHostConfig {
            allowed_extension_ids: vec![extension_id.to_owned()],
            ..self.config()
        }
    }

    pub fn unlock(&self) {
        let _: SessionStatus = self
            .client
            .request(&AgentRequest::SessionUnlock {
                mode: SessionUnlockMode::Password {
                    password: self.password.as_str().into(),
                },
            })
            .unwrap();
    }
}

impl Drop for RunningAgent {
    fn drop(&mut self) {
        let _ = self.client.shutdown();
        let deadline = Instant::now() + Duration::from_secs(3);
        while let Ok(None) = self.child.try_wait() {
            if Instant::now() >= deadline {
                let _ = self.child.kill();
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.wait();
    }
}
