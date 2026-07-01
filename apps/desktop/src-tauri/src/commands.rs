use crate::auth_tasks::{
    auth_task_status_response, finish_vault_create_task, finish_vault_unlock_task,
    prune_auth_tasks, set_auth_task_state, VaultAuthTaskStartResponse, VaultAuthTaskState,
    VaultAuthTaskStatusRequest, VaultAuthTaskStatusResponse,
};
use crate::models::{
    from_agent_sync_conflict_response, from_agent_sync_settings, from_agent_tool_config_apply,
    from_agent_tool_config_preview, into_agent_cloud_sync_provider,
    into_agent_sync_conflict_request, into_agent_sync_settings_update,
    into_agent_tool_config_request, AppPreferences, BrowserExtensionInstallResult,
    BrowserExtensionStatus, ChangePasswordRequest, CreateVaultRequest, NativeHostRepairRequest,
    NativeHostStatus, ProbeResult, ProviderAddRequest, ProviderUpdateRequest, RecoveryVaultRequest,
    SavePreferencesRequest, SaveSyncSettingsRequest, SyncCloudRequest, SyncConflictActionRequest,
    SyncConflictResponse, SyncConflictsRequest, SyncLocalRequest, SyncSettings, SyncWebDavRequest,
    ToolConfigApplyResponse, ToolConfigPreviewResponse, ToolConfigRequest, UnlockVaultRequest,
    VaultExportRequest, VaultImportRequest, VaultStatus,
};
use aipass_agent_protocol::{
    AgentRequest, LockReason, ProbeResult as AgentProbeResult, SecretValue, SensitiveString,
    SessionPolicy, SessionStatus, SessionUnlockMode,
    SyncConflictResponse as AgentSyncConflictResponse, SyncSettings as AgentSyncSettings,
    ToolConfigApplyResponse as AgentToolConfigApplyResponse,
    ToolConfigPreviewResponse as AgentToolConfigPreviewResponse,
};
use aipass_sync::SyncReport;
use aipass_vault::{DeviceRecord, EntrySummary};
use serde::de::DeserializeOwned;
use tauri::{AppHandle, State};
use uuid::Uuid;

use crate::{
    agent_request, agent_request_no_unlock, agent_status, browser_extension_status_snapshot,
    install_browser_extension, load_preferences, native_host_status_snapshot, provider_add_input,
    provider_update_input, repair_native_host_manifest, run_blocking, save_preferences, AppState,
};

async fn agent_request_async<T: DeserializeOwned + Send + 'static>(
    app: AppHandle,
    request: AgentRequest,
) -> Result<T, String> {
    run_blocking(move || agent_request(&app, request)).await
}

async fn agent_request_no_unlock_async<T: DeserializeOwned + Send + 'static>(
    app: AppHandle,
    request: AgentRequest,
) -> Result<T, String> {
    run_blocking(move || agent_request_no_unlock(&app, request)).await
}

#[tauri::command]
pub(crate) fn window_target() -> Option<String> {
    std::env::var("AIPASS_WINDOW_TARGET")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| matches!(value.as_str(), "main" | "unlock" | "quick-access" | "tray"))
}

#[tauri::command]
pub(crate) async fn vault_status(app: AppHandle) -> Result<VaultStatus, String> {
    let status = run_blocking(move || agent_status(&app)).await?;
    Ok(VaultStatus {
        exists: status.exists,
        locked: status.locked,
    })
}

#[tauri::command]
pub(crate) async fn session_touch(app: AppHandle) -> Result<VaultStatus, String> {
    let status: SessionStatus =
        agent_request_no_unlock_async(app, AgentRequest::SessionTouch).await?;
    Ok(VaultStatus {
        exists: status.exists,
        locked: status.locked,
    })
}

#[tauri::command]
pub(crate) async fn preferences_load(app: AppHandle) -> Result<AppPreferences, String> {
    run_blocking(move || {
        let local = load_preferences(&app)?;
        let policy = agent_request_no_unlock::<SessionPolicy>(&app, AgentRequest::SessionPolicyGet)
            .unwrap_or_default();
        Ok(AppPreferences {
            auto_lock_minutes: policy.idle_lock_minutes,
            clipboard_clear_seconds: local.clipboard_clear_seconds,
            lock_on_sleep: policy.lock_on_sleep,
            lock_on_screen_lock: policy.lock_on_screen_lock,
            persist_unlock: policy.persist_unlock,
            theme: local.theme,
            locale: local.locale,
        })
    })
    .await
}

