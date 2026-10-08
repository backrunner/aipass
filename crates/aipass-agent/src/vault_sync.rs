use crate::session::{AgentState, ServiceError, ServiceResult, SessionState};
use aipass_storage::atomic_write_bytes;
use aipass_sync::{snapshot_id, valid_snapshot_id, SnapshotRemote, SyncReport, SyncStatus};
use aipass_vault::{Vault, VaultSyncSnapshot};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::{fs, path::Path, sync::Arc};

mod cloudkit_migration;

#[derive(Default, Serialize, Deserialize)]
struct Checkpoint {
    heads: Vec<String>,
    revision: Option<String>,
}

fn checkpoint_path(root: &Path, target: &str) -> std::path::PathBuf {
    root.join("sync-state")
        .join(format!("{}.json", snapshot_id(target.as_bytes())))
}

// Caller holds the session lock. Only an established target has authenticated
// parents; first synchronization must inspect the remote before publishing.
fn queue_local_changes(root: &Path, vault: &Vault, target: &str) -> Result<()> {
    let path = checkpoint_path(root, target);
    if !path.exists() {
        return Ok(());
    }
    vault.ensure_sync_ready()?;
    let checkpoint: Checkpoint = serde_json::from_slice(&fs::read(&path)?)?;
    // A locked bootstrap has not authenticated its checkpoint yet. Conflict
    // resolution also leaves revision unset until its joined branch is built.
    if checkpoint.revision.is_none() {
        return Ok(());
    }
    let revision = vault.sync_revision()?;
    if checkpoint.revision.as_ref() == Some(&revision) {
        return Ok(());
    }
    let bytes = vault.export_sync_snapshot_with_parents(checkpoint.heads)?;
    let id = snapshot_id(&bytes);
    atomic_write_bytes(cache_path(root, &id)?, &bytes)?;
    atomic_write_bytes(
        root.join("sync-outbox")
            .join(snapshot_id(target.as_bytes()))
            .join(format!("{id}.aipsnapshot")),
        &bytes,
    )?;
    atomic_write_bytes(
        path,
        &serde_json::to_vec(&Checkpoint {
            heads: vec![id],
            revision: Some(revision),
        })?,
    )?;
    Ok(())
}

pub(crate) fn queue_configured_changes(
    state: &AgentState,
    vault: &Vault,
    settings: &crate::session::StoredSyncSettings,
) -> Result<()> {
    let target = if let Some(dir) = crate::sync_watch::folder_sync_dir(settings) {
        format!("folder:{}", dir.display())
    } else if settings.mode == aipass_agent_protocol::SyncMode::WebDav {
        let Some(url) = &settings.webdav_url else {
            return Ok(());
        };
        format!("webdav:{url}")
    } else if settings.mode == aipass_agent_protocol::SyncMode::ICloud {
        let Ok(account) = fs::read_to_string(state.vault_dir.join("sync-state/cloudkit-account"))
        else {
            return Ok(());
        };
        format!("cloudkit:{account}")
    } else {
        return Ok(());
    };
    queue_local_changes(&state.vault_dir, vault, &target)
}

pub(crate) fn cache_path(root: &Path, id: &str) -> Result<std::path::PathBuf> {
    if !valid_snapshot_id(id) {
        bail!("invalid snapshot id");
    }
    Ok(root.join("sync-cache").join(format!("{id}.aipsnapshot")))
}

pub(crate) fn run_cloudkit(state: &Arc<AgentState>) -> ServiceResult<SyncReport> {
    let operation = crate::operation_log::OperationLog::background("sync.cloudkit");
    if let Ok(mut status) = state.sync_status.lock() {
        *status = Some(SyncStatus::Syncing);
    }
    let _sync = state
        .sync_lock
        .lock()
        .map_err(|_| ServiceError::internal(anyhow::anyhow!("sync lock poisoned")))?;
    let result = (|| {
        let settings =
            crate::session::load_sync_settings(&state.vault_dir).map_err(ServiceError::internal)?;
        if settings.cloudkit_migration_pending {
            recover_local_pending(state)?;
            cloudkit_migration::run(state, &settings)
        } else {
            run_cloudkit_inner(state)
        }
    })();
    if let Ok(mut status) = state.sync_status.lock() {
        *status = Some(match &result {
            Ok(report) => report.status.clone(),
            Err(error) => match error.code {
                aipass_agent_protocol::AgentErrorCode::PermissionDenied => SyncStatus::AuthFailed,
                aipass_agent_protocol::AgentErrorCode::Conflict => SyncStatus::Conflict,
                _ => SyncStatus::Offline,
            },
        });
    }
    if let Some(operation) = operation {
        operation.finish(&match &result {
            Ok(report) => aipass_agent_protocol::AgentResponse::success(report),
            Err(error) => aipass_agent_protocol::AgentResponse::error(
                error.code.clone(),
                "CloudKit sync failed",
            ),
        });
    }
    result
}

pub(crate) fn run_configured(state: &Arc<AgentState>) -> ServiceResult<SyncReport> {
    let operation = crate::operation_log::OperationLog::background("sync.configured");
    let _sync = state
        .sync_lock
        .lock()
        .map_err(|_| ServiceError::internal(anyhow::anyhow!("sync lock poisoned")))?;
    if let Ok(mut status) = state.sync_status.lock() {
        *status = Some(SyncStatus::Syncing);
    }
    let result = run_configured_inner(state);
    if let Ok(mut status) = state.sync_status.lock() {
        *status = Some(match &result {
            Ok(report) => report.status.clone(),
            Err(err) => match err.code {
                aipass_agent_protocol::AgentErrorCode::PermissionDenied
                | aipass_agent_protocol::AgentErrorCode::Locked => SyncStatus::AuthFailed,
                aipass_agent_protocol::AgentErrorCode::Conflict => SyncStatus::Conflict,
                aipass_agent_protocol::AgentErrorCode::ServiceUnavailable => SyncStatus::Offline,
                _ => SyncStatus::ServerError,
            },
        });
    }
    if let Some(operation) = operation {
        operation.finish(&match &result {
            Ok(report) => aipass_agent_protocol::AgentResponse::success(report),
            Err(err) => {
                aipass_agent_protocol::AgentResponse::error(err.code.clone(), "sync failed")
            }
        });
    }
    result
}

