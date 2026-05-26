use crate::auth_tasks::{
    auth_task_status_response, finish_vault_create_task, finish_vault_unlock_task,
    prune_auth_tasks, set_auth_task_state, VaultAuthTaskStartResponse, VaultAuthTaskState,
    VaultAuthTaskStatusRequest, VaultAuthTaskStatusResponse,
};
use crate::models::{
    from_agent_sync_conflict_response, from_agent_tool_config_apply,
    from_agent_tool_config_preview, into_agent_cloud_sync_provider,
    into_agent_sync_conflict_request, into_agent_tool_config_request, AppPreferences,
    ChangePasswordRequest, CreateVaultRequest, ProbeResult, ProviderAddRequest,
    ProviderUpdateRequest, RecoveryVaultRequest, SavePreferencesRequest,
    SyncCloudRequest, SyncConflictActionRequest, SyncConflictResponse, SyncConflictsRequest,
    SyncLocalRequest, SyncWebDavRequest, ToolConfigApplyResponse, ToolConfigPreviewResponse,
    ToolConfigRequest, UnlockVaultRequest, VaultExportRequest, VaultImportRequest, VaultStatus,
};
use aipass_agent_protocol::{
    AgentRequest, LockReason, ProbeResult as AgentProbeResult, SecretValue, SensitiveString,
    SessionPolicy, SessionStatus, SessionUnlockMode,
    SyncConflictResponse as AgentSyncConflictResponse,
    ToolConfigApplyResponse as AgentToolConfigApplyResponse,
    ToolConfigPreviewResponse as AgentToolConfigPreviewResponse,
};
use aipass_sync::SyncReport;
use aipass_vault::{DeviceRecord, EntrySummary};
use tauri::{AppHandle, State};
use uuid::Uuid;

use crate::{
    agent_request, agent_request_no_unlock, agent_status, load_preferences, provider_add_input,
    provider_update_input, run_blocking, save_preferences, AppState,
};

#[tauri::command]
pub(crate) fn window_target() -> Option<String> {
    std::env::var("AIPASS_WINDOW_TARGET")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| matches!(value.as_str(), "main" | "unlock" | "quick-access"))
}

#[tauri::command]
pub(crate) fn vault_status(app: AppHandle) -> Result<VaultStatus, String> {
    let status = agent_status(&app);
    Ok(VaultStatus {
        exists: status.exists,
        locked: status.locked,
    })
}

#[tauri::command]
pub(crate) fn session_touch(app: AppHandle) -> Result<VaultStatus, String> {
    let status: SessionStatus = agent_request_no_unlock(&app, AgentRequest::SessionTouch)?;
    Ok(VaultStatus {
        exists: status.exists,
        locked: status.locked,
    })
}

#[tauri::command]
pub(crate) fn preferences_load(app: AppHandle) -> Result<AppPreferences, String> {
    let local = load_preferences(&app)?;
    let policy = agent_request_no_unlock::<SessionPolicy>(&app, AgentRequest::SessionPolicyGet)
        .unwrap_or_default();
    Ok(AppPreferences {
        auto_lock_minutes: policy.idle_lock_minutes,
        clipboard_clear_seconds: local.clipboard_clear_seconds,
        lock_on_sleep: policy.lock_on_sleep,
        lock_on_screen_lock: policy.lock_on_screen_lock,
    })
}

#[tauri::command]
pub(crate) fn preferences_save(
    app: AppHandle,
    request: SavePreferencesRequest,
) -> Result<AppPreferences, String> {
    let current_policy =
        agent_request_no_unlock::<SessionPolicy>(&app, AgentRequest::SessionPolicyGet)
            .unwrap_or_default();
    let preferences = AppPreferences {
        auto_lock_minutes: request.auto_lock_minutes.min(240),
        clipboard_clear_seconds: request.clipboard_clear_seconds.min(600),
        lock_on_sleep: request
            .lock_on_sleep
            .unwrap_or(current_policy.lock_on_sleep),
        lock_on_screen_lock: request
            .lock_on_screen_lock
            .unwrap_or(current_policy.lock_on_screen_lock),
    };
    save_preferences(&app, &preferences)?;
    let _: SessionPolicy = agent_request_no_unlock(
        &app,
        AgentRequest::SessionPolicySet {
            policy: SessionPolicy {
                idle_lock_minutes: preferences.auto_lock_minutes,
                lock_on_sleep: preferences.lock_on_sleep,
                lock_on_screen_lock: preferences.lock_on_screen_lock,
            },
        },
    )?;
    Ok(preferences)
}

