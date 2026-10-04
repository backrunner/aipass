use crate::SensitiveString;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommunityLoginInput {
    pub provider: String,
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
    pub method: Option<String>,
    pub entry_id: Option<Uuid>,
    pub error: Option<String>,
}