fn run_configured_inner(state: &Arc<AgentState>) -> ServiceResult<SyncReport> {
    let settings =
        crate::session::load_sync_settings(&state.vault_dir).map_err(ServiceError::internal)?;
    recover_local_pending(state)?;
    if settings.cloudkit_migration_pending {
        return cloudkit_migration::run(state, &settings);
    }
    run_settings_inner(state, &settings)
}

fn run_settings_inner(
    state: &Arc<AgentState>,
    settings: &crate::session::StoredSyncSettings,
) -> ServiceResult<SyncReport> {
    if settings.mode == aipass_agent_protocol::SyncMode::ICloud {
        return run_cloudkit_inner(state);
    }
    if let Some(dir) = crate::sync_watch::folder_sync_dir(settings) {
        return run_inner(
            state,
            &aipass_sync::FolderSnapshotRemote(&dir),
            &format!("folder:{}", dir.display()),
        );
    }
    if settings.mode != aipass_agent_protocol::SyncMode::WebDav {
        return Err(ServiceError::internal(anyhow::anyhow!(
            "sync folder unavailable"
        )));
    }
    let url = settings
        .webdav_url
        .as_ref()
        .context("WebDAV URL is not configured")
        .map_err(ServiceError::internal)?;
    let password = {
        let session = state
            .session
            .lock()
            .map_err(|_| ServiceError::internal(anyhow::anyhow!("session lock poisoned")))?;
        crate::session::transport_password(
            state,
            settings,
            match &*session {
                SessionState::Locked => None,
                SessionState::Unlocked(info) => Some(&info.vault),
            },
        )?
    };
    let client = aipass_sync::HttpWebDavClient::new(
        url,
        settings.webdav_username.clone(),
        password.map(|value| value.into_inner()),
    )
    .map_err(ServiceError::internal)?;
    run_inner(
        state,
        &aipass_sync::WebDavSnapshotRemote(&client),
        &format!("webdav:{url}"),
    )
}

fn run_cloudkit_inner(state: &Arc<AgentState>) -> ServiceResult<SyncReport> {
    recover_local_pending(state)?;
    let remote = crate::cloudkit::Remote::connect(&state.cloudkit).map_err(remote_error)?;
    check_cloud_account(&state.vault_dir, &remote.account).map_err(|err| {
        ServiceError::new(
            aipass_agent_protocol::AgentErrorCode::Conflict,
            err.to_string(),
        )
    })?;
    let mut migrated = 0;
    if remote.list().map_err(remote_error)?.is_empty() {
        // Upgrade the former iCloud Drive backend without abandoning its vault.
        // Discovery remains read-only until an unlocked vault can publish.
        if let Ok(dir) =
            crate::paths::cloud_sync_dir(aipass_agent_protocol::CloudSyncProvider::ICloud)
        {
            let legacy = aipass_sync::FolderSnapshotRemote(&dir);
            if !legacy.list().map_err(remote_error)?.is_empty()
                || !legacy.legacy_objects().map_err(remote_error)?.is_empty()
            {
                let report = run_inner(state, &legacy, &format!("folder:{}", dir.display()))?;
                if report.status == SyncStatus::Conflict {
                    return Ok(report);
                }
                migrated = report.downloaded;
            }
        }
    }
    let mut report = run_inner(state, &remote, &format!("cloudkit:{}", remote.account))?;
    report.downloaded += migrated;
    Ok(report)
}

fn check_cloud_account(root: &Path, account: &str) -> Result<()> {
    let path = root.join("sync-state/cloudkit-account");
    if path.exists() {
        if fs::read_to_string(&path)? != account {
            bail!("iCloud account changed. Restore the original Apple account before syncing this vault.");
        }
        return Ok(());
    }
    atomic_write_bytes(path, account.as_bytes())?;
    Ok(())
}

pub(crate) fn run(
    state: &Arc<AgentState>,
    remote: &impl SnapshotRemote,
    target: &str,
) -> ServiceResult<SyncReport> {
    let _sync = state
        .sync_lock
        .lock()
        .map_err(|_| ServiceError::internal(anyhow::anyhow!("sync lock poisoned")))?;
    recover_local_pending(state)?;
    run_inner(state, remote, target)
}

fn recover_local_pending(state: &AgentState) -> ServiceResult<()> {
    // Recovery is a local disk operation. A failed network or Apple account
    // request must not postpone making an already-unlocked vault usable again.
    let mut session = state
        .session
        .lock()
        .map_err(|_| ServiceError::internal(anyhow::anyhow!("session lock poisoned")))?;
    if let SessionState::Unlocked(info) = &mut *session {
        if state.vault_dir.join("pending-sync.aipsnapshot").exists() {
            info.vault
                .recover_pending_sync()
                .map_err(crate::session::map_vault_error)?;
            state
                .sync_revision
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            refresh_applied_vault(state, &info.vault)?;
        }
    }
    Ok(())
}

