pub use aipass_config_writers::ToolId;
use aipass_provider_registry::{
    AuthScheme, BillingRule, CredentialKind, GatewayMetadata, InterfaceType, OAuthProvider,
    ProviderEndpoint, QuotaInfo, SubscriptionSnapshot,
};
pub use aipass_proxy::{
    ModelPricing, ModelUsageAggregate, Protocol as ProxyProtocol, ProviderUsageAggregate,
    ProxyConfig, ProxyLogEntry, ProxyRouteConfig, ProxyStatus, ProxyTargetConfig, RetryPolicy,
    RouteStrategy, UsageAggregate, UsageGranularity, UsageTimeseriesModel, UsageTimeseriesPoint,
};
use aipass_sync::SyncObject;
use aipass_vault::{
    EncryptedVaultExport, EntrySummary, ProviderEntryInput, ProviderEntryUpdateInput, RecoveryKit,
    SecretMetadataInput, TtlGrantSummary,
};
use anyhow::{bail, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::PathBuf;
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
pub const AGENT_PROTOCOL_VERSION: u32 = 2;

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
    AppQuit,
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
            idle_lock_minutes: 60,
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
    /// True while the agent is still running its first sync after launch.
    /// Clients waiting for readiness must keep polling until this clears.
    #[serde(default)]
    pub initial_sync_pending: bool,
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
    Grok,
    Pi,
    Cursor,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolConfigMode {
    /// Use the provider's native official OAuth/subscription credentials.
    Official,
    Helper,
    Env,
    Plaintext,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CodexApiKeyMode {
    ExperimentalBearerToken,
    AuthJson,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolConfigRequest {
    pub tool: ToolConfigTool,
    pub id: Uuid,
    pub mode: ToolConfigMode,
    #[serde(default)]
    pub codex_api_key_mode: Option<CodexApiKeyMode>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolConfigProxyRequest {
    pub tool: ToolId,
    pub route_id: Uuid,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolConfigPreviewFile {
    pub path: String,
    pub content: String,
    /// Line diff between the current file and the planned content.
    #[serde(default)]
    pub diff: String,
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
    #[serde(default)]
    pub files: Vec<ToolConfigPreviewFile>,
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

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum UsageProbeMode {
    #[default]
    Auto,
    NewApi,
    SubApi,
    NewApiAdvanced,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UsageProbeSource {
    NewApiTokenUsage,
    NewApiUserSelf,
    SubApiV1Usage,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageProbeQuota {
    pub label: Option<String>,
    pub limit: Option<String>,
    pub used: Option<String>,
    pub remaining: Option<String>,
    pub reset_at: Option<String>,
    pub unit: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageProbeResult {
    pub ok: bool,
    pub provider_id: Option<String>,
    pub source: UsageProbeSource,
    pub endpoint: Option<String>,
    pub status: Option<u16>,
    pub quota: Option<UsageProbeQuota>,
    pub gateway: Option<GatewayMetadata>,
    pub plan_name: Option<String>,
    pub message: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FaviconBackfillRequest {
    #[serde(default)]
    pub entry_ids: Option<Vec<Uuid>>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FaviconBackfillError {
    #[serde(default)]
    pub entry_id: Option<Uuid>,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FaviconBackfillResponse {
    pub checked: usize,
    pub updated: usize,
    pub skipped: usize,
    #[serde(default)]
    pub entries: Vec<EntrySummary>,
    #[serde(default)]
    pub errors: Vec<FaviconBackfillError>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserDetectedSecretFields {
    pub origin: String,
    pub url: String,
    pub title: Option<String>,
    pub favicon_url: Option<String>,
    #[serde(default)]
    pub secret_label: Option<String>,
    pub endpoint: Option<String>,
    pub provider_id: Option<String>,
    pub interface_type: Option<InterfaceType>,
    pub auth_scheme: Option<AuthScheme>,
    pub api_key: SensitiveString,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub gateway: Option<GatewayMetadata>,
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default)]
    pub console_endpoint: Option<String>,
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub model_aliases: Vec<(String, String)>,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub notes: Option<String>,
    /// Gateway group this key belongs to. Independent of the entry: two groups
    /// on the same relay become two keys under one entry.
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub billing: Option<BillingRule>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserDetectedSecretPreview {
    pub title: String,
    #[serde(default)]
    pub secret_label: Option<String>,
    #[serde(default)]
    pub favicon_url: Option<String>,
    pub provider_id: Option<String>,
    pub endpoint: Option<String>,
    pub interface_type: InterfaceType,
    pub auth_scheme: AuthScheme,
    pub masked_secret: String,
    pub fingerprint: String,
    /// Entry this key will land in — either because the key is already stored,
    /// or because an entry for the same site exists and will gain a key.
    #[serde(default)]
    pub existing_entry_id: Option<Uuid>,
    /// Title of `existing_entry_id`, so the popup can say where it will go.
    #[serde(default)]
    pub existing_entry_title: Option<String>,
    /// Set when this exact key is already stored on that entry.
    #[serde(default)]
    pub existing_secret_id: Option<String>,
    /// Groups already stored on `existing_entry_id`, for duplicate-group hints.
    #[serde(default)]
    pub existing_groups: Vec<String>,
    #[serde(default)]
    pub is_saved: bool,
    pub tags: Vec<String>,
    #[serde(default)]
    pub gateway: Option<GatewayMetadata>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub billing: Option<BillingRule>,
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
    NativeWindowWait { timeout_ms: u64 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthenticatedAgentRequest {
    #[serde(default = "agent_protocol_version")]
    pub protocol_version: u32,
    pub auth_token: SensitiveString,
    /// Correlation only; never used for authorization or deduplication.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<Uuid>,
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
    #[serde(rename = "server.status")]
    ServerStatus,
    #[serde(rename = "server.logs")]
    ServerLogs,
    #[serde(rename = "server.start")]
    ServerStart,
    #[serde(rename = "server.stop")]
    ServerStop,
    #[serde(rename = "server.route.select")]
    ServerRouteSelect { route_id: Uuid },
    #[serde(rename = "server.route.set_enabled")]
    ServerRouteSetEnabled { route_id: Uuid, enabled: bool },
    #[serde(rename = "server.config.get")]
    ServerConfigGet,
    #[serde(rename = "server.config.set")]
    ServerConfigSet { config: ProxyConfig },
    #[serde(rename = "server.token.rotate")]
    ServerTokenRotate { route_id: Uuid },
    #[serde(rename = "server.usage.summary")]
    ServerUsageSummary {
        #[serde(default)]
        days: Option<u32>,
        #[serde(default)]
        timezone_offset_minutes: i32,
        #[serde(default)]
        granularity: UsageGranularity,
    },
    #[serde(rename = "server.usage.clear")]
    ServerUsageClear,
    #[serde(rename = "server.usage_timeseries")]
    ServerUsageTimeseries {
        days: u32,
        #[serde(default)]
        timezone_offset_minutes: i32,
        #[serde(default)]
        granularity: UsageGranularity,
    },
    #[serde(rename = "server.pricing_config.get")]
    ServerPricingConfigGet,
    #[serde(rename = "server.pricing_remote_sync")]
    ServerPricingRemoteSync { id: Uuid, timeout_seconds: u64 },
    #[serde(rename = "server.pricing_assignment.set")]
    ServerPricingAssignmentSet {
        entry_id: Uuid,
        secret_id: String,
        group_id: Option<Uuid>,
        multiplier: f64,
    },
    #[serde(rename = "server.pricing_group.upsert")]
    ServerPricingGroupUpsert {
        group: PricingGroup,
        apply_scope: PricingApplyScope,
    },
    #[serde(rename = "server.pricing_group.delete")]
    ServerPricingGroupDelete { group_id: Uuid },
    #[serde(rename = "server.pricing_group_version.delete")]
    ServerPricingGroupVersionDelete { group_id: Uuid, effective_from: i64 },
    #[serde(rename = "vault.status")]
    VaultStatus,
    #[serde(rename = "vault.create")]
    VaultCreate { password: SensitiveString },
    #[serde(rename = "vault.recover")]
    VaultRecover {
        recovery_key: SensitiveString,
        new_password: SensitiveString,
    },
    #[serde(rename = "vault.reset")]
    VaultReset,
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
    #[serde(rename = "entries.trash")]
    EntriesTrash,
    #[serde(rename = "entries.favorites")]
    EntriesFavorites,
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
    #[serde(rename = "provider.trash")]
    ProviderTrash { id: Uuid },
    #[serde(rename = "provider.favorite")]
    ProviderFavorite { id: Uuid, favorite: bool },
    #[serde(rename = "provider.delete")]
    ProviderDelete { id: Uuid },
    #[serde(rename = "trash.purge_expired")]
    TrashPurgeExpired,
    #[serde(rename = "trash.empty")]
    TrashEmpty,
    #[serde(rename = "secret.reveal_field")]
    SecretRevealField { id: Uuid, field: String },
    #[serde(rename = "secret.reveal_headers")]
    SecretRevealHeaders { id: Uuid },
    #[serde(rename = "secret.add")]
    SecretAdd {
        id: Uuid,
        label: String,
        secret: SensitiveString,
    },
    #[serde(rename = "secret.update")]
    SecretUpdate {
        id: Uuid,
        secret_id: String,
        label: String,
        secret: Option<SensitiveString>,
    },
    #[serde(rename = "secret.remove")]
    SecretRemove { id: Uuid, label: String },
    /// Set a key's gateway group, wire format and billing rule. Unset fields
    /// keep their stored value.
    #[serde(rename = "secret.metadata_set")]
    SecretMetadataSet {
        id: Uuid,
        secret_id: String,
        metadata: SecretMetadataInput,
    },
    #[serde(rename = "devices.list")]
    DevicesList,
    #[serde(rename = "device.revoke")]
    DeviceRevoke { id: Uuid },
    #[serde(rename = "provider.probe")]
    ProviderProbe { id: Uuid, timeout_seconds: u64 },
    #[serde(rename = "provider.usage_probe")]
    ProviderUsageProbe {
        id: Uuid,
        #[serde(default)]
        mode: UsageProbeMode,
        timeout_seconds: u64,
        #[serde(default)]
        base_url: Option<String>,
        #[serde(default)]
        access_token: Option<SensitiveString>,
        #[serde(default)]
        user_id: Option<String>,
    },
    #[serde(rename = "provider.usage_apply")]
    ProviderUsageApply {
        id: Uuid,
        quota: Option<QuotaInfo>,
        gateway: Option<GatewayMetadata>,
    },
    /// Discover locally authenticated official accounts and refresh their
    /// provider-owned subscription snapshots. No credential values are returned.
    #[serde(rename = "official_accounts.refresh")]
    OfficialAccountsRefresh {
        #[serde(default)]
        provider_ids: Vec<String>,
    },
    /// Detect whether CC Switch is installed and its config file exists.
    #[serde(rename = "ccswitch.detect")]
    CcSwitchDetect,
    /// Import providers from CC Switch's config into the vault.
    #[serde(rename = "ccswitch.import")]
    CcSwitchImport,
    /// Start an in-app OAuth device-code login for an official provider.
    #[serde(rename = "oauth.login.start")]
    OAuthLoginStart { provider: OAuthProvider },
    /// Poll an in-flight device-code login; returns pending/authorized/expired.
    #[serde(rename = "oauth.login.poll")]
    OAuthLoginPoll {
        provider: OAuthProvider,
        device_code: String,
    },
    /// Abandon an in-flight device-code login.
    #[serde(rename = "oauth.login.cancel")]
    OAuthLoginCancel {
        provider: OAuthProvider,
        device_code: String,
    },
    /// List managed OAuth accounts (token-free summaries). No provider filter
    /// means every supported provider.
    #[serde(rename = "oauth.accounts.list")]
    OAuthAccountsList {
        #[serde(default)]
        provider: Option<OAuthProvider>,
    },
    /// Remove a managed OAuth account and its linked provider entry secret.
    #[serde(rename = "oauth.accounts.remove")]
    OAuthAccountsRemove {
        provider: OAuthProvider,
        account_id: Uuid,
    },
    /// Choose which managed OAuth account is the default for a provider.
    #[serde(rename = "oauth.accounts.set_default")]
    OAuthAccountsSetDefault {
        provider: OAuthProvider,
        account_id: Uuid,
    },
    #[serde(rename = "provider.favicon_backfill")]
    ProviderFaviconBackfill { request: FaviconBackfillRequest },
    #[serde(rename = "tool_config.preview")]
    ToolConfigPreview { request: ToolConfigRequest },
    #[serde(rename = "tool_config.apply")]
    ToolConfigApply { request: ToolConfigRequest },
    #[serde(rename = "tool_config.proxy_preview")]
    ToolConfigProxyPreview { request: ToolConfigProxyRequest },
    #[serde(rename = "tool_config.proxy_apply")]
    ToolConfigProxyApply { request: ToolConfigProxyRequest },
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
pub struct OfficialAccountRefreshResult {
    pub provider_id: String,
    pub account_identity: Option<String>,
    pub credential_kind: CredentialKind,
    pub snapshot: Option<SubscriptionSnapshot>,
    /// One of "imported", "refreshed", "skipped", or "error".
    pub status: String,
    pub error: Option<String>,
}

/// Device-code challenge handed to the desktop so the user can authorize in a
/// browser. Contains no secrets beyond the one-time user code.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OAuthDeviceStart {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    #[serde(default)]
    pub verification_uri_complete: Option<String>,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OAuthLoginStatus {
    Pending,
    Authorized,
    Expired,
    Error,
}

/// Result of polling an in-flight device-code login. On `Authorized` the
/// token-free account summary is returned; tokens stay inside the agent/vault.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OAuthLoginPoll {
    pub status: OAuthLoginStatus,
    #[serde(default)]
    pub account: Option<OAuthAccountSummary>,
    #[serde(default)]
    pub message: Option<String>,
    /// Current server-side poll interval in seconds. Present on `pending`
    /// responses so the client backs off in step with `slow_down` bumps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_secs: Option<u64>,
}

/// Token-free view of a managed OAuth account, safe to send to the frontend.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OAuthAccountSummary {
    pub id: Uuid,
    pub provider: OAuthProvider,
    pub account_identity: Option<String>,
    #[serde(default)]
    pub chatgpt_account_id: Option<String>,
    #[serde(default)]
    pub entry_id: Option<Uuid>,
    pub is_default: bool,
    /// Unix milliseconds.
    pub authenticated_at: i64,
    #[serde(default)]
    pub credential_expires_at: Option<String>,
    #[serde(default)]
    pub requires_reauth: bool,
}

/// Whether CC Switch's config is present on this machine and, on macOS,
/// whether the app itself is installed.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CcSwitchDetection {
    pub config_exists: bool,
    pub app_installed: bool,
    #[serde(default)]
    pub config_path: Option<String>,
}

/// Ceiling for a request that has no inherent duration of its own.
const DEFAULT_RESPONSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
/// Password-derivation, whole-vault rewrites and network sync legitimately run
/// far longer than a normal request.
const LONG_RESPONSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);
/// Slack added to requests that already carry their own deadline, so the agent
/// gets a chance to answer with its own timeout error first.
const RESPONSE_TIMEOUT_SLACK: std::time::Duration = std::time::Duration::from_secs(10);

impl AgentRequest {
    /// Stable diagnostics name. Never serialize a request just to identify it.
    pub fn event_name(&self) -> &'static str {
        match self {
            Self::SessionStatus => "session.status",
            Self::SessionUnlock { .. } => "session.unlock",
            Self::SessionLock { .. } => "session.lock",
            Self::SessionTouch => "session.touch",
            Self::SessionPolicyGet => "session.policy.get",
            Self::SessionPolicySet { .. } => "session.policy.set",
            Self::ServerStatus => "server.status",
            Self::ServerLogs => "server.logs",
            Self::ServerStart => "server.start",
            Self::ServerStop => "server.stop",
            Self::ServerRouteSelect { .. } => "server.route.select",
            Self::ServerRouteSetEnabled { .. } => "server.route.set_enabled",
            Self::ServerConfigGet => "server.config.get",
            Self::ServerConfigSet { .. } => "server.config.set",
            Self::ServerTokenRotate { .. } => "server.token.rotate",
            Self::ServerUsageSummary { .. } => "server.usage.summary",
            Self::ServerUsageClear => "server.usage.clear",
            Self::ServerUsageTimeseries { .. } => "server.usage_timeseries",
            Self::ServerPricingConfigGet => "server.pricing_config.get",
            Self::ServerPricingRemoteSync { .. } => "server.pricing_remote_sync",
            Self::ServerPricingAssignmentSet { .. } => "server.pricing_assignment.set",
            Self::ServerPricingGroupUpsert { .. } => "server.pricing_group.upsert",
            Self::ServerPricingGroupDelete { .. } => "server.pricing_group.delete",
            Self::ServerPricingGroupVersionDelete { .. } => "server.pricing_group_version.delete",
            Self::VaultStatus => "vault.status",
            Self::VaultCreate { .. } => "vault.create",
            Self::VaultRecover { .. } => "vault.recover",
            Self::VaultReset => "vault.reset",
            Self::VaultChangePassword { .. } => "vault.change_password",
            Self::VaultRotate { .. } => "vault.rotate",
            Self::VaultExport { .. } => "vault.export",
            Self::VaultImport { .. } => "vault.import",
            Self::EntriesList { .. } => "entries.list",
            Self::EntriesTrash => "entries.trash",
            Self::EntriesFavorites => "entries.favorites",
            Self::EntriesSearch { .. } => "entries.search",
            Self::ProviderGet { .. } => "provider.get",
            Self::ProviderAdd { .. } => "provider.add",
            Self::ProviderUpdate { .. } => "provider.update",
            Self::ProviderArchive { .. } => "provider.archive",
            Self::ProviderRestore { .. } => "provider.restore",
            Self::ProviderTrash { .. } => "provider.trash",
            Self::ProviderFavorite { .. } => "provider.favorite",
            Self::ProviderDelete { .. } => "provider.delete",
            Self::TrashPurgeExpired => "trash.purge_expired",
            Self::TrashEmpty => "trash.empty",
            Self::SecretRevealField { .. } => "secret.reveal_field",
            Self::SecretRevealHeaders { .. } => "secret.reveal_headers",
            Self::SecretAdd { .. } => "secret.add",
            Self::SecretUpdate { .. } => "secret.update",
            Self::SecretRemove { .. } => "secret.remove",
            Self::SecretMetadataSet { .. } => "secret.metadata_set",
            Self::DevicesList => "devices.list",
            Self::DeviceRevoke { .. } => "device.revoke",
            Self::ProviderProbe { .. } => "provider.probe",
            Self::ProviderUsageProbe { .. } => "provider.usage_probe",
            Self::ProviderUsageApply { .. } => "provider.usage_apply",
            Self::OfficialAccountsRefresh { .. } => "official_accounts.refresh",
            Self::CcSwitchDetect => "ccswitch.detect",
            Self::CcSwitchImport => "ccswitch.import",
            Self::OAuthLoginStart { .. } => "oauth.login.start",
            Self::OAuthLoginPoll { .. } => "oauth.login.poll",
            Self::OAuthLoginCancel { .. } => "oauth.login.cancel",
            Self::OAuthAccountsList { .. } => "oauth.accounts.list",
            Self::OAuthAccountsRemove { .. } => "oauth.accounts.remove",
            Self::OAuthAccountsSetDefault { .. } => "oauth.accounts.set_default",
            Self::ProviderFaviconBackfill { .. } => "provider.favicon_backfill",
            Self::ToolConfigPreview { .. } => "tool_config.preview",
            Self::ToolConfigApply { .. } => "tool_config.apply",
            Self::ToolConfigProxyPreview { .. } => "tool_config.proxy_preview",
            Self::ToolConfigProxyApply { .. } => "tool_config.proxy_apply",
            Self::ToolConfigRollback { .. } => "tool_config.rollback",
            Self::SyncLocal { .. } => "sync.local",
            Self::SyncSettingsGet => "sync.settings.get",
            Self::SyncSettingsSet { .. } => "sync.settings.set",
            Self::SyncConfigured => "sync.configured",
            Self::SyncCloud { .. } => "sync.cloud",
            Self::SyncWebDav { .. } => "sync.webdav",
            Self::SyncConflicts { .. } => "sync.conflicts",
            Self::SyncAcceptConflict { .. } => "sync.accept_conflict",
            Self::SyncDiscardConflict { .. } => "sync.discard_conflict",
            Self::BrowserContextLookup { .. } => "browser.context_lookup",
            Self::BrowserEntriesSearch { .. } => "browser.entries_search",
            Self::BrowserSecretFill { .. } => "browser.secret_fill",
            Self::BrowserPreviewDetected { .. } => "browser.preview_detected",
            Self::BrowserSaveDetected { .. } => "browser.save_detected",
            Self::BrowserIgnoreOrigin { .. } => "browser.ignore_origin",
            Self::BrowserIsOriginIgnored { .. } => "browser.is_origin_ignored",
            Self::UiOpenMain => "ui.open_main",
            Self::UiOpenUnlock => "ui.open_unlock",
            Self::UiOpenQuickAccess => "ui.open_quick_access",
            Self::AgentShutdown => "agent.shutdown",
        }
    }

    /// Successful high-frequency polls need no individual operation trail.
    pub fn is_background_poll(&self) -> bool {
        matches!(
            self,
            Self::SessionStatus
                | Self::VaultStatus
                | Self::SessionTouch
                | Self::ServerStatus
                | Self::ServerLogs
                | Self::ServerUsageSummary { .. }
                | Self::ServerUsageTimeseries { .. }
        )
    }
    /// How long a client should wait for this request's response.
    ///
    /// Without a deadline a wedged agent hangs the caller forever; with a
    /// single global one, requests that are *legitimately* slow — waiting on
    /// the unlock window, deriving a key, rewrapping every record, talking to
    /// WebDAV — would be cut off mid-flight. So the bound follows the request.
    pub fn response_timeout(&self) -> std::time::Duration {
        match self {
            // Waits on the user; the agent enforces the real deadline.
            Self::SessionUnlock {
                mode: SessionUnlockMode::NativeWindowWait { timeout_ms },
            } => std::time::Duration::from_millis(*timeout_ms) + RESPONSE_TIMEOUT_SLACK,
            // Probes carry their own upstream timeout.
            Self::ProviderProbe {
                timeout_seconds, ..
            }
            | Self::ProviderUsageProbe {
                timeout_seconds, ..
            } => std::time::Duration::from_secs(*timeout_seconds) + RESPONSE_TIMEOUT_SLACK,
            // Pricing synchronization may query every credential in an entry,
            // so its total duration can exceed one upstream timeout.
            Self::ServerPricingRemoteSync { .. } => LONG_RESPONSE_TIMEOUT,
            // Argon2 derivation, full-vault rewrites, export/import.
            Self::SessionUnlock { .. }
            | Self::VaultCreate { .. }
            | Self::VaultRecover { .. }
            | Self::VaultChangePassword { .. }
            | Self::VaultRotate { .. }
            | Self::VaultExport { .. }
            | Self::VaultImport { .. }
            | Self::VaultReset
            // Network and whole-collection work.
            | Self::SyncLocal { .. }
            | Self::SyncCloud { .. }
            | Self::SyncWebDav { .. }
            | Self::SyncConfigured
            | Self::SyncConflicts { .. }
            | Self::ProviderFaviconBackfill { .. }
            | Self::OfficialAccountsRefresh { .. }
            | Self::OAuthLoginStart { .. }
            | Self::OAuthLoginPoll { .. }
            | Self::TrashPurgeExpired
            | Self::TrashEmpty
            | Self::ServerPricingGroupUpsert { .. }
            // Configuration writes may checkpoint and migrate a large Codex
            // SQLite state database before replacing the requested files.
            | Self::ToolConfigPreview { .. }
            | Self::ToolConfigApply { .. }
            | Self::ToolConfigProxyPreview { .. }
            | Self::ToolConfigProxyApply { .. } => LONG_RESPONSE_TIMEOUT,
            // Cheap local file reads only.
            Self::CcSwitchDetect | Self::CcSwitchImport => DEFAULT_RESPONSE_TIMEOUT,
            _ => DEFAULT_RESPONSE_TIMEOUT,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentResponse {
    pub protocol_version: u32,
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
            protocol_version: AGENT_PROTOCOL_VERSION,
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
            protocol_version: AGENT_PROTOCOL_VERSION,
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

fn agent_protocol_version() -> u32 {
    AGENT_PROTOCOL_VERSION
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
    /// The key this save wrote to, new or existing.
    #[serde(default)]
    pub secret_id: Option<String>,
    /// True when the key was added to an entry that already existed, rather
    /// than creating a new entry.
    #[serde(default)]
    pub merged_into_existing: bool,
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

/// Full name/value pairs of a provider entry's custom headers. Values stay
/// encrypted at rest and only leave the agent on explicit user request.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderHeaderValues {
    pub headers: Vec<(String, SensitiveString)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerTokenResponse {
    pub route_id: Uuid,
    pub token: SensitiveString,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ServerUsageSummary {
    pub request_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub estimated_cost_micros: u64,
    #[serde(default)]
    pub attempt_count: u64,
    #[serde(default)]
    pub completed_attempts: u64,
    #[serde(default)]
    pub successful_attempts: u64,
    #[serde(default)]
    pub success_rate_bps: u16,
    #[serde(default)]
    pub average_first_token_ms: Option<u64>,
    pub providers: Vec<ProviderUsageAggregate>,
    #[serde(default)]
    pub models: Vec<ModelUsageAggregate>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct OffPeakWindow {
    pub start_minute_utc: u16,
    pub end_minute_utc: u16,
    pub input_micros_per_million: u64,
    pub output_micros_per_million: u64,
    pub cache_read_micros_per_million: u64,
    pub cache_creation_micros_per_million: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct ModelPriceRule {
    pub model: String,
    pub input_micros_per_million: u64,
    pub output_micros_per_million: u64,
    pub cache_read_micros_per_million: u64,
    pub cache_creation_micros_per_million: u64,
    pub off_peak: Option<OffPeakWindow>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct GroupPriceVersion {
    pub effective_from: i64,
    pub rules: Vec<ModelPriceRule>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct PricingGroup {
    pub id: Uuid,
    pub name: String,
    pub versions: Vec<GroupPriceVersion>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct CredentialAssignment {
    pub entry_id: Uuid,
    pub secret_id: String,
    pub group_id: Option<Uuid>,
    pub multiplier: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PricingConfig {
    pub groups: Vec<PricingGroup>,
    pub assignments: Vec<CredentialAssignment>,
    pub list_price_updated_at: Option<i64>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PricingApplyScope {
    AllHistory,
    FromNow,
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

    /// An ordinary request must not be able to hang a caller indefinitely.
    #[test]
    fn ordinary_requests_use_the_default_response_timeout() {
        assert_eq!(
            AgentRequest::SessionStatus.response_timeout(),
            DEFAULT_RESPONSE_TIMEOUT
        );
        assert_eq!(
            AgentRequest::EntriesList { archived: false }.response_timeout(),
            DEFAULT_RESPONSE_TIMEOUT
        );
    }

    #[test]
    fn tool_config_requests_use_the_long_response_timeout() {
        let request = AgentRequest::ToolConfigApply {
            request: ToolConfigRequest {
                tool: ToolConfigTool::Codex,
                id: uuid::Uuid::nil(),
                mode: ToolConfigMode::Plaintext,
                codex_api_key_mode: None,
            },
        };
        assert_eq!(request.response_timeout(), LONG_RESPONSE_TIMEOUT);
    }

    /// A request that waits on the user outlives the default bound, so the
    /// client must follow the deadline the request itself carries.
    #[test]
    fn user_facing_waits_extend_past_their_own_deadline() {
        let timeout = AgentRequest::SessionUnlock {
            mode: SessionUnlockMode::NativeWindowWait {
                timeout_ms: 120_000,
            },
        }
        .response_timeout();

        assert_eq!(
            timeout,
            std::time::Duration::from_secs(120) + RESPONSE_TIMEOUT_SLACK
        );
        assert!(timeout > DEFAULT_RESPONSE_TIMEOUT);
    }

    #[test]
    fn probes_outlive_the_upstream_timeout_they_request() {
        let timeout = AgentRequest::ProviderUsageProbe {
            id: uuid::Uuid::nil(),
            mode: UsageProbeMode::default(),
            timeout_seconds: 45,
            base_url: None,
            access_token: None,
            user_id: None,
        }
        .response_timeout();

        assert_eq!(
            timeout,
            std::time::Duration::from_secs(45) + RESPONSE_TIMEOUT_SLACK
        );
    }

    #[test]
    fn pricing_sync_uses_the_long_network_timeout_for_multiple_keys() {
        assert_eq!(
            AgentRequest::ServerPricingRemoteSync {
                id: uuid::Uuid::nil(),
                timeout_seconds: 15,
            }
            .response_timeout(),
            LONG_RESPONSE_TIMEOUT
        );
    }

    #[test]
    fn official_account_refresh_uses_the_long_network_timeout() {
        assert_eq!(
            AgentRequest::OfficialAccountsRefresh {
                provider_ids: vec!["openai".to_string()]
            }
            .response_timeout(),
            LONG_RESPONSE_TIMEOUT
        );
    }

    #[test]
    fn ccswitch_requests_are_cheap_local_work() {
        assert_eq!(
            AgentRequest::CcSwitchDetect.response_timeout(),
            DEFAULT_RESPONSE_TIMEOUT
        );
        assert_eq!(
            AgentRequest::CcSwitchImport.response_timeout(),
            DEFAULT_RESPONSE_TIMEOUT
        );
    }

    #[test]
    fn ccswitch_requests_round_trip_with_stable_wire_names() {
        let detect = serde_json::to_value(AgentRequest::CcSwitchDetect).unwrap();
        assert_eq!(detect, serde_json::json!({"type": "ccswitch.detect"}));
        assert!(matches!(
            serde_json::from_value::<AgentRequest>(detect).unwrap(),
            AgentRequest::CcSwitchDetect
        ));

        let import = serde_json::to_value(AgentRequest::CcSwitchImport).unwrap();
        assert_eq!(import, serde_json::json!({"type": "ccswitch.import"}));
        assert!(matches!(
            serde_json::from_value::<AgentRequest>(import).unwrap(),
            AgentRequest::CcSwitchImport
        ));
    }

    #[test]
    fn ccswitch_detection_uses_camel_case_payload() {
        let detection = CcSwitchDetection {
            config_exists: true,
            app_installed: false,
            config_path: Some("/home/u/.cc-switch/config.json".to_string()),
        };
        let value = serde_json::to_value(&detection).unwrap();
        assert_eq!(value["configExists"], true);
        assert_eq!(value["appInstalled"], false);
        assert_eq!(value["configPath"], "/home/u/.cc-switch/config.json");
        assert!(value.get("config_exists").is_none());

        let parsed: CcSwitchDetection = serde_json::from_value(value).unwrap();
        assert_eq!(parsed, detection);
        // Older payloads without configPath still parse.
        let parsed: CcSwitchDetection = serde_json::from_value(serde_json::json!({
            "configExists": false,
            "appInstalled": false
        }))
        .unwrap();
        assert_eq!(parsed.config_path, None);
    }

    /// Key derivation and whole-vault rewrites are slow by nature; cutting them
    /// off at the default bound would fail operations that were succeeding.
    #[test]
    fn slow_vault_work_gets_the_long_timeout() {
        for request in [
            AgentRequest::VaultRotate {
                reason: "test".to_string(),
            },
            AgentRequest::SessionUnlock {
                mode: SessionUnlockMode::Password {
                    password: SensitiveString::new("pw".to_string()),
                },
            },
            AgentRequest::TrashEmpty,
        ] {
            assert_eq!(
                request.response_timeout(),
                LONG_RESPONSE_TIMEOUT,
                "{request:?}"
            );
        }
    }

    #[test]
    fn agent_response_includes_protocol_version() {
        let response = AgentResponse::empty();
        let value = serde_json::to_value(response).unwrap();
        assert_eq!(
            value["protocolVersion"],
            serde_json::json!(AGENT_PROTOCOL_VERSION)
        );
        assert!(value.get("protocol_version").is_none());
    }

    #[test]
    fn correlation_metadata_accepts_legacy_frames_and_round_trips() {
        let legacy = serde_json::json!({"protocolVersion":AGENT_PROTOCOL_VERSION,"authToken":"test-token","request":{"type":"provider.get","id":Uuid::nil()}});
        let mut request: AuthenticatedAgentRequest = serde_json::from_value(legacy).unwrap();
        assert_eq!(request.request_id, None);
        let id = Uuid::new_v4();
        request.request_id = Some(id);
        let decoded: AuthenticatedAgentRequest =
            serde_json::from_value(serde_json::to_value(request).unwrap()).unwrap();
        assert_eq!(decoded.request_id, Some(id));
        assert_eq!(decoded.request.event_name(), "provider.get");
    }

    #[test]
    fn favicon_backfill_request_uses_camel_case_payload() {
        let request = AgentRequest::ProviderFaviconBackfill {
            request: FaviconBackfillRequest {
                entry_ids: Some(vec![Uuid::nil()]),
                limit: Some(4),
            },
        };
        let value = serde_json::to_value(request).unwrap();
        assert_eq!(value["type"], "provider.favicon_backfill");
        assert_eq!(value["request"]["entryIds"][0], Uuid::nil().to_string());
        assert_eq!(value["request"]["limit"], 4);
        assert!(value["request"].get("entry_ids").is_none());
    }

    #[test]
    fn usage_clear_request_has_a_stable_wire_name() {
        let value = serde_json::to_value(AgentRequest::ServerUsageClear).unwrap();
        assert_eq!(value["type"], "server.usage.clear");
    }

    #[test]
    fn reveal_headers_round_trips_with_a_stable_wire_name() {
        let request = AgentRequest::SecretRevealHeaders { id: Uuid::nil() };
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["type"], "secret.reveal_headers");
        assert!(matches!(
            serde_json::from_value::<AgentRequest>(value).unwrap(),
            AgentRequest::SecretRevealHeaders { id } if id == Uuid::nil()
        ));

        let payload = ProviderHeaderValues {
            headers: vec![(
                "x-version".to_string(),
                SensitiveString::new("1".to_string()),
            )],
        };
        let value = serde_json::to_value(&payload).unwrap();
        assert_eq!(value["headers"][0][0], "x-version");
        assert_eq!(value["headers"][0][1], "1");
        let parsed: ProviderHeaderValues = serde_json::from_value(value).unwrap();
        assert_eq!(parsed.headers[0].0, "x-version");
        assert_eq!(parsed.headers[0].1.expose(), "1");
    }

    #[test]
    fn usage_summary_preserves_unfiltered_clients_and_accepts_periods() {
        let request: AgentRequest = serde_json::from_value(serde_json::json!({
            "type": "server.usage.summary"
        }))
        .unwrap();
        assert!(matches!(
            request,
            AgentRequest::ServerUsageSummary {
                days: None,
                timezone_offset_minutes: 0,
                granularity: UsageGranularity::Day
            }
        ));
        for (days, granularity) in [(1, "hour"), (7, "day"), (30, "day")] {
            let value = serde_json::json!({
                "type": "server.usage.summary", "days": days,
                "timezone_offset_minutes": 345, "granularity": granularity
            });
            let request: AgentRequest = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(request).unwrap(), value);
        }
    }

    #[test]
    fn usage_timeseries_defaults_to_daily_utc_for_older_clients() {
        let request: AgentRequest = serde_json::from_value(serde_json::json!({
            "type": "server.usage_timeseries",
            "days": 7
        }))
        .unwrap();
        assert!(matches!(
            request,
            AgentRequest::ServerUsageTimeseries {
                days: 7,
                timezone_offset_minutes: 0,
                granularity: UsageGranularity::Day
            }
        ));
    }

    #[test]
    fn usage_timeseries_accepts_hourly_granularity() {
        let value = serde_json::json!({
            "type": "server.usage_timeseries",
            "days": 1,
            "timezone_offset_minutes": 480,
            "granularity": "hour"
        });
        let request: AgentRequest = serde_json::from_value(value.clone()).unwrap();
        assert!(matches!(
            request,
            AgentRequest::ServerUsageTimeseries {
                days: 1,
                timezone_offset_minutes: 480,
                granularity: UsageGranularity::Hour
            }
        ));
        assert_eq!(serde_json::to_value(request).unwrap(), value);
    }
}
