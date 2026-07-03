mod auth_tasks;
mod commands;
mod models;
mod singleton;
mod tray;
mod updates;

use commands::*;
use updates::{check_for_updates, install_update};

use crate::auth_tasks::AuthTasks;
use crate::models::{
    AppPreferences, BrowserExtensionInstallMode, BrowserExtensionInstallResult,
    BrowserExtensionStatus, NativeHostStatus, ProviderAddRequest, ProviderUpdateRequest,
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
use std::thread;
use tauri::{AppHandle, LogicalSize, Manager, Size};

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

fn agent_request<T: DeserializeOwned>(app: &AppHandle, request: AgentRequest) -> Result<T, String> {
    let client = agent_client(app)?;
    client
        .ensure_running_for_desktop_companion()
        .map_err(|err| err.to_string())?;
    client.request(&request).map_err(agent_error_to_string)
}

fn agent_request_no_unlock<T: DeserializeOwned>(
    app: &AppHandle,
    request: AgentRequest,
) -> Result<T, String> {
    let client = agent_client(app)?;
    client
        .ensure_running_for_desktop_companion()
        .map_err(|err| err.to_string())?;
    client.request(&request).map_err(agent_error_to_string)
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
        secret_label: None,
        default_model: request.default_model.and_then(non_empty),
        model_aliases: clean_pairs(request.model_aliases),
        headers: request.headers,
        quota: request.quota,
        gateway: request.gateway,
        tags: clean_strings(request.tags),
        notes: request.notes.and_then(non_empty),
    }
}

fn provider_update_input(request: ProviderUpdateRequest) -> ProviderEntryUpdateInput {
    let provider_kind = provider_kind_for_id(request.provider_id.as_deref());
    ProviderEntryUpdateInput {
        title: non_empty(request.title).unwrap_or_else(|| "Custom Provider".to_string()),
        provider_kind,
        provider_id: request.provider_id,
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
        default_model: request.default_model.and_then(non_empty),
        model_aliases: clean_pairs(request.model_aliases),
        headers: request.headers,
        quota: request.quota,
        gateway: request.gateway,
        tags: clean_strings(request.tags),
        notes: request.notes.and_then(non_empty),
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

fn load_preferences(app: &AppHandle) -> Result<AppPreferences, String> {
    let path = preferences_path(app)?;
    if !path.exists() {
        return Ok(AppPreferences::default());
    }
    let bytes = fs::read(&path).map_err(|err| err.to_string())?;
    serde_json::from_slice(&bytes).or_else(|_| Ok(AppPreferences::default()))
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
    let external_install_path = primary_target
        .map(|target| default_external_extension_path(target, &package.id))
        .transpose()?
        .flatten();
    let installed_paths = installed_extension_paths(&extension_ids);
    let native_host_configured = native_hosts
        .iter()
        .any(|status| native_host_status_allows(status, &extension_ids));

    let crx_exists = package.crx_path.exists()
        && fs::metadata(&package.crx_path)
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
        crx_exists,
        crx_path: package.crx_path,
        extension_installed: !installed_paths.is_empty(),
        installed_paths,
        external_install_exists: external_install_path
            .as_ref()
            .is_some_and(|path| path.exists()),
        external_install_path,
        native_host_configured,
        install_mode: if cfg!(target_os = "linux") {
            BrowserExtensionInstallMode::ExternalCrx
        } else {
            BrowserExtensionInstallMode::ManualCrx
        },
        native_host,
        native_hosts,
    })
}

