use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateCheckResult {
    pub current_version: String,
    pub available: bool,
    pub latest_version: Option<String>,
    pub notes: Option<String>,
    pub error: Option<String>,
}

#[tauri::command]
pub(crate) async fn check_for_updates(app: AppHandle) -> Result<UpdateCheckResult, String> {
    let current_version = app.package_info().version.to_string();

    let updater = match app.updater() {
        Ok(updater) => updater,
        Err(err) => {
            return Ok(UpdateCheckResult {
                current_version,
                available: false,
                latest_version: None,
                notes: None,
                error: Some(err.to_string()),
            });
        }
    };

    match updater.check().await {
        Ok(Some(update)) => Ok(UpdateCheckResult {
            current_version,
            available: true,
            latest_version: Some(update.version.clone()),
            notes: update.body.clone(),
            error: None,
        }),
        Ok(None) => Ok(UpdateCheckResult {
            current_version,
            available: false,
            latest_version: None,
            notes: None,
            error: None,
        }),
        Err(err) => Ok(UpdateCheckResult {
            current_version,
            available: false,
            latest_version: None,
            notes: None,
            error: Some(err.to_string()),
        }),
    }
}

#[tauri::command]
pub(crate) async fn install_update(app: AppHandle) -> Result<(), String> {
    let updater = app.updater().map_err(|err| err.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "No update available".to_string())?;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|err| err.to_string())?;
    Ok(())
}
