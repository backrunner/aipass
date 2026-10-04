//! Agent-owned provider runtime preferences. Sensitive values never appear in
//! entry summaries; reads omit them and an omitted write preserves them.
use crate::SensitiveString;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderRuntimeOptions {
    pub quota_tracking: bool,
    pub quota_refresh_seconds: u32,
    pub proxy: Option<ProviderProxyOptions>,
    pub balance: Option<BalanceEndpoint>,
    pub webhooks: Vec<ProviderWebhook>,
}
impl Default for ProviderRuntimeOptions {
    fn default() -> Self {
        Self {
            quota_tracking: true,
            quota_refresh_seconds: 120,
            proxy: None,
            balance: None,
            webhooks: Vec::new(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProxyOptions {
    pub mode: aipass_proxy::UpstreamProxyMode,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub username: Option<SensitiveString>,
    #[serde(default)]
    pub password: Option<SensitiveString>,
    #[serde(default)]
    pub has_credentials: bool,
    #[serde(default)]
    pub clear_credentials: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceEndpoint {
    pub url: String,
    #[serde(default)]
    pub post: bool,
    #[serde(default)]
    pub body: Option<SensitiveString>,
    #[serde(default)]
    pub headers: Vec<(String, Option<SensitiveString>)>,
    pub json_path: String,
    pub unit: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderWebhook {
    pub id: Uuid,
    pub url: String,
    pub enabled: bool,
    pub events: Vec<ProviderWebhookEvent>,
    #[serde(default)]
    pub secret: Option<SensitiveString>,
    #[serde(default)]
    pub has_secret: bool,
    #[serde(default)]
    pub clear_secret: bool,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ProviderWebhookEvent {
    QuotaLow,
    QuotaExhausted,
    RateLimitDetected,
    ProviderError,
    ProviderDegraded,
    CredentialFailed,
    SubscriptionExpiring,
    SubscriptionExpired,
}
