use super::*;
use crate::server::tests::{sync_test_provider, sync_test_state};
use crate::session::{set_session_vault, with_vault, with_vault_mut};
use aipass_crypto::SecretString;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use tempfile::tempdir;

#[derive(Default)]
struct MemoryRemote {
    files: Mutex<BTreeMap<String, Vec<u8>>>,
    fail_put: AtomicBool,
}
impl SnapshotRemote for MemoryRemote {
    fn list(&self) -> Result<Vec<String>> {
        Ok(self.files.lock().unwrap().keys().cloned().collect())
    }
    fn get(&self, id: &str) -> Result<Vec<u8>> {
        self.files
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .context("missing")
    }
    fn publish(&self, bytes: &[u8]) -> Result<String> {
        if self.fail_put.load(Ordering::Relaxed) {
            bail!("offline");
        }
        let id = snapshot_id(bytes);
        self.files
            .lock()
            .unwrap()
            .insert(id.clone(), bytes.to_vec());
        Ok(id)
    }
}

fn first(root: &Path) -> Arc<AgentState> {
    let state = sync_test_state(root.to_path_buf());
    set_session_vault(
        &state,
        Vault::create(root, &SecretString::new("master"))
            .unwrap()
            .vault,
    );
    state
}
fn restore(root: &Path, remote: &impl SnapshotRemote) -> Arc<AgentState> {
    fs::create_dir_all(root).unwrap();
    let state = sync_test_state(root.to_path_buf());
    assert_eq!(run(&state, remote, "test").unwrap().downloaded, 1);
    assert!(crate::session::session_status(&state).unwrap().locked);
    set_session_vault(
        &state,
        Vault::open(root, &SecretString::new("master")).unwrap(),
    );
    run(&state, remote, "test").unwrap();
    state
}
fn add(state: &Arc<AgentState>, title: &str) {
    with_vault(state, false, |vault| {
        vault
            .add_provider(sync_test_provider(title, "fake-sync-secret"))
            .map_err(crate::session::map_vault_error)
    })
    .unwrap();
}
fn titles(state: &Arc<AgentState>) -> Vec<String> {
    with_vault(state, true, |vault| {
        Ok(vault
            .list_provider_summaries()
            .unwrap()
            .into_iter()
            .map(|entry| entry.title)
            .collect())
    })
    .unwrap()
}

