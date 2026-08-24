use aipass_agent_protocol::{AgentRequest, ProxyStatus, SessionStatus};
use aipass_storage::atomic_write_bytes;
use base64::Engine;
use minisign_verify::{PublicKey, Signature};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};
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

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CachedUpdate {
    channel: String,
    version: String,
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

fn endpoint_for_channel(channel: &str) -> Result<Url, String> {
    let endpoint = match channel {
        "official" => OFFICIAL_ENDPOINT,
        "beta" => BETA_ENDPOINT,
        other => return Err(format!("Unknown update channel: {other}")),
    };
    Url::parse(endpoint).map_err(|err| err.to_string())
}

fn updater_for_channel(
    app: &AppHandle,
    channel: &str,
) -> Result<tauri_plugin_updater::Updater, String> {
    let endpoint = endpoint_for_channel(channel)?;
    app.updater_builder()
        .endpoints(vec![endpoint])
        .and_then(|builder| builder.build())
        .map_err(|err| err.to_string())
}

fn update_cache_paths(app: &AppHandle) -> Result<(PathBuf, PathBuf), String> {
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|err| err.to_string())?
        .join("updates");
    Ok((dir.join("package"), dir.join("metadata.json")))
}

fn read_cached_update(app: &AppHandle) -> Result<Option<(CachedUpdate, Vec<u8>)>, String> {
    let (package_path, metadata_path) = update_cache_paths(app)?;
    let package_exists = package_path.is_file();
    let metadata_exists = metadata_path.is_file();
    if !package_exists || !metadata_exists {
        if package_exists || metadata_exists {
            clear_cached_update(app);
        }
        return Ok(None);
    }
    let metadata: CachedUpdate = match fs::read(&metadata_path)
        .map_err(|err| err.to_string())
        .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|err| err.to_string()))
    {
        Ok(metadata) => metadata,
        Err(_) => {
            clear_cached_update(app);
            return Ok(None);
        }
    };
    let package = match fs::read(&package_path) {
        Ok(package) if !package.is_empty() => package,
        _ => {
            clear_cached_update(app);
            return Ok(None);
        }
    };
    Ok(Some((metadata, package)))
}

fn clear_cached_update(app: &AppHandle) {
    if let Ok((package_path, metadata_path)) = update_cache_paths(app) {
        let _ = fs::remove_file(package_path);
        let _ = fs::remove_file(metadata_path);
    }
}