#[tauri::command]
pub(crate) async fn vault_create(
    app: AppHandle,
    state: State<'_, AppState>,
    request: CreateVaultRequest,
) -> Result<VaultAuthTaskStartResponse, String> {
    let password = request.password;
    let task_id = Uuid::new_v4();
    let app_handle = app.clone();
    let request_app = app.clone();
    let auth_tasks = state.auth_tasks.clone();
    prune_auth_tasks(&auth_tasks);
    set_auth_task_state(
        &auth_tasks,
        task_id,
        VaultAuthTaskState::Pending {
            message: "Creating encrypted vault".to_string(),
        },
    )?;
    tauri::async_runtime::spawn(async move {
        let result = run_blocking(move || {
            agent_request_no_unlock(&request_app, AgentRequest::VaultCreate { password })
        })
        .await;
        finish_vault_create_task(app_handle, auth_tasks, task_id, result);
    });
    Ok(VaultAuthTaskStartResponse { task_id })
}

#[tauri::command]
pub(crate) async fn vault_unlock(
    app: AppHandle,
    state: State<'_, AppState>,
    request: UnlockVaultRequest,
) -> Result<VaultAuthTaskStartResponse, String> {
    let password = request.password;
    let task_id = Uuid::new_v4();
    let app_handle = app.clone();
    let request_app = app.clone();
    let auth_tasks = state.auth_tasks.clone();
    prune_auth_tasks(&auth_tasks);
    set_auth_task_state(
        &auth_tasks,
        task_id,
        VaultAuthTaskState::Pending {
            message: "Unlocking vault".to_string(),
        },
    )?;
    tauri::async_runtime::spawn(async move {
        let result = run_blocking(move || {
            agent_request_no_unlock(
                &request_app,
                AgentRequest::SessionUnlock {
                    mode: SessionUnlockMode::Password { password },
                },
            )
        })
        .await;
        finish_vault_unlock_task(app_handle, auth_tasks, task_id, result);
    });
    Ok(VaultAuthTaskStartResponse { task_id })
}

#[tauri::command]
pub(crate) async fn vault_recover(
    app: AppHandle,
    state: State<'_, AppState>,
    request: RecoveryVaultRequest,
) -> Result<VaultAuthTaskStartResponse, String> {
    let recovery_key = request.recovery_key;
    let new_password = request.new_password;
    let task_id = Uuid::new_v4();
    let app_handle = app.clone();
    let request_app = app.clone();
    let auth_tasks = state.auth_tasks.clone();
    prune_auth_tasks(&auth_tasks);
    set_auth_task_state(
        &auth_tasks,
        task_id,
        VaultAuthTaskState::Pending {
            message: "Recovering vault".to_string(),
        },
    )?;
    tauri::async_runtime::spawn(async move {
        let result = run_blocking(move || {
            agent_request_no_unlock(
                &request_app,
                AgentRequest::VaultRecover {
                    recovery_key,
                    new_password,
                },
            )
        })
        .await;
        finish_vault_create_task(app_handle, auth_tasks, task_id, result);
    });
    Ok(VaultAuthTaskStartResponse { task_id })
}

#[tauri::command]
pub(crate) fn vault_auth_status(
    state: State<'_, AppState>,
    request: VaultAuthTaskStatusRequest,
) -> Result<VaultAuthTaskStatusResponse, String> {
    prune_auth_tasks(&state.auth_tasks);
    let task_id = request.task_id;
    let tasks = state
        .auth_tasks
        .lock()
        .map_err(|_| "auth task lock poisoned".to_string())?;
    let snapshot = tasks
        .get(&task_id)
        .map(|task| task.state.clone())
        .ok_or_else(|| format!("auth task {task_id} not found"))?;
    Ok(auth_task_status_response(task_id, snapshot))
}

