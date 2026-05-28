use aipass_provider_registry::{AuthScheme, InterfaceType, ProviderEndpoint};
use aipass_sync::SyncObject;
use aipass_vault::{
    EncryptedVaultExport, EntrySummary, ProviderEntryInput, ProviderEntryUpdateInput, RecoveryKit,
    TtlGrantSummary,
};
use anyhow::{bail, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::PathBuf;
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
#[serde(transparent)]
pub struct SensitiveString(String);

impl SensitiveString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn into_inner(mut self) -> String {
        std::mem::take(&mut self.0)
    }
}

impl std::fmt::Debug for SensitiveString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[redacted]")
    }
}

impl From<String> for SensitiveString {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for SensitiveString {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentErrorCode {
    Locked,
    InvalidPassword,
    ServiceUnavailable,
    GrantExpired,
    PermissionDenied,
    NotFound,
    Conflict,
    ValidationFailed,
    Internal,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LockReason {
    IdleTimeout,
    Manual,
    AgentRestart,
    SystemSleep,
    ScreenLock,
    Import,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionPolicy {
    pub idle_lock_minutes: u16,
    pub lock_on_sleep: bool,
    pub lock_on_screen_lock: bool,
}

impl Default for SessionPolicy {
    fn default() -> Self {
        Self {
            idle_lock_minutes: 15,
            lock_on_sleep: true,
            lock_on_screen_lock: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatus {
    pub exists: bool,
    pub locked: bool,
    pub policy: SessionPolicy,
    #[serde(default)]
    pub last_lock_reason: Option<LockReason>,
    #[serde(default)]
    pub vault_namespace: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub running: bool,
    pub session: SessionStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConflictScope {
    Vault,
    Sync,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum CloudSyncProvider {
    #[serde(rename = "icloud")]
    ICloud,
    #[serde(rename = "onedrive")]
    OneDrive,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SyncMode {
    #[default]
    Local,
    ICloud,
    OneDrive,
    WebDav,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncSettings {
    #[serde(default)]
    pub mode: SyncMode,
    #[serde(default)]
    pub sync_folder: Option<PathBuf>,
    #[serde(default)]
    pub webdav_url: Option<String>,
    #[serde(default)]
    pub webdav_username: Option<String>,
    #[serde(default)]
    pub has_webdav_password: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SyncSettingsUpdate {
    pub mode: SyncMode,
    pub sync_folder: Option<PathBuf>,
    pub webdav_url: Option<String>,
    pub webdav_username: Option<String>,
    pub webdav_password: Option<SensitiveString>,
    #[serde(default)]
    pub clear_webdav_password: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SyncConflictActionRequest {
    pub scope: ConflictScope,
    pub dir: Option<PathBuf>,
    #[serde(default)]
    pub provider: Option<CloudSyncProvider>,
    pub conflict_path: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncConflictResponse {
    pub scope: ConflictScope,
    pub origin: String,
    pub conflict_path: PathBuf,
    pub target_path: PathBuf,
    pub object: SyncObject,
    pub conflict_summary: Option<EntrySummary>,
    pub target_summary: Option<EntrySummary>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ToolConfigTool {
    Codex,
    ClaudeCode,
    GeminiCli,
    OpenCode,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolConfigMode {
    Helper,
    Env,
    Plaintext,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolConfigRequest {
    pub tool: ToolConfigTool,
    pub id: Uuid,
    pub mode: ToolConfigMode,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolConfigPreviewResponse {
    pub tool: ToolConfigTool,
    pub mode: ToolConfigMode,
    pub entry_id: Uuid,
    pub entry_title: String,
    pub target_path: String,
    pub summary: String,
    pub preview: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolConfigApplyResponse {
    pub tool: ToolConfigTool,
    pub mode: ToolConfigMode,
    pub entry_id: Uuid,
    pub entry_title: String,
    pub operation_id: Uuid,
    pub target_path: String,
    pub backup_path: String,
    pub summary: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub ok: bool,
    pub provider_id: Option<String>,
    pub interface_type: InterfaceType,
    pub status: Option<u16>,
    pub endpoint: Option<String>,
    pub model_count: Option<usize>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserDetectedSecretFields {
    pub origin: String,
    pub url: String,
    pub title: Option<String>,
    pub endpoint: Option<String>,
    pub provider_id: Option<String>,
    pub interface_type: Option<InterfaceType>,
    pub auth_scheme: Option<AuthScheme>,
    pub api_key: SensitiveString,
    pub environment: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserDetectedSecretPreview {
    pub title: String,
    pub provider_id: Option<String>,
    pub endpoint: Option<String>,
    pub interface_type: InterfaceType,
    pub auth_scheme: AuthScheme,
    pub masked_secret: String,
    pub fingerprint: String,
    pub environment: String,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserContextLookupData {
    pub entries: Vec<EntrySummary>,
    pub grants: Vec<TtlGrantSummary>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserFillResult {
    pub entry_id: Uuid,
    pub field: String,
    pub secret: SensitiveString,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionUnlockMode {
    Password { password: SensitiveString },
    NativeWindow,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthenticatedAgentRequest {
    pub auth_token: SensitiveString,
    pub request: AgentRequest,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum AgentRequest {
    #[serde(rename = "session.status")]
    SessionStatus,
    #[serde(rename = "session.unlock")]
    SessionUnlock { mode: SessionUnlockMode },
    #[serde(rename = "session.lock")]
    SessionLock { reason: LockReason },
    #[serde(rename = "session.touch")]
    SessionTouch,
    #[serde(rename = "session.policy.get")]
    SessionPolicyGet,
    #[serde(rename = "session.policy.set")]
    SessionPolicySet { policy: SessionPolicy },
    #[serde(rename = "vault.status")]
    VaultStatus,
    #[serde(rename = "vault.create")]
    VaultCreate { password: SensitiveString },
    #[serde(rename = "vault.recover")]
    VaultRecover {
        recovery_key: SensitiveString,
        new_password: SensitiveString,
    },
    #[serde(rename = "vault.change_password")]
    VaultChangePassword { new_password: SensitiveString },
    #[serde(rename = "vault.rotate")]
    VaultRotate { reason: String },
    #[serde(rename = "vault.export")]
    VaultExport {
        output: PathBuf,
        export_password: SensitiveString,
    },
    #[serde(rename = "vault.import")]
    VaultImport {
        input: PathBuf,
        export_password: SensitiveString,
    },
    #[serde(rename = "entries.list")]
    EntriesList { archived: bool },
    #[serde(rename = "entries.search")]
    EntriesSearch { query: String },
    #[serde(rename = "provider.get")]
    ProviderGet { id: Uuid },
    #[serde(rename = "provider.add")]
    ProviderAdd { input: ProviderEntryInput },
    #[serde(rename = "provider.update")]
    ProviderUpdate {
        id: Uuid,
        input: ProviderEntryUpdateInput,
    },
    #[serde(rename = "provider.archive")]
    ProviderArchive { id: Uuid },
    #[serde(rename = "provider.restore")]
    ProviderRestore { id: Uuid },
    #[serde(rename = "provider.delete")]
    ProviderDelete { id: Uuid },
    #[serde(rename = "secret.reveal_field")]
    SecretRevealField { id: Uuid, field: String },
    #[serde(rename = "secret.add")]
    SecretAdd {
        id: Uuid,
        label: String,
        secret: SensitiveString,
    },
    #[serde(rename = "secret.remove")]
    SecretRemove { id: Uuid, label: String },
    #[serde(rename = "devices.list")]
    DevicesList,
    #[serde(rename = "device.revoke")]
    DeviceRevoke { id: Uuid },
    #[serde(rename = "provider.probe")]
    ProviderProbe { id: Uuid, timeout_seconds: u64 },
    #[serde(rename = "tool_config.preview")]
    ToolConfigPreview { request: ToolConfigRequest },
    #[serde(rename = "tool_config.apply")]
    ToolConfigApply { request: ToolConfigRequest },
    #[serde(rename = "tool_config.rollback")]
    ToolConfigRollback { operation_id: Uuid },
    #[serde(rename = "sync.local")]
    SyncLocal { dir: PathBuf },
    #[serde(rename = "sync.settings.get")]
    SyncSettingsGet,
    #[serde(rename = "sync.settings.set")]
    SyncSettingsSet { settings: SyncSettingsUpdate },
    #[serde(rename = "sync.configured")]
    SyncConfigured,
    #[serde(rename = "sync.cloud")]
    SyncCloud { provider: CloudSyncProvider },
    #[serde(rename = "sync.webdav")]
    SyncWebDav {
        url: String,
        username: Option<String>,
        password: Option<SensitiveString>,
    },
    #[serde(rename = "sync.conflicts")]
    SyncConflicts {
        dir: Option<PathBuf>,
        #[serde(default)]
        provider: Option<CloudSyncProvider>,
    },
    #[serde(rename = "sync.accept_conflict")]
    SyncAcceptConflict { request: SyncConflictActionRequest },
    #[serde(rename = "sync.discard_conflict")]
    SyncDiscardConflict { request: SyncConflictActionRequest },
    #[serde(rename = "browser.context_lookup")]
    BrowserContextLookup { origin: String, url: String },
    #[serde(rename = "browser.entries_search")]
    BrowserEntriesSearch { origin: String, query: String },
    #[serde(rename = "browser.secret_fill")]
    BrowserSecretFill {
        entry_id: Option<Uuid>,
        grant_id: Uuid,
    },
    #[serde(rename = "browser.preview_detected")]
    BrowserPreviewDetected { fields: BrowserDetectedSecretFields },
    #[serde(rename = "browser.save_detected")]
    BrowserSaveDetected { fields: BrowserDetectedSecretFields },
    #[serde(rename = "browser.ignore_origin")]
    BrowserIgnoreOrigin { origin: String },
    #[serde(rename = "browser.is_origin_ignored")]
    BrowserIsOriginIgnored { origin: String },
    #[serde(rename = "ui.open_main")]
    UiOpenMain,
    #[serde(rename = "ui.open_unlock")]
    UiOpenUnlock,
    #[serde(rename = "ui.open_quick_access")]
    UiOpenQuickAccess,
    #[serde(rename = "agent.shutdown")]
    AgentShutdown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentResponse {
    pub ok: bool,
    #[serde(default)]
    pub code: Option<AgentErrorCode>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub data: serde_json::Value,
}

impl AgentResponse {
    pub fn success<T: Serialize>(data: T) -> Self {
        Self {
            ok: true,
            code: None,
            message: None,
            data: serde_json::to_value(data).unwrap_or_else(|_| serde_json::json!({})),
        }
    }

    pub fn empty() -> Self {
        Self::success(serde_json::json!({}))
    }

    pub fn error(code: AgentErrorCode, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            code: Some(code),
            message: Some(message.into()),
            data: serde_json::json!({}),
        }
    }

    pub fn into_result<T: DeserializeOwned>(self) -> Result<T> {
        if !self.ok {
            bail!(
                "{}:{}",
                self.code
                    .as_ref()
                    .map(error_code_name)
                    .unwrap_or("internal"),
                self.message
                    .unwrap_or_else(|| "agent request failed".to_string())
            );
        }
        Ok(serde_json::from_value(self.data)?)
    }
}

pub fn read_frame<T: DeserializeOwned>(mut reader: impl Read) -> Result<T> {
    let mut len = [0_u8; 4];
    reader.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME_BYTES {
        bail!("frame too large");
    }
    let mut body = vec![0_u8; len];
    reader.read_exact(&mut body)?;
    let parsed = serde_json::from_slice(&body);
    body.zeroize();
    Ok(parsed?)
}

pub fn write_frame<T: Serialize>(mut writer: impl Write, value: &T) -> Result<()> {
    let mut body = serde_json::to_vec(value)?;
    if body.len() > MAX_FRAME_BYTES {
        body.zeroize();
        bail!("frame too large");
    }
    let result = (|| {
        writer.write_all(&(body.len() as u32).to_le_bytes())?;
        writer.write_all(&body)?;
        Ok(())
    })();
    body.zeroize();
    result
}

pub fn error_code_name(code: &AgentErrorCode) -> &'static str {
    match code {
        AgentErrorCode::Locked => "locked",
        AgentErrorCode::InvalidPassword => "invalid_password",
        AgentErrorCode::ServiceUnavailable => "service_unavailable",
        AgentErrorCode::GrantExpired => "grant_expired",
        AgentErrorCode::PermissionDenied => "permission_denied",
        AgentErrorCode::NotFound => "not_found",
        AgentErrorCode::Conflict => "conflict",
        AgentErrorCode::ValidationFailed => "validation_failed",
        AgentErrorCode::Internal => "internal",
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveDetectedResult {
    pub entry_id: Uuid,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserIgnoreOriginResult {
    pub ignored_origins: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserIgnoredStatus {
    pub ignored: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretValue {
    pub secret: SensitiveString,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultCreateResponse {
    pub recovery_kit: RecoveryKit,
    pub session: SessionStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultImportResponse {
    pub imported: bool,
    pub export: EncryptedVaultExport,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryCopyPayload {
    pub entry: EntrySummary,
    pub secret: SensitiveString,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickAccessItem {
    pub entry: EntrySummary,
    pub api_endpoint: Option<String>,
    pub console_endpoint: Option<String>,
}

pub fn endpoint_url(endpoints: &[ProviderEndpoint]) -> Option<String> {
    endpoints
        .iter()
        .find(|endpoint| endpoint.kind == aipass_provider_registry::EndpointKind::Api)
        .and_then(|endpoint| endpoint.url.clone())
        .or_else(|| endpoints.iter().find_map(|endpoint| endpoint.url.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_frame_rejects_oversized_lengths_before_allocating_body() {
        let bytes = ((MAX_FRAME_BYTES + 1) as u32).to_le_bytes();
        let err = read_frame::<serde_json::Value>(bytes.as_slice()).unwrap_err();
        assert_eq!(err.to_string(), "frame too large");
    }

    #[test]
    fn write_frame_rejects_oversized_payloads() {
        let payload = "x".repeat(MAX_FRAME_BYTES);
        let err = write_frame(Vec::new(), &payload).unwrap_err();
        assert_eq!(err.to_string(), "frame too large");
    }
}
