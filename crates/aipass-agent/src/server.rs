use crate::desktop::open_desktop_window;
use crate::ipc;
use crate::paths::{canonical_vault_dir, cloud_sync_dir, namespace_for_vault_dir};
use crate::session::{
    apply_sync_settings_update, clamp_policy, current_policy, load_policy, load_sync_settings,
    lock_if_idle, lock_session, map_vault_error, native_host_settings_path, save_policy,
    save_sync_settings, session_status, shutdown_requested, sync_settings_password,
    sync_settings_password_requires_vault, sync_settings_password_without_vault,
    sync_settings_view, touch_session, unlock_with_password, with_vault, with_vault_mut,
    AgentState, NativeHostSettings, ServiceError, ServiceResult, SessionState,
};
use aipass_agent_protocol::{
    endpoint_url as protocol_endpoint_url, AgentErrorCode, AgentRequest, AgentResponse,
    AuthenticatedAgentRequest, BrowserContextLookupData, BrowserDetectedSecretFields,
    BrowserDetectedSecretPreview, BrowserFillResult, BrowserIgnoreOriginResult,
    BrowserIgnoredStatus, ConflictScope, LockReason, ProbeResult, SaveDetectedResult, SecretValue,
    SensitiveString, SessionUnlockMode, SyncConflictActionRequest, SyncConflictResponse, SyncMode,
    ToolConfigApplyResponse, ToolConfigMode, ToolConfigPreviewResponse, ToolConfigRequest,
    ToolConfigTool, VaultCreateResponse, MAX_FRAME_BYTES,
};
use aipass_config_writers::{
    apply_plan_encrypted, plan_claude_code, plan_claude_code_plaintext, plan_codex,
    plan_codex_plaintext, plan_gemini_cli, plan_gemini_cli_plaintext, plan_opencode,
    plan_opencode_plaintext, rollback_encrypted, ApplyResult, ConfigPlan, ToolEntry,
};
use aipass_crypto::{mask_secret, SecretString};
use aipass_provider_registry::{
    default_provider_definitions, match_provider_by_domain, provider_kind_for_id, AuthScheme,
    EndpointKind, InterfaceType, ProviderEndpoint,
};
use aipass_storage::atomic_write_bytes;
use aipass_sync::{
    accept_conflict, classify_webdav_error, discard_conflict, list_conflicts, sync_local_folder,
    sync_webdav, ConflictRecord, HttpWebDavClient, SyncReport, WebDavClient,
};
use aipass_vault::{
    EncryptedVaultExport, EntrySummary, ProviderEntryInput, TtlGrantSummary, Vault,
};
use anyhow::{bail, Context, Result};
use interprocess::local_socket::{prelude::*, ListenerNonblockingMode, Stream};
use reqwest::blocking::RequestBuilder;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::json;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use uuid::Uuid;
use zeroize::Zeroize;

const MAX_ACTIVE_CONNECTIONS: usize = 32;
const CONNECTION_IO_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug)]
pub struct ServerOptions {
    pub vault_dir: PathBuf,
}

pub fn run_server(options: ServerOptions) -> Result<()> {
    let vault_dir = canonical_vault_dir(options.vault_dir)?;
    let namespace = namespace_for_vault_dir(&vault_dir)?;
    let auth_token = ipc::load_or_create_auth_token(&vault_dir)?;
    let state = Arc::new(AgentState {
        policy: Mutex::new(load_policy(&vault_dir)?),
        vault_dir,
        namespace,
        auth_token,
        session: Mutex::new(SessionState::Locked),
        last_lock_reason: Mutex::new(Some(LockReason::AgentRestart)),
        shutdown: AtomicBool::new(false),
    });
    run_server_with_state(state)
}

#[path = "handlers.rs"]
mod handlers;

use handlers::handle_request;

fn run_server_with_state(state: Arc<AgentState>) -> Result<()> {
    let listener = ipc::listen(&state.vault_dir).with_context(|| {
        format!(
            "failed to bind agent listener for {}",
            state.vault_dir.display()
        )
    })?;
    listener
        .set_nonblocking(ListenerNonblockingMode::Accept)
        .context("failed to set agent listener to nonblocking accept mode")?;

    spawn_idle_lock_watcher(state.clone());

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
                        eprintln!("agent connection failed: {err}");
                    }
                });
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(200));
            }
            Err(err) if err.kind() == ErrorKind::Interrupted => continue,
            Err(err) => {
                eprintln!("agent accept failed: {err}");
                thread::sleep(Duration::from_millis(250));
            }
        }
    }

    let _ = ipc::clear_auth_token(&state.vault_dir);
    Ok(())
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
        Ok(payload) if auth_tokens_match(&payload.auth_token, &state.auth_token) => {
            handle_request(&state, payload.request)
        }
        Ok(_) => AgentResponse::error(AgentErrorCode::PermissionDenied, "invalid agent auth token"),
        Err(err) => AgentResponse::error(AgentErrorCode::ValidationFailed, err.to_string()),
    };
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

