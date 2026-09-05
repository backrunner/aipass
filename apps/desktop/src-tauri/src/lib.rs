mod auth_tasks;
mod commands;
mod deeplink;
mod logging;
mod models;
#[cfg(target_os = "macos")]
mod self_install;
mod singleton;
mod tray;
#[cfg(target_os = "macos")]
mod tray_swift;
mod updates;

use commands::*;
use updates::{
    check_for_updates, clear_pending_update, download_update, install_pending_update,
    install_update,
};

use crate::auth_tasks::AuthTasks;
use crate::models::{
    AppPreferences, BrowserExtensionInstallResult, BrowserExtensionStatus, NativeHostStatus,
    ProviderAddRequest, ProviderUpdateRequest,
};
use aipass_agent::{AgentClient, AgentClientConfig, AgentCommandError};
use aipass_agent_protocol::{AgentRequest, SessionStatus};
use aipass_native_host::{
    load_allowed_extension_ids, native_host_settings_path, native_manifest,
    save_allowed_extension_ids,
};
use aipass_provider_registry::{provider_kind_for_id, ProviderEndpoint};
use aipass_storage::atomic_write_bytes;
use aipass_vault::{ProviderEntryInput, ProviderEntryUpdateInput};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::{Mutex, MutexGuard};
use std::thread;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, RunEvent, Size};
use tauri_plugin_deep_link::DeepLinkExt;

use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[cfg(test)]
use crate::models::ProbeResult;
#[cfg(test)]
use aipass_provider_registry::{AuthScheme, InterfaceType};
#[cfg(test)]
use aipass_vault::EntrySummary;
#[cfg(test)]
use std::time::Duration;
#[cfg(test)]
use uuid::Uuid;

#[cfg(target_os = "windows")]
use std::ffi::OsString;

#[derive(Default)]
struct AppState {
    auth_tasks: AuthTasks,
    window: Mutex<DesktopWindowState>,
    pending_deep_links: Mutex<Vec<deeplink::PendingDeepLink>>,
}

pub(crate) static ALLOW_PROCESS_EXIT: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
struct DesktopWindowState {
    frontend_ready: bool,
    target: String,
}

const DESKTOP_WINDOW_SIZE_FILE: &str = "window-size.json";
const DESKTOP_WINDOW_SIZE_TOLERANCE: f64 = 0.25;
const MIN_DESKTOP_WINDOW_WIDTH: f64 = 960.0;
const MIN_DESKTOP_WINDOW_HEIGHT: f64 = 640.0;
const DEFAULT_DESKTOP_WINDOW_WIDTH: f64 = 1280.0;
const DEFAULT_DESKTOP_WINDOW_HEIGHT: f64 = 820.0;

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct DesktopWindowSize {
    width: f64,
    height: f64,
}

impl DesktopWindowState {
    fn set_target(&mut self, target: &str) -> bool {
        self.target = normalize_window_target(target).to_string();
        self.frontend_ready
    }

    fn complete_startup(&mut self) -> String {
        self.frontend_ready = true;
        normalize_window_target(&self.target).to_string()
    }
}

impl AppState {
    fn window_state(&self) -> MutexGuard<'_, DesktopWindowState> {
        self.window.lock().unwrap_or_else(|err| err.into_inner())
    }

    pub(crate) fn window_target(&self) -> String {
        let target = self.window_state().target.clone();
        normalize_window_target(&target).to_string()
    }

    pub(crate) fn store_pending_ccswitch_link(&self, link: deeplink::CcSwitchProviderLink) {
        let mut pending = self
            .pending_deep_links
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        pending.push(deeplink::PendingDeepLink::CcSwitch(link));
    }

    pub(crate) fn store_pending_aipass_provider_link(&self, link: deeplink::AipassProviderLink) {
        let mut pending = self
            .pending_deep_links
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        pending.push(deeplink::PendingDeepLink::AipassProvider(link));
    }

    pub(crate) fn store_pending_ccswitch_link_error(
        &self,
        payload: deeplink::CcSwitchLinkErrorPayload,
    ) {
        let mut pending = self
            .pending_deep_links
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        pending.push(deeplink::PendingDeepLink::CcSwitchError(payload));
    }

    pub(crate) fn store_pending_aipass_provider_link_error(
        &self,
        payload: deeplink::AipassProviderLinkErrorPayload,
    ) {
        let mut pending = self
            .pending_deep_links
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        pending.push(deeplink::PendingDeepLink::AipassProviderError(payload));
    }

    pub(crate) fn take_pending_deep_links(&self) -> Vec<deeplink::PendingDeepLink> {
        std::mem::take(
            &mut *self
                .pending_deep_links
                .lock()
                .unwrap_or_else(|err| err.into_inner()),
        )
    }

    pub(crate) fn clear_pending_deep_links(&self) {
        self.pending_deep_links
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clear();
    }

    fn frontend_ready(&self) -> bool {
        self.window_state().frontend_ready
    }
}

fn agent_client(_app: &AppHandle) -> Result<AgentClient, String> {
    let config = if let Some(explicit) = std::env::var_os("AIPASS_VAULT_DIR") {
        AgentClientConfig::for_vault(PathBuf::from(explicit))
    } else {
        AgentClientConfig::default_vault()
    }
    .map_err(|err| err.to_string())?;
    Ok(AgentClient::new(config))
}

pub(crate) fn ensure_agent_running_for_desktop(client: &AgentClient) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        client.ensure_running().map_err(|err| err.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        client
            .ensure_running_for_desktop_companion()
            .map_err(|err| err.to_string())
    }
}

pub(crate) fn install_tray_autostart_for_current_desktop(
    desktop_binary: &Path,
    vault_dir: &Path,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let singleton_socket =
            singleton::current_singleton_socket_path().map_err(|err| err.to_string())?;
        aipass_agent::install_tray_autostart_with_socket(
            desktop_binary,
            vault_dir,
            &singleton_socket,
        )
        .map(|_| ())
        .map_err(|err| err.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        aipass_agent::install_tray_autostart(desktop_binary, vault_dir)
            .map(|_| ())
            .map_err(|err| err.to_string())
    }
}

#[cfg(target_os = "macos")]
fn ensure_tray_autostart_for_current_desktop(
    desktop_binary: &Path,
    vault_dir: &Path,
) -> Result<(), String> {
    let singleton_socket =
        singleton::current_singleton_socket_path().map_err(|err| err.to_string())?;
    aipass_agent::ensure_tray_autostart_with_socket(desktop_binary, vault_dir, &singleton_socket)
        .map(|_| ())
        .map_err(|err| err.to_string())
}

#[cfg(target_os = "macos")]
pub(crate) fn stop_tray_autostart_for_current_desktop(vault_dir: &Path) -> Result<(), String> {
    let singleton_socket =
        singleton::current_singleton_socket_path().map_err(|err| err.to_string())?;
    aipass_agent::stop_tray_autostart_with_socket(vault_dir, &singleton_socket)
        .map(|_| ())
        .map_err(|err| err.to_string())
}

fn agent_request<T: DeserializeOwned>(app: &AppHandle, request: AgentRequest) -> Result<T, String> {
    let client = agent_client(app)?;
    ensure_agent_running_for_desktop(&client)?;
    client.request(&request).map_err(agent_error_to_string)
}

fn agent_request_no_unlock<T: DeserializeOwned>(
    app: &AppHandle,
    request: AgentRequest,
) -> Result<T, String> {
    agent_request_no_unlock_detailed(app, request).map_err(agent_error_to_string)
}

fn agent_request_no_unlock_detailed<T: DeserializeOwned>(
    app: &AppHandle,
    request: AgentRequest,
) -> Result<T, AgentCommandError> {
    let map_startup_error = |message| AgentCommandError {
        code: None,
        message,
    };
    let client = agent_client(app).map_err(map_startup_error)?;
    ensure_agent_running_for_desktop(&client).map_err(map_startup_error)?;
    client.request(&request)
}

fn agent_status(app: &AppHandle) -> Result<SessionStatus, String> {
    agent_request_no_unlock::<SessionStatus>(app, AgentRequest::SessionStatus)
}

fn agent_error_to_string(err: AgentCommandError) -> String {
    match err.code {
        Some(code) => format!(
            "{}: {}",
            aipass_agent_protocol::error_code_name(&code),
            err.message
        ),
        None => err.message,
    }
}

fn provider_add_input(request: ProviderAddRequest) -> ProviderEntryInput {
    let provider_kind = provider_kind_for_id(request.provider_id.as_deref());
    ProviderEntryInput {
        title: non_empty(request.title).unwrap_or_else(|| "Custom Provider".to_string()),
        provider_kind,
        provider_id: request.provider_id,
        credential_kind: request.credential_kind,
        account_identity: request.account_identity,
        domains: clean_strings(request.domain),
        favicon_url: request.favicon_url.and_then(non_empty),
        endpoints: endpoints_from(
            request.endpoint,
            request.endpoints,
            request.console_endpoints,
        ),
        interface_type: request.interface_type,
        auth_scheme: request.auth_scheme,
        api_key: request.api_key.into_inner(),
        secret_label: request.secret_label.and_then(non_empty),
        default_model: request.default_model.and_then(non_empty),
        model_aliases: clean_pairs(request.model_aliases),
        headers: request.headers,
        quota: request.quota,
        subscription: None,
        gateway: request.gateway,
        tags: clean_strings(request.tags),
        notes: request.notes.and_then(non_empty),
        secret_metadata: request.secret_metadata,
    }
}

fn provider_update_input(request: ProviderUpdateRequest) -> ProviderEntryUpdateInput {
    let provider_kind = provider_kind_for_id(request.provider_id.as_deref());
    ProviderEntryUpdateInput {
        title: non_empty(request.title).unwrap_or_else(|| "Custom Provider".to_string()),
        provider_kind,
        provider_id: request.provider_id,
        credential_kind: request.credential_kind,
        account_identity: request.account_identity,
        domains: clean_strings(request.domain),
        favicon_url: request.favicon_url.and_then(non_empty),
        endpoints: endpoints_from(
            request.endpoint,
            request.endpoints,
            request.console_endpoints,
        ),
        interface_type: request.interface_type,
        auth_scheme: request.auth_scheme,
        api_key: request
            .api_key
            .map(|value| value.into_inner())
            .and_then(non_empty),
        secret_label: request.secret_label.and_then(non_empty),
        default_model: request.default_model.and_then(non_empty),
        model_aliases: clean_pairs(request.model_aliases),
        headers: request.headers,
        quota: request.quota,
        subscription: None,
        gateway: request.gateway,
        tags: clean_strings(request.tags),
        notes: request.notes.and_then(non_empty),
        secret_metadata: request.secret_metadata,
    }
}