fn install_browser_extension(app: &AppHandle) -> Result<BrowserExtensionInstallResult, String> {
    let package = bundled_extension_package(app)?;
    if !package.crx_path.exists()
        || fs::metadata(&package.crx_path)
            .map(|metadata| metadata.len() == 0)
            .unwrap_or(true)
        || package.version == "0.0.0"
    {
        return Err(format!(
            "bundled Chrome extension package is missing: {}",
            package.crx_path.display()
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

    let external_install_ok =
        cfg!(target_os = "linux") && install_external_crx(target, &package).is_ok();

    let opened_chrome = open_browser_extensions_page(target).is_ok();
    let opened_package = if cfg!(target_os = "linux") {
        !external_install_ok && reveal_path(&package.crx_path).is_ok()
    } else {
        reveal_path(&package.crx_path).is_ok()
    };
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
    crx_path: PathBuf,
}

fn bundled_extension_package(app: &AppHandle) -> Result<ExtensionPackage, String> {
    let metadata_path = bundled_extension_metadata_path(app)?;
    let metadata_text = fs::read_to_string(&metadata_path).map_err(|err| {
        format!(
            "failed to read bundled extension metadata at {}: {err}",
            metadata_path.display()
        )
    })?;
    let metadata: serde_json::Value = serde_json::from_str(&metadata_text).map_err(|err| {
        format!(
            "failed to parse bundled extension metadata at {}: {err}",
            metadata_path.display()
        )
    })?;
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
    let crx_name = metadata
        .get("crx")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("aipass-extension.crx");
    let metadata_dir = metadata_path
        .parent()
        .ok_or_else(|| "bundled extension metadata path has no parent".to_string())?;
    Ok(ExtensionPackage {
        id,
        version,
        crx_path: metadata_dir.join(crx_name),
    })
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
    #[cfg(target_os = "linux")]
    external_extension_dir: Option<PathBuf>,
    #[cfg(target_os = "windows")]
    native_host_registry_key: &'static str,
}

fn default_external_extension_path(
    target: &BrowserTarget,
    extension_id: &str,
) -> Result<Option<PathBuf>, String> {
    #[cfg(target_os = "linux")]
    {
        Ok(target
            .external_extension_dir
            .as_ref()
            .map(|dir| dir.join(format!("{extension_id}.json"))))
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = target;
        let _ = extension_id;
        Ok(None)
    }
}

fn install_external_crx(target: &BrowserTarget, package: &ExtensionPackage) -> Result<(), String> {
    let install_path = default_external_extension_path(target, &package.id)?
        .ok_or_else(|| "local CRX external install is only supported on Linux".to_string())?;
    let copied_crx_path = user_extension_package_path(&package.id)?;
    if let Some(parent) = copied_crx_path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    fs::copy(&package.crx_path, &copied_crx_path).map_err(|err| err.to_string())?;
    let external = serde_json::json!({
        "external_crx": copied_crx_path,
        "external_version": package.version,
    });
    write_json_atomic(&install_path, &external)
}

fn user_extension_package_path(extension_id: &str) -> Result<PathBuf, String> {
    let dirs = directories::ProjectDirs::from("dev", "aipass", "desktop")
        .ok_or_else(|| "cannot determine AIPass project directory".to_string())?;
    Ok(dirs
        .data_dir()
        .join("browser-extension")
        .join(format!("{extension_id}.crx")))
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
                Some(PathBuf::from("/opt/google/chrome/extensions")),
            ),
            linux_browser_target(
                "edge",
                "Microsoft Edge",
                config.join("microsoft-edge"),
                &["microsoft-edge", "microsoft-edge-stable"],
                Some(PathBuf::from("/opt/microsoft/msedge/extensions")),
            ),
            linux_browser_target(
                "brave",
                "Brave",
                config.join("BraveSoftware").join("Brave-Browser"),
                &["brave-browser", "brave"],
                Some(PathBuf::from("/opt/brave.com/brave/extensions")),
            ),
            linux_browser_target(
                "chromium",
                "Chromium",
                config.join("chromium"),
                &["chromium", "chromium-browser"],
                Some(PathBuf::from("/usr/share/chromium/extensions")),
            ),
            linux_browser_target(
                "vivaldi",
                "Vivaldi",
                config.join("vivaldi"),
                &["vivaldi", "vivaldi-stable"],
                None,
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
    external_extension_dir: Option<PathBuf>,
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
        external_extension_dir,
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
    let mut ids = BTreeSet::new();
    ids.extend(
        extension_ids
            .into_iter()
            .map(|id| normalized_extension_id(&id))
            .filter(|id| !id.is_empty()),
    );
    ids.extend(discover_aipass_extension_ids());
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
        serde_json::Value::String(value) => value.contains("\"name\": \"AIPass\""),
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
    let origins = allowed_origins(&extension_ids)?;
    let targets = repair_browser_targets()?;
    for target in &targets {
        if let Some(parent) = target.manifest_path.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let manifest = native_manifest(&host_path, &origins);
        let bytes = serde_json::to_vec_pretty(&manifest).map_err(|err| err.to_string())?;
        atomic_write_bytes(&target.manifest_path, &bytes).map_err(|err| err.to_string())?;
        install_native_manifest_reference(target, &target.manifest_path)?;
    }
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
        .filter(|value| matches!(value.as_str(), "main" | "unlock" | "quick-access" | "tray"))
        .unwrap_or_else(|| "main".to_string())
}

fn configure_window_target(app: &AppHandle, target: &str) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if target == "tray" {
        #[cfg(target_os = "macos")]
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
        let _ = window.hide();
        return;
    }

    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);

    let (title, width, height) = match target {
        "unlock" => ("AIPass Unlock", 420.0, 560.0),
        "quick-access" => ("AIPass Quick Access", 520.0, 640.0),
        _ => ("AIPass", 1280.0, 820.0),
    };
    let _ = window.set_title(title);
    let _ = window.set_size(Size::Logical(LogicalSize { width, height }));
    configure_window_chrome(&window);
    let _ = window.center();
    let _ = window.show();
    let _ = window.set_focus();
}

pub(crate) fn activate_window_target(app: &AppHandle, target: &str) {
    if target == "tray" {
        return;
    }
    configure_window_target(app, target);
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
        if let Err(err) = client.ensure_running_for_desktop_companion() {
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
    const WINDOW_CORNER_RADIUS: f64 = 10.0;

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
    view.setWantsLayer(true);
    if let Some(layer) = view.layer() {
        layer.setCornerRadius(radius);
        layer.setMasksToBounds(true);
        layer.setOpaque(false);
    }
}

pub fn run() {
    let version = env!("CARGO_PKG_VERSION");
    let launch_target = launch_window_target();
    let singleton = match singleton::acquire(version, &launch_target) {
        Ok(singleton::SingletonDecision::Run(singleton)) => singleton,
        Ok(singleton::SingletonDecision::Exit) => return,
        Err(err) => {
            eprintln!("failed to acquire AIPass desktop singleton: {err}");
            return;
        }
    };
    let mut singleton = Some(singleton);

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState::default())
        .setup(move |app| {
            configure_window_target(app.handle(), &launch_target);
            if let Some(singleton) = singleton.take() {
                singleton::spawn_server(app.handle().clone(), singleton, version.to_string());
            }
            tray::setup(app)?;
            ensure_agent_resident_async(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            window_target,
            vault_status,
            session_touch,
            preferences_load,
            preferences_save,
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
            provider_add,
            provider_update,
            provider_archive,
            provider_restore,
            provider_trash,
            provider_delete,
            entries_trash_list,
            trash_purge_expired,
            trash_empty,
            secret_reveal_field,
            secret_add,
            secret_remove,
            devices_list,
            device_revoke,
            provider_probe,
            tool_config_preview,
            tool_config_apply,
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
            install_update
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, _event| {});
}

#[cfg(test)]
mod tests {
    use super::*;
    use aipass_provider_registry::{EndpointKind, ProviderKind, SecretRef};

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
            provider_id: Some("gemini".to_string()),
            provider_kind: ProviderKind::Official,
            domains: vec!["ai.google.dev".to_string()],
            favicon_url: None,
            endpoints: vec![ProviderEndpoint::api("http://127.0.0.1:9")],
            interface_type: InterfaceType::Gemini,
            auth_scheme: AuthScheme::GoogleApiKey,
            masked_secret: "AIza...test".to_string(),
            fingerprint: "fingerprint".to_string(),
            secret_refs: vec![SecretRef {
                id: "primary".to_string(),
                label: "primary".to_string(),
                masked: "AIza...test".to_string(),
                fingerprint: "fingerprint".to_string(),
            }],
            default_model: None,
            model_aliases: Vec::new(),
            quota: None,
            gateway: None,
            tags: Vec::new(),
            notes: None,
            header_names: Vec::new(),
            created_at: now,
            updated_at: now,
            last_used_at: None,
            archived_at: None,
            deleted_at: None,
        }
    }
}
