use crate::desktop::open_desktop_window;
use crate::ipc;
use crate::logging::{write_component_log, AGENT_LOG};
use crate::paths::{canonical_vault_dir, cloud_sync_dir, namespace_for_vault_dir};
use crate::session::{
    apply_sync_settings_update, clamp_policy, current_policy, load_policy, load_sync_settings,
    lock_if_idle, lock_session, map_vault_error, native_host_settings_path, reset_vault,
    save_policy, save_sync_settings, session_status, shutdown_requested, sync_settings_view,
    touch_session, unlock_with_password, wait_for_unlock, with_vault, with_vault_mut, AgentState,
    InitialSyncState, NativeHostSettings, ServiceError, ServiceResult, SessionState,
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
    plan_codex_plaintext_with_mode, plan_cursor_local, plan_cursor_local_plaintext,
    plan_gemini_cli, plan_gemini_cli_plaintext, plan_grok, plan_grok_plaintext,
    plan_grok_plaintext_with_backend, plan_opencode, plan_opencode_plaintext,
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
    accept_conflict_with_validator, discard_conflict, list_conflicts, ConflictRecord,
    HttpWebDavClient, SyncReport, SyncStatus, WebDavClient,
};
use aipass_vault::{EntrySummary, ProviderEntryInput, SecretMetadataInput, TtlGrantSummary, Vault};
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
        control_panel: Default::default(),
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
        cloudkit: Default::default(),
        webdav_transport: Mutex::new(None),
        sync_wake: std::sync::atomic::AtomicU64::new(0),
        initial_sync: Mutex::new(InitialSyncState::Pending),
        sync_revision: std::sync::atomic::AtomicU64::new(0),
        sync_status: Mutex::new(None),
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

pub(crate) use handlers::handle_request;

fn run_server_with_state(
    state: Arc<AgentState>,
    listener: Listener,
    launch_desktop_tray: bool,
) -> Result<()> {
    crate::control_panel::ControlPanel::restore(&state);
    spawn_idle_lock_watcher(state.clone());
    crate::session::spawn_power_watcher(state.clone());
    crate::pricing::spawn_list_price_refresh(state.clone());
    crate::oauth::spawn_token_refresh(state.clone());
    crate::websocket_capability::spawn(state.clone());
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

    state.control_panel.shutdown();
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
    let (done, finished) = std::sync::mpsc::channel();
    let timeout_state = state.clone();
    thread::spawn(move || {
        if finished.recv_timeout(Duration::from_secs(15)).is_err() {
            if let Ok(mut slot) = timeout_state.initial_sync.lock() {
                if *slot == InitialSyncState::Pending {
                    *slot = InitialSyncState::Failed;
                }
            }
        }
    });
    thread::spawn(move || {
        let outcome = run_initial_sync(&state);
        match state.initial_sync.lock() {
            Ok(mut slot) => *slot = outcome,
            Err(poisoned) => *poisoned.into_inner() = outcome,
        }
        let _ = done.send(());
    });
}

fn run_initial_sync(state: &Arc<AgentState>) -> InitialSyncState {
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
    if settings.mode == SyncMode::ICloud {
        return match crate::vault_sync::run_cloudkit(state) {
            Ok(report) if report.status == SyncStatus::Idle => InitialSyncState::Done,
            _ => InitialSyncState::Failed,
        };
    }
    let Some(dir) = crate::sync_watch::folder_sync_dir(&settings) else {
        return if matches!(settings.mode, SyncMode::ICloud | SyncMode::OneDrive) {
            InitialSyncState::Failed
        } else {
            InitialSyncState::Done
        };
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
            if report.status == SyncStatus::Idle {
                InitialSyncState::Done
            } else {
                InitialSyncState::Failed
            }
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

fn create_vault(
    state: &Arc<AgentState>,
    password: String,
    local_only: bool,
) -> ServiceResult<VaultCreateResponse> {
    let (recovery_kit, session) =
        crate::session::create_vault_with_options(state, password, local_only)?;
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

#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
#[path = "websocket_recovery_tests.rs"]
mod websocket_recovery_tests;

mod browser;
mod favicons;
mod probe;
mod sync;
mod tools;
use browser::*;
use favicons::*;
use probe::*;
#[cfg(test)]
pub(crate) use sync::TOOL_HOME_OVERRIDES;
use sync::{conflict_responses, conflict_root, home_dir};
pub(crate) use sync::{run_sync_configured, run_sync_local, run_sync_webdav_target};
use tools::*;