fn endpoints_from(
    endpoint: Option<String>,
    endpoints: Vec<String>,
    console_endpoints: Vec<String>,
) -> Vec<ProviderEndpoint> {
    let mut api_endpoints = endpoints
        .into_iter()
        .chain(endpoint)
        .filter_map(non_empty)
        .map(ProviderEndpoint::api)
        .collect::<Vec<_>>();
    api_endpoints.extend(
        console_endpoints
            .into_iter()
            .filter_map(non_empty)
            .map(ProviderEndpoint::console),
    );
    api_endpoints
}

fn clean_strings(values: Vec<String>) -> Vec<String> {
    values.into_iter().filter_map(non_empty).collect()
}

fn clean_pairs(values: Vec<(String, String)>) -> Vec<(String, String)> {
    values
        .into_iter()
        .filter_map(|(left, right)| Some((non_empty(left)?, non_empty(right)?)))
        .collect()
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(test)]
fn probe_entry(entry: EntrySummary, secret: String, timeout_seconds: u64) -> ProbeResult {
    let endpoint = endpoint_url(&entry.endpoints);
    let Some(endpoint) = endpoint.clone() else {
        return ProbeResult {
            ok: false,
            provider_id: entry.provider_id,
            interface_type: entry.interface_type,
            status: None,
            endpoint: None,
            model_count: None,
            error: Some("provider has no API endpoint".to_string()),
        };
    };

    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(timeout_seconds.clamp(1, 120)))
        .user_agent("AIPass/1.0")
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            return ProbeResult {
                ok: false,
                provider_id: entry.provider_id,
                interface_type: entry.interface_type,
                status: None,
                endpoint: Some(endpoint),
                model_count: None,
                error: Some(err.to_string()),
            };
        }
    };

    let (display_url, request) = match entry.interface_type {
        InterfaceType::OpenAiCompatible | InterfaceType::AzureOpenAi => {
            let url = join_url(&endpoint, "models");
            let request = apply_auth(client.get(&url), &entry.auth_scheme, &secret);
            (url, request)
        }
        InterfaceType::AnthropicMessages => {
            let url = join_url(&endpoint, "v1/models");
            let request = apply_auth(client.get(&url), &entry.auth_scheme, &secret)
                .header("anthropic-version", "2023-06-01");
            (url, request)
        }
        InterfaceType::Gemini => {
            let url = join_url(&endpoint, "v1beta/models");
            let display_url = append_query_param(&url, "key", "[redacted]");
            let request_url = append_query_param(&url, "key", &secret);
            let request = client.get(&request_url);
            (display_url, request)
        }
        InterfaceType::Bedrock | InterfaceType::CustomHttp => {
            return ProbeResult {
                ok: false,
                provider_id: entry.provider_id,
                interface_type: entry.interface_type,
                status: None,
                endpoint: Some(endpoint),
                model_count: None,
                error: Some("probe is not supported for this interface".to_string()),
            };
        }
    };

    match request.send() {
        Ok(response) => {
            let status = response.status().as_u16();
            let json = response
                .text()
                .ok()
                .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok());
            ProbeResult {
                ok: (200..300).contains(&status),
                provider_id: entry.provider_id,
                interface_type: entry.interface_type,
                status: Some(status),
                endpoint: Some(display_url),
                model_count: json.as_ref().and_then(model_count),
                error: None,
            }
        }
        Err(err) => ProbeResult {
            ok: false,
            provider_id: entry.provider_id,
            interface_type: entry.interface_type,
            status: None,
            endpoint: Some(display_url),
            model_count: None,
            error: Some(redact_error(&err.to_string(), &secret)),
        },
    }
}

#[cfg(test)]
fn apply_auth(
    request: reqwest::blocking::RequestBuilder,
    auth_scheme: &AuthScheme,
    secret: &str,
) -> reqwest::blocking::RequestBuilder {
    match auth_scheme {
        AuthScheme::Bearer => request.bearer_auth(secret),
        AuthScheme::XApiKey => request.header("x-api-key", secret),
        AuthScheme::AzureApiKey => request.header("api-key", secret),
        AuthScheme::CustomHeader => request.header("authorization", secret),
        AuthScheme::GoogleApiKey | AuthScheme::AwsProfile => request,
    }
}

#[cfg(test)]
fn endpoint_url(endpoints: &[ProviderEndpoint]) -> Option<String> {
    endpoints
        .iter()
        .find(|endpoint| endpoint.kind == aipass_provider_registry::EndpointKind::Api)
        .and_then(|endpoint| endpoint.url.clone())
        .or_else(|| endpoints.iter().find_map(|endpoint| endpoint.url.clone()))
}

#[cfg(test)]
fn join_url(base: &str, suffix: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        suffix.trim_start_matches('/')
    )
}

#[cfg(test)]
fn append_query_param(url: &str, key: &str, value: &str) -> String {
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}{key}={value}")
}

#[cfg(test)]
fn model_count(value: &serde_json::Value) -> Option<usize> {
    value
        .get("data")
        .or_else(|| value.get("models"))
        .and_then(|value| value.as_array())
        .map(Vec::len)
}

#[cfg(test)]
fn redact_error(value: &str, secret: &str) -> String {
    if secret.is_empty() {
        value.to_string()
    } else {
        value.replace(secret, "[redacted]")
    }
}

async fn run_blocking<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|err| err.to_string())?
}

async fn run_blocking_agent<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, AgentCommandError> + Send + 'static,
) -> Result<T, AgentCommandError> {
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|err| AgentCommandError {
            code: Some(aipass_agent_protocol::AgentErrorCode::Internal),
            message: err.to_string(),
        })?
}

fn load_preferences(app: &AppHandle) -> Result<AppPreferences, String> {
    let path = preferences_path(app)?;
    if !path.exists() {
        return Ok(AppPreferences::default());
    }
    let bytes = fs::read(&path).map_err(|err| err.to_string())?;
    let value: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => return Ok(AppPreferences::default()),
    };
    let had_persist_unlock = value.get("persistUnlock").is_some();
    let preferences: AppPreferences =
        serde_json::from_value(value).unwrap_or_else(|_| AppPreferences::default());
    if had_persist_unlock {
        write_json_atomic(&path, &preferences)?;
    }
    Ok(preferences)
}

fn save_preferences(app: &AppHandle, preferences: &AppPreferences) -> Result<(), String> {
    let path = preferences_path(app)?;
    write_json_atomic(&path, preferences)
}

fn preferences_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|err| err.to_string())?;
    Ok(dir.join("preferences.json"))
}

fn browser_extension_status_snapshot(app: &AppHandle) -> Result<BrowserExtensionStatus, String> {
    let package = bundled_extension_package(app)?;
    let targets = detected_browser_targets();
    let extension_ids = extension_ids_for_native_host(&package.id);
    let native_hosts = native_host_statuses_snapshot()?;
    let native_host = preferred_native_host_status(&native_hosts, &extension_ids)?;
    let primary_target = preferred_browser_target(&targets);
    let browser_path = primary_target.and_then(find_browser_path);
    let installed_paths = installed_extension_paths(&extension_ids);
    let native_host_configured = native_hosts
        .iter()
        .any(|status| native_host_status_allows(status, &extension_ids));

    let zip_exists = package.zip_path.exists()
        && fs::metadata(&package.zip_path)
            .map(|metadata| metadata.len() > 0)
            .unwrap_or(false)
        && package.version != "0.0.0";

    Ok(BrowserExtensionStatus {
        browser: primary_target
            .map(|target| target.id.to_string())
            .unwrap_or_else(|| "chromium".to_string()),
        detected_browsers: targets
            .iter()
            .map(|target| target.label.to_string())
            .collect(),
        chrome_installed: !targets.is_empty(),
        chrome_path: browser_path,
        extension_id: package.id,
        discovered_extension_ids: extension_ids,
        extension_version: package.version,
        zip_exists,
        zip_path: package.zip_path,
        extension_installed: !installed_paths.is_empty(),
        installed_paths,
        native_host_configured,
        native_host,
        native_hosts,
    })
}

fn install_browser_extension(app: &AppHandle) -> Result<BrowserExtensionInstallResult, String> {
    let package = bundled_extension_package(app)?;
    if !package.zip_path.exists()
        || fs::metadata(&package.zip_path)
            .map(|metadata| metadata.len() == 0)
            .unwrap_or(true)
        || package.version == "0.0.0"
    {
        return Err(format!(
            "bundled Chrome extension package is missing: {}",
            package.zip_path.display()
        ));
    }
    let targets = detected_browser_targets();
    let Some(target) = preferred_browser_target(&targets) else {
        return Err("A supported Chromium browser is not installed".to_string());
    };
    if find_browser_path(target).is_none() {
        return Err(format!("{} is not installed", target.label));
    }

    repair_native_host_manifest(vec![package.id.clone()])?;

    let extract_dir = bundled_extension_extract_dir(&package.id)?;
    extract_extension_package(&package.zip_path, &extract_dir).map_err(|err| {
        format!(
            "failed to extract bundled extension package to {}: {err}",
            extract_dir.display()
        )
    })?;

    let opened_chrome = open_browser_extensions_page(target).is_ok();
    let opened_package = reveal_path(&extract_dir).is_ok();
    let status = browser_extension_status_snapshot(app)?;
    Ok(BrowserExtensionInstallResult {
        status,
        opened_chrome,
        opened_package,
    })
}

#[derive(Clone, Debug)]
struct ExtensionPackage {
    id: String,
    version: String,
    manifest_version: String,
    zip_path: PathBuf,
}

