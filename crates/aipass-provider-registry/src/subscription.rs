pub mod plugin_ui;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionWindow {
    pub id: String,
    pub label: String,
    pub used_percent: Option<f64>,
    pub resets_at: Option<String>,
    pub window_minutes: Option<u64>,
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionSnapshot {
    pub plan: Option<String>,
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub console_url: Option<String>,
    #[serde(default)]
    pub quota_unavailable: bool,
    pub subscription_expires_at: Option<String>,
    pub subscription_renews_at: Option<String>,
    pub billing_period_ends_at: Option<String>,
    pub credential_expires_at: Option<String>,
    pub credits_remaining: Option<String>,
    pub credits_currency: Option<String>,
    #[serde(default)]
    pub windows: Vec<SubscriptionWindow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<plugin_ui::Section>,
    pub observed_at: String,
    pub source: String,
    #[serde(default)]
    pub stale: bool,
    #[serde(default)]
    pub error: Option<String>,
}
