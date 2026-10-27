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
pub(crate) mod control;
#[cfg(target_os = "linux")]
mod linux;

fn permission_error(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem
    )
}

fn cache_io_error(error: std::io::Error) -> String {
    if permission_error(&error) {
        "update_cache_permission_denied".into()
    } else if error.kind() == std::io::ErrorKind::StorageFull {
        "update_disk_full".into()
    } else {
        "update_cache_unavailable".into()
    }
}

fn updater_error(error: tauri_plugin_updater::Error) -> String {
    use tauri_plugin_updater::Error;
    match error {
        Error::Minisign(_) | Error::Base64(_) | Error::SignatureUtf8(_) => {
            "update_signature_invalid".into()
        }
        Error::Reqwest(_) | Error::Network(_) | Error::ReleaseNotFound => {
            "update_network_error".into()
        }
        Error::TargetsNotFound(_) | Error::TargetNotFound(_) => "update_release_unavailable".into(),
        Error::Io(error) => cache_io_error(error),
        Error::InvalidUpdaterFormat | Error::BinaryNotFoundInArchive => {
            "update_package_invalid".into()
        }
        other => other.to_string(),
    }
}

static UPDATE_OPERATION: tauri::async_runtime::Mutex<()> =
    tauri::async_runtime::Mutex::const_new(());

const OFFICIAL_ENDPOINT: &str =
    "https://github.com/backrunner/aipass/releases/latest/download/latest.json";
const BETA_ENDPOINT: &str = "https://aipass.alkinum.io/api/updates/beta/latest.json";
const NIGHTLY_ENDPOINT: &str = "https://aipass.alkinum.io/api/updates/nightly/latest.json";
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
        "nightly" => NIGHTLY_ENDPOINT,
        other => return Err(format!("Unknown update channel: {other}")),
    };
    Url::parse(endpoint).map_err(|err| err.to_string())
}

fn updater_for_channel(
    app: &AppHandle,
    channel: &str,
) -> Result<tauri_plugin_updater::Updater, String> {
    let builder = app
        .updater_builder()
        .timeout(std::time::Duration::from_secs(300));
    // Windows install() hands off to NSIS and exits inside the plugin. Code
    // after install() and Tauri's normal Exit event do not run on that path.
    #[cfg(windows)]
    let builder = builder.on_before_exit(|| {
        std::env::set_var("AIPASS_WINDOW_TARGET", control::restart_target());
        let _ = crate::secret_clipboard::clear_now();
        let _ = crate::logging::log_event("desktop.update.windows_handoff", &[]);
    });
    #[cfg(target_os = "linux")]
    let builder = builder.target(linux::target()?);
    if let Some(endpoint) = crate::runtime_check::endpoint() {
        let reinstall = crate::runtime_check::reinstall_same_version();
        return builder
            .version_comparator(move |current, release| {
                release.version > current || (reinstall && release.version == current)
            })
            .no_proxy()
            .endpoints(vec![endpoint])
            .and_then(|builder| builder.build())
            .map_err(|err| err.to_string());
    }
    let endpoint = endpoint_for_channel(channel)?;
    builder
        .endpoints(vec![endpoint])
        .and_then(|builder| builder.build())
        .map_err(|err| err.to_string())
}

fn update_cache_paths(app: &AppHandle) -> Result<(PathBuf, PathBuf), String> {
    if let Some(dir) = crate::runtime_check::path("cache/updates") {
        return Ok((dir.join("package"), dir.join("metadata.json")));
    }
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|err| err.to_string())?
        .join("updates");
    Ok((dir.join("package"), dir.join("metadata.json")))
}

fn read_cached_update(app: &AppHandle) -> Result<Option<(CachedUpdate, Vec<u8>)>, String> {
    let (package_path, metadata_path) = update_cache_paths(app)?;
    let exists = |path: &std::path::Path| -> Result<bool, String> {
        match fs::metadata(path) {
            Ok(meta) => Ok(meta.is_file()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(cache_io_error(error)),
        }
    };
    let package_exists = exists(&package_path)?;
    let metadata_exists = exists(&metadata_path)?;
    if !package_exists || !metadata_exists {
        if package_exists || metadata_exists {
            try_clear_cached_update(app)?;
        }
        return Ok(None);
    }
    let metadata: CachedUpdate = match fs::read(&metadata_path)
        .map_err(cache_io_error)
        .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|err| err.to_string()))
    {
        Ok(metadata) => metadata,
        Err(error) if error == "update_cache_permission_denied" => return Err(error),
        Err(_) => {
            try_clear_cached_update(app)?;
            return Ok(None);
        }
    };
    let package = match fs::read(&package_path) {
        Ok(package) if !package.is_empty() => package,
        Err(error) if permission_error(&error) => return Err(cache_io_error(error)),
        _ => {
            try_clear_cached_update(app)?;
            return Ok(None);
        }
    };
    Ok(Some((metadata, package)))
}

fn clear_cached_update(app: &AppHandle) {
    let _ = try_clear_cached_update(app);
}