fn bundled_extension_package(app: &AppHandle) -> Result<ExtensionPackage, String> {
    let metadata_path = bundled_extension_metadata_path(app)?;
    let metadata_text = fs::read_to_string(&metadata_path).map_err(|err| {
        format!(
            "failed to read bundled extension metadata at {}: {err}",
            metadata_path.display()
        )
    })?;
    let metadata_dir = metadata_path
        .parent()
        .ok_or_else(|| "bundled extension metadata path has no parent".to_string())?;
    parse_extension_package_metadata(&metadata_text, metadata_dir)
}

fn parse_extension_package_metadata(
    metadata_text: &str,
    metadata_dir: &Path,
) -> Result<ExtensionPackage, String> {
    let metadata: serde_json::Value = serde_json::from_str(metadata_text)
        .map_err(|err| format!("failed to parse bundled extension metadata: {err}"))?;
    let id = metadata
        .get("id")
        .and_then(|value| value.as_str())
        .map(normalized_extension_id)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "bundled extension metadata is missing id".to_string())?;
    let version = metadata
        .get("version")
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "bundled extension metadata is missing version".to_string())?;
    let zip_name = metadata
        .get("zip")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("aipass-extension.zip");
    // The version written into manifest.json inside the zip. Nightly builds
    // stamp a Chrome-compatible numeric manifest version while `version`
    // keeps the full semver for display; older metadata lacks the field.
    let manifest_version = metadata
        .get("manifest_version")
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| version.clone());
    Ok(ExtensionPackage {
        id,
        version,
        manifest_version,
        zip_path: metadata_dir.join(zip_name),
    })
}

fn bundled_extension_extract_dir(extension_id: &str) -> Result<PathBuf, String> {
    let dirs = directories::ProjectDirs::from("dev", "aipass", "desktop")
        .ok_or_else(|| "cannot determine AIPass project directory".to_string())?;
    Ok(dirs.data_dir().join("browser-extension").join(extension_id))
}

fn extract_extension_package(zip_path: &Path, extract_dir: &Path) -> Result<(), String> {
    if extract_dir.exists() {
        fs::remove_dir_all(extract_dir).map_err(|err| {
            format!(
                "failed to clear extension directory {}: {err}",
                extract_dir.display()
            )
        })?;
    }
    fs::create_dir_all(extract_dir).map_err(|err| {
        format!(
            "failed to create extension directory {}: {err}",
            extract_dir.display()
        )
    })?;
    let archive_file = fs::File::open(zip_path)
        .map_err(|err| format!("failed to open {}: {err}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(archive_file)
        .map_err(|err| format!("{} is not a valid zip archive: {err}", zip_path.display()))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|err| format!("failed to read zip entry {index}: {err}"))?;
        // Skip entries that would escape the extraction directory (zip-slip).
        let Some(relative_path) = entry.enclosed_name() else {
            continue;
        };
        let out_path = extract_dir.join(relative_path);
        if entry.is_dir() {
            fs::create_dir_all(&out_path)
                .map_err(|err| format!("failed to create {}: {err}", out_path.display()))?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
        }
        let mut out_file = fs::File::create(&out_path)
            .map_err(|err| format!("failed to create {}: {err}", out_path.display()))?;
        std::io::copy(&mut entry, &mut out_file)
            .map_err(|err| format!("failed to write {}: {err}", out_path.display()))?;
    }
    Ok(())
}

// Chrome/Edge only accept extension versions of one to four dot-separated
// integers, each at most 65535. Returns the value zero-padded to four parts
// so arrays compare with the same ordering browsers use.
fn parse_chrome_version(version: &str) -> Option<[u64; 4]> {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.is_empty() || parts.len() > 4 {
        return None;
    }
    let mut parsed = [0u64; 4];
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() || !part.chars().all(|ch| ch.is_ascii_digit()) {
            return None;
        }
        let value: u64 = part.parse().ok()?;
        if value > 65535 {
            return None;
        }
        parsed[index] = value;
    }
    Some(parsed)
}

// The highest Chrome-compatible version directory containing a manifest under
// a browser profile's Extensions/<id> directory.
fn latest_installed_extension_version(extension_dir: &Path) -> Option<[u64; 4]> {
    fs::read_dir(extension_dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && path.join("manifest.json").is_file())
        .filter_map(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .and_then(parse_chrome_version)
        })
        .max()
}

// Pushes the bundled extension package into every browser profile that already
// has it installed by extracting it as a new version directory — the same
// layout Chrome uses for self-hosted updates, picked up on the next browser
// launch. Runs silently at startup; failures are logged, never surfaced.
fn sync_installed_browser_extensions(app: &AppHandle) {
    if let Err(err) = sync_installed_browser_extensions_inner(app) {
        let _ = logging::log_event("desktop.extension_sync.failed", &[("error", &err)]);
    }
}

