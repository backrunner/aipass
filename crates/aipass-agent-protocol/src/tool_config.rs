use aipass_config_writers::ToolId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
    /// Stable credential id. Required when the site has multiple keys.
    #[serde(default)]
    pub secret_id: Option<String>,
    pub mode: ToolConfigMode,
    #[serde(default)]
    pub codex_api_key_mode: Option<CodexApiKeyMode>,
    #[serde(default)]
    pub preview_id: Option<String>,
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
    #[serde(default)]
    pub preview_id: Option<String>,
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
    #[serde(default)]
    pub outcome: ToolConfigOutcome,
    #[serde(default)]
    pub message: Option<String>,
    pub tool: ToolConfigTool,
    pub mode: ToolConfigMode,
    pub entry_id: Uuid,
    pub entry_title: String,
    pub operation_id: Uuid,
    pub target_path: String,
    pub backup_path: String,
    pub summary: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolConfigOutcome {
    #[default]
    Applied,
    LoginRequired,
    Conflict,
    StorageUnavailable,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolConfigStatus {
    pub tool: ToolConfigTool,
    pub state: String,
    pub entry_title: Option<String>,
    pub entry_id: Option<Uuid>,
    pub secret_id: Option<String>,
    pub mode: Option<ToolConfigMode>,
    pub account_identity: Option<String>,
    pub operation_id: Option<Uuid>,
    pub message: Option<String>,
    pub overrides: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolConfigLoginStatus {
    pub ticket: Uuid,
    pub status: String,
    pub url: Option<String>,
    pub user_code: Option<String>,
    pub requires_code: bool,
    pub message: Option<String>,
    pub result: Option<ToolConfigApplyResponse>,
}
