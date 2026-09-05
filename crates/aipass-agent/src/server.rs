use crate::desktop::open_desktop_window;
use crate::ipc;
use crate::logging::{write_component_log, AGENT_LOG};
use crate::paths::{canonical_vault_dir, cloud_sync_dir, namespace_for_vault_dir};
use crate::session::{
    apply_sync_settings_update, clamp_policy, current_policy, load_policy, load_sync_settings,
    lock_if_idle, lock_session, map_vault_error, native_host_settings_path, reset_vault,
    save_policy, save_sync_settings, session_status, shutdown_requested, sync_settings_password,
    sync_settings_view, touch_session, unlock_with_password, wait_for_unlock, with_vault,
    with_vault_mut, AgentState, InitialSyncState, NativeHostSettings, ServiceError, ServiceResult,
    SessionState,
};
use aipass_agent_protocol::{
    endpoint_url as protocol_endpoint_url, AgentErrorCode, AgentRequest, AgentResponse,
    AuthenticatedAgentRequest, BrowserContextLookupData, BrowserDetectedSecretFields,
    BrowserDetectedSecretPreview, BrowserFillResult, BrowserIgnoreOriginResult,
    BrowserIgnoredStatus, CodexApiKeyMode, ConflictScope, FaviconBackfillError,
    FaviconBackfillRequest, FaviconBackfillResponse, LockReason, ProbeResult, ProviderHeaderValues,
    ProxyProtocol, SaveDetectedResult, SecretValue, SensitiveString, SessionUnlockMode,
    SyncConflictActionRequest, SyncConflictResponse, SyncMode, ToolConfigApplyResponse,
    ToolConfigMode, ToolConfigPreviewFile, ToolConfigPreviewResponse, ToolConfigProxyRequest,
    ToolConfigRequest, ToolConfigTool, VaultCreateResponse, AGENT_PROTOCOL_VERSION,
    MAX_FRAME_BYTES,
};
use aipass_config_writers::{
    apply_plan_encrypted, config_backup_path, diff_preview_for_path, plan_claude_code,
    plan_claude_code_official, plan_claude_code_plaintext, plan_codex, plan_codex_official,
    plan_codex_plaintext, plan_codex_plaintext_with_mode, plan_cursor_local,
    plan_cursor_local_plaintext, plan_gemini_cli, plan_gemini_cli_plaintext, plan_grok,
    plan_grok_plaintext, plan_grok_plaintext_with_backend, plan_opencode, plan_opencode_plaintext,
    plan_opencode_plaintext_with_api, plan_pi, plan_pi_plaintext, plan_pi_plaintext_with_api,
    redacted_diff_preview, rollback_encrypted, ApplyResult,
    CodexApiKeyMode as WriterCodexApiKeyMode, ConfigPlan, GrokApiBackend, OpenCodeApi, PiApi,
    ToolEntry, ToolId,
};
use aipass_crypto::{mask_secret, SecretString};
use aipass_provider_registry::{
    default_provider_definitions, match_provider_by_domain, provider_kind_for_id, AuthScheme,
    CredentialKind, EndpointKind, InterfaceType, ProviderEndpoint, ProviderKind,
};
use aipass_storage::atomic_write_bytes;
use aipass_sync::{
    accept_conflict_with_validator, classify_webdav_error, discard_conflict, list_conflicts,
    sync_local_folder_with_validator, sync_webdav_with_validator,
    validate_sync_object_bytes_for_vault, ConflictRecord, HttpWebDavClient, SyncReport, SyncStatus,
    WebDavClient,
};
use aipass_vault::{
    EncryptedVaultExport, EntrySummary, ProviderEntryInput, SecretMetadataInput, TtlGrantSummary,
    Vault,
};
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use interprocess::local_socket::{prelude::*, Listener, ListenerNonblockingMode, Stream};
use reqwest::blocking::{Client as HttpClient, RequestBuilder};
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use reqwest::Url;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::json;
use std::collections::HashSet;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Condvar, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use uuid::Uuid;
use zeroize::Zeroize;

const MAX_ACTIVE_CONNECTIONS: usize = 32;
const CONNECTION_IO_TIMEOUT: Duration = Duration::from_secs(5);
const FAVICON_BACKFILL_DEFAULT_LIMIT: usize = 4;
const FAVICON_BACKFILL_MAX_LIMIT: usize = 8;
const FAVICON_REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_FAVICON_BYTES: usize = 96 * 1024;

#[derive(Clone, Debug)]
pub struct ServerOptions {
    pub vault_dir: PathBuf,
    pub launch_desktop_tray: bool,
}

impl ServerOptions {
    pub fn new(vault_dir: PathBuf) -> Self {
        Self {
            vault_dir,
            launch_desktop_tray: true,
        }
    }

    pub fn for_current_process(vault_dir: PathBuf) -> Self {
        if crate::desktop::tray_launch_suppressed() {
            Self::without_desktop_tray(vault_dir)
        } else {
            Self::new(vault_dir)
        }
    }

    pub fn without_desktop_tray(vault_dir: PathBuf) -> Self {
        Self {
            vault_dir,
            launch_desktop_tray: false,
        }
    }
}

pub fn run_server(options: ServerOptions) -> Result<()> {
    let started = Instant::now();
    let launch_desktop_tray = options.launch_desktop_tray;
    let vault_dir = canonical_vault_dir(options.vault_dir)?;
    let namespace = namespace_for_vault_dir(&vault_dir)?;
    write_component_log(
        AGENT_LOG,
        "INFO",
        &format!(
            "server starting vault={} namespace={} launch_desktop_tray={launch_desktop_tray}",
            vault_dir.display(),
            namespace
        ),
    );
    // Claim the per-vault singleton before initializing the agent so competing
    // launchers exit without touching any vault state.
    let listener = ipc::listen(&vault_dir)
        .with_context(|| format!("failed to bind agent listener for {}", vault_dir.display()))?;
    write_component_log(
        AGENT_LOG,
        "INFO",
        &format!(
            "listener bound vault={} namespace={}",
            vault_dir.display(),
            namespace
        ),
    );
    listener
        .set_nonblocking(ListenerNonblockingMode::Accept)
        .context("failed to set agent listener to nonblocking accept mode")?;
    let auth_token = ipc::load_or_create_auth_token(&vault_dir)?;
    let state = Arc::new(AgentState {
        policy: Mutex::new(load_policy(&vault_dir)?),
        vault_dir: vault_dir.clone(),
        namespace,
        auth_token,
        session: Mutex::new(SessionState::Locked),
        session_changed: Condvar::new(),
        last_lock_reason: Mutex::new(Some(LockReason::AgentRestart)),
        proxy: Mutex::new(crate::proxy_service::ProxyService::new(&vault_dir.clone())?),
        favicon_backfill: Mutex::new(()),
        sync_lock: Mutex::new(()),
        initial_sync: Mutex::new(InitialSyncState::Pending),
        sync_watcher: Mutex::new(None),
        shutdown: AtomicBool::new(false),
    });
    write_component_log(
        AGENT_LOG,
        "INFO",
        &format!(
            "agent state initialized elapsed_ms={}",
            started.elapsed().as_millis()
        ),
    );
    run_server_with_state(state, listener, launch_desktop_tray)
}

#[path = "handlers.rs"]
mod handlers;

use handlers::handle_request;

fn run_server_with_state(
    state: Arc<AgentState>,
    listener: Listener,
    launch_desktop_tray: bool,
) -> Result<()> {
    spawn_idle_lock_watcher(state.clone());
    crate::session::spawn_power_watcher(state.clone());
    crate::pricing::spawn_list_price_refresh(state.clone());
    crate::oauth::spawn_token_refresh(state.clone());
    spawn_initial_sync(state.clone());
    crate::sync_watch::start_sync_watcher_for_current_settings(&state);
    if launch_desktop_tray {
        ensure_desktop_tray_companion_async(state.vault_dir.clone());
    }

    let active_connections = Arc::new(AtomicUsize::new(0));
    loop {
        if shutdown_requested(&state) {
            break;
        }
        match listener.accept() {
            Ok(conn) => {
                let Some(guard) = ConnectionGuard::try_acquire(active_connections.clone()) else {
                    reject_busy(conn);
                    continue;
                };
                let state = state.clone();
                thread::spawn(move || {
                    let _guard = guard;
                    if let Err(err) = handle_connection(conn, state) {
                        write_component_log(
                            AGENT_LOG,
                            "ERROR",
                            &format!("agent connection failed: {err}"),
                        );
                        eprintln!("agent connection failed: {err}");
                    }
                });
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                ipc::wait_for_connection(&listener)?;
            }
            Err(err) if err.kind() == ErrorKind::Interrupted => continue,
            Err(err) => {
                write_component_log(AGENT_LOG, "ERROR", &format!("agent accept failed: {err}"));
                eprintln!("agent accept failed: {err}");
                thread::sleep(Duration::from_millis(250));
            }
        }
    }

    let _ = ipc::clear_auth_token(&state.vault_dir);
    Ok(())
}

fn ensure_desktop_tray_companion_async(_vault_dir: PathBuf) {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        if crate::desktop::tray_launch_suppressed() {
            return;
        }
        thread::spawn(move || {
            if let Err(err) = open_desktop_window(crate::desktop::TRAY_WINDOW_TARGET, &_vault_dir) {
                write_component_log(
                    AGENT_LOG,
                    "ERROR",
                    &format!("failed to open AIPass desktop tray companion: {err}"),
                );
                eprintln!("failed to open AIPass desktop tray companion: {err}");
            }
        });
    }
}

struct ConnectionGuard {
    active: Arc<AtomicUsize>,
}

impl ConnectionGuard {
    fn try_acquire(active: Arc<AtomicUsize>) -> Option<Self> {
        let mut current = active.load(Ordering::Acquire);
        loop {
            if current >= MAX_ACTIVE_CONNECTIONS {
                return None;
            }
            match active.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Some(Self { active }),
                Err(updated) => current = updated,
            }
        }
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::AcqRel);
    }
}

fn handle_connection(mut conn: Stream, state: Arc<AgentState>) -> Result<()> {
    conn.set_nonblocking(true)
        .context("failed to set agent stream to nonblocking mode")?;
    let response = match read_frame_with_deadline::<AuthenticatedAgentRequest>(
        &mut conn,
        CONNECTION_IO_TIMEOUT,
    ) {
        Ok(payload)
            if payload.protocol_version == AGENT_PROTOCOL_VERSION
                && auth_tokens_match(&payload.auth_token, &state.auth_token) =>
        {
            let _scope =
                crate::logging::RequestScope::new(payload.request_id.unwrap_or_else(Uuid::new_v4));
            let response = handle_request(&state, payload.request);
            if let Err(err) = write_frame_with_deadline(&mut conn, &response, CONNECTION_IO_TIMEOUT)
            {
                write_component_log(
                    AGENT_LOG,
                    "ERROR",
                    "event=ipc.response outcome=delivery_failed",
                );
                return Err(err);
            }
            conn.flush().ok();
            return Ok(());
        }
        Ok(payload) if payload.protocol_version != AGENT_PROTOCOL_VERSION => AgentResponse::error(
            AgentErrorCode::ValidationFailed,
            "unsupported agent protocol version",
        ),
        Ok(_) => AgentResponse::error(AgentErrorCode::PermissionDenied, "invalid agent auth token"),
        Err(err) => AgentResponse::error(AgentErrorCode::ValidationFailed, err.to_string()),
    };
    write_component_log(
        AGENT_LOG,
        "WARN",
        &format!(
            "event=ipc.request outcome=rejected code={:?}",
            response.code
        ),
    );
    write_frame_with_deadline(&mut conn, &response, CONNECTION_IO_TIMEOUT)?;
    conn.flush().ok();
    Ok(())
}

