mod backup;
mod models;
mod plan;
mod utils;

pub use backup::{
    apply_plan, apply_plan_encrypted, apply_plan_with_plain_backup, find_backup_by_operation,
    rollback, rollback_encrypted, rollback_plain,
};
pub use models::{ApplyResult, ConfigPlan, ConfigWriter, EncryptedBackup, ToolEntry, ToolId};
pub use plan::{
    plan_claude_code, plan_claude_code_plaintext, plan_codex, plan_codex_plaintext,
    plan_gemini_cli, plan_gemini_cli_plaintext, plan_opencode, plan_opencode_plaintext,
};
pub use utils::endpoint_url;

#[cfg(test)]
mod tests {
    use super::*;
    use aipass_provider_registry::{AuthScheme, InterfaceType};
    use tempfile::tempdir;

    fn entry(interface_type: InterfaceType, auth_scheme: AuthScheme) -> ToolEntry {
        ToolEntry {
            id: uuid::Uuid::new_v4(),
            title: "Anthropic Prod".to_string(),
            provider_id: Some("anthropic".to_string()),
            endpoint: Some("https://api.anthropic.com".to_string()),
            interface_type,
            auth_scheme,
            env_key: "ANTHROPIC_API_KEY".to_string(),
            default_model: Some("claude-sonnet-4-20250514".to_string()),
            api_key: None,
        }
    }

    #[test]
    fn claude_writer_uses_helper_not_plaintext() {
        let dir = tempdir().unwrap();
        let (plan, content) = plan_claude_code(
            dir.path(),
            &entry(InterfaceType::AnthropicMessages, AuthScheme::XApiKey),
        )
        .unwrap();
        assert!(content.contains("apiKeyHelper"));
        assert!(!content.contains("sk-ant-api03"));
        apply_plan(&plan, &content).unwrap();
        assert!(plan.target_path.exists());
        rollback(&plan).unwrap();
        assert!(!plan.target_path.exists());
    }

    #[test]
    fn codex_writer_is_idempotent_provider_block() {
        let dir = tempdir().unwrap();
        let entry = entry(InterfaceType::OpenAiCompatible, AuthScheme::Bearer);
        let (plan, content) = plan_codex(dir.path(), &entry).unwrap();
        apply_plan(&plan, &content).unwrap();
        let (_plan2, content2) = plan_codex(dir.path(), &entry).unwrap();
        assert!(content2.contains("model_providers"));
        assert!(content2.contains("[model_providers.aipass_anthropic_prod]"));
        assert!(content2.contains("env_key = \"ANTHROPIC_API_KEY\""));
        assert!(content2.contains("requires_openai_auth = false"));
        assert!(content2.contains("base_url = \"https://api.anthropic.com/v1\""));
    }

    #[test]
    fn codex_plaintext_writer_writes_auth_json_and_rolls_back() {
        let dir = tempdir().unwrap();
        let target = dir.path().join(".codex").join("config.toml");
        let auth_path = dir.path().join(".codex").join("auth.json");
        let mut entry = entry(InterfaceType::OpenAiCompatible, AuthScheme::Bearer);
        entry.endpoint = Some("https://api.openai.com".to_string());
        entry.default_model = Some("gpt-5.4".to_string());
        entry.api_key = Some("sk-test-codex".to_string());

        let (plan, content) = plan_codex_plaintext(dir.path(), &entry).unwrap();
        apply_plan_encrypted(&plan, &content, &[9_u8; aipass_crypto::KEY_LEN]).unwrap();

        let config_text = std::fs::read_to_string(&target).unwrap();
        let auth_text = std::fs::read_to_string(&auth_path).unwrap();
        assert!(config_text.contains("requires_openai_auth = true"));
        assert!(auth_text.contains("OPENAI_API_KEY"));
        assert!(auth_text.contains("sk-test-codex"));

        rollback_encrypted(&plan.backup_path, &[9_u8; aipass_crypto::KEY_LEN]).unwrap();
        assert!(!target.exists());
        assert!(!auth_path.exists());
    }