fn run_inner(
    state: &Arc<AgentState>,
    remote: &impl SnapshotRemote,
    target: &str,
) -> ServiceResult<SyncReport> {
    let outbox = state
        .vault_dir
        .join("sync-outbox")
        .join(snapshot_id(target.as_bytes()));
    {
        let session = state
            .session
            .lock()
            .map_err(|_| ServiceError::internal(anyhow::anyhow!("session lock poisoned")))?;
        if let SessionState::Unlocked(info) = &*session {
            if !state.vault_dir.join("pending-sync.aipsnapshot").exists() {
                queue_local_changes(&state.vault_dir, &info.vault, target)
                    .map_err(ServiceError::internal)?;
            }
        }
    }
    let uploaded = flush_outbox(&outbox, remote).map_err(remote_error)?;
    let mut downloaded = Vec::new();
    // Network and cloud hydration never hold the session lock. Lock/unlock,
    // status and ordinary local writes remain available while the remote stalls.
    for id in remote.list().map_err(remote_error)? {
        let cache = cache_path(&state.vault_dir, &id).map_err(ServiceError::internal)?;
        if !cache.exists() {
            let bytes = remote.get(&id).map_err(remote_error)?;
            atomic_write_bytes(&cache, &bytes).map_err(ServiceError::internal)?;
        }
        downloaded.push(id);
    }
    let legacy = if downloaded.is_empty() && !checkpoint_path(&state.vault_dir, target).exists() {
        remote.legacy_objects().map_err(remote_error)?
    } else {
        Vec::new()
    };
    let staged = StagedRemote {
        downloaded,
        root: &state.vault_dir,
        outbox: &outbox,
    };
    let mut session = state
        .session
        .lock()
        .map_err(|_| ServiceError::internal(anyhow::anyhow!("session lock poisoned")))?;
    if !legacy.is_empty() && matches!(*session, SessionState::Locked) {
        return Ok(SyncReport { uploaded: 0, downloaded: 0, conflicts: 1, quarantined: 0, status: SyncStatus::Conflict,
            message: Some("Legacy sync contains encrypted records without recovery keys. Unlock the original vault and sync from this version, or import an encrypted backup.".into()) });
    }
    let mut migrated = 0;
    let mut applied = false;
    let result = (|| -> Result<SyncReport> {
        let mut quarantined = 0;
        if let SessionState::Unlocked(info) = &mut *session {
            let pending = state.vault_dir.join("pending-sync.aipsnapshot").exists();
            info.vault.recover_pending_sync()?;
            applied |= pending;
            (migrated, quarantined) = migrate_legacy(&state.vault_dir, &mut info.vault, legacy)?;
        }
        let mut report = synchronize(
            &state.vault_dir,
            &mut session,
            &staged,
            target,
            &mut applied,
        )?;
        report.conflicts += aipass_sync::list_conflicts(&state.vault_dir)?.len();
        report.quarantined += quarantined;
        if report.conflicts > 0 {
            report.status = SyncStatus::Conflict;
        }
        Ok(report)
    })();
    // IO failures roll back in the vault layer. If rollback also fails, vault
    // access is gated by the pending journal until recovery succeeds. Neither
    // network nor disk errors change the session or stop its running proxy.
    // Applied data remains visible even if later checkpoint/publication IO fails.
    if migrated > 0 || applied {
        state
            .sync_revision
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let SessionState::Unlocked(info) = &*session {
            refresh_applied_vault(state, &info.vault)?;
        }
    }
    drop(session);
    let mut report = result.map_err(ServiceError::internal)?;
    report.downloaded += migrated;
    report.uploaded = uploaded + flush_outbox(&outbox, remote).map_err(remote_error)?;
    Ok(report)
}

fn refresh_applied_vault(state: &AgentState, vault: &Vault) -> ServiceResult<()> {
    // A later failed apply may itself require recovery. Never read a mixed
    // on-disk vault into the running proxy while its journal still gates access.
    if vault.ensure_sync_ready().is_ok() {
        let mut proxy = state
            .proxy
            .lock()
            .map_err(|_| ServiceError::internal(anyhow::anyhow!("proxy lock poisoned")))?;
        // A credential removed on a peer device must leave its route group
        // durably, not just be skipped at runtime — a stale target would fail
        // the next cold start.
        proxy.reconcile_missing_credentials(vault)?;
        proxy.reload_if_running(vault)?;
    }
    Ok(())
}

fn remote_error(err: anyhow::Error) -> ServiceError {
    use aipass_agent_protocol::AgentErrorCode;
    if let Some(error) = err.downcast_ref::<crate::cloudkit::TransportError>() {
        use aipass_agent_protocol::CloudKitErrorKind;
        return ServiceError::new(
            match error.kind {
                CloudKitErrorKind::Unavailable => AgentErrorCode::ServiceUnavailable,
                CloudKitErrorKind::Authentication => AgentErrorCode::PermissionDenied,
                CloudKitErrorKind::AccountChanged => AgentErrorCode::Conflict,
                CloudKitErrorKind::InvalidData => AgentErrorCode::ValidationFailed,
            },
            err.to_string(),
        );
    }
    let code = match aipass_sync::classify_webdav_error(&err) {
        SyncStatus::AuthFailed => AgentErrorCode::PermissionDenied,
        SyncStatus::Offline => AgentErrorCode::ServiceUnavailable,
        SyncStatus::Conflict => AgentErrorCode::Conflict,
        _ => AgentErrorCode::Internal,
    };
    ServiceError::new(code, err.to_string())
}

