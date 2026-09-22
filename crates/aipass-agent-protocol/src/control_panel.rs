use crate::SensitiveString;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ControlPanelSettings {
    pub enabled: bool,
    pub address: String,
    pub port: u16,
    #[serde(default)]
    pub https: bool,
}

impl Default for ControlPanelSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            address: "127.0.0.1".into(),
            port: 8788,
            https: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ControlPanelCertificate {
    pub certificate_pem: String,
    pub private_key_pem: SensitiveString,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlPanelStatus {
    pub settings: ControlPanelSettings,
    pub running: bool,
    pub url: Option<String>,
    pub addresses: Vec<String>,
    pub certificate_pem: Option<String>,
    pub fingerprint: Option<String>,
    pub imported_certificate: bool,
    pub error: Option<String>,
    pub has_access_code: bool,
    pub remote_unlock_enabled: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlPanelAccessCode {
    pub access_code: SensitiveString,
}

/// Explicit allowlist for the remote surface; never deserialize arbitrary AgentRequest.
#[derive(Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ControlPanelAction {
    ProxyStart,
    ProxyStop,
    RouteEnabled {
        route_id: Uuid,
        enabled: bool,
    },
    TargetUpdate {
        route_id: Uuid,
        target_id: Uuid,
        revision: String,
        provider_entry_id: Uuid,
        secret_id: String,
        enabled: bool,
        priority: u16,
        weight: u32,
        prefer_ws: bool,
    },
    ToolPreview {
        selection: ControlPanelToolSelection,
    },
    ToolApply {
        preview_id: Uuid,
    },
    VaultLock,
}

#[derive(Clone, Deserialize)]
#[serde(
    tag = "source",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ControlPanelToolSelection {
    Credential {
        request: crate::ToolConfigRequest,
    },
    Proxy {
        request: crate::ToolConfigProxyRequest,
    },
}