async fn download_and_cache_update(
    app: &AppHandle,
    channel: &str,
    update: &tauri_plugin_updater::Update,
) -> Result<(), String> {
    emit_update_progress(app, "downloading", 0, None);
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
    let (package_path, metadata_path) = update_cache_paths(app)?;
    atomic_write_bytes(&package_path, &package).map_err(|err| err.to_string())?;
    let metadata = CachedUpdate {
        channel: channel.to_string(),
        version: update.version.clone(),
    };
    atomic_write_bytes(
        &metadata_path,
        &serde_json::to_vec_pretty(&metadata).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;
    Ok(())
}

fn verify_cached_package(
    app: &AppHandle,
    update: &tauri_plugin_updater::Update,
    package: &[u8],
) -> Result<(), String> {
    let pubkey = app
        .config()
        .plugins
        .0
        .get("updater")
        .and_then(|config| config.get("pubkey"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "Updater public key is missing".to_string())?;
    let public_key = base64::engine::general_purpose::STANDARD
        .decode(pubkey)
        .map_err(|err| err.to_string())?;
    let public_key = std::str::from_utf8(&public_key).map_err(|err| err.to_string())?;
    let public_key = PublicKey::decode(public_key).map_err(|err| err.to_string())?;
    let signature = base64::engine::general_purpose::STANDARD
        .decode(&update.signature)
        .map_err(|err| err.to_string())?;
    let signature = std::str::from_utf8(&signature).map_err(|err| err.to_string())?;
    let signature = Signature::decode(signature).map_err(|err| err.to_string())?;
    public_key
        .verify(package, &signature, true)
        .map_err(|err| format!("cached update signature verification failed: {err}"))
}

async fn cached_or_downloaded_package(
    app: &AppHandle,
    channel: &str,
    update: &tauri_plugin_updater::Update,
) -> Result<Vec<u8>, String> {
    if let Some((cached, package)) = read_cached_update(app)? {
        if cached.channel == channel
            && cached.version == update.version
            && verify_cached_package(app, update, &package).is_ok()
        {
            return Ok(package);
        }
        clear_cached_update(app);
    }
    download_and_cache_update(app, channel, update).await?;
    read_cached_update(app)?
        .map(|(_, package)| package)
        .ok_or_else(|| "Update package cache is unavailable".to_string())
}

#[tauri::command]
pub(crate) async fn check_for_updates(
    app: AppHandle,
    channel: String,
) -> Result<UpdateCheckResult, String> {
    let current_version = app.package_info().version.to_string();

    let updater = match updater_for_channel(&app, &channel) {
        Ok(updater) => updater,
        Err(err) => {
            return Ok(UpdateCheckResult::unavailable(current_version, Some(err)));
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
pub(crate) async fn download_update(app: AppHandle, channel: String) -> Result<String, String> {
    let updater = updater_for_channel(&app, &channel)?;
    let update = updater
        .check()
        .await
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "No update available".to_string())?;
    let _ = cached_or_downloaded_package(&app, &channel, &update).await?;
    Ok(update.version)
}

async fn install_cached_update(app: &AppHandle, cached: CachedUpdate) -> Result<bool, String> {
    let current_version = app.package_info().version.clone();
    if cached
        .version
        .parse::<semver::Version>()
        .map(|version| version <= current_version)
        .unwrap_or(false)
    {
        clear_cached_update(app);
        return Ok(false);
    }
    let updater = updater_for_channel(app, &cached.channel)?;
    let update = updater.check().await.map_err(|err| err.to_string())?;
    let Some(update) = update else {
        clear_cached_update(app);
        return Ok(false);
    };
    if update.version != cached.version {
        clear_cached_update(app);
        return Ok(false);
    }
    let package = cached_or_downloaded_package(app, &cached.channel, &update).await?;
    install_verified_update(app, update, package)
}

fn install_verified_update(
    app: &AppHandle,
    update: tauri_plugin_updater::Update,
    package: Vec<u8>,
) -> Result<bool, String> {
    let downloaded_bytes = package.len() as u64;
    emit_update_progress(app, "installing", downloaded_bytes, None);
    if let Err(err) = stop_runtime_processes(app) {
        restore_agent_after_update_failure(app);
        return Err(err);
    }
    if let Err(err) = update.install(package) {
        restore_agent_after_update_failure(app);
        return Err(err.to_string());
    }
    clear_cached_update(app);
    // The updater only swaps the bundle on disk; relaunch so the new
    // version actually runs ("Install & restart" in the UI promises this).
    crate::ALLOW_PROCESS_EXIT.store(true, std::sync::atomic::Ordering::SeqCst);
    app.restart()
}

fn restore_agent_after_update_failure(app: &AppHandle) {
    let Ok(client) = crate::agent_client(app) else {
        return;
    };
    let _ = crate::ensure_agent_running_for_desktop(&client);
}

#[tauri::command]
pub(crate) async fn install_pending_update(
    app: AppHandle,
    channel: String,
) -> Result<bool, String> {
    let Some((cached, _)) = read_cached_update(&app)? else {
        return Ok(false);
    };
    if cached.channel != channel {
        clear_cached_update(&app);
        return Ok(false);
    }
    install_cached_update(&app, cached).await
}

#[tauri::command]
pub(crate) fn clear_pending_update(app: AppHandle) {
    clear_cached_update(&app);
}

#[tauri::command]
pub(crate) async fn install_update(app: AppHandle, channel: String) -> Result<(), String> {
    let updater = updater_for_channel(&app, &channel)?;
    let update = updater
        .check()
        .await
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "No update available".to_string())?;
    let package = cached_or_downloaded_package(&app, &channel, &update).await?;
    let _ = install_verified_update(&app, update, package)?;
    Ok(())
}

fn stop_runtime_processes(app: &AppHandle) -> Result<(), String> {
    let client = crate::agent_client(app)?;
    if client
        .request::<SessionStatus>(&AgentRequest::SessionStatus)
        .is_err()
    {
        #[cfg(target_os = "macos")]
        {
            // An unavailable agent may still be inside its LaunchAgent
            // supervisor's restart window. Stop the supervisor as part of the
            // update transaction so it cannot relaunch the old binary while
            // the bundle is being replaced.
            let _ = aipass_agent::suspend_agent_autostart(&client.config.vault_dir);
            let _ = crate::stop_tray_autostart_for_current_desktop(&client.config.vault_dir);
        }
        #[cfg(target_os = "macos")]
        crate::tray_swift::shutdown();
        return Ok(());
    }
    let _ = client.request::<ProxyStatus>(&AgentRequest::ServerStop);
    client
        .shutdown()
        .map_err(|err| format!("failed to stop AIPass agent before update: {err}"))?;

    #[cfg(target_os = "macos")]
    {
        // AgentShutdown stops the child, but the resident LaunchAgent would
        // otherwise bring it back before the updater replaces the bundle.
        let _ = aipass_agent::suspend_agent_autostart(&client.config.vault_dir);
        let _ = crate::stop_tray_autostart_for_current_desktop(&client.config.vault_dir);
    }

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

#[cfg(test)]
mod tests {
    use super::endpoint_for_channel;

    #[test]
    fn update_channels_use_the_expected_feeds() {
        assert_eq!(
            endpoint_for_channel("official").unwrap().as_str(),
            "https://github.com/backrunner/aipass/releases/latest/download/latest.json"
        );
        assert_eq!(
            endpoint_for_channel("beta").unwrap().as_str(),
            "https://aipass.alkinum.io/api/updates/beta/latest.json"
        );
        assert!(endpoint_for_channel("nightly").is_err());
    }
}
