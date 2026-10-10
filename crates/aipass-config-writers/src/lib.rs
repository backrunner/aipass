mod backup;
mod detect;
mod models;
pub mod native_auth;
mod native_plan;
mod plan;
mod preview_redaction;
pub mod transaction;
mod utils;
pub use native_plan::prepare_native_codex_plan;
pub use preview_redaction::redact_config;

pub use backup::{
    apply_plan, apply_plan_encrypted, apply_plan_with_plain_backup, find_backup_by_operation,
    rollback, rollback_encrypted, rollback_plain,
};
pub use detect::{detect_tools, ToolDetection};
pub use models::{
    ApplyResult, CodexApiKeyMode, CodexProviderMigration, CodexSessionMigration, ConfigPlan,
    ConfigWriter, EncryptedBackup, PlannedWrite, ToolEntry, ToolId,
};
pub use plan::{
    plan_claude_code, plan_claude_code_official, plan_claude_code_plaintext, plan_codex,
    plan_codex_official, plan_codex_plaintext, plan_codex_plaintext_with_mode, plan_cursor_local,
    plan_cursor_local_plaintext, plan_gemini_cli, plan_gemini_cli_plaintext, plan_grok,
    plan_grok_plaintext, plan_grok_plaintext_with_backend, plan_opencode, plan_opencode_plaintext,
    plan_opencode_plaintext_with_api, plan_pi, plan_pi_plaintext, plan_pi_plaintext_with_api,
    preview_codex, preview_codex_official, preview_codex_plaintext_with_mode, GrokApiBackend,
    OpenCodeApi, PiApi,
};
pub use utils::{
    config_backup_path, diff_preview_for_path, diff_preview_from, endpoint_url,
    redacted_diff_preview,
};

#[cfg(test)]
mod tests;
