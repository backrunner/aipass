mod backup;
mod models;
mod plan;
mod utils;

pub use backup::{
    apply_plan, apply_plan_encrypted, apply_plan_with_plain_backup, find_backup_by_operation,
    rollback, rollback_encrypted, rollback_plain,
};
pub use models::{ApplyResult, ConfigPlan, ConfigWriter, EncryptedBackup, ToolEntry, ToolId};
pub use plan::{plan_claude_code, plan_codex, plan_gemini_cli};
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
