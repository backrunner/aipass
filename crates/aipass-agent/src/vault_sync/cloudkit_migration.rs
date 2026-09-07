//! One-time upgrade of existing macOS sync targets. The persisted source
//! remains authoritative until a complete CloudKit snapshot is read back.
use super::*;
use crate::session::{with_vault, StoredSyncSettings};
use aipass_agent_protocol::{AgentErrorCode, CloudSyncProvider, SyncMode};

#[cfg(test)]
mod tests;

pub(super) fn run(
    state: &Arc<AgentState>,
    settings: &StoredSyncSettings,
) -> ServiceResult<SyncReport> {
    let legacy_directory = if settings.mode == SyncMode::ICloud {
        Some(
            crate::paths::cloud_sync_dir(CloudSyncProvider::ICloud)
                .map_err(ServiceError::internal)?,
        )
    } else {
        None
    };
    run_from_source(state, settings, legacy_directory.as_deref())
}

fn run_from_source(
    state: &Arc<AgentState>,
    settings: &StoredSyncSettings,
    legacy_directory: Option<&Path>,
) -> ServiceResult<SyncReport> {
    let unlocked = matches!(
        *state
            .session
            .lock()
            .map_err(|_| { ServiceError::internal(anyhow::anyhow!("session lock poisoned")) })?,
        SessionState::Unlocked(_)
    );
    if unlocked {
        preserve_source(state, settings)?;
    }
    let source_report = sync_source(state, settings, legacy_directory)?;
    if !unlocked
        || source_report.status != SyncStatus::Idle
        || source_report.conflicts > 0
        || source_report.quarantined > 0
    {
        return Ok(source_report);
    }
    let remote = crate::cloudkit::Remote::connect(&state.cloudkit).map_err(remote_error)?;
    check_cloud_account(&state.vault_dir, &remote.account)
        .map_err(|err| ServiceError::new(AgentErrorCode::Conflict, err.to_string()))?;
    finish(state, settings, &remote, &remote.account, source_report)
}

fn preserve_source(state: &Arc<AgentState>, settings: &StoredSyncSettings) -> ServiceResult<()> {
    with_vault(state, false, |vault| {
        let directory = state.vault_dir.join("sync-state/cloudkit-migration");
        let snapshot = directory.join("source.aipsnapshot");
        if snapshot.exists() {
            vault
                .sync_snapshot_revision(&fs::read(&snapshot).map_err(ServiceError::internal)?)
                .map_err(crate::session::map_vault_error)?;
        } else {
            let bytes = vault
                .export_sync_snapshot()
                .map_err(crate::session::map_vault_error)?;
            atomic_write_bytes(snapshot, &bytes).map_err(ServiceError::internal)?;
        }
        let source = directory.join("source-settings.json");
        if !source.exists() {
            let (bytes, _) = crate::session::prepare_sync_settings(vault, settings)
                .map_err(ServiceError::internal)?;
            atomic_write_bytes(source, &bytes).map_err(ServiceError::internal)?;
        }
        Ok(())
    })
}

fn sync_source(
    state: &Arc<AgentState>,
    settings: &StoredSyncSettings,
    legacy_directory: Option<&Path>,
) -> ServiceResult<SyncReport> {
    if settings.mode == SyncMode::ICloud {
        // The old iCloud mode referred to Drive. Read it even if another
        // upgraded device has already populated CloudKit.
        let directory = legacy_directory
            .context("legacy iCloud Drive is unavailable")
            .map_err(ServiceError::internal)?;
        return run_inner(
            state,
            &aipass_sync::FolderSnapshotRemote(directory),
            &format!("folder:{}", directory.display()),
        );
    }
    if settings.mode == SyncMode::Local && settings.sync_folder.is_none() {
        return Ok(SyncReport {
            uploaded: 0,
            downloaded: 0,
            conflicts: 0,
            quarantined: 0,
            status: SyncStatus::Idle,
            message: None,
        });
    }
    run_settings_inner(state, settings)
}

