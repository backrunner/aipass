//! Browser extension packaging and native messaging shared by both desktop shells.
use aipass_native_host::{
    load_allowed_extension_ids, native_host_settings_path, native_manifest,
    save_allowed_extension_ids,
};
use aipass_storage::atomic_write_bytes;
use serde::Serialize;
#[cfg(target_os = "windows")]
use std::ffi::OsString;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
};
mod browsers;
mod host;
mod package;
pub use browsers::looks_like_extension_id;
use browsers::*;
use host::*;
pub use host::{native_host_status_snapshot, repair_native_host_manifest};
#[cfg(test)]
use package::*;
pub use package::{
    browser_extension_status_snapshot, install_browser_extension, sync_installed_browser_extensions,
};
fn log_event(event: &str, fields: &[(&str, &str)]) -> Result<(), String> {
    log::info!("{event} {fields:?}");
    Ok(())
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeHostStatus {
    pub browser: String,
    pub browser_label: String,
    pub host_path: PathBuf,
    pub host_exists: bool,
    pub host_usable: bool,
    pub host_error: Option<String>,
    pub manifest_path: PathBuf,
    pub manifest_exists: bool,
    pub settings_path: PathBuf,
    pub allowed_extension_ids: Vec<String>,
    pub allowed_origins: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserExtensionStatus {
    pub browser: String,
    pub detected_browsers: Vec<String>,
    pub chrome_installed: bool,
    pub chrome_path: Option<PathBuf>,
    pub extension_id: String,
    pub discovered_extension_ids: Vec<String>,
    pub extension_version: String,
    pub zip_path: PathBuf,
    pub zip_exists: bool,
    pub extension_installed: bool,
    pub installed_paths: Vec<PathBuf>,
    pub native_host_configured: bool,
    pub native_host: NativeHostStatus,
    pub native_hosts: Vec<NativeHostStatus>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserExtensionInstallResult {
    pub status: BrowserExtensionStatus,
    pub opened_chrome: bool,
    pub opened_package: bool,
}

#[cfg(test)]
mod tests;