fn sync_installed_browser_extensions_inner(app: &AppHandle) -> Result<(), String> {
    let package = bundled_extension_package(app)?;
    if !package.zip_path.is_file() {
        return Ok(());
    }
    let Some(bundled_version) = parse_chrome_version(&package.manifest_version) else {
        // A non-numeric manifest version cannot be compared or loaded by the
        // browser; leave sideloaded copies untouched.
        let _ = logging::log_event(
            "desktop.extension_sync.skipped",
            &[("reason", "bundled manifest version is not Chrome-compatible")],
        );
        return Ok(());
    };

    let extension_ids = extension_ids_for_native_host(&package.id);
    for installed_dir in installed_extension_paths(&extension_ids) {
        let Some(installed_max) = latest_installed_extension_version(&installed_dir) else {
            // Store-installed copy or an unknown layout: never touch it.
            continue;
        };
        // Never downgrade or rewrite an identical version in place.
        if bundled_version <= installed_max {
            continue;
        }
        let version_dir = installed_dir.join(&package.manifest_version);
        match extract_extension_package(&package.zip_path, &version_dir) {
            Ok(()) => {
                let _ = logging::log_event(
                    "desktop.extension_sync.updated",
                    &[
                        ("path", &version_dir.display().to_string()),
                        ("version", &package.manifest_version),
                    ],
                );
            }
            Err(err) => {
                let _ = logging::log_event(
                    "desktop.extension_sync.extract_failed",
                    &[("path", &version_dir.display().to_string()), ("error", &err)],
                );
            }
        }
    }

    // Users who loaded the extension unpacked from our own data directory get
    // the same silent refresh; the browser picks the files up on relaunch.
    let unpack_dir = bundled_extension_extract_dir(&package.id)?;
    if unpack_dir.is_dir() {
        let current = fs::read_to_string(unpack_dir.join("manifest.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|value| value.get("version")?.as_str().map(ToString::to_string));
        let stale = current
            .as_deref()
            .and_then(parse_chrome_version)
            .map(|current| bundled_version > current)
            .unwrap_or(true);
        if stale {
            match extract_extension_package(&package.zip_path, &unpack_dir) {
                Ok(()) => {
                    let _ = logging::log_event(
                        "desktop.extension_sync.updated",
                        &[
                            ("path", &unpack_dir.display().to_string()),
                            ("version", &package.manifest_version),
                        ],
                    );
                }
                Err(err) => {
                    let _ = logging::log_event(
                        "desktop.extension_sync.extract_failed",
                        &[("path", &unpack_dir.display().to_string()), ("error", &err)],
                    );
                }
            }
        }
    }
    Ok(())
}

fn bundled_extension_metadata_path(app: &AppHandle) -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push(
            resource_dir
                .join("browser-extension")
                .join("aipass-extension.json"),
        );
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    candidates.push(
        manifest_dir
            .join("..")
            .join("..")
            .join("extension")
            .join("build")
            .join("aipass-extension.json"),
    );
    candidates
        .into_iter()
        .find(|path| path.exists())
        .ok_or_else(|| "bundled Chrome extension metadata is missing".to_string())
}

#[derive(Clone, Debug)]
struct BrowserTarget {
    id: &'static str,
    label: &'static str,
    manifest_path: PathBuf,
    profile_roots: Vec<PathBuf>,
    executable_candidates: Vec<PathBuf>,
    #[cfg(target_os = "windows")]
    native_host_registry_key: &'static str,
}

fn installed_extension_paths(extension_ids: &[String]) -> Vec<PathBuf> {
    known_browser_targets()
        .into_iter()
        .flat_map(|target| {
            target
                .profile_roots
                .into_iter()
                .flat_map(|profile_root| {
                    fs::read_dir(profile_root)
                        .ok()
                        .into_iter()
                        .flat_map(|items| items.filter_map(|item| item.ok()))
                        .map(|entry| entry.path())
                        .filter(|path| path.is_dir())
                        .flat_map(|path| {
                            extension_ids
                                .iter()
                                .map(move |id| path.join("Extensions").join(id))
                        })
                        .filter(|path| path.exists())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn detected_browser_targets() -> Vec<BrowserTarget> {
    let mut detected = known_browser_targets()
        .into_iter()
        .filter(browser_target_detected)
        .collect::<Vec<_>>();
    detected.sort_by_key(|target| target_sort_rank(target.id));
    detected
}

fn browser_target_detected(target: &BrowserTarget) -> bool {
    find_browser_path(target).is_some()
        || target
            .profile_roots
            .iter()
            .any(|profile_root| profile_root_has_browser_data(profile_root))
}

fn profile_root_has_browser_data(profile_root: &Path) -> bool {
    if profile_root.join("Local State").is_file() {
        return true;
    }
    for entry in fs::read_dir(profile_root)
        .ok()
        .into_iter()
        .flat_map(|items| items.filter_map(|item| item.ok()))
    {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path.file_name().and_then(|value| value.to_str()) == Some("NativeMessagingHosts") {
            continue;
        }
        if path.join("Preferences").is_file()
            || path.join("Secure Preferences").is_file()
            || path.join("Extensions").is_dir()
        {
            return true;
        }
    }
    false
}

fn repair_browser_targets() -> Result<Vec<BrowserTarget>, String> {
    let detected = detected_browser_targets();
    if !detected.is_empty() {
        return Ok(detected);
    }
    known_browser_targets()
        .into_iter()
        .next()
        .map(|target| vec![target])
        .ok_or_else(|| "native host repair is not supported on this platform".to_string())
}

fn preferred_browser_target(targets: &[BrowserTarget]) -> Option<&BrowserTarget> {
    targets
        .iter()
        .find(|target| find_browser_path(target).is_some())
        .or_else(|| targets.first())
}

fn target_sort_rank(id: &str) -> usize {
    match id {
        "chrome" => 0,
        "edge" => 1,
        "brave" => 2,
        "arc" => 3,
        "chromium" => 4,
        _ => 10,
    }
}

fn known_browser_targets() -> Vec<BrowserTarget> {
    #[cfg(target_os = "macos")]
    {
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
            return Vec::new();
        };
        let support = home.join("Library").join("Application Support");
        vec![
            mac_browser_target(
                &support,
                "chrome",
                "Google Chrome",
                "Google/Chrome",
                &["Google Chrome"],
            ),
            mac_browser_target(
                &support,
                "edge",
                "Microsoft Edge",
                "Microsoft Edge",
                &["Microsoft Edge"],
            ),
            mac_browser_target(
                &support,
                "brave",
                "Brave",
                "BraveSoftware/Brave-Browser",
                &["Brave Browser"],
            ),
            mac_browser_target(&support, "arc", "Arc", "Arc/User Data", &["Arc"]),
            mac_browser_target(&support, "chromium", "Chromium", "Chromium", &["Chromium"]),
            mac_browser_target(
                &support,
                "chrome-beta",
                "Google Chrome Beta",
                "Google/Chrome Beta",
                &["Google Chrome Beta"],
            ),
            mac_browser_target(
                &support,
                "chrome-dev",
                "Google Chrome Dev",
                "Google/Chrome Dev",
                &["Google Chrome Dev"],
            ),
            mac_browser_target(
                &support,
                "chrome-canary",
                "Google Chrome Canary",
                "Google/Chrome Canary",
                &["Google Chrome Canary"],
            ),
            mac_browser_target(
                &support,
                "edge-beta",
                "Microsoft Edge Beta",
                "Microsoft Edge Beta",
                &["Microsoft Edge Beta"],
            ),
            mac_browser_target(
                &support,
                "edge-dev",
                "Microsoft Edge Dev",
                "Microsoft Edge Dev",
                &["Microsoft Edge Dev"],
            ),
            mac_browser_target(
                &support,
                "edge-canary",
                "Microsoft Edge Canary",
                "Microsoft Edge Canary",
                &["Microsoft Edge Canary"],
            ),
            mac_browser_target(&support, "vivaldi", "Vivaldi", "Vivaldi", &["Vivaldi"]),
            mac_browser_target(
                &support,
                "vivaldi-snapshot",
                "Vivaldi Snapshot",
                "Vivaldi Snapshot",
                &["Vivaldi Snapshot"],
            ),
        ]
    }

    #[cfg(target_os = "linux")]
    {
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
            return Vec::new();
        };
        let config = home.join(".config");
        vec![
            linux_browser_target(
                "chrome",
                "Google Chrome",
                config.join("google-chrome"),
                &["google-chrome", "google-chrome-stable"],
            ),
            linux_browser_target(
                "edge",
                "Microsoft Edge",
                config.join("microsoft-edge"),
                &["microsoft-edge", "microsoft-edge-stable"],
            ),
            linux_browser_target(
                "brave",
                "Brave",
                config.join("BraveSoftware").join("Brave-Browser"),
                &["brave-browser", "brave"],
            ),
            linux_browser_target(
                "chromium",
                "Chromium",
                config.join("chromium"),
                &["chromium", "chromium-browser"],
            ),
            linux_browser_target(
                "vivaldi",
                "Vivaldi",
                config.join("vivaldi"),
                &["vivaldi", "vivaldi-stable"],
            ),
        ]
    }

    #[cfg(target_os = "windows")]
    {
        let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) else {
            return Vec::new();
        };
        let mut executable_roots = vec![local_app_data.clone()];
        executable_roots.extend(
            ["PROGRAMFILES", "PROGRAMFILES(X86)"]
                .into_iter()
                .filter_map(std::env::var_os)
                .map(PathBuf::from),
        );
        let application_candidates = |vendor: &str, browser: &str, executable: &str| {
            executable_roots
                .iter()
                .map(|root| {
                    root.join(vendor)
                        .join(browser)
                        .join("Application")
                        .join(executable)
                })
                .collect::<Vec<_>>()
        };
        let app_data = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| local_app_data.join("AIPass"));
        let shared_manifest = app_data
            .join("AIPass")
            .join("NativeMessagingHosts")
            .join("dev.aipass.native.json");
        vec![
            windows_browser_target(
                "chrome",
                "Google Chrome",
                local_app_data
                    .join("Google")
                    .join("Chrome")
                    .join("User Data"),
                shared_manifest.clone(),
                &application_candidates("Google", "Chrome", "chrome.exe"),
                r"HKCU\Software\Google\Chrome\NativeMessagingHosts\dev.aipass.native",
            ),
            windows_browser_target(
                "edge",
                "Microsoft Edge",
                local_app_data
                    .join("Microsoft")
                    .join("Edge")
                    .join("User Data"),
                shared_manifest.clone(),
                &application_candidates("Microsoft", "Edge", "msedge.exe"),
                r"HKCU\Software\Microsoft\Edge\NativeMessagingHosts\dev.aipass.native",
            ),
            windows_browser_target(
                "brave",
                "Brave",
                local_app_data
                    .join("BraveSoftware")
                    .join("Brave-Browser")
                    .join("User Data"),
                shared_manifest.clone(),
                &application_candidates("BraveSoftware", "Brave-Browser", "brave.exe"),
                r"HKCU\Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\dev.aipass.native",
            ),
        ]
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Vec::new()
    }
}

#[cfg(target_os = "macos")]
fn mac_browser_target(
    support_root: &Path,
    id: &'static str,
    label: &'static str,
    support_subdir: &str,
    app_names: &[&str],
) -> BrowserTarget {
    let profile_root = support_root.join(support_subdir);
    let executable_candidates = app_names
        .iter()
        .flat_map(|name| {
            [
                PathBuf::from("/Applications")
                    .join(format!("{name}.app"))
                    .join("Contents")
                    .join("MacOS")
                    .join(name),
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_default()
                    .join("Applications")
                    .join(format!("{name}.app"))
                    .join("Contents")
                    .join("MacOS")
                    .join(name),
            ]
        })
        .collect();
    BrowserTarget {
        id,
        label,
        manifest_path: profile_root
            .join("NativeMessagingHosts")
            .join("dev.aipass.native.json"),
        profile_roots: vec![profile_root],
        executable_candidates,
    }
}

#[cfg(target_os = "linux")]
fn linux_browser_target(
    id: &'static str,
    label: &'static str,
    profile_root: PathBuf,
    executable_names: &[&str],
) -> BrowserTarget {
    BrowserTarget {
        id,
        label,
        manifest_path: profile_root
            .join("NativeMessagingHosts")
            .join("dev.aipass.native.json"),
        profile_roots: vec![profile_root],
        executable_candidates: executable_names
            .iter()
            .filter_map(|name| find_executable_in_path(name))
            .collect(),
    }
}

#[cfg(target_os = "windows")]
fn windows_browser_target(
    id: &'static str,
    label: &'static str,
    profile_root: PathBuf,
    manifest_path: PathBuf,
    executable_candidates: &[PathBuf],
    native_host_registry_key: &'static str,
) -> BrowserTarget {
    BrowserTarget {
        id,
        label,
        manifest_path,
        profile_roots: vec![profile_root],
        executable_candidates: executable_candidates.to_vec(),
        native_host_registry_key,
    }
}

fn find_browser_path(target: &BrowserTarget) -> Option<PathBuf> {
    let env_name = format!("AIPASS_{}_PATH", target.id.replace('-', "_").to_uppercase());
    if let Some(path) = std::env::var_os(env_name).map(PathBuf::from) {
        if path.exists() {
            return Some(path);
        }
    }

    target
        .executable_candidates
        .iter()
        .find(|path| path.exists())
        .cloned()
}

#[cfg(target_os = "linux")]
fn find_executable_in_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

fn open_browser_extensions_page(target: &BrowserTarget) -> Result<(), String> {
    let browser_path =
        find_browser_path(target).ok_or_else(|| format!("{} is not installed", target.label))?;
    ProcessCommand::new(browser_path)
        .arg("chrome://extensions")
        .spawn()
        .map(|_| ())
        .map_err(|err| err.to_string())
}

fn extension_ids_for_native_host(primary_extension_id: &str) -> Vec<String> {
    merged_extension_ids_for_native_host([primary_extension_id.to_string()])
}

fn merged_extension_ids_for_native_host(
    extension_ids: impl IntoIterator<Item = String>,
) -> Vec<String> {
    merge_extension_ids(extension_ids, discover_aipass_extension_ids(), Vec::new())
}

fn merge_extension_ids(
    extension_ids: impl IntoIterator<Item = String>,
    discovered_ids: impl IntoIterator<Item = String>,
    existing_origins: impl IntoIterator<Item = String>,
) -> Vec<String> {
    let mut ids = BTreeSet::new();
    for value in extension_ids
        .into_iter()
        .chain(discovered_ids)
        .chain(existing_origins)
    {
        let id = normalized_extension_id(&value);
        if !id.is_empty() {
            ids.insert(id);
        }
    }
    ids.into_iter().collect()
}

fn discover_aipass_extension_ids() -> Vec<String> {
    let mut ids = BTreeSet::new();
    for target in known_browser_targets() {
        for manifest in installed_extension_manifest_paths(&target) {
            if manifest_is_aipass(&manifest) {
                if let Some(id) = manifest
                    .parent()
                    .and_then(Path::parent)
                    .and_then(|path| path.file_name())
                    .and_then(|name| name.to_str())
                    .filter(|id| looks_like_extension_id(id))
                {
                    ids.insert(id.to_string());
                }
            }
        }

        for preferences_path in browser_preferences_paths(&target) {
            let Ok(bytes) = fs::read(&preferences_path) else {
                continue;
            };
            let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                continue;
            };
            collect_aipass_extension_ids_from_value(&value, &mut ids);
        }
    }
    ids.into_iter().collect()
}

fn installed_extension_manifest_paths(target: &BrowserTarget) -> Vec<PathBuf> {
    target
        .profile_roots
        .iter()
        .flat_map(|profile_root| {
            fs::read_dir(profile_root)
                .ok()
                .into_iter()
                .flat_map(|items| items.filter_map(|item| item.ok()))
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .flat_map(|profile_path| {
                    fs::read_dir(profile_path.join("Extensions"))
                        .ok()
                        .into_iter()
                        .flat_map(|items| items.filter_map(|item| item.ok()))
                        .map(|entry| entry.path())
                        .filter(|path| path.is_dir())
                        .flat_map(|extension_path| {
                            fs::read_dir(&extension_path)
                                .ok()
                                .into_iter()
                                .flat_map(|items| items.filter_map(|item| item.ok()))
                                .map(|entry| entry.path().join("manifest.json"))
                                .filter(|path| path.exists())
                                .collect::<Vec<_>>()
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn browser_preferences_paths(target: &BrowserTarget) -> Vec<PathBuf> {
    target
        .profile_roots
        .iter()
        .flat_map(|profile_root| {
            let mut paths = vec![
                profile_root.join("Preferences"),
                profile_root.join("Secure Preferences"),
            ];
            if let Ok(entries) = fs::read_dir(profile_root) {
                for entry in entries.filter_map(|entry| entry.ok()) {
                    let path = entry.path();
                    if path.is_dir() {
                        paths.push(path.join("Preferences"));
                        paths.push(path.join("Secure Preferences"));
                    }
                }
            }
            paths
        })
        .filter(|path| path.exists())
        .collect()
}

fn manifest_is_aipass(path: &Path) -> bool {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .is_some_and(|value| value_contains_aipass_manifest_signal(&value))
}

fn collect_aipass_extension_ids_from_value(value: &serde_json::Value, ids: &mut BTreeSet<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                if looks_like_extension_id(key) && value_contains_aipass_manifest_signal(child) {
                    ids.insert(key.to_string());
                }
                collect_aipass_extension_ids_from_value(child, ids);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_aipass_extension_ids_from_value(item, ids);
            }
        }
        _ => {}
    }
}

fn value_contains_aipass_manifest_signal(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            if map
                .get("name")
                .and_then(|value| value.as_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("AIPass"))
            {
                return true;
            }
            if map
                .get("short_name")
                .and_then(|value| value.as_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("AIPass"))
            {
                return true;
            }
            if map
                .get("manifest")
                .is_some_and(value_contains_aipass_manifest_signal)
            {
                return true;
            }
            if let Some(manifest_text) = map.get("manifest.json").and_then(|value| value.as_str()) {
                if serde_json::from_str::<serde_json::Value>(manifest_text)
                    .ok()
                    .is_some_and(|manifest| value_contains_aipass_manifest_signal(&manifest))
                {
                    return true;
                }
            }
            map.values().any(value_contains_aipass_manifest_signal)
        }
        serde_json::Value::Array(items) => items.iter().any(value_contains_aipass_manifest_signal),
        serde_json::Value::String(value) => {
            value.contains("\"name\": \"AIPass\"") || value.contains("\"short_name\": \"AIPass\"")
        }
        _ => false,
    }
}

fn looks_like_extension_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| (b'a'..=b'p').contains(&byte))
}

fn reveal_path(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        ProcessCommand::new("open")
            .args(["-R"])
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|err| err.to_string())
    }

    #[cfg(target_os = "windows")]
    {
        let mut arg = OsString::from("/select,");
        arg.push(path.as_os_str());
        ProcessCommand::new("explorer")
            .arg(arg)
            .spawn()
            .map(|_| ())
            .map_err(|err| err.to_string())
    }

    #[cfg(target_os = "linux")]
    {
        let target = path.parent().unwrap_or(path);
        ProcessCommand::new("xdg-open")
            .arg(target)
            .spawn()
            .map(|_| ())
            .map_err(|err| err.to_string())
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = path;
        Err("opening the extension package is not supported on this platform".to_string())
    }
}