fn migrate_legacy(
    root: &Path,
    vault: &mut Vault,
    objects: Vec<(std::path::PathBuf, Vec<u8>)>,
) -> Result<(usize, usize)> {
    if objects.is_empty() {
        return Ok((0, 0));
    }
    let stage = tempfile::tempdir()?;
    for (path, bytes) in objects {
        vault.validate_sync_object_bytes(&bytes)?;
        let envelope: aipass_vault::ObjectEnvelope = serde_json::from_slice(&bytes)?;
        // Legacy deletions are unsigned. Only accept one already present
        // locally; fresh deletion markers must arrive inside a signed snapshot.
        if envelope.tombstone && fs::read(root.join(&path)).ok().as_ref() != Some(&bytes) {
            bail!("Legacy deletion requires synchronization from an updated source device");
        }
        atomic_write_bytes(stage.path().join(path), &bytes)?;
    }
    let original = vault.export_sync_snapshot()?;
    // Legacy copying is not transactional. Keep an authenticated rollback
    // journal until all records and incoming conflict branches are preserved.
    atomic_write_bytes(root.join("pending-sync.aipsnapshot"), &original)?;
    let result = (|| -> Result<(usize, usize)> {
        let report = aipass_sync::sync_local_folder_with_validator(root, stage.path(), &|bytes| {
            vault.validate_sync_object_bytes(bytes).map_err(Into::into)
        })?;
        for record in aipass_sync::list_conflicts(stage.path())? {
            // The staged conflict payload is our local branch. The original
            // remote branch remains at target_path and is the user's alternative.
            let bytes = fs::read(stage.path().join(&record.target_path))?;
            aipass_sync::quarantine_sync_object(root, &record.target_path, &bytes)?;
        }
        vault.reload_from_disk()?;
        fs::remove_file(root.join("pending-sync.aipsnapshot"))?;
        Ok((report.downloaded, report.quarantined))
    })();
    if result.is_err() {
        atomic_write_bytes(root.join("pending-sync.aipsnapshot"), &original)?;
        vault
            .recover_pending_sync()
            .context("legacy migration failed and requires journal recovery")?;
    }
    result
}

struct StagedRemote<'a> {
    downloaded: Vec<String>,
    root: &'a Path,
    outbox: &'a Path,
}

impl SnapshotRemote for StagedRemote<'_> {
    fn list(&self) -> Result<Vec<String>> {
        Ok(self.downloaded.clone())
    }
    fn get(&self, id: &str) -> Result<Vec<u8>> {
        Ok(fs::read(cache_path(self.root, id)?)?)
    }
    fn publish(&self, bytes: &[u8]) -> Result<String> {
        let id = snapshot_id(bytes);
        atomic_write_bytes(self.outbox.join(format!("{id}.aipsnapshot")), bytes)?;
        Ok(id)
    }
}

fn flush_outbox(outbox: &Path, remote: &impl SnapshotRemote) -> Result<usize> {
    if !outbox.exists() {
        return Ok(0);
    }
    let mut count = 0;
    for entry in fs::read_dir(outbox)? {
        let entry = entry?;
        if entry.path().extension().and_then(|ext| ext.to_str()) != Some("aipsnapshot") {
            continue;
        }
        remote.publish(&fs::read(entry.path())?)?;
        fs::remove_file(entry.path())?;
        count += 1;
    }
    Ok(count)
}

#[derive(Serialize, Deserialize)]
struct SnapshotMetadata {
    parents: Vec<String>,
    revision: Option<String>,
}

#[derive(Debug)]
struct ForeignVault;
impl std::fmt::Display for ForeignVault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("sync target belongs to a different vault")
    }
}
impl std::error::Error for ForeignVault {}

fn snapshot_metadata(root: &Path, id: &str, session: &SessionState) -> Result<SnapshotMetadata> {
    let path = cache_path(root, id)?;
    let metadata_path = path.with_extension("aipstate");
    let purpose = format!("sync-snapshot:{id}");
    if let SessionState::Unlocked(info) = session {
        if let Ok(bytes) = fs::read(&metadata_path) {
            if let Ok(ciphertext) = serde_json::from_slice(&bytes) {
                if let Ok(bytes) = info.vault.decrypt_local_state(&purpose, &ciphertext) {
                    return Ok(serde_json::from_slice(&bytes)?);
                }
            }
        }
    }
    let bytes = fs::read(path)?;
    if snapshot_id(&bytes) != id {
        bail!("snapshot content hash mismatch");
    }
    let snapshot = VaultSyncSnapshot::parse(&bytes)?;
    if let SessionState::Unlocked(info) = session {
        if snapshot.header.vault_id != info.vault.vault_id() {
            return Err(ForeignVault.into());
        }
    }
    let revision = match session {
        SessionState::Locked => None,
        SessionState::Unlocked(info) => Some(info.vault.sync_snapshot_revision(&bytes)?),
    };
    let metadata = SnapshotMetadata {
        parents: snapshot.parents,
        revision,
    };
    if let SessionState::Unlocked(info) = session {
        let ciphertext = info
            .vault
            .encrypt_local_state(&purpose, &serde_json::to_vec(&metadata)?)?;
        atomic_write_bytes(metadata_path, &serde_json::to_vec(&ciphertext)?)?;
    }
    Ok(metadata)
}

