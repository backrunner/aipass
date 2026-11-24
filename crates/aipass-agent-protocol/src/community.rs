use crate::SensitiveString;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommunityLoginInput {
    #[serde(default)]
    pub entry_id: Option<Uuid>,
    pub provider: String,
    /// Digest from the catalog that supplied this plugin login form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_digest: Option<String>,
    pub method: usize,
    #[serde(default)]
    pub inputs: BTreeMap<String, SensitiveString>,
    #[serde(default)]
    pub api_key: Option<SensitiveString>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommunityLoginStatus {
    pub ticket: Uuid,
    pub status: String,
    pub url: Option<String>,
    pub instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_code: Option<String>,
    pub method: Option<String>,
    pub entry_id: Option<Uuid>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum ClaudeBridgeRequest {
    ListTools,
    Call {
        name: String,
        arguments: serde_json::Value,
    },
    Poll {
        call_id: String,
    },
}

/// Local desktop management only. Trust grants never enter synchronized records.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase", deny_unknown_fields)]
pub enum SubscriptionPluginAction {
    List,
    InspectGithub {
        repository: String,
        #[serde(default)]
        native: bool,
    },
    EnableGithub {
        review: Uuid,
        digest: String,
    },
    Inspect {
        path: std::path::PathBuf,
    },
    Enable {
        path: std::path::PathBuf,
        digest: String,
    },
    Disable {
        id: String,
    },
}