fn finish(
    state: &Arc<AgentState>,
    settings: &StoredSyncSettings,
    remote: &impl SnapshotRemote,
    account: &str,
    source_report: SyncReport,
) -> ServiceResult<SyncReport> {
    let original = fs::read(
        state
            .vault_dir
            .join("sync-state/cloudkit-migration/source.aipsnapshot"),
    )
    .map_err(ServiceError::internal)?;
    let original_revision = with_vault(state, false, |vault| {
        vault
            .sync_snapshot_revision(&original)
            .map_err(crate::session::map_vault_error)
    })?;
    let mut known_base = None;
    // Inspect the destination before flushing a previous migration's outbox.
    // An unrelated or unverifiable vault must never receive local snapshots.
    for id in remote.list().map_err(remote_error)? {
        let bytes = remote.get(&id).map_err(remote_error)?;
        if snapshot_id(&bytes) != id {
            return Err(ServiceError::new(
                AgentErrorCode::ValidationFailed,
                "CloudKit snapshot hash mismatch",
            ));
        }
        let revision = with_vault(state, false, |vault| {
            let snapshot =
                VaultSyncSnapshot::parse(&bytes).map_err(crate::session::map_vault_error)?;
            if snapshot.header.vault_id != vault.vault_id() {
                return Err(ServiceError::new(
                    AgentErrorCode::Conflict,
                    ForeignVault.to_string(),
                ));
            }
            vault
                .sync_snapshot_revision(&bytes)
                .map_err(crate::session::map_vault_error)
        })?;
        if revision == original_revision {
            known_base = Some(Checkpoint {
                heads: vec![id.clone()],
                revision: Some(revision),
            });
        }
        atomic_write_bytes(
            cache_path(&state.vault_dir, &id).map_err(ServiceError::internal)?,
            &bytes,
        )
        .map_err(ServiceError::internal)?;
    }
    let target = format!("cloudkit:{account}");
    let checkpoint_file = checkpoint_path(&state.vault_dir, &target);
    if !checkpoint_file.exists() {
        if let Some(base) = known_base {
            // This remote version exactly matches the authenticated vault we
            // started with. Preserve it as the base for source-side edits.
            atomic_write_bytes(
                &checkpoint_file,
                &serde_json::to_vec(&base).map_err(ServiceError::internal)?,
            )
            .map_err(ServiceError::internal)?;
        }
    }
    let mut report = run_inner(state, remote, &target)?;
    report.downloaded += source_report.downloaded;
    report.uploaded += source_report.uploaded;
    if report.status != SyncStatus::Idle || report.conflicts > 0 || report.quarantined > 0 {
        return Ok(report);
    }
    let checkpoint: Checkpoint = serde_json::from_slice(
        &fs::read(checkpoint_path(&state.vault_dir, &target)).map_err(ServiceError::internal)?,
    )
    .map_err(ServiceError::internal)?;
    let [id] = checkpoint.heads.as_slice() else {
        return Err(ServiceError::new(
            AgentErrorCode::Conflict,
            "CloudKit migration needs one verified vault version",
        ));
    };
    // Fetch the uploaded bytes through the real transport, not the local cache.
    let bytes = remote.get(id).map_err(remote_error)?;
    if snapshot_id(&bytes) != *id {
        return Err(ServiceError::new(
            AgentErrorCode::ValidationFailed,
            "CloudKit migration read-back hash mismatch",
        ));
    }
    let saved = with_vault(state, false, |vault| {
        let revision = vault
            .sync_snapshot_revision(&bytes)
            .map_err(crate::session::map_vault_error)?;
        if checkpoint.revision.as_ref() != Some(&revision)
            || vault
                .sync_revision()
                .map_err(crate::session::map_vault_error)?
                != revision
        {
            return Err(ServiceError::new(AgentErrorCode::ServiceUnavailable,
                "The vault changed during CloudKit migration; the original sync target remains active until retry"));
        }
        let updated = StoredSyncSettings {
            mode: SyncMode::ICloud,
            cloudkit_migration_pending: false,
            ..settings.clone()
        };
        crate::session::save_sync_settings(&state.vault_dir, vault, &updated)
            .map_err(ServiceError::internal)
    })?;
    state
        .sync_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    crate::sync_watch::restart_sync_watcher(state, &saved);
    Ok(report)
}