fn synchronize(
    root: &Path,
    session: &mut SessionState,
    remote: &impl SnapshotRemote,
    target: &str,
    applied: &mut bool,
) -> Result<SyncReport> {
    let mut report = SyncReport {
        uploaded: 0,
        downloaded: 0,
        conflicts: 0,
        quarantined: 0,
        status: SyncStatus::Idle,
        message: None,
    };
    let path = checkpoint_path(root, target);
    let mut checkpoint: Checkpoint = if path.exists() {
        serde_json::from_slice(&fs::read(&path)?)?
    } else {
        Checkpoint::default()
    };
    let mut snapshots = BTreeMap::new();
    for id in remote.list()? {
        if root
            .join("sync-ignored")
            .join(format!("{id}.aipsnapshot"))
            .exists()
        {
            continue;
        }
        match snapshot_metadata(root, &id, session) {
            Ok(metadata) => {
                snapshots.insert(id, metadata);
            }
            Err(err) => {
                if err.is::<ForeignVault>() {
                    return Err(err);
                }
                atomic_write_bytes(
                    root.join("sync-quarantine")
                        .join(format!("{id}.aipsnapshot")),
                    &fs::read(cache_path(root, &id)?)?,
                )?;
                report.quarantined += 1;
            }
        }
    }
    // Cloud listings can temporarily omit an already acknowledged child.
    // Retain known heads so eventual consistency never rolls the vault back.
    for id in &checkpoint.heads {
        if !snapshots.contains_key(id) {
            snapshots.insert(id.clone(), snapshot_metadata(root, id, session)?);
        }
    }
    let mut heads = snapshots.keys().cloned().collect::<BTreeSet<_>>();
    for snapshot in snapshots.values() {
        for parent in &snapshot.parents {
            heads.remove(parent);
        }
    }
    if snapshots.is_empty() && report.quarantined > 0 {
        report.conflicts = report.quarantined;
        report.status = SyncStatus::Conflict;
        report.message = Some(
            "Unverifiable sync snapshots were quarantined; the local vault was preserved.".into(),
        );
        return Ok(report);
    }

    let SessionState::Unlocked(info) = session else {
        if report.quarantined > 0 {
            report.conflicts = report.quarantined;
            report.status = SyncStatus::Conflict;
        }
        if !root.join("manifest.aipmanifest").exists() {
            if heads.len() == 1 {
                let id = heads.first().unwrap();
                let snapshot = VaultSyncSnapshot::parse(&fs::read(cache_path(root, id)?)?)?;
                prepare_cloudkit_bootstrap(root, target)?;
                snapshot.bootstrap(root)?;
                *applied = true;
                checkpoint.heads = vec![id.clone()];
                atomic_write_bytes(&path, &serde_json::to_vec(&checkpoint)?)?;
                report.downloaded = 1;
            } else if !heads.is_empty() {
                report.conflicts = heads.len();
                report.status = SyncStatus::Conflict;
                report.message = Some("Multiple vault versions need review. Import a snapshot using its original master password.".into());
            }
        }
        return Ok(report);
    };
    let vault = &mut info.vault;
    // Authenticate the graph as well as the payload. Untrusted parent links
    // must never be allowed to hide a legitimate head or deletion conflict.
    let mut revisions = BTreeMap::new();
    for (id, metadata) in &snapshots {
        revisions.insert(
            id.clone(),
            metadata
                .revision
                .clone()
                .context("snapshot authentication missing")?,
        );
    }
    if checkpoint.revision.is_none() && checkpoint.heads.len() == 1 {
        checkpoint.revision = revisions.get(&checkpoint.heads[0]).cloned();
    }
    let local_revision = vault.sync_revision()?;
    if heads.len() == 1 {
        let id = heads.first().unwrap();
        if revisions[id] == local_revision {
            checkpoint = Checkpoint {
                heads: vec![id.clone()],
                revision: Some(local_revision.clone()),
            };
        } else if checkpoint.revision.as_ref() == Some(&local_revision)
            && checkpoint.heads != vec![id.clone()]
        {
            vault.apply_sync_snapshot(&fs::read(cache_path(root, id)?)?)?;
            *applied = true;
            checkpoint = Checkpoint {
                heads: vec![id.clone()],
                revision: Some(revisions[id].clone()),
            };
            report.downloaded += 1;
        }
    }
    // A local edit made while another device was offline need not become a
    // whole-vault conflict when the two edits affect different records.
    if checkpoint.heads.len() == 1 {
        let local_head = checkpoint.heads[0].clone();
        for incoming in heads.clone() {
            if incoming == local_head || checkpoint.heads.contains(&incoming) {
                continue;
            }
            // A completed apply followed by checkpoint IO failure can leave
            // two immutable heads with the same authenticated content. Join
            // them even across an epoch change; this is not a data conflict.
            if revisions.get(&incoming) == Some(&vault.sync_revision()?) {
                checkpoint.heads.push(incoming);
                checkpoint.revision = None;
                continue;
            }
            let Some(base) = common_ancestor(&local_head, &incoming, &snapshots) else {
                continue;
            };
            let remote_bytes = fs::read(cache_path(root, &incoming)?)?;
            let base_bytes = fs::read(cache_path(root, &base)?)?;
            if let Some(merged) = vault.merge_sync_snapshot(&remote_bytes, &base_bytes)? {
                vault.apply_sync_snapshot(&merged)?;
                *applied = true;
                checkpoint.heads.push(incoming);
                checkpoint.revision = None;
                report.downloaded += 1;
            }
        }
    }
    let current_revision = vault.sync_revision()?;
    let dirty = checkpoint.revision.as_ref() != Some(&current_revision);
    if dirty {
        // Publish an immutable branch rooted only in versions this device has
        // actually incorporated. Two concurrent writers retain both branches.
        let bytes = vault.export_sync_snapshot_with_parents(checkpoint.heads.clone())?;
        let id = remote.publish(&bytes)?;
        atomic_write_bytes(cache_path(root, &id)?, &bytes)?;
        for parent in &checkpoint.heads {
            heads.remove(parent);
        }
        heads.insert(id.clone());
        checkpoint = Checkpoint {
            heads: vec![id],
            revision: Some(current_revision),
        };
        report.uploaded += 1;
    }
    let conflict_path = root
        .join("sync-state")
        .join(format!("{}-conflicts.json", snapshot_id(target.as_bytes())));
    let conflicts = if heads.len() > 1 {
        heads.iter().cloned().collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    atomic_write_bytes(&conflict_path, &serde_json::to_vec(&conflicts)?)?;
    atomic_write_bytes(path, &serde_json::to_vec(&checkpoint)?)?;
    if !conflicts.is_empty() || report.quarantined > 0 {
        report.conflicts = conflicts.len() + report.quarantined;
        report.status = SyncStatus::Conflict;
        report.message = Some("Concurrent vault changes were preserved. Choose the vault version to keep in sync conflicts.".into());
    }
    Ok(report)
}

fn common_ancestor(
    left: &str,
    right: &str,
    snapshots: &BTreeMap<String, SnapshotMetadata>,
) -> Option<String> {
    let ancestors = |start: &str| {
        let mut visited = BTreeMap::new();
        let mut queue = std::collections::VecDeque::from([(start.to_string(), 0usize)]);
        while let Some((id, distance)) = queue.pop_front() {
            if visited.contains_key(&id) {
                continue;
            }
            visited.insert(id.clone(), distance);
            if let Some(snapshot) = snapshots.get(&id) {
                queue.extend(
                    snapshot
                        .parents
                        .iter()
                        .map(|parent| (parent.clone(), distance + 1)),
                );
            }
        }
        visited
    };
    let left = ancestors(left);
    let right = ancestors(right);
    left.keys()
        .filter(|id| right.contains_key(*id) && snapshots.contains_key(*id))
        .min_by_key(|id| left[*id] + right[*id])
        .cloned()
}

pub(crate) fn conflicts(root: &Path) -> Result<Vec<String>> {
    let dir = root.join("sync-state");
    let mut ids = BTreeSet::new();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry
            .file_name()
            .to_string_lossy()
            .ends_with("-conflicts.json")
        {
            ids.extend(serde_json::from_slice::<Vec<String>>(&fs::read(
                entry.path(),
            )?)?);
        }
    }
    Ok(ids.into_iter().collect())
}

