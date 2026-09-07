use super::*;
use crate::server::tests::{sync_test_provider, sync_test_state};
use crate::session::{
    load_sync_settings, save_sync_settings, set_session_vault, sync_settings_path,
    PersistedSyncSettings,
};
use aipass_agent_protocol::{
    CloudKitCommand, CloudKitCompletion, CloudKitErrorKind, CloudKitReply,
};
use aipass_crypto::SecretString;
use base64::{engine::general_purpose::STANDARD, Engine};
use std::sync::{
    atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
    Mutex,
};

struct Cloud {
    files: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    fault: Arc<AtomicU8>,
    calls: Arc<AtomicUsize>,
    writes: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Cloud {
    fn new(state: &Arc<AgentState>) -> Self {
        let files = Arc::new(Mutex::new(BTreeMap::<String, Vec<u8>>::new()));
        let fault = Arc::new(AtomicU8::new(0));
        let calls = Arc::new(AtomicUsize::new(0));
        let writes = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (store, failure, requests, puts, done, state) = (
            files.clone(),
            fault.clone(),
            calls.clone(),
            writes.clone(),
            stop.clone(),
            state.clone(),
        );
        let worker = std::thread::spawn(move || {
            let mut completion = None;
            while !done.load(Ordering::Relaxed) {
                let Some(task) = state.cloudkit.exchange(completion.take()).unwrap() else {
                    continue;
                };
                requests.fetch_add(1, Ordering::Relaxed);
                let mut reply = CloudKitReply {
                    account: Some("a".repeat(64)),
                    ..Default::default()
                };
                match task.command {
                    CloudKitCommand::List => {
                        reply.ids = store.lock().unwrap().keys().cloned().collect()
                    }
                    CloudKitCommand::Put { id, bytes_b64 } => {
                        if failure.load(Ordering::Relaxed) == 1 {
                            reply.error = Some("upload unavailable".into());
                            reply.error_kind = Some(CloudKitErrorKind::Unavailable);
                        } else {
                            let bytes = STANDARD.decode(bytes_b64).unwrap();
                            assert_eq!(snapshot_id(&bytes), id);
                            store.lock().unwrap().insert(id, bytes);
                            puts.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    CloudKitCommand::Get { id } => {
                        if failure.load(Ordering::Relaxed) == 2 {
                            reply.error = Some("read-back unavailable".into());
                            reply.error_kind = Some(CloudKitErrorKind::Unavailable);
                        } else {
                            // An ordinary edit can finish while network IO is in progress.
                            if failure
                                .compare_exchange(3, 0, Ordering::Relaxed, Ordering::Relaxed)
                                .is_ok()
                            {
                                add(&state, "edited during read-back");
                            }
                            reply.bytes_b64 = Some(STANDARD.encode(&store.lock().unwrap()[&id]));
                        }
                    }
                }
                completion = Some(CloudKitCompletion { id: task.id, reply });
            }
        });
        Self {
            files,
            fault,
            calls,
            writes,
            stop,
            worker: Some(worker),
        }
    }

    fn insert(&self, bytes: Vec<u8>) {
        self.files
            .lock()
            .unwrap()
            .insert(snapshot_id(&bytes), bytes);
    }
}

impl Drop for Cloud {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.worker.take().unwrap().join().unwrap();
    }
}

fn fixture() -> (tempfile::TempDir, Arc<AgentState>, StoredSyncSettings) {
    let temp = tempfile::tempdir().unwrap();
    let state = sync_test_state(temp.path().join("vault"));
    // Exercise the explicit migration call without background scheduler races.
    state.shutdown.store(true, Ordering::Relaxed);
    set_session_vault(
        &state,
        Vault::create(&state.vault_dir, &SecretString::new("master"))
            .unwrap()
            .vault,
    );
    add(&state, "migration provider");
    let settings = StoredSyncSettings {
        cloudkit_migration_pending: true,
        ..Default::default()
    };
    with_vault(&state, false, |vault| {
        save_sync_settings(&state.vault_dir, vault, &settings).map_err(ServiceError::internal)
    })
    .unwrap();
    (temp, state, settings)
}

fn add(state: &Arc<AgentState>, title: &str) {
    with_vault(state, false, |vault| {
        vault
            .add_provider(sync_test_provider(title, "migration-secret-fixture"))
            .map_err(crate::session::map_vault_error)
    })
    .unwrap();
}

fn snapshot(state: &Arc<AgentState>) -> Vec<u8> {
    with_vault(state, false, |vault| {
        vault
            .export_sync_snapshot()
            .map_err(crate::session::map_vault_error)
    })
    .unwrap()
}

fn persisted(state: &AgentState) -> PersistedSyncSettings {
    serde_json::from_slice(&fs::read(sync_settings_path(&state.vault_dir)).unwrap()).unwrap()
}

#[test]
fn local_vault_migrates_after_read_back_and_keeps_an_encrypted_backup() {
    let (_temp, state, settings) = fixture();
    let cloud = Cloud::new(&state);
    let before = with_vault(&state, false, |vault| {
        vault
            .sync_revision()
            .map_err(crate::session::map_vault_error)
    })
    .unwrap();
    let report = run(&state, &settings).unwrap();
    assert_eq!(report.status, SyncStatus::Idle);
    assert!(persisted(&state).cloudkit_migration_complete);
    assert_eq!(
        load_sync_settings(&state.vault_dir).unwrap().mode,
        SyncMode::ICloud
    );
    assert_eq!(cloud.writes.load(Ordering::Relaxed), 1);
    let backup = fs::read(
        state
            .vault_dir
            .join("sync-state/cloudkit-migration/source.aipsnapshot"),
    )
    .unwrap();
    with_vault(&state, false, |vault| {
        assert_eq!(vault.sync_snapshot_revision(&backup).unwrap(), before);
        for bytes in cloud.files.lock().unwrap().values() {
            assert_eq!(vault.sync_snapshot_revision(bytes).unwrap(), before);
            assert!(!String::from_utf8_lossy(bytes).contains("migration-secret-fixture"));
        }
        Ok(())
    })
    .unwrap();
    assert!(!String::from_utf8_lossy(&backup).contains("migration-secret-fixture"));
    assert!(matches!(
        *state.session.lock().unwrap(),
        SessionState::Unlocked(_)
    ));
}

#[test]
fn upload_and_read_back_failures_keep_the_source_and_retry_after_reopen() {
    for fault in [1, 2] {
        let (_temp, state, settings) = fixture();
        let cloud = Cloud::new(&state);
        cloud.fault.store(fault, Ordering::Relaxed);
        assert!(run(&state, &settings).is_err());
        assert!(!persisted(&state).cloudkit_migration_complete);
        assert_eq!(persisted(&state).mode, SyncMode::Local);
        let path = state
            .vault_dir
            .join("sync-state/cloudkit-migration/source.aipsnapshot");
        let backup = fs::read(&path).unwrap();
        *state.session.lock().unwrap() = SessionState::Locked;
        set_session_vault(
            &state,
            Vault::open(&state.vault_dir, &SecretString::new("master")).unwrap(),
        );
        cloud.fault.store(0, Ordering::Relaxed);
        assert_eq!(run(&state, &settings).unwrap().status, SyncStatus::Idle);
        assert!(persisted(&state).cloudkit_migration_complete);
        assert_eq!(fs::read(path).unwrap(), backup);
        assert_eq!(cloud.files.lock().unwrap().len(), 1);
    }
}

#[test]
fn edits_during_read_back_are_uploaded_before_switching_and_lock_never_uploads() {
    let (_temp, state, settings) = fixture();
    let cloud = Cloud::new(&state);
    *state.session.lock().unwrap() = SessionState::Locked;
    assert_eq!(run(&state, &settings).unwrap().status, SyncStatus::Idle);
    assert_eq!(cloud.calls.load(Ordering::Relaxed), 0);
    set_session_vault(
        &state,
        Vault::open(&state.vault_dir, &SecretString::new("master")).unwrap(),
    );
    cloud.fault.store(3, Ordering::Relaxed);
    assert!(run(&state, &settings).is_err());
    assert!(!persisted(&state).cloudkit_migration_complete);
    assert_eq!(run(&state, &settings).unwrap().status, SyncStatus::Idle);
    assert!(persisted(&state).cloudkit_migration_complete);
    with_vault(&state, false, |vault| {
        assert_eq!(vault.list_provider_summaries().unwrap().len(), 2);
        let checkpoint: Checkpoint = serde_json::from_slice(
            &fs::read(checkpoint_path(
                &state.vault_dir,
                &format!("cloudkit:{}", "a".repeat(64)),
            ))
            .unwrap(),
        )
        .unwrap();
        let bytes = &cloud.files.lock().unwrap()[&checkpoint.heads[0]];
        assert_eq!(
            vault.sync_snapshot_revision(bytes).unwrap(),
            vault.sync_revision().unwrap()
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn a_foreign_cloud_vault_is_rejected_before_any_upload() {
    let (_temp, state, settings) = fixture();
    let (_other_temp, other, _) = fixture();
    let cloud = Cloud::new(&state);
    cloud.insert(snapshot(&other));
    let before = cloud.files.lock().unwrap().clone();
    assert_eq!(
        run(&state, &settings).unwrap_err().code,
        AgentErrorCode::Conflict
    );
    assert_eq!(cloud.writes.load(Ordering::Relaxed), 0);
    assert_eq!(*cloud.files.lock().unwrap(), before);
    assert!(!persisted(&state).cloudkit_migration_complete);
}

#[test]
fn drive_updates_migrate_even_when_cloudkit_already_contains_the_original_vault() {
    let (temp, state, mut settings) = fixture();
    let drive = temp.path().join("drive");
    let source = aipass_sync::FolderSnapshotRemote(&drive);
    let target = format!("folder:{}", drive.display());
    run_inner(&state, &source, &target).unwrap();
    let baseline = source.get(&source.list().unwrap()[0]).unwrap();
    let other = sync_test_state(temp.path().join("other/vault"));
    other.shutdown.store(true, Ordering::Relaxed);
    VaultSyncSnapshot::parse(&baseline)
        .unwrap()
        .bootstrap(&other.vault_dir)
        .unwrap();
    set_session_vault(
        &other,
        Vault::open(&other.vault_dir, &SecretString::new("master")).unwrap(),
    );
    run_inner(&other, &source, &target).unwrap();
    add(&other, "late Drive edit");
    run_inner(&other, &source, &target).unwrap();
    let old_files = source.list().unwrap();
    let cloud = Cloud::new(&state);
    cloud.insert(baseline);
    settings.mode = SyncMode::ICloud;
    with_vault(&state, false, |vault| {
        save_sync_settings(&state.vault_dir, vault, &settings).map_err(ServiceError::internal)
    })
    .unwrap();
    let report = run_from_source(&state, &settings, Some(&drive)).unwrap();
    assert_eq!(report.status, SyncStatus::Idle);
    assert!(persisted(&state).cloudkit_migration_complete);
    assert!(old_files.iter().all(|id| source.get(id).is_ok()));
    with_vault(&state, false, |vault| {
        assert!(vault
            .list_provider_summaries()
            .unwrap()
            .iter()
            .any(|entry| entry.title == "late Drive edit"));
        Ok(())
    })
    .unwrap();
}

#[test]
fn folder_migration_retains_source_on_settings_failure_and_retries() {
    let (temp, state, mut settings) = fixture();
    settings.sync_folder = Some(temp.path().join("folder"));
    with_vault(&state, false, |vault| {
        save_sync_settings(&state.vault_dir, vault, &settings).map_err(ServiceError::internal)
    })
    .unwrap();
    preserve_source(&state, &settings).unwrap();
    let source_report = sync_source(&state, &settings, None).unwrap();
    let cloud = Cloud::new(&state);
    let remote = crate::cloudkit::Remote::connect(&state.cloudkit).unwrap();
    let settings_path = sync_settings_path(&state.vault_dir);
    let before = fs::read(&settings_path).unwrap();
    fs::remove_file(&settings_path).unwrap();
    fs::create_dir(&settings_path).unwrap();
    assert!(finish(&state, &settings, &remote, &remote.account, source_report).is_err());
    assert_eq!(cloud.files.lock().unwrap().len(), 1);
    fs::remove_dir(&settings_path).unwrap();
    fs::write(&settings_path, before).unwrap();
    assert!(!persisted(&state).cloudkit_migration_complete);
    assert_eq!(run(&state, &settings).unwrap().status, SyncStatus::Idle);
    assert!(persisted(&state).cloudkit_migration_complete);
    let source = aipass_sync::FolderSnapshotRemote(settings.sync_folder.as_ref().unwrap());
    assert_eq!(source.list().unwrap().len(), 1);
    assert_eq!(cloud.files.lock().unwrap().len(), 1);
}

#[test]
fn divergent_cloud_changes_stay_recoverable_until_resolved() {
    let (temp, state, settings) = fixture();
    let other = sync_test_state(temp.path().join("other/vault"));
    other.shutdown.store(true, Ordering::Relaxed);
    VaultSyncSnapshot::parse(&snapshot(&state))
        .unwrap()
        .bootstrap(&other.vault_dir)
        .unwrap();
    set_session_vault(
        &other,
        Vault::open(&other.vault_dir, &SecretString::new("master")).unwrap(),
    );
    for (vault_state, title) in [(&state, "local edit"), (&other, "cloud edit")] {
        with_vault(vault_state, false, |vault| {
            let id = vault.list_provider_summaries().unwrap()[0].id;
            let update = serde_json::from_value(
                serde_json::to_value(sync_test_provider(title, "fixture")).unwrap(),
            )
            .unwrap();
            vault.update_provider(id, update).unwrap();
            Ok(())
        })
        .unwrap();
    }
    let cloud = Cloud::new(&state);
    let remote_version = snapshot(&other);
    cloud.insert(remote_version.clone());
    assert_eq!(run(&state, &settings).unwrap().status, SyncStatus::Conflict);
    assert!(!persisted(&state).cloudkit_migration_complete);
    assert_eq!(cloud.files.lock().unwrap().len(), 2);
    let versions = conflicts(&state.vault_dir).unwrap();
    assert_eq!(versions.len(), 2);
    resolve(&state, &snapshot_id(&remote_version), false).unwrap();
    assert_eq!(run(&state, &settings).unwrap().status, SyncStatus::Idle);
    assert!(persisted(&state).cloudkit_migration_complete);
    with_vault(&state, false, |vault| {
        assert_eq!(
            vault.list_provider_summaries().unwrap()[0].title,
            "local edit"
        );
        Ok(())
    })
    .unwrap();
    assert!(cloud
        .files
        .lock()
        .unwrap()
        .contains_key(&snapshot_id(&remote_version)));
}

#[test]
fn source_settings_backup_keeps_webdav_credentials_encrypted() {
    let (_temp, state, mut settings) = fixture();
    settings.mode = SyncMode::WebDav;
    settings.webdav_url = Some("https://dav.example".into());
    settings.webdav_password = Some(crate::session::StoredSyncSecret::Plaintext(
        aipass_agent_protocol::SensitiveString::from("old-dav-password"),
    ));
    preserve_source(&state, &settings).unwrap();
    let bytes = fs::read(
        state
            .vault_dir
            .join("sync-state/cloudkit-migration/source-settings.json"),
    )
    .unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("old-dav-password"));
    let backup: PersistedSyncSettings = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(backup.mode, SyncMode::WebDav);
    let encrypted = StoredSyncSettings {
        webdav_password: backup
            .webdav_password
            .map(crate::session::StoredSyncSecret::Encrypted),
        ..settings
    };
    with_vault(&state, false, |vault| {
        assert_eq!(
            crate::session::sync_settings_password(&encrypted, vault)
                .unwrap()
                .unwrap()
                .expose(),
            "old-dav-password"
        );
        Ok(())
    })
    .unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn configured_sync_automatically_upgrades_old_local_settings() {
    let (_temp, state, _) = fixture();
    fs::write(sync_settings_path(&state.vault_dir), br#"{"mode":"local"}"#).unwrap();
    let cloud = Cloud::new(&state);
    run_configured(&state).unwrap();
    assert!(persisted(&state).cloudkit_migration_complete);
    assert_eq!(cloud.writes.load(Ordering::Relaxed), 1);
    add(&state, "subsequent CloudKit edit");
    run_configured(&state).unwrap();
    assert_eq!(cloud.writes.load(Ordering::Relaxed), 2);
}

#[test]
fn fresh_cloudkit_restore_is_not_mistaken_for_an_old_drive_vault() {
    let (temp, original, _) = fixture();
    let state = sync_test_state(temp.path().join("restored/vault"));
    state.shutdown.store(true, Ordering::Relaxed);
    fs::remove_file(sync_settings_path(&state.vault_dir)).unwrap();
    let cloud = Cloud::new(&state);
    cloud.insert(snapshot(&original));
    let remote = crate::cloudkit::Remote::connect(&state.cloudkit).unwrap();
    run_inner(&state, &remote, &format!("cloudkit:{}", remote.account)).unwrap();
    let settings = load_sync_settings(&state.vault_dir).unwrap();
    assert_eq!(settings.mode, SyncMode::ICloud);
    assert!(!settings.cloudkit_migration_pending);
    assert!(state.vault_dir.join("manifest.aipmanifest").exists());
}