fn native_host_status_snapshot() -> Result<NativeHostStatus, String> {
    let statuses = native_host_statuses_snapshot()?;
    let allowed_extension_ids = load_allowed_extension_ids().unwrap_or_default();
    preferred_native_host_status(&statuses, &allowed_extension_ids)
}

fn native_host_statuses_snapshot() -> Result<Vec<NativeHostStatus>, String> {
    let host_path = native_host_binary_path()?;
    let host_status = native_host_binary_status(&host_path);
    let settings_path = native_host_settings_path().map_err(|err| err.to_string())?;
    let allowed_extension_ids = load_allowed_extension_ids().map_err(|err| err.to_string())?;
    repair_browser_targets()?
        .into_iter()
        .map(|target| {
            let allowed_origins = read_manifest_allowed_origins(&target.manifest_path);
            Ok(NativeHostStatus {
                browser: target.id.to_string(),
                browser_label: target.label.to_string(),
                host_exists: host_status.exists,
                host_usable: host_status.usable,
                host_error: host_status.error.clone(),
                host_path: host_path.clone(),
                manifest_exists: target.manifest_path.exists(),
                manifest_path: target.manifest_path,
                settings_path: settings_path.clone(),
                allowed_extension_ids: allowed_extension_ids.clone(),
                allowed_origins,
            })
        })
        .collect()
}

fn preferred_native_host_status(
    statuses: &[NativeHostStatus],
    extension_ids: &[String],
) -> Result<NativeHostStatus, String> {
    statuses
        .iter()
        .find(|status| native_host_status_allows(status, extension_ids))
        .or_else(|| statuses.first())
        .cloned()
        .ok_or_else(|| "native host repair is not supported on this platform".to_string())
}

fn native_host_status_allows(status: &NativeHostStatus, extension_ids: &[String]) -> bool {
    status.host_usable
        && status.manifest_exists
        && extension_ids.iter().any(|extension_id| {
            let extension_id = normalized_extension_id(extension_id);
            status
                .allowed_extension_ids
                .iter()
                .any(|id| normalized_extension_id(id) == extension_id)
                || status
                    .allowed_origins
                    .iter()
                    .any(|origin| normalized_extension_id(origin) == extension_id)
        })
}

fn repair_native_host_manifest(extension_ids: Vec<String>) -> Result<NativeHostStatus, String> {
    let host_path = native_host_binary_path()?;
    ensure_native_host_binary_usable(&host_path)?;
    let extension_ids = merged_extension_ids_for_native_host(extension_ids);
    let targets = repair_browser_targets()?;
    let mut all_extension_ids: BTreeSet<String> = extension_ids.iter().cloned().collect();
    for target in &targets {
        let existing_origins = read_manifest_allowed_origins(&target.manifest_path);
        let target_extension_ids =
            merge_extension_ids(extension_ids.iter().cloned(), Vec::new(), existing_origins);
        all_extension_ids.extend(target_extension_ids.iter().cloned());
        let origins = allowed_origins(&target_extension_ids)?;
        if let Some(parent) = target.manifest_path.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let manifest = native_manifest(&host_path, &origins);
        let bytes = serde_json::to_vec_pretty(&manifest).map_err(|err| err.to_string())?;
        atomic_write_bytes(&target.manifest_path, &bytes).map_err(|err| err.to_string())?;
        install_native_manifest_reference(target, &target.manifest_path)?;
    }
    let extension_ids: Vec<String> = all_extension_ids.into_iter().collect();
    save_allowed_extension_ids(&extension_ids).map_err(|err| err.to_string())?;
    let statuses = native_host_statuses_snapshot()?;
    preferred_native_host_status(&statuses, &extension_ids)
}

fn native_host_binary_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    let host_name = if cfg!(target_os = "windows") {
        "aipass-native-host.exe"
    } else {
        "aipass-native-host"
    };
    let mut candidates = vec![exe.with_file_name(host_name)];
    if let Some(exe_dir) = exe.parent() {
        candidates.push(exe_dir.join("resources").join(host_name));
        candidates.push(exe_dir.join("Resources").join(host_name));
        if let Some(contents_dir) = exe_dir.parent() {
            candidates.push(contents_dir.join("Resources").join(host_name));
            candidates.push(contents_dir.join("resources").join(host_name));
        }
    }
    if let Some(found) = candidates
        .iter()
        .find(|candidate| native_host_binary_status(candidate).usable)
        .cloned()
    {
        Ok(found)
    } else {
        Ok(candidates.remove(0))
    }
}

#[derive(Clone, Debug)]
struct NativeHostBinaryStatus {
    exists: bool,
    usable: bool,
    error: Option<String>,
}

fn native_host_binary_status(path: &Path) -> NativeHostBinaryStatus {
    let Ok(metadata) = fs::metadata(path) else {
        return NativeHostBinaryStatus {
            exists: false,
            usable: false,
            error: Some("native host binary was not found".to_string()),
        };
    };
    if !metadata.is_file() {
        return NativeHostBinaryStatus {
            exists: true,
            usable: false,
            error: Some("native host path is not a file".to_string()),
        };
    }
    if metadata.len() == 0 {
        return NativeHostBinaryStatus {
            exists: true,
            usable: false,
            error: Some("native host binary is empty".to_string()),
        };
    }
    #[cfg(unix)]
    if metadata.permissions().mode() & 0o111 == 0 {
        return NativeHostBinaryStatus {
            exists: true,
            usable: false,
            error: Some("native host binary is not executable".to_string()),
        };
    }
    NativeHostBinaryStatus {
        exists: true,
        usable: true,
        error: None,
    }
}

fn ensure_native_host_binary_usable(path: &Path) -> Result<(), String> {
    let status = native_host_binary_status(path);
    if status.usable {
        Ok(())
    } else {
        Err(format!(
            "native host binary is not usable at {}: {}",
            path.display(),
            status
                .error
                .unwrap_or_else(|| "unknown validation error".to_string())
        ))
    }
}

fn allowed_origins(extension_ids: &[String]) -> Result<Vec<String>, String> {
    let origins = extension_ids
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| {
            if value.starts_with("chrome-extension://") {
                if value.ends_with('/') {
                    value.to_string()
                } else {
                    format!("{value}/")
                }
            } else {
                format!("chrome-extension://{value}/")
            }
        })
        .collect::<Vec<_>>();
    if origins.is_empty() {
        return Err("enter at least one browser extension id".to_string());
    }
    Ok(origins)
}

fn normalized_extension_id(value: &str) -> String {
    value
        .trim()
        .trim_start_matches("chrome-extension://")
        .trim_start_matches("chrome://")
        .trim_end_matches('/')
        .to_lowercase()
}