pub(crate) fn quarantined(root: &Path) -> Result<Vec<String>> {
    let dir = root.join("sync-quarantine");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    Ok(fs::read_dir(dir)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            let id = path.file_stem()?.to_str()?;
            valid_snapshot_id(id).then(|| id.to_string())
        })
        .collect())
}

pub(crate) fn resolve(state: &Arc<AgentState>, id: &str, accept: bool) -> ServiceResult<()> {
    let _sync = state
        .sync_lock
        .lock()
        .map_err(|_| ServiceError::internal(anyhow::anyhow!("sync lock poisoned")))?;
    crate::session::with_vault_mut(state, true, |vault| {
        let mut applied = false;
        let result = resolve_in_vault(state, vault, id, accept, &mut applied);
        if applied || result.is_ok() {
            state
                .sync_revision
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            refresh_applied_vault(state, vault)?;
        }
        result
    })
}

fn resolve_in_vault(
    state: &AgentState,
    vault: &mut Vault,
    id: &str,
    accept: bool,
    applied: &mut bool,
) -> ServiceResult<()> {
    let root = &state.vault_dir;
    if valid_snapshot_id(id)
        && root
            .join("sync-quarantine")
            .join(format!("{id}.aipsnapshot"))
            .exists()
    {
        if accept {
            return Err(ServiceError::new(
                aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                "an unverifiable snapshot cannot be accepted",
            ));
        }
        let ignored = root.join("sync-ignored");
        fs::create_dir_all(&ignored).map_err(ServiceError::internal)?;
        fs::rename(
            root.join("sync-quarantine")
                .join(format!("{id}.aipsnapshot")),
            ignored.join(format!("{id}.aipsnapshot")),
        )
        .map_err(ServiceError::internal)?;
        return Ok(());
    }
    let all = conflicts(root).map_err(ServiceError::internal)?;
    if !all.iter().any(|item| item == id) {
        return Err(ServiceError::internal(anyhow::anyhow!(
            "snapshot conflict no longer exists"
        )));
    }
    if accept {
        let bytes = fs::read(cache_path(root, id).map_err(ServiceError::internal)?)
            .map_err(ServiceError::internal)?;
        vault
            .apply_sync_snapshot(&bytes)
            .map_err(crate::session::map_vault_error)?;
        *applied = true;
    }
    // A user-selected resolution acknowledges every displayed branch.
    // The next publication names them as parents and closes the conflict.
    for entry in fs::read_dir(root.join("sync-state")).map_err(ServiceError::internal)? {
        let entry = entry.map_err(ServiceError::internal)?;
        let filename = entry.file_name().to_string_lossy().into_owned();
        let Some(prefix) = filename.strip_suffix("-conflicts.json") else {
            continue;
        };
        let heads: Vec<String> =
            serde_json::from_slice(&fs::read(entry.path()).map_err(ServiceError::internal)?)
                .map_err(ServiceError::internal)?;
        if !heads.contains(&id.to_string()) {
            continue;
        }
        let checkpoint = Checkpoint {
            heads,
            revision: None,
        };
        atomic_write_bytes(
            root.join("sync-state").join(format!("{prefix}.json")),
            &serde_json::to_vec(&checkpoint).map_err(ServiceError::internal)?,
        )
        .map_err(ServiceError::internal)?;
        atomic_write_bytes(entry.path(), b"[]").map_err(ServiceError::internal)?;
    }
    Ok(())
}

pub(crate) fn import_directory(
    root: &Path,
    input: &Path,
    password: &aipass_crypto::SecretString,
) -> Result<()> {
    let vault = Vault::open(input, password).context("cannot unlock source vault")?;
    let bytes = vault.export_sync_snapshot()?;
    VaultSyncSnapshot::parse(&bytes)?.bootstrap(root)?;
    Vault::open(root, password)?;
    Ok(())
}

