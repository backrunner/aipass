use aipass_agent::AgentCommandError;
use aipass_agent_protocol::{
    AgentErrorCode, SessionStatus, VaultCreateResponse as AgentVaultCreateResponse,
};
use aipass_vault::RecoveryKit;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

pub(crate) const AUTH_TASK_RETENTION: Duration = Duration::from_secs(300);
pub(crate) type AuthTasks = Arc<Mutex<HashMap<Uuid, StoredVaultAuthTask>>>;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VaultAuthTaskStartResponse {
    pub(crate) task_id: Uuid,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VaultAuthTaskStatusRequest {
    pub(crate) task_id: Uuid,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum VaultAuthTaskPhase {
    Pending,
    Succeeded,
    Failed,
}

#[derive(Clone, Debug)]
pub(crate) enum VaultAuthTaskState {
    Pending {
        message: String,
    },
    Succeeded {
        message: String,
        exists: bool,
        locked: bool,
        recovery_kit: Option<RecoveryKit>,
    },
    Failed {
        message: String,
        error_code: Option<AgentErrorCode>,
        error: String,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct StoredVaultAuthTask {
    pub(crate) state: VaultAuthTaskState,
    pub(crate) finished_at: Option<Instant>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VaultAuthTaskStatusResponse {
    pub(crate) task_id: Uuid,
    pub(crate) phase: VaultAuthTaskPhase,
    pub(crate) message: String,
    pub(crate) exists: Option<bool>,
    pub(crate) locked: Option<bool>,
    pub(crate) recovery_kit: Option<RecoveryKit>,
    pub(crate) error_code: Option<AgentErrorCode>,
    pub(crate) error: Option<String>,
}

pub(crate) fn set_auth_task_state(
    auth_tasks: &AuthTasks,
    task_id: Uuid,
    task: VaultAuthTaskState,
) -> Result<(), String> {
    let finished_at = if matches!(task, VaultAuthTaskState::Pending { .. }) {
        None
    } else {
        Some(Instant::now())
    };
    auth_tasks
        .lock()
        .map_err(|_| "auth task lock poisoned".to_string())?
        .insert(
            task_id,
            StoredVaultAuthTask {
                state: task,
                finished_at,
            },
        );
    Ok(())
}

pub(crate) fn prune_auth_tasks(auth_tasks: &AuthTasks) {
    if let Ok(mut tasks) = auth_tasks.lock() {
        tasks.retain(|_, task| {
            task.finished_at
                .is_none_or(|finished_at| finished_at.elapsed() < AUTH_TASK_RETENTION)
        });
    }
}

pub(crate) fn auth_task_status_response(
    task_id: Uuid,
    snapshot: VaultAuthTaskState,
) -> VaultAuthTaskStatusResponse {
    match snapshot {
        VaultAuthTaskState::Pending { message } => VaultAuthTaskStatusResponse {
            task_id,
            phase: VaultAuthTaskPhase::Pending,
            message,
            exists: None,
            locked: None,
            recovery_kit: None,
            error_code: None,
            error: None,
        },
        VaultAuthTaskState::Succeeded {
            message,
            exists,
            locked,
            recovery_kit,
        } => VaultAuthTaskStatusResponse {
            task_id,
            phase: VaultAuthTaskPhase::Succeeded,
            message,
            exists: Some(exists),
            locked: Some(locked),
            recovery_kit,
            error_code: None,
            error: None,
        },
        VaultAuthTaskState::Failed {
            message,
            error_code,
            error,
        } => VaultAuthTaskStatusResponse {
            task_id,
            phase: VaultAuthTaskPhase::Failed,
            message,
            exists: None,
            locked: None,
            recovery_kit: None,
            error_code,
            error: Some(error),
        },
    }
}

pub(crate) fn emit_auth_task_event(app: &AppHandle, response: &VaultAuthTaskStatusResponse) {
    let _ = app.emit("vault-auth-finished", response.clone());
    let _ = app.emit(crate::tray::VAULT_STATUS_CHANGED_EVENT, ());
    let _ = app.emit(crate::tray::REFRESH_PROXY_TRAY_EVENT, ());
}

pub(crate) fn finish_vault_create_task(
    app: AppHandle,
    auth_tasks: AuthTasks,
    task_id: Uuid,
    result: Result<AgentVaultCreateResponse, String>,
) {
    let next_state = match result {
        Ok(creation) => VaultAuthTaskState::Succeeded {
            message: "Vault is ready".to_string(),
            exists: creation.session.exists,
            locked: creation.session.locked,
            recovery_kit: Some(creation.recovery_kit),
        },
        Err(error) => VaultAuthTaskState::Failed {
            message: "Vault operation failed".to_string(),
            error_code: None,
            error,
        },
    };
    let _ = set_auth_task_state(&auth_tasks, task_id, next_state);
    prune_auth_tasks(&auth_tasks);
    if let Ok(tasks) = auth_tasks.lock() {
        if let Some(snapshot) = tasks.get(&task_id).map(|task| task.state.clone()) {
            emit_auth_task_event(&app, &auth_task_status_response(task_id, snapshot));
        }
    }
}

pub(crate) fn finish_vault_unlock_task(
    app: AppHandle,
    auth_tasks: AuthTasks,
    task_id: Uuid,
    result: Result<SessionStatus, AgentCommandError>,
) {
    let next_state = match result {
        Ok(status) => VaultAuthTaskState::Succeeded {
            message: "Vault unlocked".to_string(),
            exists: status.exists,
            locked: status.locked,
            recovery_kit: None,
        },
        Err(error) => VaultAuthTaskState::Failed {
            message: "Vault operation failed".to_string(),
            error_code: error.code,
            error: error.message,
        },
    };
    let _ = set_auth_task_state(&auth_tasks, task_id, next_state);
    prune_auth_tasks(&auth_tasks);
    if let Ok(tasks) = auth_tasks.lock() {
        if let Some(snapshot) = tasks.get(&task_id).map(|task| task.state.clone()) {
            emit_auth_task_event(&app, &auth_task_status_response(task_id, snapshot));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_task_response_preserves_agent_error_code() {
        let response = auth_task_status_response(
            Uuid::new_v4(),
            VaultAuthTaskState::Failed {
                message: "Vault operation failed".to_string(),
                error_code: Some(AgentErrorCode::InvalidPassword),
                error: "invalid password or corrupted vault".to_string(),
            },
        );

        assert_eq!(response.error_code, Some(AgentErrorCode::InvalidPassword));
    }
}