fn save_detected_secret(vault: &Vault, fields: BrowserDetectedSecretFields) -> ServiceResult<Uuid> {
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
    let api_key = fields.api_key.into_inner();
    vault
        .add_provider(ProviderEntryInput {
            title: preview.title,
            provider_kind,
            provider_id: preview.provider_id,
            domains: vec![domain],
            favicon_url: None,
            endpoints: preview
                .endpoint
                .clone()
                .into_iter()
                .map(ProviderEndpoint::api)
                .collect(),
            interface_type: preview.interface_type,
            auth_scheme: preview.auth_scheme,
            api_key,
            default_model: None,
            model_aliases: Vec::new(),
            headers: Vec::new(),
            quota: None,
            tags: preview.tags,
            environment: preview.environment,
            notes: Some(format!("Captured from {}", fields.origin)),
        })
        .map_err(map_vault_error)
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
    let environment = fields
        .environment
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| "browser".to_string());
    let tags = if fields.tags.is_empty() {
        vec!["browser".to_string()]
    } else {
        fields.tags.clone()
    };

    BrowserDetectedSecretPreview {
        title,
        provider_id: provider_guess,
        endpoint,
        interface_type,
        auth_scheme,
        masked_secret: mask_secret(fields.api_key.expose()),
        fingerprint: vault.fingerprint_secret(fields.api_key.expose()),
        environment,
        tags,
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
    if endpoint.contains("generativelanguage") || endpoint.contains("gemini") {
        Some(InterfaceType::Gemini)
    } else if endpoint.contains("anthropic") {
        Some(InterfaceType::AnthropicMessages)
    } else if endpoint.contains("replicate.com") {
        Some(InterfaceType::CustomHttp)
    } else if endpoint.contains("openai")
        || endpoint.contains("/v1")
        || endpoint.contains("gateway")
        || endpoint.contains("one-api")
        || endpoint.contains("new-api")
        || endpoint.contains("litellm")
        || endpoint.contains("sub2api")
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
    let entry = vault
        .get_provider_summary(request.id)
        .map_err(map_vault_error)?;
    let home = home_dir()?;
    let mut tool_entry = ToolEntry {
        id: entry.id,
        title: entry.title.clone(),
        provider_id: entry.provider_id.clone(),
        endpoint: endpoint_url(&entry.endpoints),
        interface_type: entry.interface_type.clone(),
        auth_scheme: entry.auth_scheme.clone(),
        env_key: env_key_for_entry(&entry),
        default_model: entry.default_model.clone(),
        api_key: None,
    };
    if matches!(request.mode, ToolConfigMode::Plaintext) {
        tool_entry.api_key = Some(vault.reveal_secret(entry.id).map_err(map_vault_error)?);
    }
    let (plan, content) = match (&request.tool, &request.mode) {
        (ToolConfigTool::Codex, ToolConfigMode::Helper) => {
            plan_codex(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (ToolConfigTool::Codex, ToolConfigMode::Env) => {
            plan_tool_env_helper(&home, ToolConfigTool::Codex, &tool_entry)?
        }
        (ToolConfigTool::Codex, ToolConfigMode::Plaintext) => {
            plan_codex_plaintext(&home, &tool_entry).map_err(ServiceError::internal)?
        }
        (ToolConfigTool::ClaudeCode, ToolConfigMode::Helper) => {
            plan_claude_code(&home, &tool_entry).map_err(ServiceError::internal)?
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
    };
    Ok((entry, plan, content))
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
    };
    let tool_name = match tool {
        ToolConfigTool::Codex => "codex",
        ToolConfigTool::ClaudeCode => "claude-code",
        ToolConfigTool::GeminiCli => "gemini-cli",
        ToolConfigTool::OpenCode => "opencode",
    };
    let target = home
        .join(".aipass")
        .join("tools")
        .join(format!("{tool_name}.env"));
    let operation_id = Uuid::new_v4();
    let backup_path = target
        .parent()
        .unwrap_or(home)
        .join(".aipass-backups")
        .join(format!(
            "{}-{}.aipbackup",
            operation_id,
            OffsetDateTime::now_utc().unix_timestamp()
        ));
    let mut content = format!(
        "# Generated by AIPass. This file stores helper references, not plaintext secrets.\n{}=\"$(aipass get {} --field api_key --reveal)\"\n",
        entry.env_key, entry.id
    );
    if let Some(endpoint) = &entry.endpoint {
        content.push_str(&format!("AIPASS_BASE_URL={}\n", shell_quote(endpoint)));
    }
    let plan = ConfigPlan {
        operation_id,
        tool: tool_id,
        target_path: target,
        backup_path,
        summary: format!("Configure {tool_name} env helper for {}", entry.title),
        preview: content.clone(),
        extra_writes: Vec::new(),
    };
    Ok((plan, content))
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

fn sync_webdav_report(vault_dir: &Path, client: &impl WebDavClient) -> SyncReport {
    match sync_webdav(vault_dir, client) {
        Ok(report) => report,
        Err(err) => SyncReport {
            uploaded: 0,
            downloaded: 0,
            conflicts: 0,
            quarantined: 0,
            status: classify_webdav_error(&err),
            message: Some(err.to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aipass_agent_protocol::SessionStatus;
    use aipass_crypto::SecretString;
    use std::io::Write;
    use std::time::Instant;
    use tempfile::tempdir;

    struct RunningAgent {
        vault_dir: PathBuf,
        client: crate::AgentClient,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl RunningAgent {
        fn start() -> Self {
            let dir = tempdir().expect("tempdir");
            aipass_vault::Vault::create(
                dir.path(),
                &SecretString::new("correct horse battery staple"),
            )
            .expect("create vault");
            let vault_dir = dir.keep();
            let server_vault_dir = vault_dir.clone();
            let handle = thread::spawn(move || {
                run_server(ServerOptions {
                    vault_dir: server_vault_dir,
                })
                .expect("server");
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
            let _ = fs::remove_dir_all(&self.vault_dir);
        }
    }

    #[test]
    fn constant_time_eq_checks_full_input() {
        assert!(constant_time_eq(b"same", b"same"));
        assert!(!constant_time_eq(b"same", b"some"));
        assert!(!constant_time_eq(b"same", b"same-longer"));
        assert!(!constant_time_eq(b"same-longer", b"same"));
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
}