pub(crate) fn import_file(
    state: &Arc<AgentState>,
    input: &Path,
    password: aipass_agent_protocol::SensitiveString,
) -> ServiceResult<()> {
    let _sync = state
        .sync_lock
        .lock()
        .map_err(|_| ServiceError::internal(anyhow::anyhow!("sync lock poisoned")))?;
    let root = &state.vault_dir;
    let parent = root
        .parent()
        .context("vault parent missing")
        .map_err(ServiceError::internal)?;
    let stage = tempfile::tempdir_in(parent).map_err(ServiceError::internal)?;
    let secret = aipass_crypto::SecretString::new(password.into_inner());
    if input.is_dir() {
        import_directory(stage.path(), input, &secret).map_err(ServiceError::internal)?;
    } else {
        let bytes = fs::read(input).map_err(ServiceError::internal)?;
        if let Ok(snapshot) = VaultSyncSnapshot::parse(&bytes) {
            snapshot
                .bootstrap(stage.path())
                .map_err(crate::session::map_vault_error)?;
            Vault::open(stage.path(), &secret).map_err(crate::session::map_vault_error)?;
        } else {
            let export = serde_json::from_slice(&bytes).map_err(ServiceError::internal)?;
            Vault::import_encrypted(stage.path(), &secret, &export)
                .map_err(crate::session::map_vault_error)?;
        }
    }
    // The imported vault carries its disconnected settings through the same
    // directory rename; it can never inherit the old vault's sync target.
    let settings = serde_json::to_vec(&crate::session::PersistedSyncSettings::default())
        .map_err(ServiceError::internal)?;
    install_import(state, stage.path(), &settings)?;
    crate::sync_watch::start_sync_watcher_for_current_settings(state);
    Ok(())
}

fn install_import(state: &Arc<AgentState>, stage: &Path, settings: &[u8]) -> ServiceResult<()> {
    atomic_write_bytes(stage.join("pending-import-settings.json"), settings)
        .map_err(ServiceError::internal)?;
    let mut session = state
        .session
        .lock()
        .map_err(|_| ServiceError::internal(anyhow::anyhow!("session lock poisoned")))?;
    let mut proxy = state
        .proxy
        .lock()
        .map_err(|_| ServiceError::internal(anyhow::anyhow!("proxy lock poisoned")))?;
    proxy.stop()?;
    let root = &state.vault_dir;
    let backup = root.with_file_name(format!("vault-import-backup-{}", uuid::Uuid::new_v4()));
    let had_root = root.exists();
    if had_root {
        fs::rename(root, &backup).map_err(ServiceError::internal)?;
    }
    if let Err(err) = fs::rename(stage, root) {
        if had_root {
            fs::rename(&backup, root).map_err(ServiceError::internal)?;
        }
        return Err(ServiceError::internal(err));
    }
    *session = SessionState::Locked;
    if let Ok(mut cached) = state.webdav_transport.lock() {
        *cached = None;
    }
    state.session_changed.notify_all();
    state
        .sync_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    crate::session::finish_import_settings(root, settings).map_err(ServiceError::internal)?;
    Ok(())
}

pub(crate) fn import_sync(
    state: &Arc<AgentState>,
    update: aipass_agent_protocol::SyncSettingsUpdate,
    password: aipass_agent_protocol::SensitiveString,
) -> ServiceResult<()> {
    let _sync = state
        .sync_lock
        .lock()
        .map_err(|_| ServiceError::internal(anyhow::anyhow!("sync lock poisoned")))?;
    if state.vault_dir.join("manifest.aipmanifest").exists() {
        return Err(crate::session::map_vault_error(
            aipass_vault::VaultError::AlreadyExists,
        ));
    }
    let settings = crate::session::apply_sync_settings_update(
        crate::session::StoredSyncSettings::default(),
        update,
    );
    let stage = tempfile::tempdir_in(
        state
            .vault_dir
            .parent()
            .context("vault parent missing")
            .map_err(ServiceError::internal)?,
    )
    .map_err(ServiceError::internal)?;
    let (bytes, id, target) = if let Some(dir) = crate::sync_watch::folder_sync_dir(&settings) {
        let (bytes, id) = single_snapshot(&aipass_sync::FolderSnapshotRemote(&dir))
            .map_err(ServiceError::internal)?;
        (bytes, id, format!("folder:{}", dir.display()))
    } else if settings.mode == aipass_agent_protocol::SyncMode::ICloud {
        let remote = crate::cloudkit::Remote::connect(&state.cloudkit).map_err(remote_error)?;
        let (bytes, id) = single_snapshot(&remote).map_err(ServiceError::internal)?;
        check_cloud_account(stage.path(), &remote.account).map_err(ServiceError::internal)?;
        (bytes, id, format!("cloudkit:{}", remote.account))
    } else if settings.mode == aipass_agent_protocol::SyncMode::WebDav {
        let url = settings
            .webdav_url
            .as_ref()
            .context("WebDAV URL is required")
            .map_err(ServiceError::internal)?;
        let dav_password = match &settings.webdav_password {
            Some(crate::session::StoredSyncSecret::Plaintext(value)) => {
                Some(value.clone().into_inner())
            }
            _ => None,
        };
        let client =
            aipass_sync::HttpWebDavClient::new(url, settings.webdav_username.clone(), dav_password)
                .map_err(ServiceError::internal)?;
        let (bytes, id) = single_snapshot(&aipass_sync::WebDavSnapshotRemote(&client))
            .map_err(ServiceError::internal)?;
        (bytes, id, format!("webdav:{url}"))
    } else {
        return Err(ServiceError::internal(anyhow::anyhow!(
            "sync folder unavailable"
        )));
    };
    VaultSyncSnapshot::parse(&bytes)
        .map_err(crate::session::map_vault_error)?
        .bootstrap(stage.path())
        .map_err(crate::session::map_vault_error)?;
    let vault = Vault::open(
        stage.path(),
        &aipass_crypto::SecretString::new(password.into_inner()),
    )
    .map_err(crate::session::map_vault_error)?;
    let checkpoint = Checkpoint {
        heads: vec![id.clone()],
        revision: Some(
            vault
                .sync_revision()
                .map_err(crate::session::map_vault_error)?,
        ),
    };
    atomic_write_bytes(
        cache_path(stage.path(), &id).map_err(ServiceError::internal)?,
        &bytes,
    )
    .map_err(ServiceError::internal)?;
    atomic_write_bytes(
        checkpoint_path(stage.path(), &target),
        &serde_json::to_vec(&checkpoint).map_err(ServiceError::internal)?,
    )
    .map_err(ServiceError::internal)?;
    let (persisted, saved) =
        crate::session::prepare_sync_settings(&vault, &settings).map_err(ServiceError::internal)?;
    drop(vault);
    install_import(state, stage.path(), &persisted)?;
    crate::sync_watch::restart_sync_watcher(state, &saved);
    Ok(())
}