#[tauri::command]
pub(crate) fn vault_lock(app: AppHandle) -> Result<VaultStatus, String> {
    let status: SessionStatus = agent_request_no_unlock(
        &app,
        AgentRequest::SessionLock {
            reason: LockReason::Manual,
        },
    )?;
    Ok(VaultStatus {
        exists: status.exists,
        locked: status.locked,
    })
}

#[tauri::command]
pub(crate) async fn vault_change_password(
    app: AppHandle,
    request: ChangePasswordRequest,
) -> Result<(), String> {
    let _: serde_json::Value = agent_request(
        &app,
        AgentRequest::VaultChangePassword {
            new_password: request.new_password,
        },
    )?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn vault_rotate(app: AppHandle) -> Result<(), String> {
    let _: serde_json::Value = agent_request(
        &app,
        AgentRequest::VaultRotate {
            reason: "desktop.rotate".to_string(),
        },
    )?;
    Ok(())
}

#[tauri::command]
pub(crate) fn entries_list(
    app: AppHandle,
    archived: Option<bool>,
) -> Result<Vec<EntrySummary>, String> {
    agent_request(
        &app,
        AgentRequest::EntriesList {
            archived: archived.unwrap_or(false),
        },
    )
}

#[tauri::command]
pub(crate) fn entries_search(app: AppHandle, query: String) -> Result<Vec<EntrySummary>, String> {
    agent_request(&app, AgentRequest::EntriesSearch { query })
}

#[tauri::command]
pub(crate) fn provider_add(app: AppHandle, request: ProviderAddRequest) -> Result<Uuid, String> {
    agent_request(
        &app,
        AgentRequest::ProviderAdd {
            input: provider_add_input(request),
        },
    )
}

#[tauri::command]
pub(crate) fn provider_update(
    app: AppHandle,
    request: ProviderUpdateRequest,
) -> Result<(), String> {
    let id = request.id;
    let _: serde_json::Value = agent_request(
        &app,
        AgentRequest::ProviderUpdate {
            id,
            input: provider_update_input(request),
        },
    )?;
    Ok(())
}

#[tauri::command]
pub(crate) fn provider_archive(app: AppHandle, id: Uuid) -> Result<(), String> {
    let _: serde_json::Value = agent_request(&app, AgentRequest::ProviderArchive { id })?;
    Ok(())
}

#[tauri::command]
pub(crate) fn provider_restore(app: AppHandle, id: Uuid) -> Result<(), String> {
    let _: serde_json::Value = agent_request(&app, AgentRequest::ProviderRestore { id })?;
    Ok(())
}

#[tauri::command]
pub(crate) fn provider_delete(app: AppHandle, id: Uuid) -> Result<(), String> {
    let _: serde_json::Value = agent_request(&app, AgentRequest::ProviderDelete { id })?;
    Ok(())
}

#[tauri::command]
pub(crate) fn secret_reveal_field(
    app: AppHandle,
    id: Uuid,
    field: String,
) -> Result<String, String> {
    let secret: SecretValue = agent_request(&app, AgentRequest::SecretRevealField { id, field })?;
    Ok(secret.secret.into_inner())
}

#[tauri::command]
pub(crate) fn secret_add(
    app: AppHandle,
    id: Uuid,
    label: String,
    api_key: SensitiveString,
) -> Result<String, String> {
    agent_request(
        &app,
        AgentRequest::SecretAdd {
            id,
            label,
            secret: api_key,
        },
    )
}

#[tauri::command]
pub(crate) fn secret_remove(app: AppHandle, id: Uuid, label: String) -> Result<(), String> {
    let _: serde_json::Value = agent_request(&app, AgentRequest::SecretRemove { id, label })?;
    Ok(())
}

#[tauri::command]
pub(crate) fn devices_list(app: AppHandle) -> Result<Vec<DeviceRecord>, String> {
    agent_request(&app, AgentRequest::DevicesList)
}

#[tauri::command]
pub(crate) fn device_revoke(app: AppHandle, id: Uuid) -> Result<(), String> {
    let _: serde_json::Value = agent_request(&app, AgentRequest::DeviceRevoke { id })?;
    Ok(())
}

#[tauri::command]
pub(crate) fn provider_probe(
    app: AppHandle,
    id: Uuid,
    timeout_seconds: Option<u64>,
) -> Result<ProbeResult, String> {
    let result: AgentProbeResult = agent_request(
        &app,
        AgentRequest::ProviderProbe {
            id,
            timeout_seconds: timeout_seconds.unwrap_or(15),
        },
    )?;
    Ok(ProbeResult {
        ok: result.ok,
        provider_id: result.provider_id,
        interface_type: result.interface_type,
        status: result.status,
        endpoint: result.endpoint,
        model_count: result.model_count,
        error: result.error,
    })
}

#[tauri::command]
pub(crate) fn tool_config_preview(
    app: AppHandle,
    request: ToolConfigRequest,
) -> Result<ToolConfigPreviewResponse, String> {
    let response: AgentToolConfigPreviewResponse = agent_request(
        &app,
        AgentRequest::ToolConfigPreview {
            request: into_agent_tool_config_request(request),
        },
    )?;
    Ok(from_agent_tool_config_preview(response))
}

#[tauri::command]
pub(crate) fn tool_config_apply(
    app: AppHandle,
    request: ToolConfigRequest,
) -> Result<ToolConfigApplyResponse, String> {
    let response: AgentToolConfigApplyResponse = agent_request(
        &app,
        AgentRequest::ToolConfigApply {
            request: into_agent_tool_config_request(request),
        },
    )?;
    Ok(from_agent_tool_config_apply(response))
}

#[tauri::command]
pub(crate) fn vault_export_encrypted(
    app: AppHandle,
    request: VaultExportRequest,
) -> Result<(), String> {
    let _: serde_json::Value = agent_request(
        &app,
        AgentRequest::VaultExport {
            output: request.output,
            export_password: request.export_password,
        },
    )?;
    Ok(())
}

#[tauri::command]
pub(crate) fn vault_import_encrypted(
    app: AppHandle,
    request: VaultImportRequest,
) -> Result<(), String> {
    let _: serde_json::Value = agent_request_no_unlock(
        &app,
        AgentRequest::VaultImport {
            input: request.input,
            export_password: request.export_password,
        },
    )?;
    Ok(())
}

#[tauri::command]
pub(crate) fn sync_local(app: AppHandle, request: SyncLocalRequest) -> Result<SyncReport, String> {
    agent_request(&app, AgentRequest::SyncLocal { dir: request.dir })
}

#[tauri::command]
pub(crate) fn sync_cloud(app: AppHandle, request: SyncCloudRequest) -> Result<SyncReport, String> {
    agent_request(
        &app,
        AgentRequest::SyncCloud {
            provider: into_agent_cloud_sync_provider(request.provider),
        },
    )
}

#[tauri::command]
pub(crate) fn sync_webdav_remote(
    app: AppHandle,
    request: SyncWebDavRequest,
) -> Result<SyncReport, String> {
    agent_request(
        &app,
        AgentRequest::SyncWebDav {
            url: request.url,
            username: request.username,
            password: request.password,
        },
    )
}

#[tauri::command]
pub(crate) fn sync_conflicts(
    app: AppHandle,
    request: SyncConflictsRequest,
) -> Result<Vec<SyncConflictResponse>, String> {
    let responses: Vec<AgentSyncConflictResponse> = agent_request(
        &app,
        AgentRequest::SyncConflicts {
            dir: request.dir,
            provider: request.provider.map(into_agent_cloud_sync_provider),
        },
    )?;
    Ok(responses
        .into_iter()
        .map(from_agent_sync_conflict_response)
        .collect())
}

#[tauri::command]
pub(crate) fn sync_accept_conflict(
    app: AppHandle,
    request: SyncConflictActionRequest,
) -> Result<(), String> {
    let _: serde_json::Value = agent_request(
        &app,
        AgentRequest::SyncAcceptConflict {
            request: into_agent_sync_conflict_request(request),
        },
    )?;
    Ok(())
}

#[tauri::command]
pub(crate) fn sync_discard_conflict(
    app: AppHandle,
    request: SyncConflictActionRequest,
) -> Result<(), String> {
    let _: serde_json::Value = agent_request(
        &app,
        AgentRequest::SyncDiscardConflict {
            request: into_agent_sync_conflict_request(request),
        },
    )?;
    Ok(())
}
