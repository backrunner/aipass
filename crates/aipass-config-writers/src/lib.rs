mod backup;
mod detect;
mod models;
mod plan;
mod utils;

pub use backup::{
    apply_plan, apply_plan_encrypted, apply_plan_with_plain_backup, find_backup_by_operation,
    rollback, rollback_encrypted, rollback_plain,
};
pub use detect::{detect_tools, ToolDetection};
pub use models::{
    ApplyResult, CodexApiKeyMode, CodexProviderMigration, ConfigPlan, ConfigWriter,
    EncryptedBackup, ToolEntry, ToolId,
};
pub use plan::{
    plan_claude_code, plan_claude_code_plaintext, plan_codex, plan_codex_plaintext,
    plan_codex_plaintext_with_mode, plan_gemini_cli, plan_gemini_cli_plaintext, plan_opencode,
    plan_opencode_plaintext,
};
pub use utils::{config_backup_path, diff_preview_for_path, endpoint_url, redacted_diff_preview};

#[cfg(test)]
mod tests {
    use super::*;
    use aipass_provider_registry::{AuthScheme, InterfaceType};
    use std::sync::{Mutex, OnceLock};
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

    fn codex_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
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
        let _guard = codex_env_lock().lock().unwrap();
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
    fn codex_writer_reuses_active_provider_and_preserves_custom_fields() {
        let _guard = codex_env_lock().lock().unwrap();
        let dir = tempdir().unwrap();
        let codex_dir = dir.path().join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        std::fs::write(
            codex_dir.join("config.toml"),
            "model_provider = \"openai\"\nmodel = \"old-model\"\n\n[model_providers.openai]\nname = \"My OpenAI\"\nenv_key = \"OLD_KEY\"\ncustom_reasoning = true\n",
        )
        .unwrap();

        let mut entry = entry(InterfaceType::OpenAiCompatible, AuthScheme::Bearer);
        entry.title = "Gateway Production".to_string();
        entry.provider_id = Some("openrouter".to_string());
        entry.env_key = "OPENROUTER_API_KEY".to_string();
        entry.endpoint = Some("https://openrouter.ai/api/v1".to_string());
        entry.default_model = Some("openai/gpt-5".to_string());

        let (plan, content) = plan_codex(dir.path(), &entry).unwrap();
        assert!(content.contains("[model_providers.openai]"));
        assert!(!content.contains("[model_providers.aipass_gateway_production]"));
        assert!(content.contains("name = \"My OpenAI\""));
        assert!(content.contains("custom_reasoning = true"));
        assert!(content.contains("env_key = \"OPENROUTER_API_KEY\""));
        assert!(plan.preview.contains("- env_key = \"OLD_KEY\""));
    }

    #[test]
    fn codex_writer_preserves_experimental_bearer_and_skips_auth_json() {
        let _guard = codex_env_lock().lock().unwrap();
        let dir = tempdir().unwrap();
        let codex_dir = dir.path().join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        std::fs::write(
            codex_dir.join("config.toml"),
            "model_provider = \"gateway\"\n\n[model_providers.gateway]\nname = \"Gateway\"\nexperimental_bearer_token = \"old-secret\"\ncustom_reasoning = true\n",
        )
        .unwrap();

        let mut entry = entry(InterfaceType::OpenAiCompatible, AuthScheme::Bearer);
        entry.title = "Gateway".to_string();
        entry.provider_id = Some("gateway".to_string());
        entry.endpoint = Some("https://gateway.example/v1".to_string());
        entry.api_key = Some("new-secret".to_string());

        let (helper_plan, helper_content) = plan_codex(dir.path(), &entry).unwrap();
        assert!(helper_content.contains("experimental_bearer_token = \"old-secret\""));
        assert!(!helper_content.contains("env_key ="));
        assert!(!helper_plan.preview.contains("old-secret"));

        let (plaintext_plan, plaintext_content) = plan_codex_plaintext_with_mode(
            dir.path(),
            &entry,
            CodexApiKeyMode::ExperimentalBearerToken,
        )
        .unwrap();
        assert!(plaintext_content.contains("experimental_bearer_token = \"new-secret\""));
        assert!(!plaintext_plan.preview.contains("new-secret"));
        assert!(plaintext_plan.extra_writes.is_empty());
    }

