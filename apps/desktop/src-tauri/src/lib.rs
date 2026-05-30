mod auth_tasks;
mod commands;
mod models;
mod updates;

use commands::*;
use updates::{check_for_updates, install_update};

use crate::auth_tasks::AuthTasks;
use crate::models::{AppPreferences, NativeHostStatus, ProviderAddRequest, ProviderUpdateRequest};
use aipass_agent::{AgentClient, AgentClientConfig, AgentCommandError};
use aipass_agent_protocol::{AgentRequest, LockReason, SessionPolicy, SessionStatus};
use aipass_native_host::{
    load_allowed_extension_ids, native_host_settings_path, native_manifest,
    save_allowed_extension_ids,
};
use aipass_provider_registry::{provider_kind_for_id, ProviderEndpoint};
use aipass_storage::atomic_write_bytes;
use aipass_vault::{ProviderEntryInput, ProviderEntryUpdateInput};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(target_os = "windows")]
use std::process::Command as ProcessCommand;
use tauri::{AppHandle, LogicalSize, Manager, Size};

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

#[derive(Default)]
struct AppState {
    auth_tasks: AuthTasks,
}

fn agent_client(app: &AppHandle) -> Result<AgentClient, String> {
    let config = AgentClientConfig::for_vault(vault_dir(app)?).map_err(|err| err.to_string())?;
    Ok(AgentClient::new(config))
}

fn agent_request<T: DeserializeOwned>(app: &AppHandle, request: AgentRequest) -> Result<T, String> {
    let client = agent_client(app)?;
    client.ensure_running().map_err(|err| err.to_string())?;
    client.request(&request).map_err(agent_error_to_string)
}

fn agent_request_no_unlock<T: DeserializeOwned>(
    app: &AppHandle,
    request: AgentRequest,
) -> Result<T, String> {
    let client = agent_client(app)?;
    client.ensure_running().map_err(|err| err.to_string())?;
    client.request(&request).map_err(agent_error_to_string)
}

fn agent_status(app: &AppHandle) -> SessionStatus {
    agent_request_no_unlock::<SessionStatus>(app, AgentRequest::SessionStatus).unwrap_or(
        SessionStatus {
            exists: vault_dir(app)
                .map(|root| root.join("manifest.aipmanifest").exists())
                .unwrap_or(false),
            locked: true,
            policy: SessionPolicy::default(),
            last_lock_reason: Some(LockReason::AgentRestart),
            vault_namespace: None,
        },
    )
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
        default_model: request.default_model.and_then(non_empty),
        model_aliases: clean_pairs(request.model_aliases),
        headers: request.headers,
        quota: request.quota,
        gateway: request.gateway,
        tags: clean_strings(request.tags),
        environment: non_empty(request.environment).unwrap_or_else(|| "personal".to_string()),
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
        environment: non_empty(request.environment).unwrap_or_else(|| "personal".to_string()),
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

fn native_host_status_snapshot() -> Result<NativeHostStatus, String> {
    let host_path = native_host_binary_path()?;
    let manifest_path = default_chrome_native_manifest_path()?;
    let allowed_origins = read_manifest_allowed_origins(&manifest_path);
    let settings_path = native_host_settings_path().map_err(|err| err.to_string())?;
    let allowed_extension_ids = load_allowed_extension_ids().map_err(|err| err.to_string())?;
    Ok(NativeHostStatus {
        browser: "chrome".to_string(),
        host_exists: host_path.exists(),
        host_path,
        manifest_exists: manifest_path.exists(),
        manifest_path,
        settings_path,
        allowed_extension_ids,
        allowed_origins,
    })
}

fn repair_native_host_manifest(extension_ids: Vec<String>) -> Result<NativeHostStatus, String> {
    let host_path = native_host_binary_path()?;
    let manifest_path = default_chrome_native_manifest_path()?;
    let origins = allowed_origins(&extension_ids)?;
    if let Some(parent) = manifest_path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let manifest = native_manifest(&host_path, &origins);
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(|err| err.to_string())?;
    atomic_write_bytes(&manifest_path, &bytes).map_err(|err| err.to_string())?;
    save_allowed_extension_ids(&extension_ids).map_err(|err| err.to_string())?;
    install_chrome_native_manifest_reference(&manifest_path)?;
    native_host_status_snapshot()
}

fn native_host_binary_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    let host_name = if cfg!(target_os = "windows") {
        "aipass-native-host.exe"
    } else {
        "aipass-native-host"
    };
    Ok(exe.with_file_name(host_name))
}

fn default_chrome_native_manifest_path() -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").ok_or_else(|| "HOME is not set".to_string())?;
        Ok(PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("Google")
            .join("Chrome")
            .join("NativeMessagingHosts")
            .join("dev.aipass.native.json"))
    }

    #[cfg(target_os = "linux")]
    {
        let home = std::env::var_os("HOME").ok_or_else(|| "HOME is not set".to_string())?;
        Ok(PathBuf::from(home)
            .join(".config")
            .join("google-chrome")
            .join("NativeMessagingHosts")
            .join("dev.aipass.native.json"))
    }

    #[cfg(target_os = "windows")]
    {
        let app_data =
            std::env::var_os("APPDATA").ok_or_else(|| "APPDATA is not set".to_string())?;
        Ok(PathBuf::from(app_data)
            .join("AIPass")
            .join("NativeMessagingHosts")
            .join("dev.aipass.native.json"))
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Err("native host repair is not supported on this platform".to_string())
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
        return Err("enter at least one Chrome extension id".to_string());
    }
    Ok(origins)
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

fn install_chrome_native_manifest_reference(manifest_path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let status = ProcessCommand::new("reg")
            .args([
                "add",
                r"HKCU\Software\Google\Chrome\NativeMessagingHosts\dev.aipass.native",
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
        let _ = manifest_path;
    }

    Ok(())
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|err| err.to_string())?;
    atomic_write_bytes(path, &bytes).map_err(|err| err.to_string())
}

fn vault_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|err| err.to_string())?
        .join("vault"))
}

fn configure_initial_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let target = std::env::var("AIPASS_WINDOW_TARGET").unwrap_or_else(|_| "main".to_string());
    let (title, width, height) = match target.as_str() {
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
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState::default())
        .setup(|app| {
            configure_initial_window(app.handle());
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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
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
            environment: "test".to_string(),
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