fn read_manifest_allowed_origins(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| {
            value
                .get("allowed_origins")
                .and_then(|items| items.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(ToString::to_string))
                        .collect()
                })
        })
        .unwrap_or_default()
}

fn install_native_manifest_reference(
    target: &BrowserTarget,
    manifest_path: &Path,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let status = ProcessCommand::new("reg")
            .args([
                "add",
                target.native_host_registry_key,
                "/ve",
                "/t",
                "REG_SZ",
                "/d",
                &manifest_path.display().to_string(),
                "/f",
            ])
            .status()
            .map_err(|err| err.to_string())?;
        if !status.success() {
            return Err("native host registry update failed".to_string());
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = target;
        let _ = manifest_path;
    }

    Ok(())
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|err| err.to_string())?;
    atomic_write_bytes(path, &bytes).map_err(|err| err.to_string())
}

fn launch_window_target() -> String {
    std::env::var("AIPASS_WINDOW_TARGET")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| {
            matches!(
                value.as_str(),
                "main" | "unlock" | "quick-access" | "server" | "tray"
            )
        })
        .unwrap_or_else(|| "main".to_string())
}

fn normalize_window_target(target: &str) -> &str {
    match target {
        "main" | "unlock" | "quick-access" | "server" | "tray" => target,
        _ => "main",
    }
}

fn default_window_size(target: &str) -> DesktopWindowSize {
    match target {
        "unlock" => DesktopWindowSize {
            width: 420.0,
            height: 560.0,
        },
        "quick-access" => DesktopWindowSize {
            width: 520.0,
            height: 640.0,
        },
        _ => DesktopWindowSize {
            width: DEFAULT_DESKTOP_WINDOW_WIDTH,
            height: DEFAULT_DESKTOP_WINDOW_HEIGHT,
        },
    }
}

fn window_size_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|err| err.to_string())?;
    Ok(dir.join(DESKTOP_WINDOW_SIZE_FILE))
}

fn window_size_is_valid(size: &DesktopWindowSize) -> bool {
    size.width.is_finite() && size.height.is_finite() && size.width > 0.0 && size.height > 0.0
}

fn window_size_within_tolerance(saved: &DesktopWindowSize, default: &DesktopWindowSize) -> bool {
    window_size_is_valid(saved)
        && (saved.width - default.width).abs() / default.width <= DESKTOP_WINDOW_SIZE_TOLERANCE
        && (saved.height - default.height).abs() / default.height <= DESKTOP_WINDOW_SIZE_TOLERANCE
}

fn target_uses_persisted_window_size(target: &str) -> bool {
    matches!(target, "main" | "server")
}

fn clamp_main_window_size(size: DesktopWindowSize) -> DesktopWindowSize {
    DesktopWindowSize {
        width: size.width.max(MIN_DESKTOP_WINDOW_WIDTH),
        height: size.height.max(MIN_DESKTOP_WINDOW_HEIGHT),
    }
}

fn load_saved_window_size(app: &AppHandle) -> Option<DesktopWindowSize> {
    let path = window_size_path(app).ok()?;
    let bytes = fs::read(path).ok()?;
    let size = serde_json::from_slice::<DesktopWindowSize>(&bytes).ok()?;
    window_size_is_valid(&size).then_some(size)
}

fn window_size_for_target(app: &AppHandle, target: &str) -> DesktopWindowSize {
    let default = default_window_size(target);
    if !target_uses_persisted_window_size(target) {
        return default;
    }

    load_saved_window_size(app)
        .filter(|saved| window_size_within_tolerance(saved, &default))
        .map(clamp_main_window_size)
        .unwrap_or(default)
}

fn persist_window_size_for_target(app: &AppHandle, target: &str) -> Result<(), String> {
    if !target_uses_persisted_window_size(target) {
        return Ok(());
    }

    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main desktop window is unavailable".to_string())?;
    let physical_size = window.inner_size().map_err(|err| err.to_string())?;
    let scale_factor = window.scale_factor().map_err(|err| err.to_string())?;
    if !scale_factor.is_finite() || scale_factor <= 0.0 {
        return Err("desktop window scale factor is invalid".to_string());
    }

    let size = DesktopWindowSize {
        width: f64::from(physical_size.width) / scale_factor,
        height: f64::from(physical_size.height) / scale_factor,
    };
    if !window_size_is_valid(&size) {
        return Err("desktop window size is invalid".to_string());
    }

    let path = window_size_path(app)?;
    write_json_atomic(&path, &size)
}

pub(crate) fn persist_window_size(app: &AppHandle) -> Result<(), String> {
    let target = app.state::<AppState>().window_target();
    persist_window_size_for_target(app, &target)
}

fn prepare_window_target(app: &AppHandle, target: &str, center: bool) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if target == "tray" {
        #[cfg(target_os = "macos")]
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
        let _ = window.hide();
        return;
    }

    let title = match target {
        "unlock" => "AIPass Unlock",
        "quick-access" => "AIPass Quick Access",
        _ => "AIPass",
    };
    let size = window_size_for_target(app, target);
    let _ = window.set_title(title);
    let _ = window.set_size(Size::Logical(LogicalSize {
        width: size.width,
        height: size.height,
    }));
    configure_window_chrome(&window);
    if center {
        let _ = window.center();
    }
}

fn reveal_window_target(app: &AppHandle, target: &str) -> Result<(), String> {
    if target == "tray" {
        prepare_window_target(app, target, false);
        return Ok(());
    }

    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main desktop window is unavailable".to_string())?;

    #[cfg(target_os = "macos")]
    {
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
        let _ = app.show();
    }

    window.show().map_err(|err| err.to_string())?;
    let _ = window.unminimize();
    let _ = window.set_focus();
    Ok(())
}

pub(crate) fn activate_window_target(app: &AppHandle, target: &str) {
    let target = normalize_window_target(target);
    let _ = logging::log_event("desktop.window.target_requested", &[("target", target)]);
    let state = app.state::<AppState>();
    let mut window_state = state.window_state();
    let previous_target = window_state.target.clone();
    let frontend_ready = window_state.set_target(target);
    let target = window_state.target.clone();
    if target == "tray" && target_uses_persisted_window_size(&previous_target) {
        if let Err(err) = persist_window_size_for_target(app, &previous_target) {
            let _ = logging::log_event("desktop.window.size_persist_failed", &[("error", &err)]);
        }
    }
    prepare_window_target(app, &target, !frontend_ready);
    if frontend_ready {
        let _ = reveal_window_target(app, &target);
    }
    drop(window_state);

    if target == "server" {
        let _ = app.emit("open-server-workspace", ());
    }
}

pub(crate) fn reveal_existing_window_target(app: &AppHandle, target: &str) -> bool {
    if app.get_webview_window("main").is_none() {
        return false;
    }

    let state = app.state::<AppState>();
    let mut window_state = state.window_state();
    let frontend_ready = window_state.set_target(target);
    let target = window_state.target.clone();
    if frontend_ready {
        let _ = reveal_window_target(app, &target);
    }
    drop(window_state);
    if target == "server" {
        let _ = app.emit("open-server-workspace", ());
    }
    true
}

pub(crate) fn complete_desktop_startup(app: &AppHandle) -> Result<(), String> {
    let _ = logging::log_event("desktop.frontend.ready", &[]);
    let state = app.state::<AppState>();
    let mut window_state = state.window_state();
    let target = window_state.complete_startup();
    prepare_window_target(app, &target, false);
    let result = reveal_window_target(app, &target);
    drop(window_state);
    result
}

/// Process deep links from both the live listener and the platform's cold-start
/// argument path so every entry surface follows the same validation and UI flow.
pub(crate) fn handle_deep_link_urls(app: &AppHandle, urls: Vec<url::Url>) {
    for url in urls {
        match url.scheme() {
            "ccswitch" => match deeplink::parse_ccswitch_link(&url) {
                Ok(link) => {
                    let state = app.state::<AppState>();
                    if state.frontend_ready() {
                        let _ = app.emit("ccswitch-provider-import", &link);
                    } else {
                        state.store_pending_ccswitch_link(link);
                    }
                    activate_window_target(app, "main");
                }
                Err(err) => {
                    let state = app.state::<AppState>();
                    let payload = err.payload();
                    if state.frontend_ready() {
                        let _ = app.emit("ccswitch-provider-import-error", &payload);
                    } else {
                        state.store_pending_ccswitch_link_error(payload);
                    }
                    activate_window_target(app, "main");
                }
            },
            "aipass-provider" => match deeplink::parse_aipass_provider_link(&url) {
                Ok(link) => {
                    let state = app.state::<AppState>();
                    if state.frontend_ready() {
                        let _ = app.emit("aipass-provider-add", &link);
                    } else {
                        state.store_pending_aipass_provider_link(link);
                    }
                    activate_window_target(app, "main");
                }
                Err(err) => {
                    let state = app.state::<AppState>();
                    let payload = err.payload();
                    if state.frontend_ready() {
                        let _ = app.emit("aipass-provider-add-error", &payload);
                    } else {
                        state.store_pending_aipass_provider_link_error(payload);
                    }
                    activate_window_target(app, "main");
                }
            },
            "aipass" | "aipass-dev" => {
                let extension_id = url
                    .query_pairs()
                    .find(|(key, _)| key == "extensionId")
                    .map(|(_, value)| value.into_owned())
                    .filter(|value| looks_like_extension_id(value));
                if let Some(extension_id) = extension_id {
                    let _ = thread::Builder::new()
                        .name("deep-link-native-host-repair".to_string())
                        .spawn(move || {
                            if let Err(err) = repair_native_host_manifest(vec![extension_id]) {
                                eprintln!(
                                    "failed to repair native host manifest from deep link: {err}"
                                );
                            }
                        });
                }
                if let Some(target) = url.path_segments().and_then(|mut segments| segments.next()) {
                    activate_window_target(app, target);
                } else {
                    activate_window_target(app, "main");
                }
            }
            _ => {}
        }
    }
}