fn reject_busy(mut conn: Stream) {
    let _ = conn.set_nonblocking(true);
    let response = AgentResponse::error(AgentErrorCode::ServiceUnavailable, "agent is busy");
    let _ = write_frame_with_deadline(&mut conn, &response, CONNECTION_IO_TIMEOUT);
    let _ = conn.flush();
}

fn read_frame_with_deadline<T: DeserializeOwned>(
    conn: &mut Stream,
    timeout: Duration,
) -> Result<T> {
    let deadline = Instant::now() + timeout;
    let mut len = [0_u8; 4];
    read_exact_with_deadline(conn, &mut len, deadline)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME_BYTES {
        bail!("frame too large");
    }
    let mut body = vec![0_u8; len];
    if let Err(err) = read_exact_with_deadline(conn, &mut body, deadline) {
        body.zeroize();
        return Err(err);
    }
    let parsed = serde_json::from_slice(&body);
    body.zeroize();
    Ok(parsed?)
}

fn write_frame_with_deadline<T: Serialize>(
    conn: &mut Stream,
    value: &T,
    timeout: Duration,
) -> Result<()> {
    let mut body = serde_json::to_vec(value)?;
    if body.len() > MAX_FRAME_BYTES {
        body.zeroize();
        bail!("frame too large");
    }
    let deadline = Instant::now() + timeout;
    let len = (body.len() as u32).to_le_bytes();
    let result = write_all_with_deadline(conn, &len, deadline)
        .and_then(|_| write_all_with_deadline(conn, &body, deadline));
    body.zeroize();
    result
}

fn read_exact_with_deadline(conn: &mut Stream, buf: &mut [u8], deadline: Instant) -> Result<()> {
    let mut offset = 0;
    while offset < buf.len() {
        match conn.read(&mut buf[offset..]) {
            Ok(0) => {
                return Err(std::io::Error::from(ErrorKind::UnexpectedEof).into());
            }
            Ok(count) => offset += count,
            Err(err) if err.kind() == ErrorKind::Interrupted => {}
            Err(err) if err.kind() == ErrorKind::WouldBlock => wait_for_io(deadline)?,
            Err(err) => return Err(err.into()),
        }
    }
    Ok(())
}

fn write_all_with_deadline(conn: &mut Stream, buf: &[u8], deadline: Instant) -> Result<()> {
    let mut offset = 0;
    while offset < buf.len() {
        match conn.write(&buf[offset..]) {
            Ok(0) => {
                return Err(std::io::Error::from(ErrorKind::WriteZero).into());
            }
            Ok(count) => offset += count,
            Err(err) if err.kind() == ErrorKind::Interrupted => {}
            Err(err) if err.kind() == ErrorKind::WouldBlock => wait_for_io(deadline)?,
            Err(err) => return Err(err.into()),
        }
    }
    Ok(())
}

fn wait_for_io(deadline: Instant) -> Result<()> {
    let now = Instant::now();
    if now >= deadline {
        bail!("agent IPC timed out");
    }
    thread::sleep((deadline - now).min(Duration::from_millis(5)));
    Ok(())
}

fn auth_tokens_match(left: &SensitiveString, right: &SensitiveString) -> bool {
    constant_time_eq(left.expose().as_bytes(), right.expose().as_bytes())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut diff = left.len() ^ right.len();
    let max_len = left.len().max(right.len());
    for index in 0..max_len {
        let left_byte = left.get(index).copied().unwrap_or(0);
        let right_byte = right.get(index).copied().unwrap_or(0);
        diff |= (left_byte ^ right_byte) as usize;
    }
    diff == 0
}

fn spawn_idle_lock_watcher(state: Arc<AgentState>) {
    thread::spawn(move || loop {
        if shutdown_requested(&state) {
            break;
        }
        let _ = lock_if_idle(&state);
        thread::sleep(Duration::from_secs(1));
    });
}

/// Run one sync at agent startup against the configured folder backend so a
/// freshly launched agent serves synced data. SessionStatus reports
/// initial_sync_pending until this settles; a failure clears the flag too —
/// readiness must never be blocked by an unreachable sync folder.
fn spawn_initial_sync(state: Arc<AgentState>) {
    thread::spawn(move || {
        let outcome = run_initial_sync(&state);
        match state.initial_sync.lock() {
            Ok(mut slot) => *slot = outcome,
            Err(poisoned) => *poisoned.into_inner() = outcome,
        }
    });
}

fn run_initial_sync(state: &Arc<AgentState>) -> InitialSyncState {
    if !crate::session::manifest_path(&state.vault_dir).exists() {
        return InitialSyncState::Done;
    }
    let settings = match load_sync_settings(&state.vault_dir) {
        Ok(settings) => settings,
        Err(_) => {
            write_component_log(
                AGENT_LOG,
                "WARN",
                "initial sync skipped: failed to load sync settings",
            );
            return InitialSyncState::Failed;
        }
    };
    let Some(dir) = crate::sync_watch::folder_sync_dir(&settings) else {
        return InitialSyncState::Done;
    };
    match run_sync_local(state, &dir) {
        Ok(report) => {
            write_component_log(
                AGENT_LOG,
                "INFO",
                &format!(
                    "initial sync finished: uploaded={} downloaded={} conflicts={}",
                    report.uploaded, report.downloaded, report.conflicts
                ),
            );
            InitialSyncState::Done
        }
        Err(err) => {
            write_component_log(
                AGENT_LOG,
                "WARN",
                &format!("initial sync failed code={:?}", err.code),
            );
            InitialSyncState::Failed
        }
    }
}

fn create_vault(state: &Arc<AgentState>, password: String) -> ServiceResult<VaultCreateResponse> {
    let (recovery_kit, session) = crate::session::create_vault(state, password)?;
    Ok(VaultCreateResponse {
        recovery_kit,
        session,
    })
}

fn recover_vault(
    state: &Arc<AgentState>,
    recovery_key: String,
    new_password: String,
) -> ServiceResult<VaultCreateResponse> {
    let (recovery_kit, session) = crate::session::recover_vault(state, recovery_key, new_password)?;
    Ok(VaultCreateResponse {
        recovery_kit,
        session,
    })
}

fn is_origin_ignored(vault_dir: &Path, origin: &str) -> Result<bool> {
    let origin = normalize_origin(origin)?;
    Ok(load_native_host_settings(vault_dir)?
        .ignored_origins
        .iter()
        .any(|value| value == &origin))
}

fn ignore_origin(vault_dir: &Path, origin: &str) -> Result<Vec<String>> {
    let origin = normalize_origin(origin)?;
    let mut settings = load_native_host_settings(vault_dir)?;
    if !settings
        .ignored_origins
        .iter()
        .any(|value| value == &origin)
    {
        settings.ignored_origins.push(origin);
        settings.ignored_origins.sort();
        settings.ignored_origins.dedup();
        save_native_host_settings(vault_dir, &settings)?;
    }
    Ok(settings.ignored_origins)
}