    #[test]
    fn codex_experimental_bearer_keeps_oauth_auth_json_unchanged() {
        let _guard = codex_env_lock().lock().unwrap();
        let dir = tempdir().unwrap();
        let codex_dir = dir.path().join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        std::fs::write(
            codex_dir.join("config.toml"),
            "model_provider = \"gateway\"\n\n[model_providers.gateway]\nname = \"Gateway\"\nrequires_openai_auth = true\n",
        )
        .unwrap();
        let oauth = r#"{"auth_mode":"chatgpt","tokens":{"access_token":"oauth-access-secret","refresh_token":"oauth-refresh-secret"}}"#;
        std::fs::write(codex_dir.join("auth.json"), oauth).unwrap();

        let mut entry = entry(InterfaceType::OpenAiCompatible, AuthScheme::Bearer);
        entry.title = "Gateway".to_string();
        entry.provider_id = Some("gateway".to_string());
        entry.api_key = Some("third-party-secret".to_string());

        let (plan, content) = plan_codex_plaintext_with_mode(
            dir.path(),
            &entry,
            CodexApiKeyMode::ExperimentalBearerToken,
        )
        .unwrap();
        assert!(content.contains("experimental_bearer_token = \"third-party-secret\""));
        assert!(content.contains("requires_openai_auth = false"));
        assert!(plan.extra_writes.is_empty());
        assert!(!plan.preview.contains("third-party-secret"));
        assert!(!plan.preview.contains("oauth-access-secret"));

        apply_plan_encrypted(&plan, &content, &[9_u8; aipass_crypto::KEY_LEN]).unwrap();
        assert_eq!(
            std::fs::read_to_string(codex_dir.join("auth.json")).unwrap(),
            oauth
        );
    }