fn try_clear_cached_update(app: &AppHandle) -> Result<(), String> {
    let operation = crate::logging::DesktopOperation::start("desktop.update.clear_cache");
    let result = update_cache_paths(app).and_then(|(package, metadata)| {
        remove_cache_files(&[package, metadata]).map_err(cache_io_error)
    });
    operation.finish(result.is_ok());
    result
}

fn cleanup_old_update_artifacts(app: &AppHandle) -> Result<(), String> {
    let cache_dir = if let Some(dir) = crate::runtime_check::path("cache/updates") {
        dir
    } else {
        app.path()
            .app_cache_dir()
            .map_err(|err| err.to_string())?
            .join("updates")
    };

    if !cache_dir.exists() {
        return Ok(());
    }

    let (current_package, current_metadata) = update_cache_paths(app)?;
    let current_package_name = current_package.file_name();
    let current_metadata_name = current_metadata.file_name();

    // Remove any files in the updates directory that aren't the current package/metadata
    if let Ok(entries) = fs::read_dir(&cache_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let file_name = path.file_name();
            // Keep the current package and metadata files
            if file_name == current_package_name || file_name == current_metadata_name {
                continue;
            }

            // Remove any other files (old packages, temp files, etc.)
            let _ = fs::remove_file(&path);
        }
    }

    Ok(())
}

fn remove_cache_files(paths: &[PathBuf]) -> std::io::Result<()> {
    let mut result = Ok(());
    for path in paths {
        if let Err(err) = fs::remove_file(path) {
            if err.kind() != std::io::ErrorKind::NotFound {
                result = Err(err);
            }
        }
    }
    result
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
        .map_err(updater_error)?;
    let (package_path, metadata_path) = update_cache_paths(app)?;
    atomic_write_bytes(&package_path, &package).map_err(cache_io_error)?;
    let metadata = CachedUpdate {
        channel: channel.to_string(),
        version: update.version.clone(),
    };
    atomic_write_bytes(
        &metadata_path,
        &serde_json::to_vec_pretty(&metadata).map_err(|err| err.to_string())?,
    )
    .map_err(cache_io_error)?;
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
    let _guard = UPDATE_OPERATION.try_lock().map_err(|_| "update_busy")?;
    let operation = crate::logging::DesktopOperation::start("desktop.update.check");
    let result = check_for_updates_inner(app, channel).await;
    operation.finish(result.as_ref().is_ok_and(|value| value.error.is_none()));
    result
}

async fn check_for_updates_inner(
    app: AppHandle,
    channel: String,
) -> Result<UpdateCheckResult, String> {
    let current_version = app.package_info().version.to_string();

    // Clean up old update artifacts before checking for new updates
    let _ = cleanup_old_update_artifacts(&app);

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
            Some(updater_error(err)),
        )),
    }
}

#[tauri::command]
pub(crate) async fn download_update(app: AppHandle, channel: String) -> Result<String, String> {
    let _guard = UPDATE_OPERATION.try_lock().map_err(|_| "update_busy")?;
    crate::logging::log_task(
        "desktop.update.download",
        download_update_inner(app, channel),
    )
    .await
}

async fn download_update_inner(app: AppHandle, channel: String) -> Result<String, String> {
    let updater = updater_for_channel(&app, &channel)?;
    let update = updater
        .check()
        .await
        .map_err(updater_error)?
        .ok_or_else(|| "No update available".to_string())?;
    let _ = cached_or_downloaded_package(&app, &channel, &update).await?;
    Ok(update.version)
}

async fn install_cached_update(app: &AppHandle, cached: CachedUpdate) -> Result<bool, String> {
    // Distro-package installs may request administrator authorization. Never
    // produce an auth dialog merely because a background download was cached.
    #[cfg(target_os = "linux")]
    if linux::kind()? != linux::Kind::AppImage {
        return Ok(false);
    }
    let current_version = app.package_info().version.clone();
    if cached
        .version
        .parse::<semver::Version>()
        .map(|version| {
            version < current_version
                || (version == current_version && !crate::runtime_check::reinstall_same_version())
        })
        .unwrap_or(false)
    {
        clear_cached_update(app);
        return Ok(false);
    }
    let updater = updater_for_channel(app, &cached.channel)?;
    let update = updater.check().await.map_err(updater_error)?;
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
    // Recheck the exact bytes read back from disk, including the initial
    // download path. The plugin's install() does not verify signatures itself.
    verify_cached_package(app, &update, &package).map_err(|_| "update_signature_invalid")?;
    #[cfg(target_os = "linux")]
    let plan = linux::prepare(app, &package)?;
    let downloaded_bytes = package.len() as u64;
    emit_update_progress(app, "installing", downloaded_bytes, None);
    let updating = crate::runtime_lifecycle::RUNTIME.update()?;
    // The verified bytes are already in memory (and the Linux system package
    // has its own private staging file). Remove the expendable download cache
    // before replacing anything: a readable package in a read-only cache must
    // not install successfully and leave a stale pending update behind.
    try_clear_cached_update(app)?;
    // Linux can replace running executables. Authorize and install first so
    // denial/cancellation keeps the unlocked Agent and active proxy untouched.
    #[cfg(target_os = "linux")]
    linux::install(plan, &update, &package)?;
    if let Err(err) = stop_runtime_processes(app) {
        drop(updating);
        restore_agent_after_update_failure(app);
        return Err(err);
    }
    #[cfg(not(target_os = "linux"))]
    if let Err(err) = update.install(package).map_err(updater_error) {
        drop(updating);
        restore_agent_after_update_failure(app);
        return Err(err);
    }
    // A background check may have finished downloading during authorization.
    clear_cached_update(app);
    // Clean up any remaining old artifacts after successful installation
    let _ = cleanup_old_update_artifacts(app);
    crate::runtime_check::installed()?;
    // The updater only swaps the bundle on disk; relaunch so the new
    // version actually runs ("Install & restart" in the UI promises this).
    // Restart retains argv as well as environment. An explicit main target
    // also overrides --login-start, so a login-launched Windows instance
    // shows the updated window after "Install & restart".
    std::env::set_var("AIPASS_WINDOW_TARGET", control::restart_target());
    crate::ALLOW_PROCESS_EXIT.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = crate::logging::log_event("desktop.update.installed_restarting", &[]);
    app.restart()
}

