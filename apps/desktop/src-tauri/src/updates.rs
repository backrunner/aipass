use aipass_agent_protocol::{AgentRequest, ProxyStatus, SessionStatus};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;
use url::Url;

const OFFICIAL_ENDPOINT: &str =
    "https://github.com/backrunner/aipass/releases/latest/download/latest.json";
const BETA_ENDPOINT: &str = "https://aipass.alkinum.io/api/updates/beta/latest.json";
pub(crate) const UPDATE_PROGRESS_EVENT: &str = "update-progress";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateProgress {
    pub phase: &'static str,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
}

fn emit_update_progress(
    app: &AppHandle,
    phase: &'static str,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
) {
    let _ = app.emit(
        UPDATE_PROGRESS_EVENT,
        UpdateProgress {
            phase,
            downloaded_bytes,
            total_bytes,
        },
    );
}

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
    emit_update_progress(&app, "downloading", 0, None);
    // Download and verify while the app is still fully operational. Once the
    // bytes are verified, stop every AIPass-owned runtime before replacing any
    // executable so no old agent/proxy process can keep stale code or files
    // open during the upgrade.
    let progress_app = app.clone();
    let package = update
        .download(
            {
                let mut downloaded_bytes = 0_u64;
                move |chunk_length, content_length| {
                    downloaded_bytes = downloaded_bytes.saturating_add(chunk_length as u64);
                    emit_update_progress(
                        &progress_app,
                        "downloading",
                        downloaded_bytes,
                        content_length,
                    );
                }
            },
            || {},
        )
        .await
        .map_err(|err| err.to_string())?;
    let downloaded_bytes = package.len() as u64;
    emit_update_progress(&app, "installing", downloaded_bytes, None);
    stop_runtime_processes(&app)?;
    update.install(package).map_err(|err| err.to_string())?;
    // The updater only swaps the bundle on disk; relaunch so the new
    // version actually runs ("Install & restart" in the UI promises this).
    crate::ALLOW_PROCESS_EXIT.store(true, std::sync::atomic::Ordering::SeqCst);
    app.restart()
}

fn stop_runtime_processes(app: &AppHandle) -> Result<(), String> {
    let client = crate::agent_client(app)?;
    if client
        .request::<SessionStatus>(&AgentRequest::SessionStatus)
        .is_err()
    {
        #[cfg(target_os = "macos")]
        crate::tray_swift::shutdown();
        return Ok(());
    }
    let _ = client.request::<ProxyStatus>(&AgentRequest::ServerStop);
    client
        .shutdown()
        .map_err(|err| format!("failed to stop AIPass agent before update: {err}"))?;

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if client
            .request::<SessionStatus>(&AgentRequest::SessionStatus)
            .is_err()
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    if client
        .request::<SessionStatus>(&AgentRequest::SessionStatus)
        .is_ok()
    {
        return Err("AIPass agent did not exit before update".to_string());
    }

    #[cfg(target_os = "macos")]
    crate::tray_swift::shutdown();
    Ok(())
}