fn single_snapshot(remote: &impl SnapshotRemote) -> Result<(Vec<u8>, String)> {
    let mut snapshots = BTreeMap::new();
    for id in remote.list()? {
        let bytes = remote.get(&id)?;
        snapshots.insert(id, (VaultSyncSnapshot::parse(&bytes)?, bytes));
    }
    let mut heads = snapshots.keys().cloned().collect::<BTreeSet<_>>();
    for (snapshot, _) in snapshots.values() {
        for parent in &snapshot.parents {
            heads.remove(parent);
        }
    }
    if heads.is_empty() {
        bail!("No recoverable vault snapshot found. Sync once from the original device using the current AIPass version, or import an encrypted backup.");
    }
    if heads.len() != 1 {
        bail!("The remote has conflicting vault versions. Resolve them on an existing device, or import a selected .aipsnapshot file.");
    }
    let id = heads.into_iter().next().unwrap();
    Ok((snapshots.remove(&id).unwrap().1, id))
}

fn reject_unrecoverable_legacy(remote: &impl SnapshotRemote) -> ServiceResult<()> {
    if !remote.legacy_objects().map_err(remote_error)?.is_empty() {
        return Err(ServiceError::new(
            aipass_agent_protocol::AgentErrorCode::Conflict,
            "Existing encrypted records require the original vault keys. Import a backup or sync a full snapshot from the original device before creating a vault.",
        ));
    }
    Ok(())
}

pub(crate) fn bootstrap_before_create(state: &Arc<AgentState>) -> ServiceResult<bool> {
    use aipass_sync::SnapshotRemote;
    if state.vault_dir.join("manifest.aipmanifest").exists() {
        return Ok(true);
    }
    let settings =
        crate::session::load_sync_settings(&state.vault_dir).map_err(ServiceError::internal)?;
    let (bytes, id, target) = if settings.mode == aipass_agent_protocol::SyncMode::ICloud {
        let remote = crate::cloudkit::Remote::connect(&state.cloudkit).map_err(remote_error)?;
        check_cloud_account(&state.vault_dir, &remote.account).map_err(ServiceError::internal)?;
        if remote.list().map_err(remote_error)?.is_empty() {
            // A recoverable Drive snapshot still takes precedence over creation.
            let Ok(dir) =
                crate::paths::cloud_sync_dir(aipass_agent_protocol::CloudSyncProvider::ICloud)
            else {
                return Ok(false);
            };
            let legacy = aipass_sync::FolderSnapshotRemote(&dir);
            if legacy.list().map_err(remote_error)?.is_empty() {
                reject_unrecoverable_legacy(&legacy)?;
                return Ok(false);
            }
            let (bytes, id) = single_snapshot(&legacy).map_err(ServiceError::internal)?;
            (bytes, id, format!("folder:{}", dir.display()))
        } else {
            let (bytes, id) = single_snapshot(&remote).map_err(ServiceError::internal)?;
            (bytes, id, format!("cloudkit:{}", remote.account))
        }
    } else {
        let Some(dir) = crate::sync_watch::folder_sync_dir(&settings) else {
            return Ok(false);
        };
        let remote = aipass_sync::FolderSnapshotRemote(&dir);
        if remote.list().map_err(remote_error)?.is_empty() {
            reject_unrecoverable_legacy(&remote)?;
            return Ok(false);
        }
        let (bytes, id) = single_snapshot(&remote).map_err(ServiceError::internal)?;
        (bytes, id, format!("folder:{}", dir.display()))
    };
    let snapshot = VaultSyncSnapshot::parse(&bytes).map_err(crate::session::map_vault_error)?;
    prepare_cloudkit_bootstrap(&state.vault_dir, &target).map_err(ServiceError::internal)?;
    snapshot
        .bootstrap(&state.vault_dir)
        .map_err(crate::session::map_vault_error)?;
    atomic_write_bytes(
        cache_path(&state.vault_dir, &id).map_err(ServiceError::internal)?,
        &bytes,
    )
    .map_err(ServiceError::internal)?;
    let checkpoint = Checkpoint {
        heads: vec![id],
        revision: None,
    };
    atomic_write_bytes(
        checkpoint_path(&state.vault_dir, &target),
        &serde_json::to_vec(&checkpoint).map_err(ServiceError::internal)?,
    )
    .map_err(ServiceError::internal)?;
    state
        .sync_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(true)
}

fn prepare_cloudkit_bootstrap(root: &Path, target: &str) -> Result<()> {
    if !target.starts_with("cloudkit:") || root.join("manifest.aipmanifest").exists() {
        return Ok(());
    }
    // A fresh CloudKit restore is already on the destination. Record that
    // before installing its manifest so it cannot look like an old Drive vault.
    let path = crate::session::sync_settings_path(root);
    let mut settings: crate::session::PersistedSyncSettings = if path.exists() {
        serde_json::from_slice(&fs::read(&path)?)?
    } else {
        crate::session::PersistedSyncSettings {
            mode: aipass_agent_protocol::SyncMode::ICloud,
            ..Default::default()
        }
    };
    settings.cloudkit_migration_complete = true;
    atomic_write_bytes(path, &serde_json::to_vec(&settings)?)?;
    Ok(())
}

#[cfg(test)]
mod tests;