fn load_native_host_settings(vault_dir: &Path) -> Result<NativeHostSettings> {
    let path = native_host_settings_path(vault_dir);
    if !path.exists() {
        return Ok(NativeHostSettings::default());
    }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn save_native_host_settings(vault_dir: &Path, settings: &NativeHostSettings) -> Result<()> {
    let path = native_host_settings_path(vault_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    atomic_write_bytes(&path, &serde_json::to_vec_pretty(settings)?)?;
    Ok(())
}

fn normalize_origin(origin: &str) -> Result<String> {
    let normalized = origin.trim().trim_end_matches('/').to_lowercase();
    if normalized.is_empty() {
        bail!("origin is required");
    }
    Ok(normalized)
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn save_detected_secret(
    vault: &Vault,
    fields: BrowserDetectedSecretFields,
) -> ServiceResult<SaveDetectedResult> {
    let domain = host_from_origin(&fields.origin);
    let provider_guess = fields
        .provider_id
        .clone()
        .or_else(|| match_provider_by_domain(&domain).map(|provider| provider.id.to_string()));
    let provider_kind = provider_guess
        .as_deref()
        .map(|id| provider_kind_for_id(Some(id)))
        .unwrap_or(aipass_provider_registry::ProviderKind::Unknown);
    let preview = detected_secret_preview(vault, &fields);
    let secret_metadata = detected_secret_metadata(&preview);
    if let Some(existing_entry_id) = preview.existing_entry_id {
        // Deliberately no entry-level gateway write here: an entry can now hold
        // several groups, so stamping one of them onto the entry would be wrong.
        if preview
            .favicon_url
            .as_deref()
            .is_some_and(|favicon| favicon.starts_with("data:image/"))
        {
            vault
                .replace_provider_favicon_url(
                    existing_entry_id,
                    preview.favicon_url.clone().unwrap(),
                )
                .map_err(map_vault_error)?;
        }

        // The exact key is already stored: refresh its group / format / billing
        // rather than storing it twice.
        if let Some(secret_id) = preview.existing_secret_id.clone() {
            if !secret_metadata.is_empty() {
                vault
                    .set_secret_metadata(existing_entry_id, &secret_id, &secret_metadata)
                    .map_err(map_vault_error)?;
            }
            return Ok(SaveDetectedResult {
                entry_id: existing_entry_id,
                secret_id: Some(secret_id),
                merged_into_existing: true,
            });
        }

        // A different key for a site we already track — most often a second
        // gateway group. It belongs on the same entry as another key.
        let existing = vault
            .get_provider_summary(existing_entry_id)
            .map_err(map_vault_error)?;
        let label = unique_secret_label(
            &existing,
            preview
                .secret_label
                .as_deref()
                .or(preview.group.as_deref())
                .unwrap_or("key"),
        );
        let secret_id = vault
            .add_secret_with_metadata(
                existing_entry_id,
                label,
                fields.api_key.into_inner(),
                &secret_metadata,
            )
            .map_err(map_vault_error)?;
        return Ok(SaveDetectedResult {
            entry_id: existing_entry_id,
            secret_id: Some(secret_id),
            merged_into_existing: true,
        });
    }
    let api_key = fields.api_key.into_inner();
    let mut domains = vec![domain];
    for extra in &fields.domains {
        let cleaned = extra.trim();
        if !cleaned.is_empty() && !domains.iter().any(|item| item == cleaned) {
            domains.push(cleaned.to_string());
        }
    }
    let mut endpoints: Vec<ProviderEndpoint> = preview
        .endpoint
        .clone()
        .into_iter()
        .map(ProviderEndpoint::api)
        .collect();
    endpoints.extend(
        fields
            .console_endpoint
            .clone()
            .and_then(non_empty)
            .into_iter()
            .map(ProviderEndpoint::console),
    );
    let entry_id = vault
        .add_provider(ProviderEntryInput {
            title: preview.title,
            provider_kind,
            provider_id: preview.provider_id,
            credential_kind: Default::default(),
            account_identity: None,
            domains,
            favicon_url: preview.favicon_url,
            endpoints,
            interface_type: preview.interface_type,
            auth_scheme: preview.auth_scheme,
            api_key: api_key.clone(),
            secret_label: preview.secret_label,
            default_model: fields.default_model.clone().and_then(non_empty),
            model_aliases: fields
                .model_aliases
                .iter()
                .filter_map(|(alias, model)| {
                    Some((non_empty(alias.clone())?, non_empty(model.clone())?))
                })
                .collect(),
            headers: fields.headers.clone(),
            quota: None,
            subscription: None,
            gateway: preview.gateway,
            tags: preview.tags,
            notes: fields.notes.clone().and_then(non_empty),
            secret_metadata,
        })
        .map_err(map_vault_error)?;
    let secret_id = vault
        .find_secret_id_by_value(entry_id, &api_key)
        .map_err(map_vault_error)?;
    Ok(SaveDetectedResult {
        entry_id,
        secret_id,
        merged_into_existing: false,
    })
}

/// The per-key attributes a detected draft carries. The wire format is always
/// recorded on the key: on a relay one group may speak Anthropic while another
/// speaks OpenAI, and the entry can only name one of them.
fn detected_secret_metadata(preview: &BrowserDetectedSecretPreview) -> SecretMetadataInput {
    SecretMetadataInput {
        group: preview.group.clone(),
        interface_type: Some(preview.interface_type.clone()),
        billing: preview.billing.clone().filter(|rule| !rule.is_empty()),
    }
}

/// Labels are unique within an entry, so a second key from the same group gets
/// a numeric suffix instead of failing the save.
fn unique_secret_label(entry: &EntrySummary, preferred: &str) -> String {
    let preferred = preferred.trim();
    let base = if preferred.is_empty() {
        "key"
    } else {
        preferred
    };
    if !entry.secret_refs.iter().any(|secret| secret.label == base) {
        return base.to_string();
    }
    for suffix in 2..100 {
        let candidate = format!("{base} {suffix}");
        if !entry
            .secret_refs
            .iter()
            .any(|secret| secret.label == candidate)
        {
            return candidate;
        }
    }
    format!("{base} {}", Uuid::new_v4())
}

fn detected_secret_preview(
    vault: &Vault,
    fields: &BrowserDetectedSecretFields,
) -> BrowserDetectedSecretPreview {
    let domain = host_from_origin(&fields.origin);
    let provider_guess = fields
        .provider_id
        .clone()
        .or_else(|| match_provider_by_domain(&domain).map(|provider| provider.id.to_string()));
    let provider_definition = provider_guess.as_deref().and_then(|id| {
        default_provider_definitions()
            .into_iter()
            .find(|provider| provider.id == id)
    });
    let endpoint = fields
        .endpoint
        .clone()
        .or_else(|| {
            provider_definition.as_ref().and_then(|provider| {
                provider
                    .endpoints
                    .iter()
                    .find(|(_, kind, _)| *kind == EndpointKind::Api)
                    .map(|(_, _, url)| (*url).to_string())
            })
        })
        .or_else(|| Some(fields.url.clone()));
    let interface_type = fields.interface_type.clone().unwrap_or_else(|| {
        endpoint
            .as_deref()
            .and_then(infer_interface_from_endpoint)
            .or_else(|| {
                provider_definition
                    .as_ref()
                    .and_then(|provider| provider.interfaces.first().cloned())
            })
            .or_else(|| provider_guess_interface(&fields.origin))
            .unwrap_or(InterfaceType::CustomHttp)
    });
    let auth_scheme = fields.auth_scheme.clone().unwrap_or_else(|| {
        provider_definition
            .as_ref()
            .and_then(|provider| provider.auth_schemes.first().cloned())
            .unwrap_or_else(|| default_auth_for_interface(&interface_type))
    });
    let title = fields
        .title
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            provider_definition
                .as_ref()
                .map(|provider| provider.display_name.to_string())
        })
        .unwrap_or_else(|| "Browser Provider".to_string());
    let tags = fields.tags.clone();

    // Resolve where this key belongs. An exact key match wins; otherwise any
    // entry already tracking this site adopts the key, so a relay with several
    // groups stays one entry with one key per group.
    let stored = vault
        .search(fields.api_key.expose())
        .ok()
        .and_then(|matches| matches.into_iter().next());
    let existing_secret_id = stored.as_ref().and_then(|entry| {
        vault
            .find_secret_id_by_value(entry.id, fields.api_key.expose())
            .ok()
            .flatten()
    });
    let is_saved = stored.is_some();
    let target = stored.or_else(|| {
        vault
            .lookup_by_origin(&fields.origin)
            .ok()
            .into_iter()
            .flatten()
            .next()
    });

    let gateway = clean_gateway(fields.gateway.clone());
    // Group is its own field now; accept it from the dedicated field and fall
    // back to the legacy entry-level gateway blob for older drafts.
    let group = clean_gateway_field(fields.group.as_deref().unwrap_or_default(), 48)
        .or_else(|| gateway.as_ref().and_then(|value| value.group.clone()));
    let billing = clean_billing(fields.billing.clone(), gateway.as_ref());

    BrowserDetectedSecretPreview {
        title,
        secret_label: clean_secret_label(fields.secret_label.as_deref()),
        favicon_url: clean_favicon_url(fields.favicon_url.as_deref()),
        provider_id: provider_guess,
        endpoint,
        interface_type,
        auth_scheme,
        masked_secret: mask_secret(fields.api_key.expose()),
        fingerprint: vault.fingerprint_secret(fields.api_key.expose()),
        existing_entry_id: target.as_ref().map(|entry| entry.id),
        existing_entry_title: target.as_ref().map(|entry| entry.title.clone()),
        existing_secret_id,
        existing_groups: target
            .as_ref()
            .map(|entry| {
                entry
                    .secret_refs
                    .iter()
                    .filter_map(|secret| secret.group.clone())
                    .collect()
            })
            .unwrap_or_default(),
        is_saved,
        tags,
        gateway,
        group,
        billing,
    }
}

/// Billing details are scraped from a console page, so they get the same
/// length and secret-shaped sanitising as the other gateway fields. The legacy
/// entry-level `gateway.rate` seeds the rate when no rule was sent.
fn clean_billing(
    billing: Option<aipass_provider_registry::BillingRule>,
    gateway: Option<&aipass_provider_registry::GatewayMetadata>,
) -> Option<aipass_provider_registry::BillingRule> {
    const MAX_LEN: usize = 48;
    const MAX_NOTE_LEN: usize = 160;
    let mut rule = billing.unwrap_or_default();
    rule.rate = rule
        .rate
        .and_then(|value| clean_gateway_field(&value, MAX_LEN))
        .or_else(|| gateway.and_then(|value| value.rate.clone()));
    rule.currency = rule
        .currency
        .and_then(|value| clean_gateway_field(&value, MAX_LEN));
    rule.unit_price = rule
        .unit_price
        .and_then(|value| clean_gateway_field(&value, MAX_LEN));
    rule.note = rule
        .note
        .and_then(|value| clean_gateway_field(&value, MAX_NOTE_LEN));
    (!rule.is_empty()).then_some(rule)
}

fn clean_secret_label(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty()
        || value.len() > 64
        || value.chars().any(char::is_control)
        || value.eq_ignore_ascii_case("api key")
        || value.eq_ignore_ascii_case("token")
        || value.eq_ignore_ascii_case("secret")
        || value == "密钥"
        || value == "令牌"
        || looks_like_secret_or_masked(value)
    {
        None
    } else {
        Some(value.to_string())
    }
}

/// Defense against page-scraped metadata that is actually the API key itself,
/// either raw or elided (e.g. `sk-abc…xyz`, `sk-abc...xyz`, `sk-abc***xyz`).
fn looks_like_secret_or_masked(value: &str) -> bool {
    let has_mask_run = value.contains('…')
        || value.contains("...")
        || value.contains("***")
        || value.contains("•••");
    if has_mask_run
        && value.len() >= 8
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '…' | '.' | '*' | '•'))
    {
        return true;
    }
    const SECRET_PREFIXES: &[&str] = &[
        "sk-", "r8_", "gsk_", "fw_", "xai-", "pplx-", "csk", "nvapi-", "hf_", "AIza",
    ];
    value.len() >= 16
        && SECRET_PREFIXES
            .iter()
            .any(|prefix| value.starts_with(prefix))
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

fn clean_favicon_url(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() || value.len() > 512 * 1024 || value.chars().any(char::is_control) {
        return None;
    }
    let lower = value.to_lowercase();
    if lower.starts_with("https://")
        || lower.starts_with("http://")
        || (lower.starts_with("data:image/") && lower.contains(";base64,"))
    {
        Some(value.to_string())
    } else {
        None
    }
}

fn clean_gateway(
    gateway: Option<aipass_provider_registry::GatewayMetadata>,
) -> Option<aipass_provider_registry::GatewayMetadata> {
    const MAX_GROUP_LEN: usize = 48;
    const MAX_RATE_LEN: usize = 24;
    let mut gateway = gateway?;
    gateway.group = gateway
        .group
        .and_then(|value| clean_gateway_field(&value, MAX_GROUP_LEN));
    gateway.rate = gateway
        .rate
        .and_then(|value| clean_gateway_field(&value, MAX_RATE_LEN));
    if gateway.group.is_none() && gateway.rate.is_none() {
        None
    } else {
        Some(gateway)
    }
}

fn clean_gateway_field(value: &str, max_len: usize) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > max_len
        || value.chars().any(char::is_control)
        || looks_like_secret_or_masked(value)
    {
        None
    } else {
        Some(value.to_string())
    }
}

fn host_from_origin(value: &str) -> String {
    let trimmed = value.trim().to_lowercase();
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(&trimmed);
    without_scheme
        .split('/')
        .next()
        .unwrap_or(without_scheme)
        .split('@')
        .next_back()
        .unwrap_or(without_scheme)
        .split(':')
        .next()
        .unwrap_or(without_scheme)
        .to_string()
}

fn provider_guess_interface(origin: &str) -> Option<InterfaceType> {
    match_provider_by_domain(origin).and_then(|provider| provider.interfaces.first().cloned())
}

fn infer_interface_from_endpoint(endpoint: &str) -> Option<InterfaceType> {
    let endpoint = endpoint.to_lowercase();
    // Keep in sync with OPENAI_COMPATIBLE_ENDPOINT_PATTERN in
    // packages/schemas/src/index.ts. `/v\d` covers versioned API paths such
    // as /v2, /api/paas/v4, and /v1beta.
    const OPENAI_COMPATIBLE_EVIDENCE: &[&str] = &[
        "openai",
        "chat/completions",
        "messages",
        "gateway",
        "one-api",
        "one_api",
        "one api",
        "oneapi",
        "new-api",
        "new_api",
        "new api",
        "newapi",
        "litellm",
        "sub2api",
        "openrouter",
        "veloera",
        "omniroute",
        "metapi",
        "onehub",
        "donehub",
        "anyrouter",
        "siliconflow",
        "deepseek",
        "moonshot",
        "dashscope",
        "qwen",
        "bigmodel",
        "zhipu",
        "volcengine",
        "volces",
        "ark",
        "together",
        "fireworks",
        "groq",
        "x.ai",
        "mistral",
        "perplexity",
        "cerebras",
        "nvidia",
        "nim",
        "novita",
        "huggingface",
        "hugging face",
    ];
    let has_versioned_api_path = endpoint
        .as_bytes()
        .windows(3)
        .any(|window| window[0] == b'/' && window[1] == b'v' && window[2].is_ascii_digit());
    if endpoint.contains("generativelanguage") || endpoint.contains("gemini") {
        Some(InterfaceType::Gemini)
    } else if endpoint.contains("anthropic") || endpoint.contains("claude") {
        Some(InterfaceType::AnthropicMessages)
    } else if has_versioned_api_path
        || OPENAI_COMPATIBLE_EVIDENCE
            .iter()
            .any(|keyword| endpoint.contains(keyword))
    {
        Some(InterfaceType::OpenAiCompatible)
    } else {
        None
    }
}

fn default_auth_for_interface(interface_type: &InterfaceType) -> AuthScheme {
    match interface_type {
        InterfaceType::AnthropicMessages => AuthScheme::XApiKey,
        InterfaceType::Gemini => AuthScheme::GoogleApiKey,
        InterfaceType::AzureOpenAi => AuthScheme::AzureApiKey,
        InterfaceType::Bedrock => AuthScheme::AwsProfile,
        InterfaceType::OpenAiCompatible => AuthScheme::Bearer,
        InterfaceType::CustomHttp => AuthScheme::CustomHeader,
    }
}