#[tauri::command]
pub(crate) async fn preferences_save(
    app: AppHandle,
    request: SavePreferencesRequest,
) -> Result<AppPreferences, String> {
    run_blocking(move || {
        let current_policy =
            agent_request_no_unlock::<SessionPolicy>(&app, AgentRequest::SessionPolicyGet)
                .unwrap_or_default();
        let stored = load_preferences(&app).unwrap_or_default();
        let preferences = AppPreferences {
            auto_lock_minutes: request.auto_lock_minutes.min(240),
            clipboard_clear_seconds: request.clipboard_clear_seconds.min(600),
            lock_on_sleep: request
                .lock_on_sleep
                .unwrap_or(current_policy.lock_on_sleep),
            lock_on_screen_lock: request
                .lock_on_screen_lock
                .unwrap_or(current_policy.lock_on_screen_lock),
            persist_unlock: request
                .persist_unlock
                .unwrap_or(current_policy.persist_unlock),
            theme: request.theme.unwrap_or(stored.theme),
            locale: request.locale.unwrap_or(stored.locale),
        };
        save_preferences(&app, &preferences)?;
        let _: SessionPolicy = agent_request_no_unlock(
            &app,
            AgentRequest::SessionPolicySet {
                policy: SessionPolicy {
                    idle_lock_minutes: preferences.auto_lock_minutes,
                    lock_on_sleep: preferences.lock_on_sleep,
                    lock_on_screen_lock: preferences.lock_on_screen_lock,
                    persist_unlock: preferences.persist_unlock,
                },
            },
        )?;
        Ok(preferences)
    })
    .await
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
pub(crate) async fn vault_reset(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<VaultAuthTaskStartResponse, String> {
    let task_id = Uuid::new_v4();
    let app_handle = app.clone();
    let request_app = app.clone();
    let auth_tasks = state.auth_tasks.clone();
    prune_auth_tasks(&auth_tasks);
    set_auth_task_state(
        &auth_tasks,
        task_id,
        VaultAuthTaskState::Pending {
            message: "Resetting vault".to_string(),
        },
    )?;
    tauri::async_runtime::spawn(async move {
        let result =
            run_blocking(move || agent_request_no_unlock(&request_app, AgentRequest::VaultReset))
                .await;
        finish_vault_unlock_task(app_handle, auth_tasks, task_id, result);
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
pub(crate) async fn vault_lock(app: AppHandle) -> Result<VaultStatus, String> {
    let status: SessionStatus = agent_request_no_unlock_async(
        app,
        AgentRequest::SessionLock {
            reason: LockReason::Manual,
        },
    )
    .await?;
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
    let _: serde_json::Value = agent_request_async(
        app,
        AgentRequest::VaultChangePassword {
            new_password: request.new_password,
        },
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn vault_rotate(app: AppHandle) -> Result<(), String> {
    let _: serde_json::Value = agent_request_async(
        app,
        AgentRequest::VaultRotate {
            reason: "desktop.rotate".to_string(),
        },
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn entries_list(
    app: AppHandle,
    archived: Option<bool>,
) -> Result<Vec<EntrySummary>, String> {
    agent_request_async(
        app,
        AgentRequest::EntriesList {
            archived: archived.unwrap_or(false),
        },
    )
    .await
}

#[tauri::command]
pub(crate) async fn entries_search(
    app: AppHandle,
    query: String,
) -> Result<Vec<EntrySummary>, String> {
    agent_request_async(app, AgentRequest::EntriesSearch { query }).await
}

#[tauri::command]
pub(crate) async fn provider_add(
    app: AppHandle,
    request: ProviderAddRequest,
) -> Result<Uuid, String> {
    agent_request_async(
        app,
        AgentRequest::ProviderAdd {
            input: provider_add_input(request),
        },
    )
    .await
}

#[tauri::command]
pub(crate) async fn provider_update(
    app: AppHandle,
    request: ProviderUpdateRequest,
) -> Result<(), String> {
    let id = request.id;
    let _: serde_json::Value = agent_request_async(
        app,
        AgentRequest::ProviderUpdate {
            id,
            input: provider_update_input(request),
        },
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn provider_archive(app: AppHandle, id: Uuid) -> Result<(), String> {
    let _: serde_json::Value =
        agent_request_async(app, AgentRequest::ProviderArchive { id }).await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn provider_restore(app: AppHandle, id: Uuid) -> Result<(), String> {
    let _: serde_json::Value =
        agent_request_async(app, AgentRequest::ProviderRestore { id }).await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn provider_trash(app: AppHandle, id: Uuid) -> Result<(), String> {
    let _: serde_json::Value = agent_request_async(app, AgentRequest::ProviderTrash { id }).await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn provider_delete(app: AppHandle, id: Uuid) -> Result<(), String> {
    let _: serde_json::Value =
        agent_request_async(app, AgentRequest::ProviderDelete { id }).await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn entries_trash_list(app: AppHandle) -> Result<Vec<EntrySummary>, String> {
    agent_request_async(app, AgentRequest::EntriesTrash).await
}

#[tauri::command]
pub(crate) async fn trash_purge_expired(app: AppHandle) -> Result<serde_json::Value, String> {
    agent_request_async(app, AgentRequest::TrashPurgeExpired).await
}

#[tauri::command]
pub(crate) async fn trash_empty(app: AppHandle) -> Result<serde_json::Value, String> {
    agent_request_async(app, AgentRequest::TrashEmpty).await
}

#[tauri::command]
pub(crate) async fn secret_reveal_field(
    app: AppHandle,
    id: Uuid,
    field: String,
) -> Result<String, String> {
    let secret: SecretValue =
        agent_request_async(app, AgentRequest::SecretRevealField { id, field }).await?;
    Ok(secret.secret.into_inner())
}

#[tauri::command]
pub(crate) async fn secret_add(
    app: AppHandle,
    id: Uuid,
    label: String,
    api_key: SensitiveString,
) -> Result<String, String> {
    agent_request_async(
        app,
        AgentRequest::SecretAdd {
            id,
            label,
            secret: api_key,
        },
    )
    .await
}

#[tauri::command]
pub(crate) async fn secret_remove(app: AppHandle, id: Uuid, label: String) -> Result<(), String> {
    let _: serde_json::Value =
        agent_request_async(app, AgentRequest::SecretRemove { id, label }).await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn devices_list(app: AppHandle) -> Result<Vec<DeviceRecord>, String> {
    agent_request_async(app, AgentRequest::DevicesList).await
}

#[tauri::command]
pub(crate) async fn device_revoke(app: AppHandle, id: Uuid) -> Result<(), String> {
    let _: serde_json::Value = agent_request_async(app, AgentRequest::DeviceRevoke { id }).await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn provider_probe(
    app: AppHandle,
    id: Uuid,
    timeout_seconds: Option<u64>,
) -> Result<ProbeResult, String> {
    let result: AgentProbeResult = agent_request_async(
        app,
        AgentRequest::ProviderProbe {
            id,
            timeout_seconds: timeout_seconds.unwrap_or(15),
        },
    )
    .await?;
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
pub(crate) async fn tool_config_preview(
    app: AppHandle,
    request: ToolConfigRequest,
) -> Result<ToolConfigPreviewResponse, String> {
    let response: AgentToolConfigPreviewResponse = agent_request_async(
        app,
        AgentRequest::ToolConfigPreview {
            request: into_agent_tool_config_request(request),
        },
    )
    .await?;
    Ok(from_agent_tool_config_preview(response))
}

#[tauri::command]
pub(crate) async fn tool_config_apply(
    app: AppHandle,
    request: ToolConfigRequest,
) -> Result<ToolConfigApplyResponse, String> {
    let response: AgentToolConfigApplyResponse = agent_request_async(
        app,
        AgentRequest::ToolConfigApply {
            request: into_agent_tool_config_request(request),
        },
    )
    .await?;
    Ok(from_agent_tool_config_apply(response))
}

#[tauri::command]
pub(crate) async fn native_host_status() -> Result<NativeHostStatus, String> {
    run_blocking(native_host_status_snapshot).await
}

#[tauri::command]
pub(crate) async fn native_host_repair(
    request: NativeHostRepairRequest,
) -> Result<NativeHostStatus, String> {
    run_blocking(move || repair_native_host_manifest(request.extension_ids)).await
}

#[tauri::command]
pub(crate) async fn browser_extension_status(
    app: AppHandle,
) -> Result<BrowserExtensionStatus, String> {
    run_blocking(move || browser_extension_status_snapshot(&app)).await
}

#[tauri::command]
pub(crate) async fn browser_extension_install(
    app: AppHandle,
) -> Result<BrowserExtensionInstallResult, String> {
    run_blocking(move || install_browser_extension(&app)).await
}

#[tauri::command]
pub(crate) async fn vault_export_encrypted(
    app: AppHandle,
    request: VaultExportRequest,
) -> Result<(), String> {
    let _: serde_json::Value = agent_request_async(
        app,
        AgentRequest::VaultExport {
            output: request.output,
            export_password: request.export_password,
        },
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn vault_import_encrypted(
    app: AppHandle,
    request: VaultImportRequest,
) -> Result<(), String> {
    let _: serde_json::Value = agent_request_no_unlock_async(
        app,
        AgentRequest::VaultImport {
            input: request.input,
            export_password: request.export_password,
        },
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn sync_local(
    app: AppHandle,
    request: SyncLocalRequest,
) -> Result<SyncReport, String> {
    agent_request_async(app, AgentRequest::SyncLocal { dir: request.dir }).await
}

#[tauri::command]
pub(crate) async fn sync_settings_load(app: AppHandle) -> Result<SyncSettings, String> {
    let settings: AgentSyncSettings =
        agent_request_no_unlock_async(app, AgentRequest::SyncSettingsGet).await?;
    Ok(from_agent_sync_settings(settings))
}

#[tauri::command]
pub(crate) async fn sync_settings_save(
    app: AppHandle,
    request: SaveSyncSettingsRequest,
) -> Result<SyncSettings, String> {
    let settings: AgentSyncSettings = agent_request_async(
        app,
        AgentRequest::SyncSettingsSet {
            settings: into_agent_sync_settings_update(request),
        },
    )
    .await?;
    Ok(from_agent_sync_settings(settings))
}

#[tauri::command]
pub(crate) async fn sync_run_configured(app: AppHandle) -> Result<SyncReport, String> {
    agent_request_async(app, AgentRequest::SyncConfigured).await
}

#[tauri::command]
pub(crate) async fn sync_cloud(
    app: AppHandle,
    request: SyncCloudRequest,
) -> Result<SyncReport, String> {
    agent_request_async(
        app,
        AgentRequest::SyncCloud {
            provider: into_agent_cloud_sync_provider(request.provider),
        },
    )
    .await
}

#[tauri::command]
pub(crate) async fn sync_webdav_remote(
    app: AppHandle,
    request: SyncWebDavRequest,
) -> Result<SyncReport, String> {
    agent_request_async(
        app,
        AgentRequest::SyncWebDav {
            url: request.url,
            username: request.username,
            password: request.password,
        },
    )
    .await
}

#[tauri::command]
pub(crate) async fn sync_conflicts(
    app: AppHandle,
    request: SyncConflictsRequest,
) -> Result<Vec<SyncConflictResponse>, String> {
    let responses: Vec<AgentSyncConflictResponse> = agent_request_async(
        app,
        AgentRequest::SyncConflicts {
            dir: request.dir,
            provider: request.provider.map(into_agent_cloud_sync_provider),
        },
    )
    .await?;
    Ok(responses
        .into_iter()
        .map(from_agent_sync_conflict_response)
        .collect())
}

#[tauri::command]
pub(crate) async fn sync_accept_conflict(
    app: AppHandle,
    request: SyncConflictActionRequest,
) -> Result<(), String> {
    let _: serde_json::Value = agent_request_async(
        app,
        AgentRequest::SyncAcceptConflict {
            request: into_agent_sync_conflict_request(request),
        },
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn sync_discard_conflict(
    app: AppHandle,
    request: SyncConflictActionRequest,
) -> Result<(), String> {
    let _: serde_json::Value = agent_request_async(
        app,
        AgentRequest::SyncDiscardConflict {
            request: into_agent_sync_conflict_request(request),
        },
    )
    .await?;
    Ok(())
}
