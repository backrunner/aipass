//! Compatibility results for existing account operations.
use super::*;

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
