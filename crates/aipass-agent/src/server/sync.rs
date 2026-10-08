//! Conflict views and serialized sync entry points.
use super::*;

pub(super) fn conflict_responses(
    scope: ConflictScope,
    root: &Path,
    vault: &Vault,
) -> ServiceResult<Vec<SyncConflictResponse>> {
    list_conflicts(root)
        .map_err(ServiceError::internal)?
        .into_iter()
        .map(|record| conflict_response(scope.clone(), root, vault, record))
        .collect()
}

pub(super) fn conflict_response(
    scope: ConflictScope,
    root: &Path,
    vault: &Vault,
    record: ConflictRecord,
) -> ServiceResult<SyncConflictResponse> {
    let conflict_summary = summary_from_conflict_path(vault, root, &record.conflict_path, &record);
    let target_summary = summary_from_conflict_path(vault, root, &record.target_path, &record);
    Ok(SyncConflictResponse {
        scope,
        origin: record.origin,
        conflict_path: record.conflict_path,
        target_path: record.target_path,
        object: record.object,
        conflict_summary,
        target_summary,
        snapshot_summary: None,
    })
}

pub(super) fn summary_from_conflict_path(
    vault: &Vault,
    root: &Path,
    relative_path: &Path,
    record: &ConflictRecord,
) -> Option<EntrySummary> {
    if record.object.object_type != "provider_entry" {
        return None;
    }
    vault
        .get_provider_summary_from_path(root.join(relative_path))
        .ok()
}

pub(super) fn conflict_root(
    vault_dir: &Path,
    request: &SyncConflictActionRequest,
) -> ServiceResult<PathBuf> {
    match request.scope {
        ConflictScope::Vault => Ok(vault_dir.to_path_buf()),
        ConflictScope::Sync => {
            if let Some(provider) = request.provider {
                return cloud_sync_dir(provider).map_err(ServiceError::internal);
            }
            request.dir.clone().ok_or_else(|| {
                ServiceError::new(
                    AgentErrorCode::ValidationFailed,
                    "sync conflict scope requires a local or cloud sync target",
                )
            })
        }
    }
}

#[cfg(test)]
pub(crate) static TOOL_HOME_OVERRIDES: std::sync::LazyLock<
    Mutex<std::collections::HashMap<Uuid, PathBuf>>,
> = std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

pub(super) fn home_dir(_vault: &Vault) -> ServiceResult<PathBuf> {
    #[cfg(test)]
    if let Some(home) = TOOL_HOME_OVERRIDES.lock().unwrap().get(&_vault.vault_id()) {
        return Ok(home.clone());
    }

    std::env::var("HOME")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("USERPROFILE").map(PathBuf::from))
        .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "home directory unavailable"))
}

pub(crate) fn run_sync_local(state: &Arc<AgentState>, dir: &Path) -> ServiceResult<SyncReport> {
    let operation = crate::operation_log::OperationLog::background("sync.local");
    if let Ok(mut status) = state.sync_status.lock() {
        *status = Some(SyncStatus::Syncing);
    }
    let result = run_sync_local_inner(state, dir);
    if let Ok(mut status) = state.sync_status.lock() {
        *status = Some(
            result
                .as_ref()
                .map(|report| report.status.clone())
                .unwrap_or(SyncStatus::ServerError),
        );
    }
    if let Some(operation) = operation {
        operation.finish(&match &result {
            Ok(report) => AgentResponse::success(report),
            Err(err) => AgentResponse::error(err.code.clone(), "sync failed"),
        });
    }
    result
}

pub(super) fn run_sync_local_inner(
    state: &Arc<AgentState>,
    dir: &Path,
) -> ServiceResult<SyncReport> {
    crate::vault_sync::run(
        state,
        &aipass_sync::FolderSnapshotRemote(dir),
        &format!("folder:{}", dir.display()),
    )
}

pub(crate) fn run_sync_webdav_target(
    state: &Arc<AgentState>,
    client: &impl WebDavClient,
    target: &str,
) -> SyncReport {
    let operation = crate::operation_log::OperationLog::background("sync.webdav");
    if let Ok(mut status) = state.sync_status.lock() {
        *status = Some(SyncStatus::Syncing);
    }
    let report =
        match crate::vault_sync::run(state, &aipass_sync::WebDavSnapshotRemote(client), target) {
            Ok(report) => report,
            Err(err) => SyncReport {
                uploaded: 0,
                downloaded: 0,
                conflicts: 0,
                quarantined: 0,
                status: match err.code {
                    AgentErrorCode::PermissionDenied => SyncStatus::AuthFailed,
                    AgentErrorCode::ServiceUnavailable => SyncStatus::Offline,
                    AgentErrorCode::Conflict => SyncStatus::Conflict,
                    _ => SyncStatus::ServerError,
                },
                message: Some(err.message),
            },
        };
    if let Ok(mut status) = state.sync_status.lock() {
        *status = Some(report.status.clone());
    }
    if let Some(operation) = operation {
        operation.finish(&AgentResponse::success(&report));
    }
    report
}

pub(crate) fn run_sync_configured(state: &Arc<AgentState>) -> ServiceResult<SyncReport> {
    crate::vault_sync::run_configured(state)
}