fn build_tool_config_plan(
    vault: &Vault,
    request: &ToolConfigRequest,
) -> ServiceResult<(EntrySummary, ConfigPlan, String)> {
    if request.codex_api_key_mode.is_some()
        && (!matches!(request.tool, ToolConfigTool::Codex)
            || !matches!(request.mode, ToolConfigMode::Plaintext))
    {
        return Err(ServiceError::new(
            AgentErrorCode::ValidationFailed,
            "codex_api_key_mode requires tool=codex and mode=plaintext",
        ));
    }
    let entry = vault
        .get_provider_summary(request.id)
        .map_err(map_vault_error)?;
    if matches!(request.mode, ToolConfigMode::Official) {
        // Official mode reuses the tool's own CLI credential store, so the
        // vault entry must be that provider's official OAuth account.
        let expected_provider = match request.tool {
            ToolConfigTool::Codex => "openai",
            ToolConfigTool::ClaudeCode => "anthropic",
            _ => "",
        };
        if expected_provider.is_empty()
            || !matches!(&entry.provider_kind, ProviderKind::Official)
            || !matches!(&entry.credential_kind, CredentialKind::OAuth)
            || entry.provider_id.as_deref() != Some(expected_provider)
        {
            return Err(ServiceError::new(
                AgentErrorCode::ValidationFailed,
                "official mode requires the tool's own official OAuth account",
            ));
        }
    }
    if matches!(request.mode, ToolConfigMode::Plaintext)
        && matches!(
            request.tool,
            ToolConfigTool::Codex | ToolConfigTool::ClaudeCode
        )
        && !matches!(&entry.credential_kind, CredentialKind::Api)
    {
        return Err(ServiceError::new(
            AgentErrorCode::ValidationFailed,
            "API key mode requires an API credential",
        ));
    }
    let home = home_dir()?;
    let mut tool_entry = ToolEntry {
        id: entry.id,
        title: entry.title.clone(),
        provider_id: entry.provider_id.clone(),
        endpoint: endpoint_url(&entry.endpoints),
        interface_type: entry.interface_type.clone(),
        auth_scheme: entry.auth_scheme.clone(),
        env_key: match &request.tool {
            ToolConfigTool::ClaudeCode if matches!(&entry.auth_scheme, AuthScheme::Bearer) => {
                "ANTHROPIC_AUTH_TOKEN".to_string()
            }
            ToolConfigTool::ClaudeCode => "ANTHROPIC_API_KEY".to_string(),
            ToolConfigTool::GeminiCli => "GEMINI_API_KEY".to_string(),
            _ => env_key_for_entry(&entry),
        },
        default_model: entry.default_model.clone(),
        api_key: None,
    };
    if matches!(request.mode, ToolConfigMode::Plaintext) {
        tool_entry.api_key = Some(vault.reveal_secret(entry.id).map_err(map_vault_error)?);
    }
    let (plan, content) = match (&request.tool, &request.mode) {
        (ToolConfigTool::Codex, ToolConfigMode::Official) => {
            plan_codex_official(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (ToolConfigTool::Codex, ToolConfigMode::Helper) => {
            plan_codex(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (ToolConfigTool::Codex, ToolConfigMode::Env) => {
            plan_tool_env_helper(&home, ToolConfigTool::Codex, &tool_entry)?
        }
        (ToolConfigTool::Codex, ToolConfigMode::Plaintext) => {
            let mode = request
                .codex_api_key_mode
                .as_ref()
                .map(|mode| match mode {
                    CodexApiKeyMode::ExperimentalBearerToken => {
                        WriterCodexApiKeyMode::ExperimentalBearerToken
                    }
                    CodexApiKeyMode::AuthJson => WriterCodexApiKeyMode::AuthJson,
                })
                .unwrap_or(WriterCodexApiKeyMode::AuthJson);
            plan_codex_plaintext_with_mode(&home, &tool_entry, mode)
                .map_err(ServiceError::internal)?
        }
        (ToolConfigTool::ClaudeCode, ToolConfigMode::Helper) => {
            plan_claude_code(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (ToolConfigTool::ClaudeCode, ToolConfigMode::Official) => {
            plan_claude_code_official(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (ToolConfigTool::ClaudeCode, ToolConfigMode::Env) => {
            plan_tool_env_helper(&home, ToolConfigTool::ClaudeCode, &tool_entry)?
        }
        (ToolConfigTool::ClaudeCode, ToolConfigMode::Plaintext) => {
            plan_claude_code_plaintext(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (ToolConfigTool::GeminiCli, ToolConfigMode::Helper) => {
            plan_gemini_cli(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (ToolConfigTool::GeminiCli, ToolConfigMode::Env) => {
            plan_tool_env_helper(&home, ToolConfigTool::GeminiCli, &tool_entry)?
        }
        (ToolConfigTool::GeminiCli, ToolConfigMode::Plaintext) => {
            plan_gemini_cli_plaintext(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (ToolConfigTool::OpenCode, ToolConfigMode::Helper) => {
            plan_opencode(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (ToolConfigTool::OpenCode, ToolConfigMode::Env) => {
            plan_tool_env_helper(&home, ToolConfigTool::OpenCode, &tool_entry)?
        }
        (ToolConfigTool::OpenCode, ToolConfigMode::Plaintext) => {
            plan_opencode_plaintext(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (ToolConfigTool::Grok, ToolConfigMode::Helper) => {
            plan_grok(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (ToolConfigTool::Grok, ToolConfigMode::Env) => {
            plan_tool_env_helper(&home, ToolConfigTool::Grok, &tool_entry)?
        }
        (ToolConfigTool::Grok, ToolConfigMode::Plaintext) => {
            plan_grok_plaintext(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (ToolConfigTool::Pi, ToolConfigMode::Helper) => {
            plan_pi(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (ToolConfigTool::Pi, ToolConfigMode::Env) => {
            plan_tool_env_helper(&home, ToolConfigTool::Pi, &tool_entry)?
        }
        (ToolConfigTool::Pi, ToolConfigMode::Plaintext) => {
            plan_pi_plaintext(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (ToolConfigTool::Cursor, ToolConfigMode::Plaintext) => {
            plan_cursor_local_plaintext(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (ToolConfigTool::Cursor, ToolConfigMode::Helper | ToolConfigMode::Env) => {
            plan_cursor_local(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        _ => {
            return Err(ServiceError::new(
                AgentErrorCode::ValidationFailed,
                "official mode is only supported for Codex and Claude Code",
            ))
        }
    };
    Ok((entry, plan, content))
}

fn build_tool_config_proxy_plan(
    vault: &Vault,
    state: &Arc<AgentState>,
    request: &ToolConfigProxyRequest,
) -> ServiceResult<(ToolEntry, ConfigPlan, String)> {
    let (bind_addr, route) = {
        let mut proxy = state
            .proxy
            .lock()
            .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "proxy lock poisoned"))?;
        let config = proxy.config(vault)?;
        let route = config
            .routes
            .iter()
            .find(|route| route.id == request.route_id)
            .cloned()
            .ok_or_else(|| ServiceError::new(AgentErrorCode::NotFound, "proxy route not found"))?;
        (config.bind_addr.clone(), route)
    };
    if route.token.is_empty() {
        return Err(ServiceError::new(
            AgentErrorCode::ValidationFailed,
            "proxy route has no local token; rotate the route token first",
        ));
    }
    if !route.enabled {
        return Err(ServiceError::new(
            AgentErrorCode::ValidationFailed,
            "cannot configure a disabled proxy route",
        ));
    }
    ensure_proxy_tool_protocol(&request.tool, route.inbound_protocol)?;
    let tool_bind_addr = advertised_bind_addr(&bind_addr);
    let endpoint = proxy_endpoint_for_tool(&request.tool, route.inbound_protocol, &tool_bind_addr);
    let anthropic = route.inbound_protocol == ProxyProtocol::AnthropicMessages;
    let default_model = route
        .targets
        .iter()
        .filter(|target| target.enabled)
        .filter_map(|target| vault.get_provider_summary(target.provider_entry_id).ok())
        .find_map(|entry| entry.default_model.filter(|model| !model.trim().is_empty()));
    if matches!(request.tool, ToolId::Grok | ToolId::Pi) && default_model.is_none() {
        return Err(ServiceError::new(
            AgentErrorCode::ValidationFailed,
            "Grok and Pi integration requires a default model on a route credential",
        ));
    }
    let tool_entry = ToolEntry {
        id: route.id,
        title: route.name.clone(),
        provider_id: None,
        endpoint: Some(endpoint),
        interface_type: if anthropic {
            InterfaceType::AnthropicMessages
        } else {
            InterfaceType::OpenAiCompatible
        },
        auth_scheme: if anthropic {
            AuthScheme::XApiKey
        } else {
            AuthScheme::Bearer
        },
        env_key: "AIPASS_PROXY_TOKEN".to_string(),
        default_model,
        api_key: Some(route.token.clone()),
    };
    let home = home_dir()?;
    let (plan, content) = match request.tool {
        ToolId::Codex => {
            plan_codex_plaintext(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        ToolId::ClaudeCode => {
            plan_claude_code_plaintext(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        ToolId::GeminiCli => return Err(ServiceError::new(
            AgentErrorCode::ValidationFailed,
            "Gemini CLI requires a Gemini-native model endpoint; the AIPass local proxy currently exposes OpenAI Responses, OpenAI Chat Completions, and Anthropic Messages",
        )),
        ToolId::OpenCode => {
            let api = match route.inbound_protocol {
                ProxyProtocol::OpenAiResponses => OpenCodeApi::OpenAiResponses,
                ProxyProtocol::OpenAiChatCompletions => OpenCodeApi::OpenAiChatCompletions,
                ProxyProtocol::AnthropicMessages => OpenCodeApi::AnthropicMessages,
            };
            plan_opencode_plaintext_with_api(&home, &tool_entry, api)
                .map_err(ServiceError::internal)?
        }
        ToolId::Grok => {
            let backend = match route.inbound_protocol {
                ProxyProtocol::OpenAiResponses => GrokApiBackend::Responses,
                ProxyProtocol::OpenAiChatCompletions => GrokApiBackend::ChatCompletions,
                ProxyProtocol::AnthropicMessages => GrokApiBackend::Messages,
            };
            plan_grok_plaintext_with_backend(&home, &tool_entry, backend)
                .map_err(ServiceError::internal)?
        }
        ToolId::Pi => {
            let api = match route.inbound_protocol {
                ProxyProtocol::OpenAiResponses => PiApi::OpenAiResponses,
                ProxyProtocol::OpenAiChatCompletions => PiApi::OpenAiCompletions,
                ProxyProtocol::AnthropicMessages => PiApi::AnthropicMessages,
            };
            plan_pi_plaintext_with_api(&home, &tool_entry, api)
                .map_err(ServiceError::internal)?
        }
        ToolId::Cursor => {
            plan_cursor_local_plaintext(&home, &tool_entry).map_err(ServiceError::internal)?
        }
    };
    Ok((tool_entry, plan, content))
}

fn ensure_proxy_tool_protocol(tool: &ToolId, protocol: ProxyProtocol) -> ServiceResult<()> {
    let supported = match tool {
        ToolId::Codex => protocol == ProxyProtocol::OpenAiResponses,
        ToolId::ClaudeCode => protocol == ProxyProtocol::AnthropicMessages,
        ToolId::OpenCode | ToolId::Grok | ToolId::Pi => true,
        ToolId::GeminiCli => {
            return Err(ServiceError::new(
                AgentErrorCode::ValidationFailed,
                "Gemini CLI requires a Gemini-native model endpoint; the AIPass local proxy currently exposes OpenAI Responses, OpenAI Chat Completions, and Anthropic Messages",
            ));
        }
        ToolId::Cursor => true,
    };
    if supported {
        Ok(())
    } else {
        Err(ServiceError::new(
            AgentErrorCode::ValidationFailed,
            "the selected tool cannot speak the route's inbound model API protocol",
        ))
    }
}

/// SDKs disagree about whether an Anthropic base URL includes `/v1`.
/// Claude Code appends the versioned path itself, while the AI SDK providers
/// used by OpenCode, Grok, and Pi expect the versioned base URL. Keep that
/// transport detail here instead of leaking it into each config writer.
fn proxy_endpoint_for_tool(tool: &ToolId, protocol: ProxyProtocol, bind_addr: &str) -> String {
    let origin = format!("http://{bind_addr}");
    match (tool, protocol) {
        (ToolId::ClaudeCode, ProxyProtocol::AnthropicMessages) => origin,
        (ToolId::Cursor, ProxyProtocol::AnthropicMessages) => format!("{origin}/v1/messages"),
        _ => format!("{origin}/v1"),
    }
}

fn tool_config_preview_files(plan: &ConfigPlan, content: &str) -> Vec<ToolConfigPreviewFile> {
    let mut files = Vec::with_capacity(plan.extra_writes.len() + 1);
    files.push(ToolConfigPreviewFile {
        path: plan.target_path.display().to_string(),
        content: content.to_string(),
        diff: aipass_config_writers::diff_preview_for_path(&plan.target_path, content),
    });
    for write in &plan.extra_writes {
        files.push(ToolConfigPreviewFile {
            path: write.target_path.display().to_string(),
            content: write.content.clone(),
            diff: aipass_config_writers::diff_preview_for_path(&write.target_path, &write.content),
        });
    }
    files
}

fn combined_tool_config_preview(files: &[ToolConfigPreviewFile]) -> String {
    if files.len() == 1 {
        return files[0].diff.clone();
    }
    files
        .iter()
        .map(|file| format!("--- {}\n{}", file.path, file.diff))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn advertised_bind_addr(bind_addr: &str) -> String {
    bind_addr
        .parse::<std::net::SocketAddr>()
        .ok()
        .and_then(|addr| {
            addr.ip().is_unspecified().then(|| match addr.ip() {
                IpAddr::V4(_) => format!("127.0.0.1:{}", addr.port()),
                IpAddr::V6(_) => format!("[::1]:{}", addr.port()),
            })
        })
        .unwrap_or_else(|| bind_addr.to_string())
}

fn tool_config_tool_for(tool: &ToolId) -> ToolConfigTool {
    match tool {
        ToolId::Codex => ToolConfigTool::Codex,
        ToolId::ClaudeCode => ToolConfigTool::ClaudeCode,
        ToolId::GeminiCli => ToolConfigTool::GeminiCli,
        ToolId::OpenCode => ToolConfigTool::OpenCode,
        ToolId::Grok => ToolConfigTool::Grok,
        ToolId::Pi => ToolConfigTool::Pi,
        ToolId::Cursor => ToolConfigTool::Cursor,
    }
}

fn tool_apply_response(
    request: ToolConfigRequest,
    entry: EntrySummary,
    plan: ConfigPlan,
    result: ApplyResult,
) -> ToolConfigApplyResponse {
    ToolConfigApplyResponse {
        tool: request.tool,
        mode: request.mode,
        entry_id: entry.id,
        entry_title: entry.title,
        operation_id: result.operation_id,
        target_path: result.target_path.display().to_string(),
        backup_path: result.backup_path.display().to_string(),
        summary: plan.summary,
    }
}

fn env_key_for_entry(item: &EntrySummary) -> String {
    match item.provider_id.as_deref() {
        Some("anthropic") => "ANTHROPIC_API_KEY".to_string(),
        Some("gemini") => "GEMINI_API_KEY".to_string(),
        Some("openrouter") => "OPENROUTER_API_KEY".to_string(),
        Some("deepseek") => "DEEPSEEK_API_KEY".to_string(),
        Some("moonshot") => "MOONSHOT_API_KEY".to_string(),
        Some("qwen") => "DASHSCOPE_API_KEY".to_string(),
        Some("zhipu") => "ZHIPUAI_API_KEY".to_string(),
        Some("volcengine") => "ARK_API_KEY".to_string(),
        Some("groq") => "GROQ_API_KEY".to_string(),
        Some("replicate") => "REPLICATE_API_TOKEN".to_string(),
        Some("together") => "TOGETHER_API_KEY".to_string(),
        Some("fireworks") => "FIREWORKS_API_KEY".to_string(),
        _ => match item.auth_scheme {
            AuthScheme::GoogleApiKey => "GEMINI_API_KEY".to_string(),
            AuthScheme::AzureApiKey => "AZURE_OPENAI_API_KEY".to_string(),
            _ => "AIPASS_API_KEY".to_string(),
        },
    }
}

fn plan_tool_env_helper(
    home: &Path,
    tool: ToolConfigTool,
    entry: &ToolEntry,
) -> ServiceResult<(ConfigPlan, String)> {
    let tool_id = match tool {
        ToolConfigTool::Codex => aipass_config_writers::ToolId::Codex,
        ToolConfigTool::ClaudeCode => aipass_config_writers::ToolId::ClaudeCode,
        ToolConfigTool::GeminiCli => aipass_config_writers::ToolId::GeminiCli,
        ToolConfigTool::OpenCode => aipass_config_writers::ToolId::OpenCode,
        ToolConfigTool::Grok => aipass_config_writers::ToolId::Grok,
        ToolConfigTool::Pi => aipass_config_writers::ToolId::Pi,
        ToolConfigTool::Cursor => aipass_config_writers::ToolId::Cursor,
    };
    let tool_name = match tool {
        ToolConfigTool::Codex => "codex",
        ToolConfigTool::ClaudeCode => "claude-code",
        ToolConfigTool::GeminiCli => "gemini-cli",
        ToolConfigTool::OpenCode => "opencode",
        ToolConfigTool::Grok => "grok",
        ToolConfigTool::Pi => "pi",
        ToolConfigTool::Cursor => "cursor",
    };
    let target = home
        .join(".aipass")
        .join("tools")
        .join(format!("{tool_name}.env"));
    let operation_id = Uuid::new_v4();
    let backup_path = config_backup_path(&target);
    let env_key = match tool {
        ToolConfigTool::ClaudeCode if matches!(&entry.auth_scheme, AuthScheme::Bearer) => {
            "ANTHROPIC_AUTH_TOKEN"
        }
        ToolConfigTool::ClaudeCode => "ANTHROPIC_API_KEY",
        ToolConfigTool::GeminiCli => "GEMINI_API_KEY",
        _ => entry.env_key.as_str(),
    };
    let mut content = format!(
        "# Generated by AIPass. This file stores helper references, not plaintext secrets.\nexport {env_key}=\"$(aipass get {} --field api_key --reveal)\"\n",
        entry.id
    );
    if let Some(endpoint) = &entry.endpoint {
        let endpoint_key = match tool {
            ToolConfigTool::ClaudeCode => "ANTHROPIC_BASE_URL",
            ToolConfigTool::GeminiCli => "GOOGLE_GEMINI_BASE_URL",
            _ => "AIPASS_BASE_URL",
        };
        content.push_str(&format!(
            "export {endpoint_key}={}\n",
            shell_quote(endpoint)
        ));
    }
    if let Some(model) = &entry.default_model {
        let model_key = match tool {
            ToolConfigTool::ClaudeCode => Some("ANTHROPIC_MODEL"),
            ToolConfigTool::GeminiCli => Some("GEMINI_MODEL"),
            _ => None,
        };
        if let Some(model_key) = model_key {
            content.push_str(&format!("export {model_key}={}\n", shell_quote(model)));
        }
    }
    let plan = ConfigPlan {
        operation_id,
        tool: tool_id,
        target_path: target.clone(),
        backup_path,
        summary: format!("Configure {tool_name} env helper for {}", entry.title),
        preview: redacted_diff_preview(&diff_preview_for_path(&target, &content), &[]),
        extra_writes: Vec::new(),
        codex_provider_migration: None,
    };
    Ok((plan, content))
}

fn backfill_provider_favicons(
    state: &Arc<AgentState>,
    request: FaviconBackfillRequest,
) -> ServiceResult<FaviconBackfillResponse> {
    let _backfill_guard = state.favicon_backfill.lock().map_err(|_| {
        ServiceError::new(AgentErrorCode::Internal, "favicon backfill lock poisoned")
    })?;
    let limit = request
        .limit
        .unwrap_or(FAVICON_BACKFILL_DEFAULT_LIMIT)
        .min(FAVICON_BACKFILL_MAX_LIMIT);
    let mut response = FaviconBackfillResponse::default();
    let entries = with_vault(state, false, |vault| {
        favicon_backfill_entries(vault, request.entry_ids, &mut response).map_err(map_vault_error)
    })?;
    let client = HttpClient::builder()
        .timeout(FAVICON_REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("AIPass/1.0")
        .build()
        .map_err(ServiceError::internal)?;

    for entry in entries {
        if favicon_backfill_entry_is_skippable(&entry) {
            response.skipped += 1;
            continue;
        }
        if response.checked >= limit {
            response.skipped += 1;
            continue;
        }
        response.checked += 1;
        let Some(favicon_url) = resolve_favicon_data_url(&client, &entry) else {
            response.skipped += 1;
            continue;
        };
        match with_vault(state, false, |vault| {
            vault
                .replace_provider_favicon_url(entry.id, favicon_url)
                .map_err(map_vault_error)
        }) {
            Ok(Some(updated)) => {
                response.updated += 1;
                response.entries.push(updated);
            }
            Ok(None) => response.skipped += 1,
            Err(err) if err.code == AgentErrorCode::Locked => return Err(err),
            Err(err) => response.errors.push(FaviconBackfillError {
                entry_id: Some(entry.id),
                message: err.message,
            }),
        }
    }

    Ok(response)
}

fn favicon_backfill_entries(
    vault: &Vault,
    entry_ids: Option<Vec<Uuid>>,
    response: &mut FaviconBackfillResponse,
) -> Result<Vec<EntrySummary>, aipass_vault::VaultError> {
    match entry_ids {
        Some(entry_ids) => {
            let mut seen = HashSet::new();
            let mut entries = Vec::new();
            for entry_id in entry_ids {
                if !seen.insert(entry_id) {
                    response.skipped += 1;
                    continue;
                }
                match vault.get_provider_summary(entry_id) {
                    Ok(entry) => entries.push(entry),
                    Err(err) => response.errors.push(FaviconBackfillError {
                        entry_id: Some(entry_id),
                        message: err.to_string(),
                    }),
                }
            }
            Ok(entries)
        }
        None => vault.list_provider_summaries(),
    }
}

fn favicon_backfill_entry_is_skippable(entry: &EntrySummary) -> bool {
    entry.archived_at.is_some()
        || entry.deleted_at.is_some()
        || entry
            .favicon_url
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| value.starts_with("data:image/"))
}

fn resolve_favicon_data_url(client: &HttpClient, entry: &EntrySummary) -> Option<String> {
    favicon_url_candidates(entry)
        .into_iter()
        .find_map(|candidate| download_favicon_data_url(client, &candidate))
}

fn favicon_url_candidates(entry: &EntrySummary) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    if let Some(favicon_url) = entry.favicon_url.as_deref() {
        push_direct_favicon_candidate(&mut candidates, &mut seen, favicon_url);
    }

    if let Some(provider_id) = entry.provider_id.as_deref() {
        for provider in default_provider_definitions()
            .into_iter()
            .filter(|provider| provider.id == provider_id)
        {
            for (_, kind, url) in provider.endpoints {
                if kind == &EndpointKind::Console {
                    push_favicon_candidate(&mut candidates, &mut seen, url);
                }
            }
        }
    }

    for endpoint in &entry.endpoints {
        if endpoint.kind == EndpointKind::Console {
            if let Some(url) = endpoint.url.as_deref() {
                push_favicon_candidate(&mut candidates, &mut seen, url);
            }
        }
    }

    for domain in &entry.domains {
        push_favicon_candidate(&mut candidates, &mut seen, domain);
    }

    for endpoint in &entry.endpoints {
        if endpoint.kind == EndpointKind::Api {
            if let Some(url) = endpoint.url.as_deref() {
                push_favicon_candidate(&mut candidates, &mut seen, url);
            }
        }
    }

    candidates
}

fn push_direct_favicon_candidate(
    candidates: &mut Vec<String>,
    seen: &mut HashSet<String>,
    value: &str,
) {
    let Ok(mut url) = Url::parse(value.trim()) else {
        return;
    };
    if !matches!(url.scheme(), "https" | "http")
        || !url.username().is_empty()
        || url.password().is_some()
        || favicon_host_is_blocked(&url)
    {
        return;
    }
    url.set_fragment(None);
    let candidate = url.to_string();
    if seen.insert(candidate.clone()) {
        candidates.push(candidate);
    }
}

fn push_favicon_candidate(candidates: &mut Vec<String>, seen: &mut HashSet<String>, value: &str) {
    if let Some(candidate) = favicon_url_from_origin_candidate(value) {
        if seen.insert(candidate.clone()) {
            candidates.push(candidate);
        }
    }
}

fn favicon_url_from_origin_candidate(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let candidate = if value.starts_with("https://") || value.starts_with("http://") {
        value.to_string()
    } else {
        format!("https://{value}")
    };
    let mut url = Url::parse(&candidate).ok()?;
    if !matches!(url.scheme(), "https" | "http") || favicon_host_is_blocked(&url) {
        return None;
    }
    url.set_path("/favicon.ico");
    url.set_query(None);
    url.set_fragment(None);
    Some(url.to_string())
}

fn favicon_host_is_blocked(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return true;
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    host.trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
        .map(ip_addr_is_blocked_for_favicon)
        .unwrap_or(false)
}

fn ip_addr_is_blocked_for_favicon(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_multicast()
        }
        IpAddr::V6(ip) => {
            let first = ip.segments()[0];
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
        }
    }
}

fn download_favicon_data_url(client: &HttpClient, candidate: &str) -> Option<String> {
    if !favicon_candidate_resolves_publicly(candidate) {
        return None;
    }
    let response = client
        .get(candidate)
        .header(
            ACCEPT,
            "image/avif,image/webp,image/apng,image/png,image/jpeg,image/gif,image/x-icon,image/vnd.microsoft.icon,image/*;q=0.8",
        )
        .send()
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    if response
        .content_length()
        .is_some_and(|length| length == 0 || length > MAX_FAVICON_BYTES as u64)
    {
        return None;
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .map(str::to_ascii_lowercase);
    if content_type.as_deref().is_some_and(|value| {
        !favicon_content_type_is_image(value) && value != "application/octet-stream"
    }) {
        return None;
    }
    let mut bytes = Vec::new();
    response
        .take((MAX_FAVICON_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.is_empty() || bytes.len() > MAX_FAVICON_BYTES {
        return None;
    }
    let mime = favicon_image_mime(&bytes)?;
    Some(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

fn favicon_candidate_resolves_publicly(candidate: &str) -> bool {
    let Ok(url) = Url::parse(candidate) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    if let Ok(ip) = host.parse::<IpAddr>() {
        return !ip_addr_is_blocked_for_favicon(ip);
    }
    let Some(port) = url.port_or_known_default() else {
        return false;
    };
    let Ok(addresses) = (host, port).to_socket_addrs() else {
        return false;
    };
    favicon_resolved_addresses_are_public(addresses.map(|address| address.ip()))
}

fn favicon_resolved_addresses_are_public(addresses: impl IntoIterator<Item = IpAddr>) -> bool {
    let mut found = false;
    for address in addresses {
        found = true;
        if ip_addr_is_blocked_for_favicon(address) {
            return false;
        }
    }
    found
}

fn favicon_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.starts_with(&[0, 0, 1, 0]) {
        return Some("image/x-icon");
    }
    if bytes.starts_with(b"BM") {
        return Some("image/bmp");
    }
    if bytes.len() >= 16
        && &bytes[4..8] == b"ftyp"
        && bytes[8..]
            .chunks(4)
            .take(5)
            .any(|brand| brand == b"avif" || brand == b"avis")
    {
        return Some("image/avif");
    }
    None
}

fn favicon_content_type_is_image(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    value.starts_with("image/") || value.contains("svg") || value.contains("icon")
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

fn apply_auth(request: RequestBuilder, auth_scheme: &AuthScheme, secret: &str) -> RequestBuilder {
    match auth_scheme {
        AuthScheme::Bearer => request.bearer_auth(secret),
        AuthScheme::XApiKey => request.header("x-api-key", secret),
        AuthScheme::AzureApiKey => request.header("api-key", secret),
        AuthScheme::CustomHeader => request.header("authorization", secret),
        AuthScheme::GoogleApiKey | AuthScheme::AwsProfile => request,
    }
}

fn endpoint_url(endpoints: &[ProviderEndpoint]) -> Option<String> {
    protocol_endpoint_url(endpoints)
}

fn join_url(base: &str, suffix: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        suffix.trim_start_matches('/')
    )
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
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
) -> ServiceResult<Vec<SyncConflictResponse>> {
    list_conflicts(root)
        .map_err(ServiceError::internal)?
        .into_iter()
        .map(|record| conflict_response(scope.clone(), root, vault, record))
        .collect()
}

fn conflict_response(
    scope: ConflictScope,
    root: &Path,
    vault: &Vault,
    record: ConflictRecord,
) -> ServiceResult<SyncConflictResponse> {
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

fn conflict_root(vault_dir: &Path, request: &SyncConflictActionRequest) -> ServiceResult<PathBuf> {
    match request.scope {
        ConflictScope::Vault => Ok(vault_dir.to_path_buf()),
        ConflictScope::Sync => {
            if let Some(provider) = request.provider {
                return cloud_sync_dir(provider).map_err(ServiceError::internal);
            }
            request.dir.clone().ok_or_else(|| {
                ServiceError::new(
                    AgentErrorCode::ValidationFailed,
                    "sync conflict scope requires a local or cloud sync target",
                )
            })
        }
    }
}

fn home_dir() -> ServiceResult<PathBuf> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("USERPROFILE").map(PathBuf::from))
        .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "home directory unavailable"))
}

pub(crate) fn run_sync_local(state: &Arc<AgentState>, dir: &Path) -> ServiceResult<SyncReport> {
    let operation = crate::operation_log::OperationLog::background("sync.local");
    let result = run_sync_local_inner(state, dir);
    if let Some(operation) = operation {
        operation.finish(&match &result {
            Ok(report) => AgentResponse::success(report),
            Err(err) => AgentResponse::error(err.code.clone(), "sync failed"),
        });
    }
    result
}

fn run_sync_local_inner(state: &Arc<AgentState>, dir: &Path) -> ServiceResult<SyncReport> {
    // Lock order is sync_lock -> session -> proxy everywhere: a sync in flight
    // and its follow-up vault/proxy reloads stay atomic against vault writes,
    // which all go through the session lock.
    let _guard = state
        .sync_lock
        .lock()
        .map_err(|_| ServiceError::internal(anyhow::anyhow!("sync lock poisoned")))?;
    let report = {
        let mut session = state
            .session
            .lock()
            .map_err(|_| ServiceError::internal(anyhow::anyhow!("session lock poisoned")))?;
        match &mut *session {
            SessionState::Locked => {
                let vault_id = Vault::vault_id_from_manifest(&state.vault_dir)
                    .map_err(ServiceError::internal)?;
                sync_local_folder_with_validator(&state.vault_dir, dir, &|bytes| {
                    validate_sync_object_bytes_for_vault(bytes, vault_id)
                })
                .map_err(ServiceError::internal)?
            }
            SessionState::Unlocked(info) => {
                let report = sync_local_folder_with_validator(&state.vault_dir, dir, &|bytes| {
                    info.vault
                        .validate_sync_object_bytes(bytes)
                        .map_err(Into::into)
                })
                .map_err(ServiceError::internal)?;
                if report.downloaded > 0 {
                    // The sync may have replaced records (and key wrapping) on
                    // disk; refresh the in-memory vault before any later write
                    // can persist stale state over the downloaded files.
                    info.vault.reload_from_disk().map_err(map_vault_error)?;
                }
                report
            }
        }
    };
    if report.downloaded > 0 {
        reload_proxy_after_sync_download(state);
    }
    Ok(report)
}

pub(crate) fn run_sync_webdav(state: &Arc<AgentState>, client: &impl WebDavClient) -> SyncReport {
    let operation = crate::operation_log::OperationLog::background("sync.webdav");
    let report = run_sync_webdav_inner(state, client);
    if let Some(operation) = operation {
        operation.finish(&AgentResponse::success(&report));
    }
    report
}

fn run_sync_webdav_inner(state: &Arc<AgentState>, client: &impl WebDavClient) -> SyncReport {
    let sync_error = |message: &str| SyncReport {
        uploaded: 0,
        downloaded: 0,
        conflicts: 0,
        quarantined: 0,
        status: SyncStatus::ServerError,
        message: Some(message.to_string()),
    };
    let Ok(_guard) = state.sync_lock.lock() else {
        return sync_error("sync lock poisoned");
    };
    let report = {
        let Ok(mut session) = state.session.lock() else {
            return sync_error("session lock poisoned");
        };
        match &mut *session {
            SessionState::Locked => {
                let Ok(vault_id) = Vault::vault_id_from_manifest(&state.vault_dir) else {
                    return sync_error("vault manifest unavailable");
                };
                match sync_webdav_with_validator(&state.vault_dir, client, &|bytes| {
                    validate_sync_object_bytes_for_vault(bytes, vault_id)
                }) {
                    Ok(report) => report,
                    Err(err) => {
                        let message = err.to_string();
                        let status = classify_webdav_error(&err);
                        SyncReport {
                            uploaded: 0,
                            downloaded: 0,
                            conflicts: 0,
                            quarantined: 0,
                            status,
                            message: Some(message),
                        }
                    }
                }
            }
            SessionState::Unlocked(info) => {
                match sync_webdav_with_validator(&state.vault_dir, client, &|bytes| {
                    info.vault
                        .validate_sync_object_bytes(bytes)
                        .map_err(Into::into)
                }) {
                    Ok(report) => {
                        if report.downloaded > 0 {
                            if let Err(err) = info.vault.reload_from_disk() {
                                write_component_log(
                                    AGENT_LOG,
                                    "WARN",
                                    &format!("vault reload after webdav sync failed: {err}"),
                                );
                            }
                        }
                        report
                    }
                    Err(err) => {
                        let message = err.to_string();
                        let status = classify_webdav_error(&err);
                        SyncReport {
                            uploaded: 0,
                            downloaded: 0,
                            conflicts: 0,
                            quarantined: 0,
                            status,
                            message: Some(message),
                        }
                    }
                }
            }
        }
    };
    if report.downloaded > 0 {
        reload_proxy_after_sync_download(state);
    }
    report
}

/// Downloaded sync objects can carry changed provider credentials; rebuild the
/// running proxy's runtime snapshot so it does not keep serving stale keys.
/// Failures are logged rather than failing the sync: reload_if_running has
/// already stopped the proxy safely when the new snapshot cannot load.
fn reload_proxy_after_sync_download(state: &Arc<AgentState>) {
    let session = match state.session.lock() {
        Ok(session) => session,
        Err(_) => {
            write_component_log(
                AGENT_LOG,
                "WARN",
                "proxy reload after sync skipped: session lock poisoned",
            );
            return;
        }
    };
    let SessionState::Unlocked(info) = &*session else {
        return;
    };
    let mut proxy = match state.proxy.lock() {
        Ok(proxy) => proxy,
        Err(poisoned) => {
            write_component_log(
                AGENT_LOG,
                "WARN",
                "recovering poisoned proxy lock after sync download",
            );
            poisoned.into_inner()
        }
    };
    if let Err(err) = proxy.reload_if_running(&info.vault) {
        write_component_log(
            AGENT_LOG,
            "WARN",
            &format!(
                "proxy reload after sync download failed code={:?}",
                err.code
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aipass_agent_protocol::{SessionPolicy, SessionStatus};
    use aipass_crypto::SecretString;
    use std::io::Write;
    use std::sync::{Mutex, OnceLock};
    use std::time::Instant;
    use tempfile::tempdir;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn proxy_endpoint_matches_each_client_sdk_contract() {
        let bind = "127.0.0.1:8787";
        assert_eq!(
            proxy_endpoint_for_tool(&ToolId::ClaudeCode, ProxyProtocol::AnthropicMessages, bind),
            "http://127.0.0.1:8787"
        );
        assert_eq!(
            proxy_endpoint_for_tool(&ToolId::OpenCode, ProxyProtocol::AnthropicMessages, bind),
            "http://127.0.0.1:8787/v1"
        );
        assert_eq!(
            proxy_endpoint_for_tool(&ToolId::OpenCode, ProxyProtocol::OpenAiResponses, bind),
            "http://127.0.0.1:8787/v1"
        );
        assert_eq!(
            proxy_endpoint_for_tool(&ToolId::Cursor, ProxyProtocol::OpenAiChatCompletions, bind),
            "http://127.0.0.1:8787/v1"
        );
        assert_eq!(
            proxy_endpoint_for_tool(&ToolId::Cursor, ProxyProtocol::AnthropicMessages, bind),
            "http://127.0.0.1:8787/v1/messages"
        );
    }

    #[test]
    fn proxy_tool_capabilities_distinguish_endpoint_and_protocol_limits() {
        assert!(
            ensure_proxy_tool_protocol(&ToolId::OpenCode, ProxyProtocol::OpenAiResponses).is_ok()
        );
        assert!(
            ensure_proxy_tool_protocol(&ToolId::OpenCode, ProxyProtocol::AnthropicMessages).is_ok()
        );
        let codex_error =
            ensure_proxy_tool_protocol(&ToolId::Codex, ProxyProtocol::OpenAiChatCompletions)
                .expect_err("Codex should require Responses");
        assert!(codex_error.message.contains("protocol"));
        assert!(
            ensure_proxy_tool_protocol(&ToolId::Cursor, ProxyProtocol::OpenAiChatCompletions)
                .is_ok()
        );
        assert!(
            ensure_proxy_tool_protocol(&ToolId::Cursor, ProxyProtocol::AnthropicMessages).is_ok()
        );
        assert!(
            ensure_proxy_tool_protocol(&ToolId::Cursor, ProxyProtocol::OpenAiResponses).is_ok()
        );
    }

    struct EnvRestore {
        name: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvRestore {
        fn capture(name: &'static str) -> Self {
            Self {
                name,
                previous: std::env::var_os(name),
            }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.name, value),
                None => std::env::remove_var(self.name),
            }
        }
    }

    struct RunningAgent {
        root: PathBuf,
        vault_dir: PathBuf,
        client: crate::AgentClient,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl RunningAgent {
        fn start() -> Self {
            let dir = tempdir().expect("tempdir");
            let vault_dir = dir.path().join("vault");
            aipass_vault::Vault::create(
                &vault_dir,
                &SecretString::new("correct horse battery staple"),
            )
            .expect("create vault");
            // Pin sync to a temp folder: macOS defaults to iCloud when no
            // sync settings file exists, and startup sync must never write
            // into a real cloud directory on the host running the tests.
            let settings = crate::session::PersistedSyncSettings {
                mode: SyncMode::Local,
                sync_folder: Some(dir.path().join("sync")),
                ..Default::default()
            };
            let settings_path = crate::session::sync_settings_path(&vault_dir);
            fs::create_dir_all(settings_path.parent().expect("settings parent"))
                .expect("create settings dir");
            atomic_write_bytes(
                &settings_path,
                &serde_json::to_vec_pretty(&settings).expect("encode settings"),
            )
            .expect("write sync settings");
            let root = dir.keep();
            let server_vault_dir = vault_dir.clone();
            let handle = thread::spawn(move || {
                run_server(ServerOptions::without_desktop_tray(server_vault_dir)).expect("server");
            });
            let client = crate::AgentClient::for_vault(vault_dir.clone()).expect("client");
            for _ in 0..50 {
                if client
                    .request::<SessionStatus>(&AgentRequest::SessionStatus)
                    .is_ok()
                {
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }
            Self {
                root,
                vault_dir,
                client,
                handle: Some(handle),
            }
        }
    }

    impl Drop for RunningAgent {
        fn drop(&mut self) {
            let _ = self.client.shutdown();
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn sequential_status_requests_do_not_pay_an_accept_poll_delay() {
        let agent = RunningAgent::start();
        let started = Instant::now();
        for _ in 0..12 {
            agent
                .client
                .request::<SessionStatus>(&AgentRequest::SessionStatus)
                .expect("session status");
        }
        let elapsed = started.elapsed();
        eprintln!("12 sequential status requests: {elapsed:?}");
        assert!(elapsed < Duration::from_secs(1), "elapsed {elapsed:?}");
    }

    #[test]
    fn constant_time_eq_checks_full_input() {
        assert!(constant_time_eq(b"same", b"same"));
        assert!(!constant_time_eq(b"same", b"some"));
        assert!(!constant_time_eq(b"same", b"same-longer"));
        assert!(!constant_time_eq(b"same-longer", b"same"));
    }

    #[test]
    fn endpoint_interface_inference_follows_shared_ai_evidence() {
        assert_eq!(
            infer_interface_from_endpoint("https://llm.example.test/api/paas/v4"),
            Some(InterfaceType::OpenAiCompatible)
        );
        assert_eq!(
            infer_interface_from_endpoint("https://llm.example.test/v1beta/models"),
            Some(InterfaceType::OpenAiCompatible)
        );
        assert_eq!(
            infer_interface_from_endpoint("https://claude-relay.example.test/v1/messages"),
            Some(InterfaceType::AnthropicMessages)
        );
        assert_eq!(
            infer_interface_from_endpoint("https://gemini-proxy.example.test/v1beta/models"),
            Some(InterfaceType::Gemini)
        );
        assert_eq!(
            infer_interface_from_endpoint("https://api.minimaxi.com/v1"),
            Some(InterfaceType::OpenAiCompatible)
        );
        assert_eq!(
            infer_interface_from_endpoint("https://example.test/hooks/deploy"),
            None
        );
        assert_eq!(
            infer_interface_from_endpoint("https://api.replicate.com"),
            None
        );
    }

    #[test]
    fn current_process_options_respect_tray_suppression_env() {
        let _guard = env_lock().lock().unwrap();
        let _restore = EnvRestore::capture(crate::desktop::SUPPRESS_TRAY_ENV);
        let vault_dir = PathBuf::from("/tmp/aipass-test-vault");

        std::env::remove_var(crate::desktop::SUPPRESS_TRAY_ENV);
        assert!(ServerOptions::for_current_process(vault_dir.clone()).launch_desktop_tray);

        std::env::set_var(crate::desktop::SUPPRESS_TRAY_ENV, "1");
        assert!(!ServerOptions::for_current_process(vault_dir.clone()).launch_desktop_tray);

        std::env::set_var(crate::desktop::SUPPRESS_TRAY_ENV, "0");
        assert!(ServerOptions::for_current_process(vault_dir).launch_desktop_tray);
    }

    #[test]
    fn auth_json_preview_includes_plaintext_credentials() {
        let dir = tempdir().expect("tempdir");
        let target = dir.path().join("auth.json");
        fs::write(
            &target,
            r#"{"OPENAI_API_KEY":"sk-old-secret","other":true}"#,
        )
        .expect("write old config");
        let content = "{\n  \"OPENAI_API_KEY\": \"sk-new-secret\",\n  \"other\": true\n}";
        let plan = ConfigPlan {
            operation_id: Uuid::new_v4(),
            tool: ToolId::Codex,
            target_path: target.clone(),
            backup_path: target.with_extension("backup"),
            summary: "preview".to_string(),
            preview: aipass_config_writers::diff_preview_for_path(&target, content),
            extra_writes: Vec::new(),
            codex_provider_migration: None,
        };

        let files = tool_config_preview_files(&plan, content);
        assert_eq!(files.len(), 1);
        assert!(files[0].content.contains("sk-new-secret"));
        assert!(files[0].diff.contains("sk-old-secret"));
        assert!(files[0].diff.contains("sk-new-secret"));
        assert_eq!(combined_tool_config_preview(&files), files[0].diff);
    }

    #[test]
    fn agent_starts_locked_even_when_the_vault_already_exists() {
        let agent = RunningAgent::start();
        let status = agent
            .client
            .request::<SessionStatus>(&AgentRequest::SessionStatus)
            .expect("status response");

        assert!(status.exists);
        assert!(status.locked);
        assert_eq!(status.last_lock_reason, Some(LockReason::AgentRestart));
    }

    #[test]
    fn initial_sync_runs_at_startup_and_clears_the_pending_flag() {
        let agent = RunningAgent::start();

        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let status = agent
                .client
                .request::<SessionStatus>(&AgentRequest::SessionStatus)
                .expect("status response");
            if !status.initial_sync_pending {
                break;
            }
            assert!(Instant::now() < deadline, "initial sync stuck pending");
            thread::sleep(Duration::from_millis(50));
        }

        // The vault was synced into the configured local folder. A fresh vault
        // has no provider objects, but it always has a device record.
        let sync_devices = agent.root.join("sync").join("devices");
        assert!(sync_devices.is_dir());
        assert!(fs::read_dir(sync_devices).expect("sync devices").count() > 0);
    }

    fn sync_test_state(vault_dir: PathBuf) -> Arc<AgentState> {
        Arc::new(AgentState {
            policy: Mutex::new(SessionPolicy::default()),
            vault_dir: vault_dir.clone(),
            namespace: "test".to_string(),
            auth_token: SensitiveString::from("token"),
            session: Mutex::new(SessionState::Locked),
            session_changed: Condvar::new(),
            last_lock_reason: Mutex::new(None),
            proxy: Mutex::new(
                crate::proxy_service::ProxyService::new(&vault_dir).expect("proxy service"),
            ),
            favicon_backfill: Mutex::new(()),
            sync_lock: Mutex::new(()),
            initial_sync: Mutex::new(InitialSyncState::Done),
            sync_watcher: Mutex::new(None),
            shutdown: AtomicBool::new(false),
        })
    }

    fn sync_test_provider(title: &str, api_key: &str) -> ProviderEntryInput {
        ProviderEntryInput {
            title: title.to_string(),
            provider_kind: ProviderKind::Unknown,
            provider_id: Some("openai".to_string()),
            credential_kind: Default::default(),
            account_identity: None,
            domains: Vec::new(),
            favicon_url: None,
            endpoints: vec![ProviderEndpoint::api("http://127.0.0.1:9/v1")],
            interface_type: InterfaceType::OpenAiCompatible,
            auth_scheme: AuthScheme::Bearer,
            api_key: api_key.to_string(),
            secret_label: None,
            default_model: None,
            model_aliases: Vec::new(),
            headers: Vec::new(),
            quota: None,
            subscription: None,
            gateway: None,
            tags: Vec::new(),
            notes: None,
            secret_metadata: Default::default(),
        }
    }

    #[test]
    fn provider_operations_are_correlated_persistent_and_secret_free() {
        let temp = tempdir().unwrap();
        let vault_dir = temp.path().join("vault");
        let creation =
            Vault::create(&vault_dir, &SecretString::new("test-master-password")).unwrap();
        let state = sync_test_state(vault_dir);
        crate::session::set_session_vault(&state, creation.vault);
        let log_dir = temp.path().join("logs");
        let request_id = Uuid::new_v4();
        let provider_id = crate::logging::with_test_log_dir(&log_dir, || {
            let _scope = crate::logging::RequestScope::new(request_id);
            let input = sync_test_provider("private-provider-title", "fake-private-api-key");
            let response = handle_request(
                &state,
                AgentRequest::ProviderAdd {
                    input: input.clone(),
                },
            );
            assert!(response.ok);
            let id: Uuid = serde_json::from_value(response.data).unwrap();
            let update = serde_json::from_value(serde_json::to_value(input).unwrap()).unwrap();
            for request in [
                AgentRequest::ProviderUpdate { id, input: update },
                AgentRequest::SecretAdd {
                    id,
                    label: "private-label".into(),
                    secret: "fake-second-key".into(),
                },
                AgentRequest::ProviderArchive { id },
                AgentRequest::ProviderRestore { id },
                AgentRequest::ProviderTrash { id },
                AgentRequest::ProviderDelete { id },
            ] {
                assert!(handle_request(&state, request).ok);
            }
            assert!(!handle_request(&state, AgentRequest::ProviderGet { id }).ok);
            assert!(handle_request(&state, AgentRequest::SessionStatus).ok);
            id
        });
        let logs = fs::read_to_string(log_dir.join("agent.log")).unwrap();
        for event in [
            "provider.add",
            "provider.update",
            "secret.add",
            "provider.archive",
            "provider.restore",
            "provider.trash",
            "provider.delete",
        ] {
            assert!(logs.contains(&format!("event={event} outcome=started")));
            assert!(logs.contains(&format!(
                "event={event} outcome=completed resource_id={provider_id}"
            )));
        }
        assert!(logs.contains("event=provider.get outcome=failed"));
        assert!(logs
            .lines()
            .all(|line| line.contains(&format!("request_id={request_id}"))));
        for forbidden in [
            "private-provider-title",
            "fake-private-api-key",
            "fake-second-key",
            "private-label",
            "test-master-password",
            "127.0.0.1:9",
            "event=session.status",
        ] {
            assert!(
                !logs.contains(forbidden),
                "unexpected log content: {forbidden}"
            );
        }
    }

    #[test]
    fn sync_download_reloads_the_unlocked_vault_and_keeps_the_proxy_serving() {
        let temp = tempdir().expect("tempdir");
        let vault_dir = temp.path().join("vault");
        let sync_dir = temp.path().join("sync");
        let password = SecretString::new("correct horse battery staple");
        let creation = Vault::create(&vault_dir, &password).expect("create vault");
        let state = sync_test_state(vault_dir.clone());
        crate::session::set_session_vault(&state, creation.vault);

        // Provider A is wired into a running proxy route group.
        let provider_a = with_vault(&state, false, |vault| {
            vault
                .add_provider(sync_test_provider("Upstream A", "key-a"))
                .map_err(map_vault_error)
        })
        .expect("add provider A");
        let secret_a = with_vault(&state, true, |vault| {
            vault
                .get_provider_summary(provider_a)
                .map_err(map_vault_error)
        })
        .expect("provider A summary")
        .secret_refs[0]
            .id
            .clone();
        let proxy_probe =
            std::net::TcpListener::bind("127.0.0.1:0").expect("reserve proxy address");
        let proxy_addr = proxy_probe.local_addr().expect("proxy address").to_string();
        drop(proxy_probe);
        with_vault(&state, false, |vault| {
            let mut proxy = state
                .proxy
                .lock()
                .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "proxy lock poisoned"))?;
            proxy.set_config(
                vault,
                aipass_proxy::ProxyConfig {
                    bind_addr: proxy_addr,
                    routes: vec![aipass_proxy::ProxyRouteConfig {
                        id: Uuid::new_v4(),
                        name: "default".into(),
                        token: "route-token".into(),
                        inbound_protocol: aipass_proxy::Protocol::OpenAiResponses,
                        upstream_protocol: aipass_proxy::Protocol::OpenAiResponses,
                        conversion_enabled: false,
                        strategy: aipass_proxy::RouteStrategy::Fallback,
                        targets: vec![aipass_proxy::ProxyTargetConfig {
                            id: Uuid::new_v4(),
                            provider_entry_id: provider_a,
                            secret_id: secret_a,
                            label: "primary".into(),
                            base_url: "http://127.0.0.1:9/v1".into(),
                            auth_scheme: "bearer".into(),
                            headers: Vec::new(),
                            group: None,
                            priority: 0,
                            weight: 1,
                            enabled: true,
                            protocol: None,
                        }],
                        retry: aipass_proxy::RetryPolicy::default(),
                        enabled: true,
                    }],
                    ..Default::default()
                },
            )?;
            proxy.start(vault)?;
            Ok(())
        })
        .expect("start proxy");

        let first = run_sync_local(&state, &sync_dir).expect("initial sync");
        assert_eq!(first.downloaded, 0);

        // A second writer adds provider B; the object reaches the sync folder
        // without the local vault copy, simulating a download from another
        // device.
        let provider_b = {
            let other = Vault::open(&vault_dir, &password).expect("second vault handle");
            other
                .add_provider(sync_test_provider("Upstream B", "key-b"))
                .expect("add provider B")
        };
        let object_b = vault_dir
            .join("objects")
            .join(format!("{provider_b}.aipobj"));
        fs::create_dir_all(sync_dir.join("objects")).expect("sync objects dir");
        fs::copy(
            &object_b,
            sync_dir
                .join("objects")
                .join(format!("{provider_b}.aipobj")),
        )
        .expect("seed remote object");
        fs::remove_file(&object_b).expect("remove local object B");

        let second = run_sync_local(&state, &sync_dir).expect("sync with download");
        assert_eq!(second.downloaded, 1);

        // The in-memory session vault sees the downloaded record, and the
        // running proxy rebuilt its snapshot without going down.
        let titles = with_vault(&state, true, |vault| {
            vault.list_provider_summaries().map_err(map_vault_error)
        })
        .expect("list providers")
        .into_iter()
        .map(|entry| entry.title)
        .collect::<Vec<_>>();
        assert!(titles.iter().any(|title| title == "Upstream B"));
        assert!(state.proxy.lock().expect("proxy lock").status().running);
    }

    #[test]
    fn incomplete_connection_does_not_block_subsequent_requests() {
        let agent = RunningAgent::start();
        let mut stuck = crate::ipc::connect(&agent.vault_dir).expect("connect stuck client");
        stuck.write_all(&8_u32.to_le_bytes()).expect("write length");

        let started = Instant::now();
        let status = agent
            .client
            .request::<SessionStatus>(&AgentRequest::SessionStatus)
            .expect("status response");

        assert!(status.exists);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "status request was blocked behind incomplete connection"
        );
    }

    fn favicon_test_entry() -> EntrySummary {
        EntrySummary {
            id: Uuid::new_v4(),
            title: "Example".to_string(),
            favorite: false,
            provider_id: None,
            provider_kind: aipass_provider_registry::ProviderKind::Unknown,
            credential_kind: Default::default(),
            account_identity: None,
            domains: vec!["example.com".to_string()],
            favicon_url: None,
            endpoints: vec![ProviderEndpoint::api("https://api.example.com/v1")],
            interface_type: InterfaceType::OpenAiCompatible,
            auth_scheme: AuthScheme::Bearer,
            masked_secret: "****".to_string(),
            fingerprint: "fp".to_string(),
            secret_refs: Vec::new(),
            default_model: None,
            model_aliases: Vec::new(),
            quota: None,
            subscription: None,
            gateway: None,
            tags: Vec::new(),
            notes: None,
            header_names: Vec::new(),
            created_at: OffsetDateTime::now_utc(),
            updated_at: OffsetDateTime::now_utc(),
            last_used_at: None,
            archived_at: None,
            deleted_at: None,
        }
    }

    #[test]
    fn favicon_candidates_follow_expected_source_order() {
        let mut entry = favicon_test_entry();
        entry.provider_id = Some("anthropic".to_string());
        entry.domains = vec!["domain.example".to_string()];
        entry.endpoints = vec![
            ProviderEndpoint::console("https://portal.example/settings"),
            ProviderEndpoint::api("https://api.example/v1"),
        ];

        let candidates = favicon_url_candidates(&entry);

        assert_eq!(
            candidates,
            vec![
                "https://console.anthropic.com/favicon.ico".to_string(),
                "https://portal.example/favicon.ico".to_string(),
                "https://domain.example/favicon.ico".to_string(),
                "https://api.example/favicon.ico".to_string(),
            ]
        );
    }

    #[test]
    fn favicon_candidates_skip_localhost_and_private_ip_literals() {
        for value in [
            "localhost",
            "http://localhost:3000/app",
            "127.0.0.1",
            "10.0.0.1",
            "169.254.1.1",
            "http://[::1]/",
            "http://[fe80::1]/",
            "http://[fc00::1]/",
        ] {
            assert_eq!(favicon_url_from_origin_candidate(value), None, "{value}");
        }
        assert_eq!(
            favicon_url_from_origin_candidate("example.com/path").as_deref(),
            Some("https://example.com/favicon.ico")
        );
    }

    #[test]
    fn favicon_dns_results_reject_any_private_address() {
        assert!(favicon_resolved_addresses_are_public([
            "1.1.1.1".parse().unwrap(),
            "2606:4700:4700::1111".parse().unwrap(),
        ]));
        assert!(!favicon_resolved_addresses_are_public([
            "1.1.1.1".parse().unwrap(),
            "127.0.0.1".parse().unwrap(),
        ]));
        assert!(!favicon_resolved_addresses_are_public(["10.0.0.4"
            .parse()
            .unwrap(),]));
        assert!(!favicon_resolved_addresses_are_public([]));
    }

    #[test]
    fn favicon_magic_detection_accepts_bounded_bitmap_formats() {
        assert_eq!(
            favicon_image_mime(b"\x89PNG\r\n\x1a\nrest"),
            Some("image/png")
        );
        assert_eq!(
            favicon_image_mime(&[0xff, 0xd8, 0xff, 0xe0]),
            Some("image/jpeg")
        );
        assert_eq!(favicon_image_mime(b"GIF89arest"), Some("image/gif"));
        assert_eq!(
            favicon_image_mime(b"RIFF\x04\x00\x00\x00WEBPrest"),
            Some("image/webp")
        );
        assert_eq!(
            favicon_image_mime(&[0, 0, 1, 0, 1, 0]),
            Some("image/x-icon")
        );
        assert_eq!(favicon_image_mime(b"<svg></svg>"), None);
        assert_eq!(favicon_image_mime(b"not an image"), None);
    }

    #[test]
    fn favicon_backfill_migrates_remote_urls_and_skips_cached_data() {
        let mut entry = favicon_test_entry();
        entry.favicon_url = Some("https://example.com/favicon.ico".to_string());
        assert!(!favicon_backfill_entry_is_skippable(&entry));
        assert_eq!(
            favicon_url_candidates(&entry).first().map(String::as_str),
            Some("https://example.com/favicon.ico")
        );

        entry.favicon_url = Some("data:image/png;base64,iVBORw0KGgo=".to_string());
        assert!(favicon_backfill_entry_is_skippable(&entry));
    }
}
