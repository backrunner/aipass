use aipass_provider_registry::{AuthScheme, InterfaceType};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolId {
    Codex,
    ClaudeCode,
    GeminiCli,
    OpenCode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodexApiKeyMode {
    ExperimentalBearerToken,
    AuthJson,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolEntry {
    pub id: Uuid,
    pub title: String,
    pub provider_id: Option<String>,
    pub endpoint: Option<String>,
    pub interface_type: InterfaceType,
    pub auth_scheme: AuthScheme,
    pub env_key: String,
    pub default_model: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Clone, Debug)]
pub struct PlannedWrite {
    pub target_path: PathBuf,
    pub backup_path: PathBuf,
    pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CodexProviderMigration {
    pub from_provider: String,
    pub to_provider: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigPlan {
    pub operation_id: Uuid,
    pub tool: ToolId,
    pub target_path: PathBuf,
    pub backup_path: PathBuf,
    pub summary: String,
    pub preview: String,
    #[serde(skip, default)]
    pub extra_writes: Vec<PlannedWrite>,
    #[serde(skip, default)]
    pub codex_provider_migration: Option<CodexProviderMigration>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApplyResult {
    pub operation_id: Uuid,
    pub target_path: PathBuf,
    pub backup_path: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EncryptedBackup {
    pub format: String,
    pub version: u16,
    pub operation_id: Uuid,
    pub target_path: PathBuf,
    pub target_existed: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    pub ciphertext: aipass_crypto::Ciphertext,
}

pub trait ConfigWriter {
    fn plan(&self, home: &Path, entry: &ToolEntry) -> Result<ConfigPlan>;
    fn apply(&self, plan: &ConfigPlan, content: &str) -> Result<ApplyResult>;
}