fn restore_agent_after_update_failure(app: &AppHandle) {
    let Ok(client) = crate::agent_client(app) else {
        return;
    };
    let _ = crate::ensure_agent_running_for_desktop(&client);
    #[cfg(target_os = "macos")]
    {
        if crate::runtime_check::active() {
            return;
        }
        if let Ok(desktop_binary) = std::env::current_exe() {
            let _ = crate::ensure_tray_autostart_for_current_desktop(
                &desktop_binary,
                &client.config.vault_dir,
            );
        }
    }
}

#[tauri::command]
pub(crate) async fn install_pending_update(
    app: AppHandle,
    channel: String,
) -> Result<bool, String> {
    let _guard = UPDATE_OPERATION.try_lock().map_err(|_| "update_busy")?;
    crate::logging::log_task(
        "desktop.update.install_pending",
        install_pending_update_inner(app, channel),
    )
    .await
}

async fn install_pending_update_inner(app: AppHandle, channel: String) -> Result<bool, String> {
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
pub(crate) fn clear_pending_update(app: AppHandle) -> Result<(), String> {
    let _guard = UPDATE_OPERATION.try_lock().map_err(|_| "update_busy")?;
    try_clear_cached_update(&app)
}

#[tauri::command]
pub(crate) async fn install_update(app: AppHandle, channel: String) -> Result<(), String> {
    let _guard = UPDATE_OPERATION.try_lock().map_err(|_| "update_busy")?;
    crate::logging::log_task("desktop.update.install", install_update_inner(app, channel)).await
}

async fn install_update_inner(app: AppHandle, channel: String) -> Result<(), String> {
    let updater = updater_for_channel(&app, &channel)?;
    let update = updater
        .check()
        .await
        .map_err(updater_error)?
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
            aipass_agent::suspend_tray_autostart(&client.config.vault_dir)
                .map_err(|err| err.to_string())?;
        }
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
        aipass_agent::suspend_tray_autostart(&client.config.vault_dir)
            .map_err(|err| err.to_string())?;
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

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::endpoint_for_channel;

    #[test]
    fn cache_cleanup_reports_failures_and_still_removes_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let package = dir.path().join("package");
        let metadata = dir.path().join("metadata.json");
        std::fs::create_dir(&package).unwrap();
        std::fs::write(&metadata, "{}").unwrap();
        assert!(super::remove_cache_files(&[package.clone(), metadata.clone()]).is_err());
        assert!(!metadata.exists());
        std::fs::remove_dir(&package).unwrap();
        assert!(super::remove_cache_files(&[package, metadata]).is_ok());
    }

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
        assert_eq!(
            endpoint_for_channel("nightly").unwrap().as_str(),
            "https://aipass.alkinum.io/api/updates/nightly/latest.json"
        );
        assert!(endpoint_for_channel("canary").is_err());
    }

    #[test]
    fn channel_versions_never_downgrade_across_channels() {
        // semver orders prerelease identifiers lexically, so within one base
        // version beta < nightly < stable. The install path rejects any
        // candidate <= the running version, so switching channels can never
        // overwrite the app with an older build regardless of direction.
        let beta: semver::Version = "0.3.0-beta.1".parse().unwrap();
        let nightly: semver::Version = "0.3.0-nightly.20260905".parse().unwrap();
        let stable: semver::Version = "0.3.0".parse().unwrap();
        assert!(beta < nightly && nightly < stable);

        // Nightly builds use a base version above the latest stable, so a
        // nightly install is newer than anything the official feed serves.
        let latest_stable: semver::Version = "0.2.0".parse().unwrap();
        assert!(nightly > latest_stable);
    }
}