struct HttpDav {
    url: String,
    files: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl HttpDav {
    fn start() -> Self {
        use std::io::{BufRead, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/dav", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let files = Arc::new(Mutex::new(BTreeMap::<String, Vec<u8>>::new()));
        let remote_files = files.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();
        let thread = std::thread::spawn(move || {
            while !stop_thread.load(Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    continue;
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                    .unwrap();
                let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let parts = line.split_whitespace().collect::<Vec<_>>();
                let (method, path) = (parts[0].to_string(), parts[1].to_string());
                let mut headers = BTreeMap::new();
                loop {
                    line.clear();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':') {
                        headers.insert(name.to_lowercase(), value.trim().to_string());
                    }
                }
                let len = headers
                    .get("content-length")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                let mut body = vec![0; len];
                reader.read_exact(&mut body).unwrap();
                let mut files = remote_files.lock().unwrap();
                let (status, response) = if headers.get("authorization").map(String::as_str)
                    != Some("Basic dXNlcjpkYXYtcGFzc3dvcmQ=")
                {
                    (401, Vec::new())
                } else {
                    match method.as_str() {
                        "PROPFIND" => {
                            let mut xml = String::from("<d:multistatus xmlns:d=\"DAV:\">");
                            for (name, bytes) in files.iter().filter(|(name, _)| {
                                name.starts_with(&format!("{}/", path.trim_end_matches('/')))
                            }) {
                                xml.push_str(&format!("<d:response><d:href>{name}</d:href><d:propstat><d:prop><d:getcontentlength>{}</d:getcontentlength></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>", bytes.len()));
                            }
                            xml.push_str("</d:multistatus>");
                            (207, xml.into_bytes())
                        }
                        "MKCOL" => (201, Vec::new()),
                        "GET" => files
                            .get(&path)
                            .map(|bytes| (200, bytes.clone()))
                            .unwrap_or((404, Vec::new())),
                        "PUT" => {
                            assert_eq!(headers.get("if-none-match").map(String::as_str), Some("*"));
                            if let std::collections::btree_map::Entry::Vacant(entry) =
                                files.entry(path)
                            {
                                entry.insert(body);
                                (201, Vec::new())
                            } else {
                                (412, Vec::new())
                            }
                        }
                        _ => (405, Vec::new()),
                    }
                };
                drop(files);
                write!(
                    stream,
                    "HTTP/1.1 {status} Result\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    response.len()
                )
                .unwrap();
                stream.write_all(&response).unwrap();
            }
        });
        Self {
            url,
            files,
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for HttpDav {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}

#[test]
fn webdav_first_install_imports_with_encrypted_credentials_and_polls_remote_changes() {
    use aipass_agent_protocol::{SyncMode, SyncSettingsUpdate};
    let temp = tempdir().unwrap();
    let dav = HttpDav::start();
    let a = first(&temp.path().join("a/vault"));
    add(&a, "initial");
    let update = SyncSettingsUpdate {
        mode: SyncMode::WebDav,
        sync_folder: None,
        webdav_url: Some(dav.url.clone()),
        webdav_username: Some("user".into()),
        webdav_password: Some("dav-password".into()),
        clear_webdav_password: false,
    };
    let settings = crate::session::apply_sync_settings_update(Default::default(), update.clone());
    with_vault(&a, false, |vault| {
        crate::session::save_sync_settings(&a.vault_dir, vault, &settings)
            .map_err(ServiceError::internal)
    })
    .unwrap();
    assert_eq!(crate::server::run_sync_configured(&a).unwrap().uploaded, 1);
    let b_dir = temp.path().join("b/vault");
    fs::create_dir_all(&b_dir).unwrap();
    let b = sync_test_state(b_dir.clone());
    import_sync(&b, update, "master".into()).unwrap();
    assert!(crate::session::session_status(&b).unwrap().exists);
    assert!(
        !fs::read_to_string(crate::session::sync_settings_path(&b_dir))
            .unwrap()
            .contains("dav-password")
    );
    crate::session::unlock_with_password(&b, "master".into()).unwrap();
    assert_eq!(titles(&b), vec!["initial"]);
    add(&a, "from remote");
    crate::server::run_sync_configured(&a).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(35);
    while !titles(&b).contains(&"from remote".to_string()) {
        assert!(
            std::time::Instant::now() < deadline,
            "WebDAV changes must arrive without a manual sync"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    b.shutdown.store(true, Ordering::Relaxed);
    b.sync_watcher.lock().unwrap().take();
    for bytes in dav.files.lock().unwrap().values() {
        assert!(!String::from_utf8_lossy(bytes).contains("fake-sync-secret"));
        assert!(!String::from_utf8_lossy(bytes).contains("initial"));
    }
    let unauthorized = aipass_sync::HttpWebDavClient::new(&dav.url, None, None).unwrap();
    assert_eq!(
        crate::server::run_sync_webdav_target(&a, &unauthorized, "unauthorized").status,
        SyncStatus::AuthFailed
    );
}

#[test]
fn lock_queues_the_latest_edit_and_webdav_transfers_it_without_unlocking() {
    use aipass_agent_protocol::{LockReason, SyncMode, SyncSettingsUpdate};
    let temp = tempdir().unwrap();
    let dav = HttpDav::start();
    let state = first(&temp.path().join("a/vault"));
    let settings = crate::session::apply_sync_settings_update(
        Default::default(),
        SyncSettingsUpdate {
            mode: SyncMode::WebDav,
            sync_folder: None,
            webdav_url: Some(dav.url.clone()),
            webdav_username: Some("user".into()),
            webdav_password: Some("dav-password".into()),
            clear_webdav_password: false,
        },
    );
    with_vault(&state, false, |vault| {
        crate::session::save_sync_settings(&state.vault_dir, vault, &settings)
            .map_err(ServiceError::internal)
    })
    .unwrap();
    crate::server::run_sync_configured(&state).unwrap();
    add(&state, "edit immediately before lock");
    crate::session::lock_session(&state, LockReason::Manual);
    assert!(crate::session::session_status(&state).unwrap().locked);
    assert_eq!(
        crate::server::run_sync_configured(&state).unwrap().uploaded,
        1
    );
    assert!(crate::session::session_status(&state).unwrap().locked);
    let client = aipass_sync::HttpWebDavClient::new(
        &dav.url,
        Some("user".into()),
        Some("dav-password".into()),
    )
    .unwrap();
    let (bytes, _) = single_snapshot(&aipass_sync::WebDavSnapshotRemote(&client)).unwrap();
    let restored = temp.path().join("b/vault");
    VaultSyncSnapshot::parse(&bytes)
        .unwrap()
        .bootstrap(&restored)
        .unwrap();
    assert_eq!(
        Vault::open(restored, &SecretString::new("master"))
            .unwrap()
            .list_provider_summaries()
            .unwrap()[0]
            .title,
        "edit immediately before lock"
    );
    let mut changed_settings = crate::session::load_sync_settings(&state.vault_dir).unwrap();
    changed_settings.webdav_url = Some("https://another.example/vault".into());
    assert!(
        crate::session::transport_password(&state, &changed_settings, None).is_err(),
        "never reuse transport credentials for another destination"
    );
}

#[test]
fn cloudkit_bridge_restores_an_absent_vault_and_rejects_account_switches() {
    use aipass_agent_protocol::{CloudKitCommand, CloudKitCompletion, CloudKitReply};
    use base64::{engine::general_purpose::STANDARD, Engine};
    let temp = tempdir().unwrap();
    let source = first(&temp.path().join("a/vault"));
    add(&source, "from CloudKit ciphertext");
    let bytes = with_vault(&source, false, |vault| {
        vault
            .export_sync_snapshot()
            .map_err(crate::session::map_vault_error)
    })
    .unwrap();
    let id = snapshot_id(&bytes);
    let target = sync_test_state(temp.path().join("b/vault"));
    fs::create_dir_all(&target.vault_dir).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let worker_state = target.clone();
    let account = Arc::new(Mutex::new("a".repeat(64)));
    let worker_account = account.clone();
    let worker = std::thread::spawn(move || {
        let mut completion = None;
        while !worker_stop.load(Ordering::Relaxed) {
            if let Some(task) = worker_state.cloudkit.exchange(completion.take()).unwrap() {
                let mut reply = CloudKitReply {
                    account: Some(worker_account.lock().unwrap().clone()),
                    ..Default::default()
                };
                match task.command {
                    CloudKitCommand::List => reply.ids = vec![id.clone()],
                    CloudKitCommand::Get { id: requested } => {
                        assert_eq!(requested, id);
                        reply.bytes_b64 = Some(STANDARD.encode(&bytes));
                    }
                    CloudKitCommand::Put { .. } => panic!("locked restore must never upload"),
                }
                completion = Some(CloudKitCompletion { id: task.id, reply });
            }
        }
    });
    assert_eq!(run_cloudkit(&target).unwrap().downloaded, 1);
    assert!(crate::session::session_status(&target).unwrap().locked);
    let vault = Vault::open(&target.vault_dir, &SecretString::new("master")).unwrap();
    assert_eq!(
        vault.list_provider_summaries().unwrap()[0].title,
        "from CloudKit ciphertext"
    );
    *account.lock().unwrap() = "b".repeat(64);
    assert!(run_cloudkit(&target).is_err());
    assert_eq!(
        fs::read_to_string(target.vault_dir.join("sync-state/cloudkit-account")).unwrap(),
        "a".repeat(64)
    );
    assert!(crate::session::session_status(&target).unwrap().locked);
    stop.store(true, Ordering::Relaxed);
    worker.join().unwrap();
}

#[test]
fn independent_offline_edits_merge_without_a_whole_vault_conflict() {
    let temp = tempdir().unwrap();
    let remote = MemoryRemote::default();
    let a = first(&temp.path().join("a/vault"));
    run(&a, &remote, "test").unwrap();
    let b = restore(&temp.path().join("b/vault"), &remote);
    add(&a, "from A");
    add(&b, "from B");
    run(&a, &remote, "test").unwrap();
    assert_eq!(run(&b, &remote, "test").unwrap().status, SyncStatus::Idle);
    assert_eq!(run(&a, &remote, "test").unwrap().status, SyncStatus::Idle);
    assert_eq!(titles(&a).len(), 2);
    assert_eq!(titles(&b).len(), 2);
    assert!(conflicts(&a.vault_dir).unwrap().is_empty());
}

#[test]
fn create_rechecks_cloud_and_restores_a_vault_that_arrived_after_startup() {
    let temp = tempdir().unwrap();
    let cloud = temp.path().join("cloud");
    let a = first(&temp.path().join("a/vault"));
    add(&a, "existing");
    crate::server::run_sync_local(&a, &cloud).unwrap();
    let b = sync_test_state(temp.path().join("b/vault"));
    let settings = crate::session::PersistedSyncSettings {
        mode: aipass_agent_protocol::SyncMode::Local,
        sync_folder: Some(cloud),
        ..Default::default()
    };
    atomic_write_bytes(
        crate::session::sync_settings_path(&b.vault_dir),
        &serde_json::to_vec(&settings).unwrap(),
    )
    .unwrap();
    assert!(crate::session::create_vault(&b, "unrelated-new-password".into()).is_err());
    let status = crate::session::session_status(&b).unwrap();
    assert!(status.exists && status.locked);
    let restored = Vault::open(&b.vault_dir, &SecretString::new("master")).unwrap();
    assert_eq!(
        restored.list_provider_summaries().unwrap()[0].title,
        "existing"
    );
    assert!(Vault::open(&b.vault_dir, &SecretString::new("unrelated-new-password")).is_err());
}

#[test]
fn legacy_encrypted_records_are_migrated_before_first_snapshot_publication() {
    let temp = tempdir().unwrap();
    let cloud = temp.path().join("legacy");
    let a = first(&temp.path().join("vault"));
    add(&a, "legacy-provider");
    let object = aipass_sync::list_sync_files(&a.vault_dir)
        .unwrap()
        .into_iter()
        .find(|object| object.object_type == "provider_entry")
        .unwrap();
    fs::create_dir_all(cloud.join("objects")).unwrap();
    fs::rename(
        a.vault_dir.join(&object.relative_path),
        cloud.join(&object.relative_path),
    )
    .unwrap();
    crate::server::run_sync_local(&a, &cloud).unwrap();
    assert_eq!(titles(&a), vec!["legacy-provider"]);
    assert_eq!(
        aipass_sync::FolderSnapshotRemote(&cloud)
            .list()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn legacy_conflicts_preserve_the_remote_branch_until_the_user_accepts_it() {
    let temp = tempdir().unwrap();
    let cloud = temp.path().join("legacy");
    let a = first(&temp.path().join("a/vault"));
    add(&a, "shared");
    aipass_sync::sync_local_folder(&a.vault_dir, &cloud).unwrap();
    let object = aipass_sync::list_sync_files(&a.vault_dir)
        .unwrap()
        .into_iter()
        .find(|object| object.object_type == "provider_entry")
        .unwrap();
    let local_path = a.vault_dir.join(&object.relative_path);
    let envelope: serde_json::Value =
        serde_json::from_slice(&fs::read(&local_path).unwrap()).unwrap();
    // Different authenticated wire representations with the same Lamport
    // value exercise the legacy conflict path without changing AEAD metadata.
    let alternate = serde_json::to_vec(&envelope).unwrap();
    assert_ne!(alternate, fs::read(&local_path).unwrap());
    fs::write(&local_path, alternate).unwrap();
    let report = crate::server::run_sync_local(&a, &cloud).unwrap();
    assert_eq!(report.status, SyncStatus::Conflict);
    assert_eq!(report.conflicts, 1);
    assert_eq!(titles(&a), vec!["shared"]);
    let incoming = aipass_sync::list_conflicts(&a.vault_dir).unwrap();
    assert_eq!(incoming.len(), 1);
    let incoming = &incoming[0];
    let original_remote = fs::read(cloud.join(&incoming.target_path)).unwrap();
    assert_eq!(
        fs::read(a.vault_dir.join(&incoming.conflict_path)).unwrap(),
        original_remote
    );
    assert_eq!(
        crate::server::run_sync_local(&a, &cloud).unwrap().status,
        SyncStatus::Conflict,
        "an unresolved legacy conflict must remain visible on subsequent polls"
    );
    with_vault_mut(&a, false, |vault| {
        aipass_sync::accept_conflict_with_validator(
            &a.vault_dir,
            &incoming.conflict_path,
            &|bytes| vault.validate_sync_object_bytes(bytes).map_err(Into::into),
        )
        .map_err(ServiceError::internal)?;
        vault
            .reload_from_disk()
            .map_err(crate::session::map_vault_error)
    })
    .unwrap();
    assert_eq!(fs::read(&local_path).unwrap(), original_remote);
    assert_eq!(
        crate::server::run_sync_local(&a, &cloud).unwrap().status,
        SyncStatus::Idle
    );
    assert_eq!(
        fs::read(cloud.join(&incoming.target_path)).unwrap(),
        original_remote
    );
}

#[test]
fn legacy_migration_rolls_back_records_when_checkpoint_writing_fails() {
    let temp = tempdir().unwrap();
    let cloud = temp.path().join("legacy");
    let state = first(&temp.path().join("vault"));
    add(&state, "incoming");
    let object = aipass_sync::list_sync_files(&state.vault_dir)
        .unwrap()
        .into_iter()
        .find(|object| object.object_type == "provider_entry")
        .unwrap();
    fs::create_dir_all(cloud.join("objects")).unwrap();
    fs::rename(
        state.vault_dir.join(&object.relative_path),
        cloud.join(&object.relative_path),
    )
    .unwrap();
    fs::create_dir(state.vault_dir.join("sync-checkpoint.aipcheckpoint")).unwrap();
    assert!(crate::server::run_sync_local(&state, &cloud).is_err());
    assert!(!state.vault_dir.join(&object.relative_path).exists());
    assert!(titles(&state).is_empty());
    assert!(!state.vault_dir.join("pending-sync.aipsnapshot").exists());
    assert!(cloud.join(&object.relative_path).exists());
    assert!(aipass_sync::FolderSnapshotRemote(&cloud)
        .list()
        .unwrap()
        .is_empty());
}

#[test]
fn applied_snapshots_refresh_proxy_even_when_followup_io_fails() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;
    // Cover normal download, explicit conflict acceptance, and journal
    // recovery. Each applies new credentials before a later metadata error.
    for mode in ["download", "resolve", "recover"] {
        let temp = tempdir().unwrap();
        let remote = MemoryRemote::default();
        let a = first(&temp.path().join("a/vault"));
        let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/v1", upstream.local_addr().unwrap());
        let mut input = sync_test_provider("shared", "old-key");
        input.supports_websockets = Some(false);
        input.endpoints = vec![aipass_provider_registry::ProviderEndpoint::api(&endpoint)];
        let provider =
            with_vault(&a, false, |vault| Ok(vault.add_provider(input).unwrap())).unwrap();
        run(&a, &remote, "test").unwrap();
        let b = restore(&temp.path().join("b/vault"), &remote);
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = probe.local_addr().unwrap();
        drop(probe);
        with_vault(&b, false, |vault| {
            let secret_id = vault.get_provider_summary(provider).unwrap().secret_refs[0]
                .id
                .clone();
            let mut proxy = b.proxy.lock().unwrap();
            proxy.set_config(
                vault,
                aipass_proxy::ProxyConfig {
                    bind_addr: address.to_string(),
                    upstream_proxy: aipass_proxy::UpstreamProxyConfig {
                        mode: aipass_proxy::UpstreamProxyMode::Direct,
                        custom_url: None,
                    },
                    routes: vec![aipass_proxy::ProxyRouteConfig {
                        id: uuid::Uuid::new_v4(),
                        name: "test".into(),
                        token: "route-token".into(),
                        inbound_protocol: aipass_proxy::Protocol::OpenAiResponses,
                        upstream_protocol: aipass_proxy::Protocol::OpenAiResponses,
                        conversion_enabled: false,
                        strategy: aipass_proxy::RouteStrategy::Fallback,
                        retry: Default::default(),
                        enabled: true,
                        targets: vec![aipass_proxy::ProxyTargetConfig {
                            id: uuid::Uuid::new_v4(),
                            provider_entry_id: provider,
                            secret_id,
                            label: "test".into(),
                            base_url: endpoint.clone(),
                            auth_scheme: "bearer".into(),
                            headers: vec![],
                            group: None,
                            priority: 0,
                            weight: 1,
                            enabled: true,
                            protocol: None,
                            prefer_ws: false,
                            model: None,
                        }],
                    }],
                    ..Default::default()
                },
            )?;
            proxy.start(vault)?;
            Ok(())
        })
        .unwrap();
        with_vault(&a, false, |vault| {
            let mut update = sync_test_provider("changed", "new-key");
            update.supports_websockets = Some(false);
            update.endpoints = vec![aipass_provider_registry::ProviderEndpoint::api(&endpoint)];
            vault
                .update_provider(
                    provider,
                    serde_json::from_value(serde_json::to_value(update).unwrap()).unwrap(),
                )
                .unwrap();
            Ok(())
        })
        .unwrap();
        with_vault_mut(&a, false, |vault| {
            vault.advance_epoch_and_rewrap("fault-test").unwrap();
            Ok(())
        })
        .unwrap();
        run(&a, &remote, "test").unwrap();
        let head: Checkpoint =
            serde_json::from_slice(&fs::read(checkpoint_path(&a.vault_dir, "test")).unwrap())
                .unwrap();
        let id = &head.heads[0];
        let incoming = remote.get(id).unwrap();
        let conflict = b
            .vault_dir
            .join("sync-state")
            .join(format!("{}-conflicts.json", snapshot_id(b"test")));
        if mode == "resolve" {
            atomic_write_bytes(cache_path(&b.vault_dir, id).unwrap(), &incoming).unwrap();
            atomic_write_bytes(&conflict, &serde_json::to_vec(&vec![id]).unwrap()).unwrap();
            let checkpoint = checkpoint_path(&b.vault_dir, "test");
            fs::remove_file(&checkpoint).unwrap();
            fs::create_dir(checkpoint).unwrap();
        } else {
            fs::remove_file(&conflict).unwrap();
            fs::create_dir(&conflict).unwrap();
            if mode == "recover" {
                atomic_write_bytes(b.vault_dir.join("pending-sync.aipsnapshot"), &incoming)
                    .unwrap();
                remote.fail_put.store(true, Ordering::Relaxed);
            }
        }
        let activity = match &*b.session.lock().unwrap() {
            SessionState::Unlocked(info) => info.last_activity_at,
            _ => panic!("unlocked"),
        };
        let revision = b.sync_revision.load(Ordering::Relaxed);
        let result = if mode == "resolve" {
            resolve(&b, id, true)
        } else {
            run(&b, &remote, "test").map(|_| ())
        };
        assert!(result.is_err(), "{mode}");
        assert!(
            b.sync_revision.load(Ordering::Relaxed) > revision,
            "{mode}: {result:?}"
        );
        assert!(
            matches!(&*b.session.lock().unwrap(), SessionState::Unlocked(info) if info.last_activity_at == activity)
        );
        assert!(b.proxy.lock().unwrap().status().running);
        upstream.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                let mut stream = loop {
                    match upstream.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(std::time::Instant::now() < deadline);
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                // TCP reads can split the headers and body. Consume the whole
                // request before closing so unread bytes cannot reset the reply.
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    assert!(request.len() < 8192);
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
                // Local development listener discovery can send GET / before
                // this fixture's API request. Ignore only unauthenticated root
                // probes; keep the POST, body and rotated-key assertions.
                if request.starts_with("get / http/1.1\r\n")
                    && !request.contains("\r\nauthorization:")
                {
                    write!(
                        stream,
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                    .unwrap();
                    assert!(std::time::Instant::now() < deadline);
                    continue;
                }
                assert!(
                    request.starts_with("post /v1/responses "),
                    "unexpected upstream request: {:?}",
                    request.lines().next()
                );
                let length: usize = request
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length: "))
                    .unwrap()
                    .parse()
                    .unwrap();
                assert_eq!(length, 2);
                let mut input = [0; 2];
                stream.read_exact(&mut input).unwrap();
                assert_eq!(&input, b"{}");
                let body = r#"{"id":"ok","status":"completed","output":[]}"#;
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
                break request;
            }
        });
        let response = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(4))
            .build()
            .unwrap()
            .post(format!("http://{address}/v1/responses"))
            .bearer_auth("route-token")
            .body("{}")
            .send()
            .unwrap();
        let status = response.status();
        assert!(
            status.is_success(),
            "{mode}: {status}: {}",
            response.text().unwrap()
        );
        assert!(
            server
                .join()
                .unwrap()
                .contains("authorization: bearer new-key"),
            "{mode}"
        );
        if mode == "resolve" {
            fs::remove_dir(checkpoint_path(&b.vault_dir, "test")).unwrap();
        } else {
            fs::remove_dir(&conflict).unwrap();
        }
        remote.fail_put.store(false, Ordering::Relaxed);
        assert_eq!(
            run(&b, &remote, "test").unwrap().status,
            SyncStatus::Idle,
            "{mode}"
        );
        let published = remote.list().unwrap().len();
        for _ in 0..3 {
            run(&b, &remote, "test").unwrap();
        }
        assert_eq!(
            remote.list().unwrap().len(),
            published,
            "sync refresh must not publish another edit"
        );
        b.proxy.lock().unwrap().stop().unwrap();
    }
}

#[test]
fn fresh_device_restores_then_receives_password_epoch_and_record_changes() {
    let temp = tempdir().unwrap();
    let remote = MemoryRemote::default();
    let a = first(&temp.path().join("a/vault"));
    add(&a, "private-title");
    assert_eq!(run(&a, &remote, "test").unwrap().uploaded, 1);
    let b = restore(&temp.path().join("b/vault"), &remote);
    assert_eq!(titles(&b), vec!["private-title"]);
    add(&a, "second");
    with_vault_mut(&a, false, |vault| {
        vault.advance_epoch_and_rewrap("test").unwrap();
        vault
            .change_master_password(&SecretString::new("new-master"))
            .unwrap();
        Ok(())
    })
    .unwrap();
    run(&a, &remote, "test").unwrap();
    assert_eq!(run(&b, &remote, "test").unwrap().downloaded, 1);
    assert!(titles(&b).contains(&"second".to_string()));
    assert!(Vault::open(&b.vault_dir, &SecretString::new("new-master")).is_ok());
    for bytes in remote.files.lock().unwrap().values() {
        for forbidden in [
            "fake-sync-secret",
            "private-title",
            "new-master",
            "127.0.0.1",
        ] {
            assert!(!String::from_utf8_lossy(bytes).contains(forbidden));
        }
    }
}

#[test]
fn concurrent_writers_preserve_both_versions_until_explicit_resolution() {
    let temp = tempdir().unwrap();
    let remote = MemoryRemote::default();
    let a = first(&temp.path().join("a/vault"));
    add(&a, "shared");
    run(&a, &remote, "test").unwrap();
    let b = restore(&temp.path().join("b/vault"), &remote);
    for (state, title) in [(&a, "from A"), (&b, "from B")] {
        with_vault(state, false, |vault| {
            let id = vault.list_provider_summaries().unwrap()[0].id;
            let update = serde_json::from_value(
                serde_json::to_value(sync_test_provider(title, "fake-sync-secret")).unwrap(),
            )
            .unwrap();
            vault.update_provider(id, update).unwrap();
            Ok(())
        })
        .unwrap();
    }
    run(&a, &remote, "test").unwrap();
    assert_eq!(
        run(&b, &remote, "test").unwrap().status,
        SyncStatus::Conflict
    );
    assert_eq!(
        run(&a, &remote, "test").unwrap().status,
        SyncStatus::Conflict
    );
    assert_eq!(titles(&a), vec!["from A"]);
    assert_eq!(titles(&b), vec!["from B"]);
    let versions = conflicts(&a.vault_dir).unwrap();
    assert_eq!(versions.len(), 2);
    resolve(&a, &versions[0], false).unwrap();
    assert_eq!(run(&a, &remote, "test").unwrap().status, SyncStatus::Idle);
    assert_eq!(run(&b, &remote, "test").unwrap().downloaded, 1);
    assert_eq!(titles(&b), vec!["from A"]);
    assert_eq!(
        remote.files.lock().unwrap().len(),
        4,
        "both branches remain recoverable"
    );
}

#[test]
fn failed_upload_is_retried_from_durable_outbox_and_stale_listing_cannot_rollback() {
    let temp = tempdir().unwrap();
    let remote = MemoryRemote::default();
    let state = first(&temp.path().join("vault"));
    remote.fail_put.store(true, Ordering::Relaxed);
    assert!(run(&state, &remote, "test").is_err());
    remote.fail_put.store(false, Ordering::Relaxed);
    run(&state, &remote, "test").unwrap();
    assert_eq!(remote.files.lock().unwrap().len(), 1);
    let base = remote.files.lock().unwrap().clone();
    add(&state, "retained");
    run(&state, &remote, "test").unwrap();
    *remote.files.lock().unwrap() = base;
    assert_eq!(run(&state, &remote, "test").unwrap().downloaded, 0);
    assert_eq!(titles(&state), vec!["retained"]);
}

#[test]
fn configured_sync_reads_target_only_after_acquiring_the_sync_lock() {
    let temp = tempdir().unwrap();
    let state = first(&temp.path().join("vault"));
    let settings_path = crate::session::sync_settings_path(&state.vault_dir);
    atomic_write_bytes(&settings_path, b"invalid old settings").unwrap();
    let guard = state.sync_lock.lock().unwrap();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let worker_state = state.clone();
    let worker = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        done_tx.send(run_configured(&worker_state)).unwrap();
    });
    started_rx.recv().unwrap();
    assert!(matches!(
        done_rx.recv_timeout(std::time::Duration::from_millis(100)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));
    let remote = temp.path().join("new-target");
    let settings = crate::session::PersistedSyncSettings {
        sync_folder: Some(remote.clone()),
        ..Default::default()
    };
    atomic_write_bytes(&settings_path, &serde_json::to_vec(&settings).unwrap()).unwrap();
    drop(guard);
    assert_eq!(
        done_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap()
            .unwrap()
            .uploaded,
        1
    );
    worker.join().unwrap();
    assert_eq!(
        aipass_sync::FolderSnapshotRemote(&remote)
            .list()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn import_settings_journal_prevents_reusing_the_previous_vault_remote_after_io_failure() {
    let temp = tempdir().unwrap();
    let state = first(&temp.path().join("vault"));
    add(&state, "old-vault");
    let incoming = first(&temp.path().join("incoming/vault"));
    add(&incoming, "imported-vault");
    let old_settings = crate::session::PersistedSyncSettings {
        sync_folder: Some(temp.path().join("previous-remote")),
        ..Default::default()
    };
    let path = crate::session::sync_settings_path(&state.vault_dir);
    // Fail only the final sibling settings write, after the vault rename.
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(import_file(&state, &incoming.vault_dir, "master".into()).is_err());
    assert!(crate::session::session_status(&state).unwrap().locked);
    assert!(state
        .vault_dir
        .join("pending-import-settings.json")
        .exists());
    assert!(crate::session::load_sync_settings(&state.vault_dir)
        .unwrap()
        .sync_folder
        .is_none());
    let imported = Vault::open(&state.vault_dir, &SecretString::new("master")).unwrap();
    assert_eq!(
        imported.list_provider_summaries().unwrap()[0].title,
        "imported-vault"
    );
    fs::remove_dir(&path).unwrap();
    atomic_write_bytes(&path, &serde_json::to_vec(&old_settings).unwrap()).unwrap();
    let restarted = sync_test_state(state.vault_dir.clone());
    assert!(crate::session::load_sync_settings(&restarted.vault_dir)
        .unwrap()
        .sync_folder
        .is_none());
    crate::session::save_sync_settings(
        &state.vault_dir,
        &imported,
        &crate::session::StoredSyncSettings::default(),
    )
    .unwrap();
    assert!(!state
        .vault_dir
        .join("pending-import-settings.json")
        .exists());
    assert!(crate::session::load_sync_settings(&state.vault_dir)
        .unwrap()
        .sync_folder
        .is_none());
}

#[test]
fn create_rejects_legacy_records_without_recoverable_vault_keys() {
    let temp = tempdir().unwrap();
    let old = first(&temp.path().join("old/vault"));
    add(&old, "legacy-provider");
    let object = aipass_sync::list_sync_files(&old.vault_dir)
        .unwrap()
        .into_iter()
        .find(|object| object.object_type == "provider_entry")
        .unwrap();
    let remote = temp.path().join("remote");
    atomic_write_bytes(
        remote.join(&object.relative_path),
        &fs::read(old.vault_dir.join(&object.relative_path)).unwrap(),
    )
    .unwrap();
    let state = sync_test_state(temp.path().join("new/vault"));
    atomic_write_bytes(
        crate::session::sync_settings_path(&state.vault_dir),
        &serde_json::to_vec(&crate::session::PersistedSyncSettings {
            sync_folder: Some(remote.clone()),
            ..Default::default()
        })
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        bootstrap_before_create(&state).unwrap_err().code,
        aipass_agent_protocol::AgentErrorCode::Conflict
    );
    assert!(!state.vault_dir.join("manifest.aipmanifest").exists());
    assert!(aipass_sync::FolderSnapshotRemote(&remote)
        .list()
        .unwrap()
        .is_empty());
}

#[test]
fn corrupted_remote_and_wrong_import_password_leave_existing_vault_untouched() {
    let temp = tempdir().unwrap();
    let remote = MemoryRemote::default();
    let state = first(&temp.path().join("vault"));
    add(&state, "retained");
    run(&state, &remote, "test").unwrap();
    let bytes = remote
        .files
        .lock()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .clone();
    let mut snapshot = VaultSyncSnapshot::parse(&bytes).unwrap();
    snapshot.parents.push("a".repeat(64));
    remote
        .publish(&serde_json::to_vec(&snapshot).unwrap())
        .unwrap();
    assert_eq!(run(&state, &remote, "test").unwrap().quarantined, 1);
    assert_eq!(titles(&state), vec!["retained"]);
    let input = temp.path().join("backup.aipsnapshot");
    fs::write(&input, bytes).unwrap();
    assert!(import_file(&state, &input, "wrong".into()).is_err());
    assert_eq!(titles(&state), vec!["retained"]);
    assert!(!state.vault_dir.join("pending-sync.aipsnapshot").exists());
}

#[test]
fn local_writes_publish_automatically_without_manual_sync() {
    let temp = tempdir().unwrap();
    let state = first(&temp.path().join("vault"));
    let dir = temp.path().join("cloud");
    let settings = crate::session::StoredSyncSettings {
        mode: aipass_agent_protocol::SyncMode::Local,
        sync_folder: Some(dir.clone()),
        ..Default::default()
    };
    with_vault(&state, true, |vault| {
        crate::session::save_sync_settings(&state.vault_dir, vault, &settings)
            .map_err(ServiceError::internal)
    })
    .unwrap();
    crate::sync_watch::restart_sync_watcher(&state, &settings);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    let remote = aipass_sync::FolderSnapshotRemote(&dir);
    while remote.list().unwrap().is_empty() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    add(&state, "automatic");
    while remote.list().unwrap().len() < 2 {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    state.shutdown.store(true, Ordering::Relaxed);
    state.sync_watcher.lock().unwrap().take();
}
