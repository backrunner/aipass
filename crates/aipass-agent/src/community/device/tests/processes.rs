use super::*;
use aipass_agent_protocol::{LockReason, SessionStatus, SessionUnlockMode, SyncMode};
use std::process::{Child, Command, Stdio};

#[test]
fn agent_child() {
    let Some(path) = std::env::var_os("AIPASS_LOCAL_GRANT_TEST_VAULT") else {
        return;
    };
    crate::server::run_server(crate::server::ServerOptions::without_desktop_tray(
        PathBuf::from(path),
    ))
    .unwrap();
}

struct Process {
    child: Child,
    client: crate::AgentClient,
}
impl Process {
    fn start(path: &Path) -> Self {
        let settings = crate::session::PersistedSyncSettings {
            mode: SyncMode::Local,
            sync_folder: Some(path.join("test-sync")),
            ..Default::default()
        };
        aipass_storage::atomic_write_bytes(
            crate::session::sync_settings_path(path),
            &serde_json::to_vec(&settings).unwrap(),
        )
        .unwrap();
        let log = std::fs::File::create(path.join("test-agent.log")).unwrap();
        let mut command = if let Some(binary) = std::env::var_os("AIPASS_EXAMPLE_TEST_AGENT") {
            let mut cmd = Command::new(binary);
            cmd.arg("--vault")
                .arg(path)
                .env(crate::desktop::SUPPRESS_TRAY_ENV, "1");
            cmd
        } else {
            let mut cmd = Command::new(std::env::current_exe().unwrap());
            cmd.args([
                "--exact",
                "community::device::tests::processes::agent_child",
                "--nocapture",
            ]);
            cmd
        };
        let child = command
            .env("AIPASS_LOCAL_GRANT_TEST_VAULT", path)
            .env("AIPASS_LOG_DIR", path.join("test-logs"))
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .spawn()
            .unwrap();
        let client = crate::AgentClient::for_vault(path.to_owned()).unwrap();
        let mut process = Self { child, client };
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if process
                .client
                .request::<SessionStatus>(&AgentRequest::SessionStatus)
                .is_ok()
            {
                break;
            }
            assert!(
                process.child.try_wait().unwrap().is_none(),
                "fixture Agent exited; see test-agent.log"
            );
            assert!(Instant::now() < deadline, "fixture Agent did not start");
            std::thread::sleep(Duration::from_millis(25));
        }
        process.unlock();
        process
    }
    fn unlock(&self) {
        self.client
            .request::<SessionStatus>(&AgentRequest::SessionUnlock {
                mode: SessionUnlockMode::Password {
                    password: SensitiveString::new("fixture-password"),
                },
            })
            .unwrap();
    }
    fn read(&self, id: Uuid, marker: &str) -> Result<SensitiveString, crate::AgentCommandError> {
        self.client.request(&AgentRequest::CommunityAccountRead {
            entry_id: id,
            marker: SensitiveString::new(marker),
        })
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.client.shutdown();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn two_actual_agents_use_only_their_local_grants_over_authenticated_ipc() {
    let root = tempfile::tempdir().unwrap();
    let password = SecretString::new("fixture-password");
    let a_path = root.path().join("a");
    let b_path = root.path().join("b");
    let a = Vault::create(&a_path, &password).unwrap().vault;
    let id = persist(&a, account()).unwrap();
    let marker = a.reveal_secret(id).unwrap();
    let mut options = crate::provider_runtime::load(&a, id).unwrap();
    options.quota_tracking = false; // Never contact a vendor from this fixture.
    crate::provider_runtime::save(&a, id, options).unwrap();
    aipass_vault::VaultSyncSnapshot::parse(
        &a.export_sync_snapshot_with_parents(Vec::new()).unwrap(),
    )
    .unwrap()
    .bootstrap(&b_path)
    .unwrap();
    drop(a);
    let a = Process::start(&a_path);
    let b = Process::start(&b_path);
    assert!(a
        .read(id, &marker)
        .unwrap()
        .expose()
        .contains("fixture-refresh"));
    assert!(b.read(id, &marker).is_err());
    drop(b);
    let b = Vault::open(&b_path, &password).unwrap();
    let mut own = account();
    own.auth = SensitiveString::new(json!({"type":"oauth","access":"device-b-access","refresh":"device-b-refresh","accountId":"alice"}).to_string());
    bind(&b, id, own).unwrap();
    drop(b);
    let b = Process::start(&b_path);
    let before = decode(a.read(id, &marker).unwrap().expose()).unwrap();
    let mut next = before.clone();
    next.revision += 1;
    next.auth = SensitiveString::new(json!({"type":"oauth","access":"device-a-new-access","refresh":"device-a-new-refresh","accountId":"alice"}).to_string());
    let update = AgentRequest::CommunityAccountWrite {
        entry_id: id,
        marker: SensitiveString::new(&marker),
        revision: before.revision,
        bundle: SensitiveString::new(serde_json::to_string(&next).unwrap()),
    };
    a.client.request::<Value>(&update).unwrap();
    assert!(a.client.request::<Value>(&update).is_err());
    assert!(b.client.request::<Value>(&update).is_err());
    assert!(a
        .read(id, &marker)
        .unwrap()
        .expose()
        .contains("device-a-new-refresh"));
    assert!(b
        .read(id, &marker)
        .unwrap()
        .expose()
        .contains("device-b-refresh"));
    a.client
        .request::<SessionStatus>(&AgentRequest::SessionLock {
            reason: LockReason::Manual,
        })
        .unwrap();
    assert!(a.read(id, &marker).is_err());
    a.unlock();
    assert!(a
        .read(id, &marker)
        .unwrap()
        .expose()
        .contains("device-a-new-refresh"));
}

mod plugin;
