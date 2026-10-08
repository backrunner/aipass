//! Local official CLI capability and sign-in results.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeCliStatus {
    pub available: bool,
    /// missing, unusable, or unsupported; never subprocess output.
    pub reason: Option<String>,
    pub version: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeLoginStatus {
    pub ticket: Uuid,
    /// pending, authorized, expired, or error.
    pub status: String,
    pub url: Option<String>,
    pub entry_id: Option<Uuid>,
    pub message: Option<String>,
}
