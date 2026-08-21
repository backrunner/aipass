use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;
use url::Url;

const OFFICIAL_ENDPOINT: &str =
    "https://github.com/backrunner/aipass/releases/latest/download/latest.json";
const BETA_ENDPOINT: &str = "https://github.com/backrunner/aipass/releases/download/beta/latest.json";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateCheckResult {
    pub current_version: String,
    pub available: bool,
    pub latest_version: Option<String>,
    pub notes: Option<String>,
    pub error: Option<String>,
}

impl UpdateCheckResult {
    fn unavailable(current_version: String, error: Option<String>) -> Self {
        Self {
            current_version,
            available: false,
            latest_version: None,
            notes: None,
            error,
        }
    }
}

#[tauri::command]
pub(crate) async fn check_for_updates(
    app: AppHandle,
    channel: String,
) -> Result<UpdateCheckResult, String> {
    let current_version = app.package_info().version.to_string();

    let endpoint = match channel.as_str() {
        "official" => OFFICIAL_ENDPOINT,
        "beta" => BETA_ENDPOINT,
        other => {
            return Ok(UpdateCheckResult::unavailable(
                current_version,
                Some(format!("Unknown update channel: {other}")),
            ));
        }
    };

    let endpoint = match Url::parse(endpoint) {
        Ok(endpoint) => endpoint,
        Err(err) => {
            return Ok(UpdateCheckResult::unavailable(
                current_version,
                Some(err.to_string()),
            ));
        }
    };

    let updater = match app
        .updater_builder()
        .endpoints(vec![endpoint])
        .and_then(|builder| builder.build())
    {
        Ok(updater) => updater,
        Err(err) => {
            return Ok(UpdateCheckResult::unavailable(
                current_version,
                Some(err.to_string()),
            ));
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
        Ok(None) => Ok(UpdateCheckResult::unavailable(current_version, None)),
        Err(err) => Ok(UpdateCheckResult::unavailable(
            current_version,
            Some(err.to_string()),
        )),
    }
}

#[tauri::command]
pub(crate) async fn install_update(app: AppHandle, channel: String) -> Result<(), String> {
    let endpoint = match channel.as_str() {
        "official" => OFFICIAL_ENDPOINT,
        "beta" => BETA_ENDPOINT,
        other => return Err(format!("Unknown update channel: {other}")),
    };
    let endpoint = Url::parse(endpoint).map_err(|err| err.to_string())?;
    let updater = app
        .updater_builder()
        .endpoints(vec![endpoint])
        .and_then(|builder| builder.build())
        .map_err(|err| err.to_string())?;
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
