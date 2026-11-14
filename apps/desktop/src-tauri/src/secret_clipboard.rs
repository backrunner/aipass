//! Tauri confirmation adapter for the shared native clipboard service.
use aipass_agent_protocol::{AgentRequest, SecretValue, SensitiveString};
pub(crate) use aipass_desktop_platform::clipboard::clear_now;
use aipass_desktop_platform::clipboard::CopyPermit;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use uuid::Uuid;

#[tauri::command]
pub(crate) async fn clipboard_clear_sensitive() -> Result<(), String> {
    crate::run_blocking(clear_now).await
}

fn prepare_copy(app: &AppHandle) -> Result<Option<(CopyPermit, u16)>, String> {
    let Some(permit) = CopyPermit::begin()? else {
        return Ok(None);
    };
    let preferences = crate::load_preferences(app)?;
    if preferences.confirm_secret_copy {
        let language = crate::commands::load_language_settings(app, preferences.locale)?;
        let (title, message, copy, cancel) =
            aipass_desktop_platform::clipboard::confirmation(language.resolved_locale);
        let mut dialog = app.dialog().message(message).title(title).buttons(
            MessageDialogButtons::OkCancelCustom(copy.into(), cancel.into()),
        );
        if let Some(window) = app.get_webview_window("main") {
            dialog = dialog.parent(&window);
        }
        if !dialog.blocking_show() {
            return Ok(None);
        }
    }
    Ok(Some((
        permit,
        crate::preferences::normalize_clipboard_seconds(preferences.clipboard_clear_seconds),
    )))
}
#[tauri::command]
pub(crate) async fn secret_copy(app: AppHandle, id: Uuid, field: String) -> Result<bool, String> {
    crate::run_blocking(move || {
        let Some((permit, seconds)) = prepare_copy(&app)? else {
            return Ok(false);
        };
        if !permit.is_current() {
            return Ok(false);
        }
        let secret: SecretValue =
            crate::agent_request(&app, AgentRequest::SecretRevealField { id, field })?;
        permit.write(secret.secret, seconds)
    })
    .await
}
#[tauri::command]
pub(crate) async fn clipboard_write_sensitive(
    app: AppHandle,
    value: SensitiveString,
) -> Result<bool, String> {
    crate::run_blocking(move || {
        let Some((permit, seconds)) = prepare_copy(&app)? else {
            return Ok(false);
        };
        permit.write(value, seconds)
    })
    .await
}
