use aipass_agent_protocol::{
    CloudSyncProvider as AgentCloudSyncProvider, SensitiveString,
    SyncConflictActionRequest as AgentSyncConflictActionRequest,
    SyncConflictResponse as AgentSyncConflictResponse, SyncMode as AgentSyncMode,
    SyncSettings as AgentSyncSettings, SyncSettingsUpdate as AgentSyncSettingsUpdate,
    ToolConfigApplyResponse as AgentToolConfigApplyResponse, ToolConfigMode as AgentToolConfigMode,
    ToolConfigPreviewResponse as AgentToolConfigPreviewResponse,
    ToolConfigRequest as AgentToolConfigRequest, ToolConfigTool as AgentToolConfigTool,
};
use aipass_provider_registry::{AuthScheme, GatewayMetadata, InterfaceType, QuotaInfo};
use aipass_sync::SyncObject;
use aipass_vault::EntrySummary;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VaultStatus {
    pub(crate) exists: bool,
    pub(crate) locked: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppPreferences {
    pub(crate) auto_lock_minutes: u16,
    pub(crate) clipboard_clear_seconds: u16,
    pub(crate) lock_on_sleep: bool,
    pub(crate) lock_on_screen_lock: bool,
    #[serde(default)]
    pub(crate) theme: ThemePreference,
    #[serde(default)]
    pub(crate) locale: LocalePreference,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum LocalePreference {
    #[default]
    #[serde(rename = "system")]
    System,
    #[serde(rename = "en")]
    En,
    #[serde(rename = "zh-CN")]
    ZhCn,
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            auto_lock_minutes: 60,
            clipboard_clear_seconds: 45,
            lock_on_sleep: true,
            lock_on_screen_lock: true,
            theme: ThemePreference::System,
            locale: LocalePreference::System,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SyncMode {
    #[default]
    Local,
    ICloud,
    OneDrive,
    WebDav,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncSettings {
    #[serde(default)]
    pub(crate) mode: SyncMode,
    #[serde(default)]
    pub(crate) sync_folder: Option<PathBuf>,
    #[serde(default)]
    pub(crate) webdav_url: Option<String>,
    #[serde(default)]
    pub(crate) webdav_username: Option<String>,
    #[serde(default)]
    pub(crate) has_webdav_password: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SavePreferencesRequest {
    pub(crate) auto_lock_minutes: u16,
    pub(crate) clipboard_clear_seconds: u16,
    pub(crate) lock_on_sleep: Option<bool>,
    pub(crate) lock_on_screen_lock: Option<bool>,
    pub(crate) theme: Option<ThemePreference>,
    pub(crate) locale: Option<LocalePreference>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateVaultRequest {
    pub(crate) password: SensitiveString,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UnlockVaultRequest {
    pub(crate) password: SensitiveString,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecoveryVaultRequest {
    pub(crate) recovery_key: SensitiveString,
    pub(crate) new_password: SensitiveString,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChangePasswordRequest {
    pub(crate) new_password: SensitiveString,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderAddRequest {
    pub(crate) title: String,
    pub(crate) provider_id: Option<String>,
    #[serde(default)]
    pub(crate) domain: Vec<String>,
    pub(crate) endpoint: Option<String>,
    #[serde(default)]
    pub(crate) endpoints: Vec<String>,
    #[serde(default)]
    pub(crate) console_endpoints: Vec<String>,
    pub(crate) favicon_url: Option<String>,
    pub(crate) interface_type: InterfaceType,
    pub(crate) auth_scheme: AuthScheme,
    pub(crate) api_key: SensitiveString,
    pub(crate) default_model: Option<String>,
    #[serde(default)]
    pub(crate) model_aliases: Vec<(String, String)>,
    #[serde(default)]
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) quota: Option<QuotaInfo>,
    pub(crate) gateway: Option<GatewayMetadata>,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    pub(crate) notes: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderUpdateRequest {
    pub(crate) id: Uuid,
    pub(crate) title: String,
    pub(crate) provider_id: Option<String>,
    #[serde(default)]
    pub(crate) domain: Vec<String>,
    pub(crate) endpoint: Option<String>,
    #[serde(default)]
    pub(crate) endpoints: Vec<String>,
    #[serde(default)]
    pub(crate) console_endpoints: Vec<String>,
    pub(crate) favicon_url: Option<String>,
    pub(crate) interface_type: InterfaceType,
    pub(crate) auth_scheme: AuthScheme,
    pub(crate) api_key: Option<SensitiveString>,
    pub(crate) default_model: Option<String>,
    #[serde(default)]
    pub(crate) model_aliases: Vec<(String, String)>,
    pub(crate) headers: Option<Vec<(String, String)>>,
    pub(crate) quota: Option<QuotaInfo>,
    pub(crate) gateway: Option<GatewayMetadata>,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    pub(crate) notes: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProbeResult {
    pub(crate) ok: bool,
    pub(crate) provider_id: Option<String>,
    pub(crate) interface_type: InterfaceType,
    pub(crate) status: Option<u16>,
    pub(crate) endpoint: Option<String>,
    pub(crate) model_count: Option<usize>,
    pub(crate) error: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VaultExportRequest {
    pub(crate) output: PathBuf,
    pub(crate) export_password: SensitiveString,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VaultImportRequest {
    pub(crate) input: PathBuf,
    pub(crate) export_password: SensitiveString,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncLocalRequest {
    pub(crate) dir: PathBuf,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum CloudSyncProvider {
    #[serde(rename = "icloud")]
    ICloud,
    #[serde(rename = "onedrive")]
    OneDrive,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncCloudRequest {
    pub(crate) provider: CloudSyncProvider,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SaveSyncSettingsRequest {
    pub(crate) mode: SyncMode,
    pub(crate) sync_folder: Option<PathBuf>,
    pub(crate) webdav_url: Option<String>,
    pub(crate) webdav_username: Option<String>,
    pub(crate) webdav_password: Option<SensitiveString>,
    #[serde(default)]
    pub(crate) clear_webdav_password: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncWebDavRequest {
    pub(crate) url: String,
    pub(crate) username: Option<String>,
    pub(crate) password: Option<SensitiveString>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncConflictsRequest {
    pub(crate) dir: Option<PathBuf>,
    #[serde(default)]
    pub(crate) provider: Option<CloudSyncProvider>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConflictScope {
    Vault,
    Sync,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncConflictActionRequest {
    pub(crate) scope: ConflictScope,
    pub(crate) dir: Option<PathBuf>,
    #[serde(default)]
    pub(crate) provider: Option<CloudSyncProvider>,
    pub(crate) conflict_path: PathBuf,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncConflictResponse {
    pub(crate) scope: ConflictScope,
    pub(crate) origin: String,
    pub(crate) conflict_path: PathBuf,
    pub(crate) target_path: PathBuf,
    pub(crate) object: SyncObject,
    pub(crate) conflict_summary: Option<EntrySummary>,
    pub(crate) target_summary: Option<EntrySummary>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ToolConfigTool {
    Codex,
    ClaudeCode,
    GeminiCli,
    OpenCode,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ToolConfigMode {
    Helper,
    Env,
    Plaintext,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolConfigRequest {
    pub(crate) tool: ToolConfigTool,
    pub(crate) id: Uuid,
    pub(crate) mode: ToolConfigMode,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolConfigPreviewResponse {
    pub(crate) tool: ToolConfigTool,
    pub(crate) mode: ToolConfigMode,
    pub(crate) entry_id: Uuid,
    pub(crate) entry_title: String,
    pub(crate) target_path: String,
    pub(crate) summary: String,
    pub(crate) preview: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolConfigApplyResponse {
    pub(crate) tool: ToolConfigTool,
    pub(crate) mode: ToolConfigMode,
    pub(crate) entry_id: Uuid,
    pub(crate) entry_title: String,
    pub(crate) operation_id: Uuid,
    pub(crate) target_path: String,
    pub(crate) backup_path: String,
    pub(crate) summary: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeHostRepairRequest {
    #[serde(default)]
    pub(crate) extension_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeHostStatus {
    pub(crate) browser: String,
    pub(crate) browser_label: String,
    pub(crate) host_path: PathBuf,
    pub(crate) host_exists: bool,
    pub(crate) host_usable: bool,
    pub(crate) host_error: Option<String>,
    pub(crate) manifest_path: PathBuf,
    pub(crate) manifest_exists: bool,
    pub(crate) settings_path: PathBuf,
    pub(crate) allowed_extension_ids: Vec<String>,
    pub(crate) allowed_origins: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserExtensionStatus {
    pub(crate) browser: String,
    pub(crate) detected_browsers: Vec<String>,
    pub(crate) chrome_installed: bool,
    pub(crate) chrome_path: Option<PathBuf>,
    pub(crate) extension_id: String,
    pub(crate) discovered_extension_ids: Vec<String>,
    pub(crate) extension_version: String,
    pub(crate) crx_path: PathBuf,
    pub(crate) crx_exists: bool,
    pub(crate) extension_installed: bool,
    pub(crate) installed_paths: Vec<PathBuf>,
    pub(crate) external_install_path: Option<PathBuf>,
    pub(crate) external_install_exists: bool,
    pub(crate) native_host_configured: bool,
    pub(crate) install_mode: BrowserExtensionInstallMode,
    pub(crate) native_host: NativeHostStatus,
    pub(crate) native_hosts: Vec<NativeHostStatus>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum BrowserExtensionInstallMode {
    ExternalCrx,
    ManualCrx,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserExtensionInstallResult {
    pub(crate) status: BrowserExtensionStatus,
    pub(crate) opened_chrome: bool,
    pub(crate) opened_package: bool,
}

pub(crate) fn into_agent_tool_config_request(request: ToolConfigRequest) -> AgentToolConfigRequest {
    AgentToolConfigRequest {
        tool: match request.tool {
            ToolConfigTool::Codex => AgentToolConfigTool::Codex,
            ToolConfigTool::ClaudeCode => AgentToolConfigTool::ClaudeCode,
            ToolConfigTool::GeminiCli => AgentToolConfigTool::GeminiCli,
            ToolConfigTool::OpenCode => AgentToolConfigTool::OpenCode,
        },
        id: request.id,
        mode: match request.mode {
            ToolConfigMode::Helper => AgentToolConfigMode::Helper,
            ToolConfigMode::Env => AgentToolConfigMode::Env,
            ToolConfigMode::Plaintext => AgentToolConfigMode::Plaintext,
        },
    }
}

pub(crate) fn from_agent_tool_config_preview(
    response: AgentToolConfigPreviewResponse,
) -> ToolConfigPreviewResponse {
    ToolConfigPreviewResponse {
        tool: from_agent_tool(response.tool),
        mode: from_agent_tool_mode(response.mode),
        entry_id: response.entry_id,
        entry_title: response.entry_title,
        target_path: response.target_path,
        summary: response.summary,
        preview: response.preview,
    }
}

pub(crate) fn from_agent_tool_config_apply(
    response: AgentToolConfigApplyResponse,
) -> ToolConfigApplyResponse {
    ToolConfigApplyResponse {
        tool: from_agent_tool(response.tool),
        mode: from_agent_tool_mode(response.mode),
        entry_id: response.entry_id,
        entry_title: response.entry_title,
        operation_id: response.operation_id,
        target_path: response.target_path,
        backup_path: response.backup_path,
        summary: response.summary,
    }
}

pub(crate) fn from_agent_sync_conflict_response(
    response: AgentSyncConflictResponse,
) -> SyncConflictResponse {
    SyncConflictResponse {
        scope: match response.scope {
            aipass_agent_protocol::ConflictScope::Vault => ConflictScope::Vault,
            aipass_agent_protocol::ConflictScope::Sync => ConflictScope::Sync,
        },
        origin: response.origin,
        conflict_path: response.conflict_path,
        target_path: response.target_path,
        object: response.object,
        conflict_summary: response.conflict_summary,
        target_summary: response.target_summary,
    }
}

pub(crate) fn into_agent_sync_conflict_request(
    request: SyncConflictActionRequest,
) -> AgentSyncConflictActionRequest {
    AgentSyncConflictActionRequest {
        scope: match request.scope {
            ConflictScope::Vault => aipass_agent_protocol::ConflictScope::Vault,
            ConflictScope::Sync => aipass_agent_protocol::ConflictScope::Sync,
        },
        dir: request.dir,
        provider: request.provider.map(into_agent_cloud_sync_provider),
        conflict_path: request.conflict_path,
    }
}

pub(crate) fn into_agent_cloud_sync_provider(
    provider: CloudSyncProvider,
) -> AgentCloudSyncProvider {
    match provider {
        CloudSyncProvider::ICloud => AgentCloudSyncProvider::ICloud,
        CloudSyncProvider::OneDrive => AgentCloudSyncProvider::OneDrive,
    }
}

pub(crate) fn into_agent_sync_settings_update(
    request: SaveSyncSettingsRequest,
) -> AgentSyncSettingsUpdate {
    AgentSyncSettingsUpdate {
        mode: into_agent_sync_mode(request.mode),
        sync_folder: request.sync_folder,
        webdav_url: request.webdav_url,
        webdav_username: request.webdav_username,
        webdav_password: request.webdav_password,
        clear_webdav_password: request.clear_webdav_password,
    }
}

pub(crate) fn from_agent_sync_settings(settings: AgentSyncSettings) -> SyncSettings {
    SyncSettings {
        mode: from_agent_sync_mode(settings.mode),
        sync_folder: settings.sync_folder,
        webdav_url: settings.webdav_url,
        webdav_username: settings.webdav_username,
        has_webdav_password: settings.has_webdav_password,
    }
}

fn from_agent_tool(tool: AgentToolConfigTool) -> ToolConfigTool {
    match tool {
        AgentToolConfigTool::Codex => ToolConfigTool::Codex,
        AgentToolConfigTool::ClaudeCode => ToolConfigTool::ClaudeCode,
        AgentToolConfigTool::GeminiCli => ToolConfigTool::GeminiCli,
        AgentToolConfigTool::OpenCode => ToolConfigTool::OpenCode,
    }
}

fn into_agent_sync_mode(mode: SyncMode) -> AgentSyncMode {
    match mode {
        SyncMode::Local => AgentSyncMode::Local,
        SyncMode::ICloud => AgentSyncMode::ICloud,
        SyncMode::OneDrive => AgentSyncMode::OneDrive,
        SyncMode::WebDav => AgentSyncMode::WebDav,
    }
}

fn from_agent_sync_mode(mode: AgentSyncMode) -> SyncMode {
    match mode {
        AgentSyncMode::Local => SyncMode::Local,
        AgentSyncMode::ICloud => SyncMode::ICloud,
        AgentSyncMode::OneDrive => SyncMode::OneDrive,
        AgentSyncMode::WebDav => SyncMode::WebDav,
    }
}

fn from_agent_tool_mode(mode: AgentToolConfigMode) -> ToolConfigMode {
    match mode {
        AgentToolConfigMode::Helper => ToolConfigMode::Helper,
        AgentToolConfigMode::Env => ToolConfigMode::Env,
        AgentToolConfigMode::Plaintext => ToolConfigMode::Plaintext,
    }
}