    #[test]
    fn codex_writer_uses_codex_home_when_directory_exists() {
        let dir = tempdir().unwrap();
        let codex_home = dir.path().join("custom-codex-home");
        std::fs::create_dir_all(&codex_home).unwrap();
        let original = std::env::var_os("CODEX_HOME");
        std::env::set_var("CODEX_HOME", &codex_home);

        let (plan, _content) = plan_codex(
            dir.path(),
            &entry(InterfaceType::OpenAiCompatible, AuthScheme::Bearer),
        )
        .unwrap();

        match original {
            Some(value) => std::env::set_var("CODEX_HOME", value),
            None => std::env::remove_var("CODEX_HOME"),
        }

        assert_eq!(plan.target_path, codex_home.join("config.toml"));
        assert_eq!(
            plan.backup_path.parent().unwrap(),
            codex_home.join(".aipass-backups")
        );
    }

    #[test]
    fn gemini_plaintext_writer_targets_real_env_file() {
        let dir = tempdir().unwrap();
        let mut entry = entry(InterfaceType::Gemini, AuthScheme::GoogleApiKey);
        entry.endpoint = Some("https://generativelanguage.googleapis.com".to_string());
        entry.default_model = Some("gemini-2.5-pro".to_string());
        entry.api_key = Some("AIza-test-key".to_string());
        let (plan, content) = plan_gemini_cli_plaintext(dir.path(), &entry).unwrap();
        assert_eq!(plan.tool, ToolId::GeminiCli);
        assert_eq!(plan.target_path, dir.path().join(".gemini").join(".env"));
        assert!(content.contains("GEMINI_API_KEY=\"AIza-test-key\""));
        assert!(content.contains("GOOGLE_GEMINI_BASE_URL="));
        assert!(content.contains("GEMINI_MODEL=\"gemini-2.5-pro\""));
        assert!(!plan.preview.contains("AIza-test-key"));
    }

    #[test]
    fn opencode_helper_writer_uses_env_reference() {
        let dir = tempdir().unwrap();
        let mut entry = entry(InterfaceType::OpenAiCompatible, AuthScheme::Bearer);
        entry.provider_id = Some("openrouter".to_string());
        entry.env_key = "OPENROUTER_API_KEY".to_string();
        entry.endpoint = Some("https://openrouter.ai/api/v1".to_string());
        entry.default_model = Some("openai/gpt-4.1-mini".to_string());
        let (plan, content) = plan_opencode(dir.path(), &entry).unwrap();
        assert_eq!(
            plan.target_path,
            dir.path()
                .join(".config")
                .join("opencode")
                .join("opencode.json")
        );
        assert!(content.contains("\"npm\": \"@ai-sdk/openai-compatible\""));
        assert!(content.contains("\"apiKey\": \"{env:OPENROUTER_API_KEY}\""));
        assert!(content.contains("\"model\": \"aipass_anthropic_prod/openai/gpt-4.1-mini\""));
    }

    #[test]
    fn encrypted_backup_does_not_leak_original_plaintext_secret() {
        let dir = tempdir().unwrap();
        let target = dir.path().join(".claude").join("settings.json");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(
            &target,
            r#"{"apiKey":"sk-ant-api03-plaintext-in-original"}"#,
        )
        .unwrap();
        let plan = ConfigPlan {
            operation_id: uuid::Uuid::new_v4(),
            tool: ToolId::ClaudeCode,
            target_path: target.clone(),
            backup_path: dir.path().join(".backups").join("config.aipbackup"),
            summary: "test encrypted backup".to_string(),
            preview: "{}".to_string(),
            extra_writes: Vec::new(),
        };

        apply_plan_encrypted(&plan, "{}", &[7_u8; aipass_crypto::KEY_LEN]).unwrap();
        let backup_text = std::fs::read_to_string(&plan.backup_path).unwrap();
        assert!(!backup_text.contains("sk-ant-api03-plaintext-in-original"));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "{}");

        rollback_encrypted(&plan.backup_path, &[7_u8; aipass_crypto::KEY_LEN]).unwrap();
        assert!(std::fs::read_to_string(&target)
            .unwrap()
            .contains("sk-ant-api03-plaintext-in-original"));
    }
}