    #[test]
    fn codex_auth_json_mode_replaces_oauth_credentials_and_previews_both_files() {
        let _guard = codex_env_lock().lock().unwrap();
        let dir = tempdir().unwrap();
        let codex_dir = dir.path().join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        std::fs::write(
            codex_dir.join("config.toml"),
            "model_provider = \"gateway\"\n\n[model_providers.gateway]\nname = \"Gateway\"\nrequires_openai_auth = true\n",
        )
        .unwrap();
        let oauth = r#"{"auth_mode":"chatgpt","tokens":{"access_token":"oauth-access-secret","refresh_token":"oauth-refresh-secret"},"last_refresh":"2026-07-20T00:00:00Z"}"#;
        std::fs::write(codex_dir.join("auth.json"), oauth).unwrap();

        let mut entry = entry(InterfaceType::OpenAiCompatible, AuthScheme::Bearer);
        entry.title = "Gateway".to_string();
        entry.provider_id = Some("gateway".to_string());
        entry.api_key = Some("third-party-secret".to_string());

        let (plan, content) =
            plan_codex_plaintext_with_mode(dir.path(), &entry, CodexApiKeyMode::AuthJson).unwrap();
        assert!(content.contains("requires_openai_auth = true"));
        assert!(content.contains("cli_auth_credentials_store = \"file\""));
        assert!(!content.contains("experimental_bearer_token"));
        assert_eq!(plan.extra_writes.len(), 1);
        assert!(plan.preview.contains("config.toml"));
        assert!(plan.preview.contains("auth.json"));
        assert!(!plan.preview.contains("oauth-access-secret"));
        assert!(!plan.preview.contains("oauth-refresh-secret"));
        assert!(!plan.preview.contains("third-party-secret"));

        let auth: serde_json::Value = serde_json::from_str(&plan.extra_writes[0].content).unwrap();
        assert_eq!(
            auth.get("auth_mode").and_then(serde_json::Value::as_str),
            Some("apikey")
        );
        assert_eq!(
            auth.get("OPENAI_API_KEY")
                .and_then(serde_json::Value::as_str),
            Some("third-party-secret")
        );
        assert!(auth.get("tokens").is_none());

        apply_plan_encrypted(&plan, &content, &[9_u8; aipass_crypto::KEY_LEN]).unwrap();
        let applied: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(codex_dir.join("auth.json")).unwrap())
                .unwrap();
        assert_eq!(
            applied.get("auth_mode").and_then(serde_json::Value::as_str),
            Some("apikey")
        );
        assert!(applied.get("tokens").is_none());
        rollback_encrypted(&plan.backup_path, &[9_u8; aipass_crypto::KEY_LEN]).unwrap();
        assert_eq!(
            std::fs::read_to_string(codex_dir.join("auth.json")).unwrap(),
            oauth
        );
    }

    #[test]
    fn codex_provider_rename_updates_profile_references() {
        let _guard = codex_env_lock().lock().unwrap();
        let dir = tempdir().unwrap();
        let codex_dir = dir.path().join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        std::fs::write(
            codex_dir.join("config.toml"),
            "model_provider = \"old-provider\"\n\n[profiles.default]\nmodel_provider = \"old-provider\"\n",
        )
        .unwrap();
        let mut entry = entry(InterfaceType::OpenAiCompatible, AuthScheme::Bearer);
        entry.title = "New Provider".to_string();
        entry.provider_id = None;
        let (_plan, content) = plan_codex(dir.path(), &entry).unwrap();
        assert!(!content.contains("old-provider"));
        assert!(content.contains("model_provider = \"aipass_new_provider\""));
    }

    #[test]
    fn codex_provider_rename_migrates_jsonl_session_metadata() {
        let _guard = codex_env_lock().lock().unwrap();
        let dir = tempdir().unwrap();
        let codex_dir = dir.path().join(".codex");
        let sessions = codex_dir.join("sessions").join("2026");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(
            codex_dir.join("config.toml"),
            "model_provider = \"missing-provider\"\n\n[model_providers.other]\nname = \"Other\"\n",
        )
        .unwrap();
        let session = sessions.join("rollout.jsonl");
        std::fs::write(
            &session,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"thread-1\",\"model_provider\":\"missing-provider\"}}\n{\"type\":\"response\",\"payload\":{\"text\":\"keep this content\"}}\n",
        )
        .unwrap();

        let mut entry = entry(InterfaceType::OpenAiCompatible, AuthScheme::Bearer);
        entry.title = "New Gateway".to_string();
        entry.provider_id = None;
        entry.endpoint = Some("https://gateway.example/v1".to_string());
        entry.env_key = "GATEWAY_API_KEY".to_string();
        let (plan, content) = plan_codex(dir.path(), &entry).unwrap();
        assert!(content.contains("model_provider = \"aipass_new_gateway\""));
        assert_eq!(plan.extra_writes.len(), 1);
        assert!(plan
            .preview
            .contains("missing-provider -> aipass_new_gateway"));

        apply_plan(&plan, &content).unwrap();
        let migrated = std::fs::read_to_string(&session).unwrap();
        assert!(migrated.contains("aipass_new_gateway"));
        assert!(migrated.contains("keep this content"));
        rollback(&plan).unwrap();
        assert!(std::fs::read_to_string(&session)
            .unwrap()
            .contains("missing-provider"));
    }

    #[test]
    fn codex_provider_rename_migrates_sqlite_thread_catalog() {
        let _guard = codex_env_lock().lock().unwrap();
        let dir = tempdir().unwrap();
        let codex_dir = dir.path().join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        std::fs::write(
            codex_dir.join("config.toml"),
            "model_provider = \"missing-provider\"\n",
        )
        .unwrap();
        let database = codex_dir.join("state_5.sqlite");
        let connection = rusqlite::Connection::open(&database).unwrap();
        connection
            .execute(
                "create table threads (id text primary key, model_provider text not null)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "insert into threads (id, model_provider) values ('thread-1', 'missing-provider')",
                [],
            )
            .unwrap();
        drop(connection);

        let mut entry = entry(InterfaceType::OpenAiCompatible, AuthScheme::Bearer);
        entry.title = "SQLite Gateway".to_string();
        entry.provider_id = None;
        entry.endpoint = Some("https://gateway.example/v1".to_string());
        let (plan, content) = plan_codex(dir.path(), &entry).unwrap();
        apply_plan(&plan, &content).unwrap();

        let connection = rusqlite::Connection::open(&database).unwrap();
        let provider: String = connection
            .query_row(
                "select model_provider from threads where id = 'thread-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(provider, "aipass_sqlite_gateway");
        drop(connection);
        rollback(&plan).unwrap();

        let connection = rusqlite::Connection::open(&database).unwrap();
        let restored: String = connection
            .query_row(
                "select model_provider from threads where id = 'thread-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(restored, "missing-provider");
    }

    #[test]
    fn codex_plaintext_writer_writes_auth_json_and_rolls_back() {
        let _guard = codex_env_lock().lock().unwrap();
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
        assert!(auth_text.contains("\"auth_mode\": \"apikey\""));
        assert!(auth_text.contains("OPENAI_API_KEY"));
        assert!(auth_text.contains("sk-test-codex"));

        rollback_encrypted(&plan.backup_path, &[9_u8; aipass_crypto::KEY_LEN]).unwrap();
        assert!(!target.exists());
        assert!(!auth_path.exists());
    }

    #[test]
    fn codex_plaintext_preview_redacts_replaced_auth_value() {
        let _guard = codex_env_lock().lock().unwrap();
        let dir = tempdir().unwrap();
        let codex_dir = dir.path().join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        std::fs::write(
            codex_dir.join("auth.json"),
            "{\"OPENAI_API_KEY\":\"sk-old-secret\",\"other\":true}",
        )
        .unwrap();
        let mut entry = entry(InterfaceType::OpenAiCompatible, AuthScheme::Bearer);
        entry.api_key = Some("sk-new-secret".to_string());

        let (plan, _) = plan_codex_plaintext(dir.path(), &entry).unwrap();
        assert!(plan.preview.contains("auth.json"));
        assert!(!plan.preview.contains("sk-old-secret"));
        assert!(!plan.preview.contains("sk-new-secret"));
    }

    #[test]
    fn codex_writer_uses_codex_home_when_directory_exists() {
        let _guard = codex_env_lock().lock().unwrap();
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
        assert_eq!(
            plan.backup_path,
            codex_home
                .join(".aipass-backups")
                .join("config.toml.aipbackup")
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
    fn gemini_plaintext_preserves_unmanaged_env_values() {
        let dir = tempdir().unwrap();
        let target = dir.path().join(".gemini").join(".env");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(
            &target,
            "# keep this\nOTHER_SETTING=1\nGEMINI_API_KEY=old\n",
        )
        .unwrap();
        let mut entry = entry(InterfaceType::Gemini, AuthScheme::GoogleApiKey);
        entry.api_key = Some("AIza-new".to_string());
        let (_plan, content) = plan_gemini_cli_plaintext(dir.path(), &entry).unwrap();
        assert!(content.contains("# keep this"));
        assert!(content.contains("OTHER_SETTING=1"));
        assert!(content.contains("GEMINI_API_KEY=\"AIza-new\""));
        assert!(!content.contains("GEMINI_API_KEY=old"));
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
    fn opencode_writer_reuses_provider_and_preserves_custom_fields() {
        let dir = tempdir().unwrap();
        let target = dir
            .path()
            .join(".config")
            .join("opencode")
            .join("opencode.json");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(
            &target,
            r#"{
  "$schema": "https://opencode.ai/config.json",
  "provider": {
    "openrouter": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "My Gateway",
      "options": {"apiKey": "old", "headers": {"X-Org": "keep"}},
      "models": {"old-model": {"name": "old-model", "limit": {"context": 1000}}}
    }
  },
  "model": "openrouter/old-model"
}"#,
        )
        .unwrap();
        let mut entry = entry(InterfaceType::OpenAiCompatible, AuthScheme::Bearer);
        entry.provider_id = Some("openrouter".to_string());
        entry.endpoint = Some("https://openrouter.ai/api/v1".to_string());
        entry.default_model = Some("new-model".to_string());
        let (_plan, content) = plan_opencode(dir.path(), &entry).unwrap();
        assert!(content.contains("\"X-Org\": \"keep\""));
        assert!(content.contains("\"old-model\""));
        assert!(content.contains("\"model\": \"openrouter/new-model\""));
        assert!(!content.contains("aipass_anthropic_prod"));
    }

    #[test]
    fn opencode_writer_uses_existing_jsonc_file() {
        let dir = tempdir().unwrap();
        let target = dir
            .path()
            .join(".config")
            .join("opencode")
            .join("opencode.jsonc");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "{ provider: { custom: { options: {} } } }").unwrap();
        let (_plan, _content) = plan_opencode(
            dir.path(),
            &entry(InterfaceType::OpenAiCompatible, AuthScheme::Bearer),
        )
        .unwrap();
        assert_eq!(
            _plan.target_path,
            dir.path()
                .join(".config")
                .join("opencode")
                .join("opencode.jsonc")
        );
    }

    #[test]
    fn writers_reject_non_object_json_configurations() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude").join("settings.json");
        std::fs::create_dir_all(claude.parent().unwrap()).unwrap();
        std::fs::write(&claude, "[]").unwrap();
        assert!(plan_claude_code(
            dir.path(),
            &entry(InterfaceType::AnthropicMessages, AuthScheme::XApiKey)
        )
        .is_err());

        let opencode = dir
            .path()
            .join(".config")
            .join("opencode")
            .join("opencode.json");
        std::fs::create_dir_all(opencode.parent().unwrap()).unwrap();
        std::fs::write(&opencode, "{ \"provider\": [] }").unwrap();
        assert!(plan_opencode(
            dir.path(),
            &entry(InterfaceType::OpenAiCompatible, AuthScheme::Bearer)
        )
        .is_err());
    }

    #[test]
    fn claude_plaintext_writer_supports_bearer_auth() {
        let dir = tempdir().unwrap();
        let mut entry = entry(InterfaceType::AnthropicMessages, AuthScheme::Bearer);
        entry.api_key = Some("bearer-secret".to_string());
        let (_plan, content) = plan_claude_code_plaintext(dir.path(), &entry).unwrap();
        assert!(content.contains("ANTHROPIC_AUTH_TOKEN"));
        assert!(!content.contains("ANTHROPIC_API_KEY"));
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
            codex_provider_migration: None,
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

    #[test]
    fn codex_plaintext_uses_one_stable_backup_per_config_file() {
        let _guard = codex_env_lock().lock().unwrap();
        let dir = tempdir().unwrap();
        let codex_dir = dir.path().join(".codex");
        let mut entry = entry(InterfaceType::OpenAiCompatible, AuthScheme::Bearer);
        entry.endpoint = Some("https://api.openai.com".to_string());
        entry.api_key = Some("sk-test-codex".to_string());

        let (plan, content) = plan_codex_plaintext(dir.path(), &entry).unwrap();
        assert_eq!(
            plan.backup_path,
            codex_dir
                .join(".aipass-backups")
                .join("config.toml.aipbackup")
        );
        assert_eq!(
            plan.extra_writes[0].backup_path,
            codex_dir
                .join(".aipass-backups")
                .join("auth.json.aipbackup")
        );

        apply_plan_encrypted(&plan, &content, &[9_u8; aipass_crypto::KEY_LEN]).unwrap();
        assert_eq!(
            find_backup_by_operation(dir.path(), plan.operation_id).unwrap(),
            plan.extra_writes[0].backup_path
        );
        rollback_encrypted(&plan.backup_path, &[9_u8; aipass_crypto::KEY_LEN]).unwrap();
        assert!(!codex_dir.join("config.toml").exists());
        assert!(!codex_dir.join("auth.json").exists());
    }

    #[test]
    fn encrypted_apply_prunes_legacy_backups_for_same_target() {
        let dir = tempdir().unwrap();
        let target = dir.path().join(".claude").join("settings.json");
        let backup_dir = target.parent().unwrap().join(".aipass-backups");
        let legacy_plan = ConfigPlan {
            operation_id: uuid::Uuid::new_v4(),
            tool: ToolId::ClaudeCode,
            target_path: target.clone(),
            backup_path: backup_dir.join(format!("{}-123.aipbackup", uuid::Uuid::new_v4())),
            summary: "legacy backup".to_string(),
            preview: "{}".to_string(),
            extra_writes: Vec::new(),
            codex_provider_migration: None,
        };
        apply_plan_encrypted(
            &legacy_plan,
            r#"{"env":{"ANTHROPIC_MODEL":"old"}}"#,
            &[7_u8; aipass_crypto::KEY_LEN],
        )
        .unwrap();
        assert!(legacy_plan.backup_path.exists());

        let (plan, content) = plan_claude_code(
            dir.path(),
            &entry(InterfaceType::AnthropicMessages, AuthScheme::XApiKey),
        )
        .unwrap();
        apply_plan_encrypted(&plan, &content, &[7_u8; aipass_crypto::KEY_LEN]).unwrap();

        assert_eq!(plan.backup_path, backup_dir.join("settings.json.aipbackup"));
        assert!(plan.backup_path.exists());
        assert_eq!(
            find_backup_by_operation(dir.path(), plan.operation_id).unwrap(),
            plan.backup_path
        );
        assert!(!legacy_plan.backup_path.exists());
        let backup_count = std::fs::read_dir(&backup_dir)
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .and_then(|value| value.to_str())
                    == Some("aipbackup")
            })
            .count();
        assert_eq!(backup_count, 1);
    }
}