fn ensure_agent_resident_async(app: AppHandle) {
    thread::spawn(move || {
        let client = match agent_client(&app) {
            Ok(client) => client,
            Err(err) => {
                eprintln!("failed to resolve AIPass agent config: {err}");
                return;
            }
        };

        #[cfg(target_os = "macos")]
        match aipass_agent::agent_binary_path() {
            Ok(agent_binary) => {
                if let Err(err) =
                    aipass_agent::ensure_agent_autostart(&agent_binary, &client.config.vault_dir)
                {
                    eprintln!("failed to refresh AIPass agent autostart: {err}");
                }
            }
            Err(err) => {
                eprintln!("failed to resolve AIPass agent binary: {err}");
            }
        }

        #[cfg(target_os = "macos")]
        if singleton::should_install_tray_autostart() {
            match std::env::current_exe() {
                Ok(desktop_binary) => {
                    if let Err(err) = ensure_tray_autostart_for_current_desktop(
                        &desktop_binary,
                        &client.config.vault_dir,
                    ) {
                        eprintln!("failed to refresh AIPass tray autostart: {err}");
                    }
                }
                Err(err) => {
                    eprintln!("failed to resolve AIPass desktop binary: {err}");
                }
            }
        }

        if let Err(err) = ensure_agent_running_for_desktop(&client) {
            eprintln!("failed to ensure AIPass agent is running: {err}");
        }
        if let Err(err) = repair_bundled_native_host_manifest(&app) {
            eprintln!("failed to repair bundled AIPass native host manifest: {err}");
        }
    });
}

fn repair_bundled_native_host_manifest(app: &AppHandle) -> Result<NativeHostStatus, String> {
    let package = bundled_extension_package(app)?;
    repair_native_host_manifest(vec![package.id])
}

#[cfg(target_os = "macos")]
fn configure_window_chrome(window: &tauri::WebviewWindow) {
    const WINDOW_CORNER_RADIUS: f64 = 12.0;

    let _ = window.set_background_color(Some(tauri::webview::Color(0, 0, 0, 0)));
    let _ = window.with_webview(|webview| unsafe {
        use objc2_app_kit::{NSColor, NSView, NSWindow};

        let ns_window: &NSWindow = &*webview.ns_window().cast();
        let clear = NSColor::clearColor();
        ns_window.setOpaque(false);
        ns_window.setBackgroundColor(Some(&clear));
        ns_window.setHasShadow(true);

        if let Some(content_view) = ns_window.contentView() {
            round_macos_view(&content_view, WINDOW_CORNER_RADIUS);
            if let Some(frame_view) = content_view.superview() {
                round_macos_view(&frame_view, WINDOW_CORNER_RADIUS);
            }
        }

        let webview_view: &NSView = &*webview.inner().cast();
        round_macos_view(webview_view, WINDOW_CORNER_RADIUS);
        ns_window.invalidateShadow();
    });
}

#[cfg(not(target_os = "macos"))]
fn configure_window_chrome(_: &tauri::WebviewWindow) {}

#[cfg(target_os = "macos")]
fn round_macos_view(view: &objc2_app_kit::NSView, radius: f64) {
    use objc2_quartz_core::kCACornerCurveContinuous;

    view.setWantsLayer(true);
    if let Some(layer) = view.layer() {
        layer.setCornerRadius(radius);
        let continuous_curve = unsafe { kCACornerCurveContinuous };
        layer.setCornerCurve(continuous_curve);
        layer.setMasksToBounds(true);
        layer.setOpaque(false);
    }
}

