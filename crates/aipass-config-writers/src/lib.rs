mod env_helper;
pub use env_helper::{env_credential, env_literal, helper_path, shell_env, shell_quote};
mod byok_tools;
pub use byok_tools::byok_config_paths;
mod additional_tools;
pub use additional_tools::{
    dsh_home, kimi_config_dir, plan_additional_tool, read_yaml, zed_config_path, ModelApi,
};
mod backup;
pub mod cli_discovery;
mod detect;
mod installed_apps;
mod models;
pub mod native_auth;
mod native_plan;
mod plan;
mod preview_redaction;
pub mod proxy_recovery;
pub mod transaction;
mod utils;
pub use native_plan::prepare_native_codex_plan;
pub use preview_redaction::redact_config;

pub use backup::{
    apply_plan, apply_plan_encrypted, apply_plan_with_plain_backup, find_backup_by_operation,
    prepare_rollback_encrypted, rollback, rollback_encrypted, rollback_plain, PreparedRollback,
};
pub use detect::{detect_tool_in, detect_tools, detect_tools_in, ToolDetection};
pub use models::{
    ApplyResult, CodexApiKeyMode, CodexProviderMigration, CodexSessionMigration, ConfigPlan,
    ConfigWriter, EncryptedBackup, PlannedWrite, ToolEntry, ToolId,
};
pub use plan::{
    gemini_config_dir, plan_claude_code, plan_claude_code_official, plan_claude_code_plaintext,
    plan_codex, plan_codex_official, plan_codex_plaintext, plan_codex_plaintext_with_mode,
    plan_cursor_local, plan_cursor_local_plaintext, plan_cursor_local_proxy_plaintext,
    plan_gemini_cli, plan_gemini_cli_plaintext, plan_grok, plan_grok_plaintext,
    plan_grok_plaintext_with_backend, plan_opencode, plan_opencode_plaintext,
    plan_opencode_plaintext_with_api, plan_pi, plan_pi_plaintext, plan_pi_plaintext_with_api,
    preview_codex, preview_codex_official, preview_codex_plaintext_with_mode, GrokApiBackend,
    OpenCodeApi, PiApi,
};
pub use utils::{
    config_backup_path, diff_preview_for_path, diff_preview_from, endpoint_url, grok_config_dir,
    opencode_auth_path, opencode_config_path, pi_config_dir, redacted_diff_preview, user_home_dir,
    xdg_config_home, xdg_data_home,
};

#[cfg(test)]
mod tests;
