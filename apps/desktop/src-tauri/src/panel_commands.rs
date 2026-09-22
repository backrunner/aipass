use aipass_agent_protocol::{
    AgentRequest, ControlPanelAccessCode, ControlPanelCertificate, ControlPanelSettings,
    ControlPanelStatus,
};
use tauri::{AppHandle, Emitter};

#[tauri::command]
pub(crate) async fn control_panel_status(app: AppHandle) -> Result<ControlPanelStatus, String> {
    crate::agent_request_no_unlock_async(app, AgentRequest::ControlPanelStatus).await
}

#[tauri::command]
pub(crate) async fn control_panel_configure(
    app: AppHandle,
    settings: ControlPanelSettings,
    certificate: Option<ControlPanelCertificate>,
    regenerate_certificate: bool,
) -> Result<ControlPanelStatus, String> {
    let result = crate::agent_request_async(
        app.clone(),
        AgentRequest::ControlPanelConfigure {
            settings,
            certificate,
            regenerate_certificate,
        },
    )
    .await;
    let _ = app.emit(crate::tray::REFRESH_PROXY_TRAY_EVENT, ());
    result
}

#[tauri::command]
pub(crate) async fn control_panel_stop(app: AppHandle) -> Result<ControlPanelStatus, String> {
    let result =
        crate::agent_request_no_unlock_async(app.clone(), AgentRequest::ControlPanelStop).await;
    let _ = app.emit(crate::tray::REFRESH_PROXY_TRAY_EVENT, ());
    result
}

#[tauri::command]
pub(crate) async fn control_panel_rotate_access_code(
    app: AppHandle,
    allow_remote_unlock: bool,
) -> Result<ControlPanelAccessCode, String> {
    crate::agent_request_async(
        app,
        AgentRequest::ControlPanelRotateAccessCode {
            allow_remote_unlock,
        },
    )
    .await
}

#[tauri::command]
pub(crate) async fn control_panel_disable_remote_unlock(
    app: AppHandle,
) -> Result<ControlPanelStatus, String> {
    crate::agent_request_no_unlock_async(app, AgentRequest::ControlPanelDisableRemoteUnlock).await
}

pub(crate) fn open_panel(app: &AppHandle) -> Result<(), String> {
    let status: ControlPanelStatus =
        crate::agent_request_no_unlock(app, AgentRequest::ControlPanelStatus)?;
    let url = status
        .url
        .ok_or("Enable the control panel in Settings first.")?;
    crate::oauth_browser::browser_command()
        .arg(url)
        .spawn()
        .map_err(|_| "Could not open the control panel.".to_string())?;
    Ok(())
}

pub(crate) fn copy_panel_address(app: &AppHandle) -> Result<(), String> {
    // Keep clipboard ownership alive on platforms where the contents are served by this process.
    static CLIPBOARD: std::sync::Mutex<Option<arboard::Clipboard>> = std::sync::Mutex::new(None);
    let status: ControlPanelStatus =
        crate::agent_request_no_unlock(app, AgentRequest::ControlPanelStatus)?;
    let url = status.url.ok_or("Control panel is stopped.")?;
    let mut clipboard = CLIPBOARD.lock().map_err(|_| "Clipboard unavailable.")?;
    if clipboard.is_none() {
        *clipboard = Some(arboard::Clipboard::new().map_err(|_| "Clipboard unavailable.")?);
    }
    clipboard
        .as_mut()
        .ok_or("Clipboard unavailable.")?
        .set_text(url)
        .map_err(|_| "Could not copy the panel address.".to_string())
}

#[tauri::command]
pub(crate) async fn control_panel_open(app: AppHandle) -> Result<(), String> {
    crate::run_blocking(move || open_panel(&app)).await
}

#[tauri::command]
pub(crate) async fn control_panel_export_certificate(
    app: AppHandle,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    crate::run_blocking(move || {
        let status: ControlPanelStatus =
            crate::agent_request_no_unlock(&app, AgentRequest::ControlPanelStatus)?;
        let pem = status
            .certificate_pem
            .ok_or("Generate or import a certificate first.")?;
        let Some(file) = app
            .dialog()
            .file()
            .set_file_name("aipass-panel.crt")
            .add_filter("Certificate", &["crt"])
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = file.into_path().map_err(|e| e.to_string())?;
        aipass_storage::atomic_write_bytes(&path, pem.as_bytes()).map_err(|e| e.to_string())?;
        Ok(Some(path.display().to_string()))
    })
    .await
}