pub fn run() {
    let version = env!("CARGO_PKG_VERSION");
    let launch_target = launch_window_target();
    logging::init();
    #[cfg(target_os = "macos")]
    if self_install::install_from_dmg_if_needed() {
        return;
    }
    let _ = logging::log_event(
        "desktop.startup.begin",
        &[("version", version), ("target", &launch_target)],
    );
    let launch_deep_link_urls = singleton::deep_link_urls_from_args();
    let startup_deep_link_urls = launch_deep_link_urls.clone();
    let singleton = match singleton::acquire(version, &launch_target, launch_deep_link_urls) {
        Ok(singleton::SingletonDecision::Run(singleton)) => {
            let _ = logging::log_event("desktop.singleton.acquired", &[]);
            singleton
        }
        Ok(singleton::SingletonDecision::Exit) => {
            let _ = logging::log_event("desktop.singleton.existing_instance", &[]);
            return;
        }
        Err(err) => {
            let _ = logging::log_event("desktop.singleton.failed", &[]);
            eprintln!("failed to acquire AIPass desktop singleton: {err}");
            return;
        }
    };
    let mut singleton = Some(singleton);

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_deep_link::init())
        .manage(AppState::default())
        .setup(move |app| {
            let _ = logging::log_event("desktop.setup.begin", &[]);
            activate_window_target(app.handle(), &launch_target);
            let handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                handle_deep_link_urls(&handle, event.urls());
            });
            let mut startup_urls = match app.deep_link().get_current() {
                Ok(Some(urls)) => urls,
                Ok(None) => Vec::new(),
                Err(err) => {
                    eprintln!("failed to read cold-start deep link: {err}");
                    Vec::new()
                }
            };
            for url in startup_deep_link_urls
                .iter()
                .filter_map(|value| value.parse::<url::Url>().ok())
            {
                if !startup_urls.iter().any(|existing| existing == &url) {
                    startup_urls.push(url);
                }
            }
            handle_deep_link_urls(app.handle(), startup_urls);
            #[cfg(all(debug_assertions, not(target_os = "macos")))]
            if let Err(err) = app.deep_link().register_all() {
                eprintln!("failed to register development deep-link scheme: {err}");
            }
            if let Some(singleton) = singleton.take() {
                singleton::spawn_server(app.handle().clone(), singleton, version.to_string());
            }
            if let Err(err) = tray::setup(app) {
                let _ = logging::log_event("desktop.tray.failed", &[]);
                return Err(err.into());
            }
            let _ = logging::log_event("desktop.tray.ready", &[]);
            ensure_agent_resident_async(app.handle().clone());
            let extension_sync_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let _ = run_blocking(move || {
                    sync_installed_browser_extensions(&extension_sync_handle);
                    Ok::<(), String>(())
                })
                .await;
            });
            let _ = logging::log_event("desktop.setup.complete", &[]);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            window_target,
            desktop_ready,
            desktop_startup_stage,
            vault_status,
            session_touch,
            preferences_load,
            preferences_save,
            server_status,
            server_logs,
            server_start,
            server_stop,
            server_config_get,
            server_config_set,
            server_token_rotate,
            server_usage_summary,
            server_usage_clear,
            server_usage_timeseries,
            pricing_config_get,
            pricing_remote_sync,
            pricing_assignment_set,
            pricing_group_upsert,
            pricing_group_delete,
            pricing_group_version_delete,
            tool_config_detect,
            vault_create,
            vault_unlock,
            vault_recover,
            vault_reset,
            vault_auth_status,
            vault_lock,
            vault_change_password,
            vault_rotate,
            entries_list,
            entries_search,
            official_accounts_refresh,
            oauth_login_start,
            oauth_login_poll,
            oauth_login_cancel,
            oauth_accounts_list,
            oauth_accounts_remove,
            oauth_accounts_set_default,
            ccswitch_detect,
            ccswitch_import,
            provider_favicon_backfill,
            provider_add,
            provider_update,
            provider_archive,
            provider_restore,
            provider_trash,
            provider_favorite,
            provider_delete,
            entries_trash_list,
            entries_favorites_list,
            trash_purge_expired,
            trash_empty,
            secret_reveal_field,
            secret_reveal_headers,
            secret_add,
            secret_update,
            secret_metadata_set,
            secret_remove,
            devices_list,
            device_revoke,
            provider_probe,
            provider_usage_probe,
            provider_usage_apply,
            tool_config_preview,
            tool_config_apply,
            tool_config_proxy_preview,
            tool_config_proxy_apply,
            native_host_status,
            native_host_repair,
            browser_extension_status,
            browser_extension_install,
            vault_export_encrypted,
            vault_import_encrypted,
            sync_settings_load,
            sync_settings_save,
            sync_run_configured,
            sync_local,
            sync_cloud,
            sync_webdav_remote,
            sync_conflicts,
            sync_accept_conflict,
            sync_discard_conflict,
            check_for_updates,
            clear_pending_update,
            download_update,
            install_pending_update,
            install_update,
            take_pending_deep_links
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            RunEvent::ExitRequested { api, code, .. } => {
                let _ = persist_window_size(app);
                let explicitly_allowed = ALLOW_PROCESS_EXIT.swap(false, Ordering::SeqCst);
                if code.is_some() || explicitly_allowed {
                    let _ = logging::log_event("desktop.exit.allowed", &[]);
                } else {
                    api.prevent_exit();
                    let _ = logging::log_event("desktop.exit.intercepted", &[]);
                    activate_window_target(app, "tray");
                }
            }
            RunEvent::Exit => {
                let _ = logging::log_event("desktop.exit.completed", &[]);
            }
            _ => {}
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use aipass_provider_registry::{CredentialKind, EndpointKind, ProviderKind, SecretRef};

    #[test]
    fn extension_package_metadata_reads_zip_field_with_default_fallback() {
        let metadata_dir = Path::new("/bundle/browser-extension");
        let with_zip = r#"{"id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "version": "1.2.3", "zip": "custom.zip"}"#;
        let package = parse_extension_package_metadata(with_zip, metadata_dir).unwrap();
        assert_eq!(package.id, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert_eq!(package.version, "1.2.3");
        assert_eq!(package.zip_path, metadata_dir.join("custom.zip"));

        let without_zip = r#"{"id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "version": "1.2.3"}"#;
        let package = parse_extension_package_metadata(without_zip, metadata_dir).unwrap();
        assert_eq!(package.zip_path, metadata_dir.join("aipass-extension.zip"));
        // Older metadata has no manifest_version; it falls back to version.
        assert_eq!(package.manifest_version, "1.2.3");
    }

    #[test]
    fn extension_package_metadata_prefers_manifest_version() {
        let metadata_dir = Path::new("/bundle/browser-extension");
        let nightly = r#"{"id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "version": "0.3.0-nightly.20260905", "manifest_version": "0.3.0.1045"}"#;
        let package = parse_extension_package_metadata(nightly, metadata_dir).unwrap();
        assert_eq!(package.version, "0.3.0-nightly.20260905");
        assert_eq!(package.manifest_version, "0.3.0.1045");
    }

    #[test]
    fn parse_chrome_version_accepts_only_browser_compatible_versions() {
        assert_eq!(parse_chrome_version("0.3.0.1045"), Some([0, 3, 0, 1045]));
        assert_eq!(parse_chrome_version("1.2"), Some([1, 2, 0, 0]));
        assert_eq!(parse_chrome_version("0.3.0-nightly.20260905"), None);
        assert_eq!(parse_chrome_version("0.3.0.65536"), None);
        assert_eq!(parse_chrome_version("1.2.3.4.5"), None);
        assert_eq!(parse_chrome_version(""), None);
    }

    #[test]
    fn chrome_version_arrays_order_like_browser_versions() {
        let older = parse_chrome_version("0.3.0.1044").unwrap();
        let newer = parse_chrome_version("0.3.0.1045").unwrap();
        let stable = parse_chrome_version("0.3.0").unwrap();
        assert!(older < newer);
        assert!(stable < newer);
    }

    #[test]
    fn latest_installed_extension_version_picks_highest_valid_version_dir() {
        let root = std::env::temp_dir().join(format!("aipass-ext-ver-{}", Uuid::new_v4()));
        let extension_dir = root.join("Extensions").join("aabbccddeeffgghhiijjkkllmmnnoopp");
        fs::create_dir_all(extension_dir.join("0.3.0.1044")).unwrap();
        fs::write(extension_dir.join("0.3.0.1044").join("manifest.json"), b"{}").unwrap();
        fs::create_dir_all(extension_dir.join("0.3.0.1045")).unwrap();
        fs::write(extension_dir.join("0.3.0.1045").join("manifest.json"), b"{}").unwrap();
        // Invalid names and manifest-less dirs are ignored.
        fs::create_dir_all(extension_dir.join("0.3.0-nightly.20260905")).unwrap();
        fs::create_dir_all(extension_dir.join("0.3.0.9999")).unwrap();

        assert_eq!(
            latest_installed_extension_version(&extension_dir),
            Some([0, 3, 0, 1045])
        );
        assert_eq!(latest_installed_extension_version(&root.join("missing")), None);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn extract_extension_package_skips_entries_escaping_target_dir() {
        use std::io::Write;

        let root = std::env::temp_dir().join(format!("aipass-zip-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let zip_path = root.join("package.zip");
        {
            let file = fs::File::create(&zip_path).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            writer.start_file("../evil.txt", options).unwrap();
            writer.write_all(b"evil").unwrap();
            writer.start_file("nested/manifest.json", options).unwrap();
            writer.write_all(b"{}").unwrap();
            writer.finish().unwrap();
        }

        let extract_dir = root.join("extract");
        extract_extension_package(&zip_path, &extract_dir).unwrap();

        assert!(!root.join("evil.txt").exists());
        assert_eq!(
            fs::read_to_string(extract_dir.join("nested").join("manifest.json")).unwrap(),
            "{}"
        );

        // Re-extraction clears stale files from a previous version.
        fs::write(extract_dir.join("stale.txt"), b"stale").unwrap();
        extract_extension_package(&zip_path, &extract_dir).unwrap();
        assert!(!extract_dir.join("stale.txt").exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn endpoints_from_preserves_api_and_console_kinds() {
        let endpoints = endpoints_from(
            Some("https://api.example.com".to_string()),
            vec!["https://api-backup.example.com".to_string()],
            vec!["https://console.example.com".to_string()],
        );

        assert_eq!(endpoints.len(), 3);
        assert_eq!(endpoints[0].kind, EndpointKind::Api);
        assert_eq!(endpoints[1].kind, EndpointKind::Api);
        assert_eq!(endpoints[2].kind, EndpointKind::Console);
    }

    #[test]
    fn desktop_window_stays_gated_until_frontend_is_ready() {
        let mut state = DesktopWindowState::default();

        assert!(!state.set_target("main"));
        assert!(!state.set_target("server"));
        assert_eq!(state.complete_startup(), "server");
        assert!(state.set_target("main"));
    }

    #[test]
    fn desktop_window_target_defaults_to_main() {
        let mut state = DesktopWindowState::default();

        assert_eq!(state.complete_startup(), "main");
        assert!(state.set_target("unknown"));
        assert_eq!(state.target, "main");
    }

    #[test]
    fn desktop_window_size_restores_at_or_below_twenty_five_percent_deviation() {
        let default = default_window_size("main");
        let saved = DesktopWindowSize {
            width: default.width * 1.25,
            height: default.height * 0.75,
        };

        assert!(window_size_within_tolerance(&saved, &default));
    }

    #[test]
    fn desktop_window_size_falls_back_when_either_dimension_exceeds_tolerance() {
        let default = default_window_size("main");
        let saved = DesktopWindowSize {
            width: default.width * 1.251,
            height: default.height,
        };

        assert!(!window_size_within_tolerance(&saved, &default));
    }

    #[test]
    fn desktop_window_size_rejects_non_positive_and_non_finite_values() {
        let default = default_window_size("main");

        for saved in [
            DesktopWindowSize {
                width: 0.0,
                height: default.height,
            },
            DesktopWindowSize {
                width: f64::NAN,
                height: default.height,
            },
            DesktopWindowSize {
                width: default.width,
                height: f64::INFINITY,
            },
        ] {
            assert!(!window_size_within_tolerance(&saved, &default));
        }
    }

    #[test]
    fn desktop_main_window_size_never_drops_below_tauri_minimum() {
        let clamped = clamp_main_window_size(DesktopWindowSize {
            width: 960.0,
            height: 615.0,
        });

        assert_eq!(clamped.width, MIN_DESKTOP_WINDOW_WIDTH);
        assert_eq!(clamped.height, MIN_DESKTOP_WINDOW_HEIGHT);
    }

    #[test]
    fn desktop_special_window_targets_keep_their_fixed_sizes() {
        let main = default_window_size("main");

        assert!(target_uses_persisted_window_size("main"));
        assert!(target_uses_persisted_window_size("server"));
        assert!(!target_uses_persisted_window_size("unlock"));
        assert!(!target_uses_persisted_window_size("quick-access"));
        assert_eq!(default_window_size("unlock").width, 420.0);
        assert_eq!(default_window_size("quick-access").height, 640.0);
        assert_eq!(default_window_size("server").width, main.width);
    }

    #[test]
    fn native_messaging_dir_alone_does_not_count_as_browser_profile() {
        let root = std::env::temp_dir().join(format!("aipass-profile-test-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("NativeMessagingHosts")).unwrap();

        assert!(!profile_root_has_browser_data(&root));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn browser_profile_preferences_count_as_browser_profile() {
        let root = std::env::temp_dir().join(format!("aipass-profile-test-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("Default")).unwrap();
        fs::write(root.join("Default").join("Preferences"), "{}").unwrap();

        assert!(profile_root_has_browser_data(&root));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn gemini_probe_does_not_return_api_key_in_endpoint_or_error() {
        let secret = "AIzaSy-super-secret-test-key";
        let result = probe_entry(gemini_summary(), secret.to_string(), 1);
        let endpoint = result.endpoint.unwrap_or_default();

        assert!(!endpoint.contains(secret));
        assert!(endpoint.contains("key=[redacted]"));
        if let Some(error) = result.error {
            assert!(!error.contains(secret));
        }
    }

    fn gemini_summary() -> EntrySummary {
        let now = time::OffsetDateTime::now_utc();
        EntrySummary {
            id: Uuid::new_v4(),
            title: "Gemini".to_string(),
            favorite: false,
            provider_id: Some("gemini".to_string()),
            provider_kind: ProviderKind::Official,
            credential_kind: CredentialKind::Api,
            account_identity: None,
            domains: vec!["ai.google.dev".to_string()],
            favicon_url: None,
            endpoints: vec![ProviderEndpoint::api("http://127.0.0.1:9")],
            interface_type: InterfaceType::Gemini,
            auth_scheme: AuthScheme::GoogleApiKey,
            masked_secret: "AIza...test".to_string(),
            fingerprint: "fingerprint".to_string(),
            secret_refs: vec![SecretRef::new(
                "primary",
                "primary",
                "AIza...test",
                "fingerprint",
            )],
            default_model: None,
            model_aliases: Vec::new(),
            quota: None,
            gateway: None,
            tags: Vec::new(),
            notes: None,
            subscription: None,
            header_names: Vec::new(),
            created_at: now,
            updated_at: now,
            last_used_at: None,
            archived_at: None,
            deleted_at: None,
        }
    }

    #[test]
    fn aipass_manifest_signal_matches_short_name_when_name_is_placeholder() {
        let manifest = serde_json::json!({
            "name": "__MSG_extensionName__",
            "short_name": "AIPass",
            "permissions": ["nativeMessaging"]
        });

        assert!(value_contains_aipass_manifest_signal(&manifest));
    }

    #[test]
    fn aipass_manifest_signal_ignores_unrelated_manifests() {
        let native_messaging_only = serde_json::json!({
            "name": "Some Other Tool",
            "permissions": ["nativeMessaging"]
        });
        let unrelated_short_name = serde_json::json!({
            "name": "__MSG_extensionName__",
            "short_name": "Definitely Not AIPass"
        });

        assert!(!value_contains_aipass_manifest_signal(
            &native_messaging_only
        ));
        assert!(!value_contains_aipass_manifest_signal(
            &unrelated_short_name
        ));
    }

    #[test]
    fn merge_extension_ids_unions_normalizes_and_dedups() {
        let merged = merge_extension_ids(
            vec!["chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/".to_string()],
            vec!["BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB".to_string()],
            vec![
                "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                "chrome-extension://cccccccccccccccccccccccccccccccc/".to_string(),
            ],
        );

        assert_eq!(
            merged,
            vec![
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
                "cccccccccccccccccccccccccccccccc".to_string(),
            ]
        );
    }

    #[test]
    fn merge_extension_ids_handles_empty_and_partial_inputs() {
        let merged = merge_extension_ids(Vec::new(), Vec::new(), Vec::new());
        assert!(merged.is_empty());

        let merged = merge_extension_ids(
            Vec::<String>::new(),
            Vec::new(),
            vec!["chrome-extension://dddddddddddddddddddddddddddddddd/".to_string()],
        );
        assert_eq!(merged, vec!["dddddddddddddddddddddddddddddddd".to_string()]);
    }
}
