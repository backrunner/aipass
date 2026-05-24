use aipass_crypto::SecretString;
use aipass_provider_registry::{
    provider_kind_for_id, AuthScheme, EndpointKind, InterfaceType, ProviderEndpoint, QuotaInfo,
};
use aipass_sync::{
    accept_conflict, discard_conflict, list_conflicts, sync_local_folder, sync_webdav,
    ConflictRecord, HttpWebDavClient, SyncObject, SyncReport,
};
use aipass_vault::{
    DeviceRecord, EncryptedVaultExport, EntrySummary, ProviderEntryInput, ProviderEntryUpdateInput,
    RecoveryKit, Vault,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

#[derive(Default)]
struct AppState {
    session: Mutex<Option<Vault>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct VaultStatus {
    exists: bool,
    locked: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateVaultRequest {
    password: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateVaultResponse {
    exists: bool,
    locked: bool,
    recovery_kit: RecoveryKit,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UnlockVaultRequest {
    password: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryVaultRequest {
    recovery_key: String,
    new_password: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChangePasswordRequest {
    new_password: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderAddRequest {
    title: String,
    provider_id: Option<String>,
    #[serde(default)]
    domain: Vec<String>,
    endpoint: Option<String>,
    favicon_url: Option<String>,
    interface_type: InterfaceType,
    auth_scheme: AuthScheme,
    api_key: String,
    default_model: Option<String>,
    #[serde(default)]
    headers: Vec<(String, String)>,
    quota: Option<QuotaInfo>,
    environment: String,
    #[serde(default)]
    tags: Vec<String>,
    notes: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderUpdateRequest {
    id: Uuid,
    title: String,
    provider_id: Option<String>,
    #[serde(default)]
    domain: Vec<String>,
    endpoint: Option<String>,
    favicon_url: Option<String>,
    interface_type: InterfaceType,
    auth_scheme: AuthScheme,
    api_key: Option<String>,
    default_model: Option<String>,
    headers: Option<Vec<(String, String)>>,
    quota: Option<QuotaInfo>,
    environment: String,
    #[serde(default)]
    tags: Vec<String>,
    notes: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProbeResult {
    ok: bool,
    provider_id: Option<String>,
    interface_type: InterfaceType,
    status: Option<u16>,
    endpoint: Option<String>,
    model_count: Option<usize>,
    error: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VaultExportRequest {
    output: PathBuf,
    export_password: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VaultImportRequest {
    input: PathBuf,
    export_password: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncLocalRequest {
    dir: PathBuf,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncWebDavRequest {
    url: String,
    username: Option<String>,
    password: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncConflictsRequest {
    dir: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ConflictScope {
    Vault,
    Sync,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncConflictActionRequest {
    scope: ConflictScope,
    dir: Option<PathBuf>,
    conflict_path: PathBuf,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncConflictResponse {
    scope: ConflictScope,
    origin: String,
    conflict_path: PathBuf,
    target_path: PathBuf,
    object: SyncObject,
    conflict_summary: Option<EntrySummary>,
    target_summary: Option<EntrySummary>,
}

#[tauri::command]
fn vault_status(app: AppHandle, state: State<'_, AppState>) -> Result<VaultStatus, String> {
    let root = vault_dir(&app)?;
    let locked = state
        .session
        .lock()
        .map_err(|_| "session lock poisoned".to_string())?
        .is_none();
    Ok(VaultStatus {
        exists: root.join("manifest.aipmanifest").exists(),
        locked,
    })
}

#[tauri::command]
fn vault_create(
    app: AppHandle,
    state: State<'_, AppState>,
    request: CreateVaultRequest,
) -> Result<CreateVaultResponse, String> {
    let root = vault_dir(&app)?;
    let creation = Vault::create(&root, &SecretString::new(request.password))
        .map_err(|err| err.to_string())?;
    let recovery_kit = creation.recovery_kit;
    *state
        .session
        .lock()
        .map_err(|_| "session lock poisoned".to_string())? = Some(creation.vault);
    Ok(CreateVaultResponse {
        exists: true,
        locked: false,
        recovery_kit,
    })
}

#[tauri::command]
fn vault_unlock(
    app: AppHandle,
    state: State<'_, AppState>,
    request: UnlockVaultRequest,
) -> Result<VaultStatus, String> {
    let root = vault_dir(&app)?;
    let vault =
        Vault::open(&root, &SecretString::new(request.password)).map_err(|err| err.to_string())?;
    *state
        .session
        .lock()
        .map_err(|_| "session lock poisoned".to_string())? = Some(vault);
    Ok(VaultStatus {
        exists: true,
        locked: false,
    })
}

#[tauri::command]
fn vault_recover(
    app: AppHandle,
    state: State<'_, AppState>,
    request: RecoveryVaultRequest,
) -> Result<CreateVaultResponse, String> {
    let root = vault_dir(&app)?;
    let creation = Vault::recover_master_password(
        &root,
        &SecretString::new(request.recovery_key),
        &SecretString::new(request.new_password),
    )
    .map_err(|err| err.to_string())?;
    let recovery_kit = creation.recovery_kit;
    *state
        .session
        .lock()
        .map_err(|_| "session lock poisoned".to_string())? = Some(creation.vault);
    Ok(CreateVaultResponse {
        exists: true,
        locked: false,
        recovery_kit,
    })
}

#[tauri::command]
fn vault_lock(state: State<'_, AppState>) -> Result<VaultStatus, String> {
    *state
        .session
        .lock()
        .map_err(|_| "session lock poisoned".to_string())? = None;
    Ok(VaultStatus {
        exists: true,
        locked: true,
    })
}

#[tauri::command]
fn vault_change_password(
    state: State<'_, AppState>,
    request: ChangePasswordRequest,
) -> Result<(), String> {
    with_vault_mut(state, |vault| {
        vault
            .change_master_password(&SecretString::new(request.new_password))
            .map_err(|err| err.to_string())
    })
}

#[tauri::command]
fn vault_rotate(state: State<'_, AppState>) -> Result<(), String> {
    with_vault_mut(state, |vault| {
        vault
            .advance_epoch_and_rewrap("desktop.rotate")
            .map(|_| ())
            .map_err(|err| err.to_string())
    })
}

#[tauri::command]
fn entries_list(
    state: State<'_, AppState>,
    archived: Option<bool>,
) -> Result<Vec<EntrySummary>, String> {
    with_vault(state, |vault| {
        if archived.unwrap_or(false) {
            vault
                .list_archived_provider_summaries()
                .map_err(|err| err.to_string())
        } else {
            vault
                .list_provider_summaries()
                .map_err(|err| err.to_string())
        }
    })
}

#[tauri::command]
fn entries_search(state: State<'_, AppState>, query: String) -> Result<Vec<EntrySummary>, String> {
    with_vault(state, |vault| {
        vault.search(&query).map_err(|err| err.to_string())
    })
}

#[tauri::command]
fn provider_add(state: State<'_, AppState>, request: ProviderAddRequest) -> Result<Uuid, String> {
    with_vault(state, |vault| {
        vault
            .add_provider(provider_add_input(request))
            .map_err(|err| err.to_string())
    })
}

#[tauri::command]
fn provider_update(
    state: State<'_, AppState>,
    request: ProviderUpdateRequest,
) -> Result<(), String> {
    with_vault(state, |vault| {
        let id = request.id;
        vault
            .update_provider(id, provider_update_input(request))
            .map_err(|err| err.to_string())
    })
}

#[tauri::command]
fn provider_archive(state: State<'_, AppState>, id: Uuid) -> Result<(), String> {
    with_vault(state, |vault| {
        vault.archive_provider(id).map_err(|err| err.to_string())
    })
}

#[tauri::command]
fn provider_restore(state: State<'_, AppState>, id: Uuid) -> Result<(), String> {
    with_vault(state, |vault| {
        vault.restore_provider(id).map_err(|err| err.to_string())
    })
}

#[tauri::command]
fn provider_delete(state: State<'_, AppState>, id: Uuid) -> Result<(), String> {
    with_vault(state, |vault| {
        vault
            .delete_provider_permanently(id)
            .map_err(|err| err.to_string())
    })
}

#[tauri::command]
fn secret_reveal_field(
    state: State<'_, AppState>,
    id: Uuid,
    field: String,
) -> Result<String, String> {
    with_vault(state, |vault| {
        vault
            .reveal_secret_field(id, &field)
            .map_err(|err| err.to_string())
    })
}

#[tauri::command]
fn secret_add(
    state: State<'_, AppState>,
    id: Uuid,
    label: String,
    api_key: String,
) -> Result<String, String> {
    with_vault(state, |vault| {
        vault
            .add_secret(id, label, api_key)
            .map_err(|err| err.to_string())
    })
}

#[tauri::command]
fn secret_remove(state: State<'_, AppState>, id: Uuid, label: String) -> Result<(), String> {
    with_vault(state, |vault| {
        vault
            .remove_secret(id, &label)
            .map_err(|err| err.to_string())
    })
}

#[tauri::command]
fn devices_list(state: State<'_, AppState>) -> Result<Vec<DeviceRecord>, String> {
    with_vault(state, |vault| {
        vault.list_devices().map_err(|err| err.to_string())
    })
}

#[tauri::command]
fn device_revoke(state: State<'_, AppState>, id: Uuid) -> Result<(), String> {
    with_vault_mut(state, |vault| {
        vault.revoke_device(id).map_err(|err| err.to_string())
    })
}

#[tauri::command]
fn provider_probe(
    state: State<'_, AppState>,
    id: Uuid,
    timeout_seconds: Option<u64>,
) -> Result<ProbeResult, String> {
    let (entry, secret) = with_vault(state, |vault| {
        Ok((
            vault
                .get_provider_summary(id)
                .map_err(|err| err.to_string())?,
            vault.reveal_secret(id).map_err(|err| err.to_string())?,
        ))
    })?;
    Ok(probe_entry(entry, secret, timeout_seconds.unwrap_or(15)))
}

#[tauri::command]
fn vault_export_encrypted(
    state: State<'_, AppState>,
    request: VaultExportRequest,
) -> Result<(), String> {
    with_vault(state, |vault| {
        let export = vault
            .export_encrypted(&SecretString::new(request.export_password))
            .map_err(|err| err.to_string())?;
        if let Some(parent) = request.output.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        fs::write(
            &request.output,
            serde_json::to_vec_pretty(&export).map_err(|err| err.to_string())?,
        )
        .map_err(|err| err.to_string())
    })
}

#[tauri::command]
fn vault_import_encrypted(
    app: AppHandle,
    state: State<'_, AppState>,
    request: VaultImportRequest,
) -> Result<(), String> {
    let root = vault_dir(&app)?;
    let export: EncryptedVaultExport =
        serde_json::from_slice(&fs::read(&request.input).map_err(|err| err.to_string())?)
            .map_err(|err| err.to_string())?;
    let backup = if root.exists() {
        let backup = root.with_file_name(format!(
            "vault-import-backup-{}",
            time::OffsetDateTime::now_utc().unix_timestamp()
        ));
        fs::rename(&root, &backup).map_err(|err| err.to_string())?;
        Some(backup)
    } else {
        None
    };

    let import_result =
        Vault::import_encrypted(&root, &SecretString::new(request.export_password), &export)
            .map_err(|err| err.to_string());
    if let Err(err) = import_result {
        if let Some(backup) = backup {
            let _ = fs::remove_dir_all(&root);
            let _ = fs::rename(backup, &root);
        }
        return Err(err);
    }

    *state
        .session
        .lock()
        .map_err(|_| "session lock poisoned".to_string())? = None;
    Ok(())
}

#[tauri::command]
fn sync_local(app: AppHandle, request: SyncLocalRequest) -> Result<SyncReport, String> {
    sync_local_folder(&vault_dir(&app)?, &request.dir).map_err(|err| err.to_string())
}

#[tauri::command]
fn sync_webdav_remote(app: AppHandle, request: SyncWebDavRequest) -> Result<SyncReport, String> {
    let client = HttpWebDavClient::new(&request.url, request.username, request.password)
        .map_err(|err| err.to_string())?;
    sync_webdav(&vault_dir(&app)?, &client).map_err(|err| err.to_string())
}

#[tauri::command]
fn sync_conflicts(
    app: AppHandle,
    state: State<'_, AppState>,
    request: SyncConflictsRequest,
) -> Result<Vec<SyncConflictResponse>, String> {
    with_vault(state, |vault| {
        let mut conflicts = conflict_responses(ConflictScope::Vault, &vault_dir(&app)?, vault)?;
        if let Some(dir) = request.dir {
            conflicts.extend(conflict_responses(ConflictScope::Sync, &dir, vault)?);
        }
        Ok(conflicts)
    })
}

#[tauri::command]
fn sync_accept_conflict(app: AppHandle, request: SyncConflictActionRequest) -> Result<(), String> {
    let root = conflict_root(&app, &request)?;
    accept_conflict(&root, &request.conflict_path).map_err(|err| err.to_string())
}

#[tauri::command]
fn sync_discard_conflict(app: AppHandle, request: SyncConflictActionRequest) -> Result<(), String> {
    let root = conflict_root(&app, &request)?;
    discard_conflict(&root, &request.conflict_path).map_err(|err| err.to_string())
}

fn provider_add_input(request: ProviderAddRequest) -> ProviderEntryInput {
    let provider_kind = provider_kind_for_id(request.provider_id.as_deref());
    ProviderEntryInput {
        title: non_empty(request.title).unwrap_or_else(|| "Custom Provider".to_string()),
        provider_kind,
        provider_id: request.provider_id,
        domains: clean_strings(request.domain),
        favicon_url: request.favicon_url.and_then(non_empty),
        endpoints: endpoints_from(request.endpoint),
        interface_type: request.interface_type,
        auth_scheme: request.auth_scheme,
        api_key: request.api_key,
        default_model: request.default_model.and_then(non_empty),
        headers: request.headers,
        quota: request.quota,
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
        endpoints: endpoints_from(request.endpoint),
        interface_type: request.interface_type,
        auth_scheme: request.auth_scheme,
        api_key: request.api_key.and_then(non_empty),
        default_model: request.default_model.and_then(non_empty),
        headers: request.headers,
        quota: request.quota,
        tags: clean_strings(request.tags),
        environment: non_empty(request.environment).unwrap_or_else(|| "personal".to_string()),
        notes: request.notes.and_then(non_empty),
    }
}

fn endpoints_from(endpoint: Option<String>) -> Vec<ProviderEndpoint> {
    endpoint
        .and_then(non_empty)
        .map(ProviderEndpoint::api)
        .into_iter()
        .collect()
}

fn clean_strings(values: Vec<String>) -> Vec<String> {
    values.into_iter().filter_map(non_empty).collect()
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

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

fn endpoint_url(endpoints: &[ProviderEndpoint]) -> Option<String> {
    endpoints
        .iter()
        .find(|endpoint| endpoint.kind == EndpointKind::Api)
        .and_then(|endpoint| endpoint.url.clone())
        .or_else(|| endpoints.iter().find_map(|endpoint| endpoint.url.clone()))
}

fn join_url(base: &str, suffix: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        suffix.trim_start_matches('/')
    )
}

fn append_query_param(url: &str, key: &str, value: &str) -> String {
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}{key}={value}")
}

fn model_count(value: &serde_json::Value) -> Option<usize> {
    value
        .get("data")
        .or_else(|| value.get("models"))
        .and_then(|value| value.as_array())
        .map(Vec::len)
}

fn redact_error(value: &str, secret: &str) -> String {
    if secret.is_empty() {
        value.to_string()
    } else {
        value.replace(secret, "[redacted]")
    }
}

fn conflict_responses(
    scope: ConflictScope,
    root: &Path,
    vault: &Vault,
) -> Result<Vec<SyncConflictResponse>, String> {
    list_conflicts(root)
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(|record| conflict_response(scope.clone(), root, vault, record))
        .collect()
}

fn conflict_response(
    scope: ConflictScope,
    root: &Path,
    vault: &Vault,
    record: ConflictRecord,
) -> Result<SyncConflictResponse, String> {
    let conflict_summary = summary_from_conflict_path(vault, root, &record.conflict_path, &record);
    let target_summary = summary_from_conflict_path(vault, root, &record.target_path, &record);
    Ok(SyncConflictResponse {
        scope,
        origin: record.origin,
        conflict_path: record.conflict_path,
        target_path: record.target_path,
        object: record.object,
        conflict_summary,
        target_summary,
    })
}

fn summary_from_conflict_path(
    vault: &Vault,
    root: &Path,
    relative_path: &Path,
    record: &ConflictRecord,
) -> Option<EntrySummary> {
    if record.object.object_type != "provider_entry" {
        return None;
    }
    vault
        .get_provider_summary_from_path(root.join(relative_path))
        .ok()
}

fn conflict_root(app: &AppHandle, request: &SyncConflictActionRequest) -> Result<PathBuf, String> {
    match request.scope {
        ConflictScope::Vault => vault_dir(app),
        ConflictScope::Sync => request
            .dir
            .clone()
            .ok_or_else(|| "sync conflict scope requires a local sync dir".to_string()),
    }
}

fn with_vault<T>(
    state: State<'_, AppState>,
    f: impl FnOnce(&Vault) -> Result<T, String>,
) -> Result<T, String> {
    let guard = state
        .session
        .lock()
        .map_err(|_| "session lock poisoned".to_string())?;
    let vault = guard
        .as_ref()
        .ok_or_else(|| "vault is locked".to_string())?;
    f(vault)
}

fn with_vault_mut<T>(
    state: State<'_, AppState>,
    f: impl FnOnce(&mut Vault) -> Result<T, String>,
) -> Result<T, String> {
    let mut guard = state
        .session
        .lock()
        .map_err(|_| "session lock poisoned".to_string())?;
    let vault = guard
        .as_mut()
        .ok_or_else(|| "vault is locked".to_string())?;
    f(vault)
}

fn vault_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|err| err.to_string())?
        .join("vault"))
}

pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            vault_status,
            vault_create,
            vault_unlock,
            vault_recover,
            vault_lock,
            vault_change_password,
            vault_rotate,
            entries_list,
            entries_search,
            provider_add,
            provider_update,
            provider_archive,
            provider_restore,
            provider_delete,
            secret_reveal_field,
            secret_add,
            secret_remove,
            devices_list,
            device_revoke,
            provider_probe,
            vault_export_encrypted,
            vault_import_encrypted,
            sync_local,
            sync_webdav_remote,
            sync_conflicts,
            sync_accept_conflict,
            sync_discard_conflict
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use aipass_provider_registry::{ProviderKind, SecretRef};

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
            quota: None,
            tags: Vec::new(),
            environment: "test".to_string(),
            notes: None,
            header_names: Vec::new(),
            created_at: now,
            updated_at: now,
            last_used_at: None,
            archived_at: None,
        }
    }
}
