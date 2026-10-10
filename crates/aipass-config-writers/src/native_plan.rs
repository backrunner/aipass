//! Native transactions retain custom provider IDs without rewriting live history.
use crate::{native_auth::Resource, ConfigPlan};
use anyhow::{Context, Result};
use toml_edit::{DocumentMut, Item, Table};

pub fn prepare_native_codex_plan(plan: &mut ConfigPlan, content: &str) -> Result<String> {
    let raw = Resource::File(plan.target_path.clone()).read()?;
    let before: DocumentMut = raw
        .as_deref()
        .map(|v| std::str::from_utf8(v))
        .transpose()?
        .unwrap_or("")
        .parse()?;
    let mut after: DocumentMut = content.parse()?;
    // This path retains provider IDs rather than migrating live session files.
    plan.codex_provider_migration = None;
    if let Some(old) = before
        .get("model_provider")
        .and_then(Item::as_str)
        .filter(|id| {
            !matches!(
                *id,
                "openai" | "ollama" | "lmstudio" | "amazon_bedrock" | "amazon_bedrock_runtime"
            )
        })
    {
        let new = after
            .get("model_provider")
            .and_then(Item::as_str)
            .unwrap_or(old)
            .to_owned();
        if old != new {
            let providers = after
                .get_mut("model_providers")
                .and_then(Item::as_table_mut)
                .context("Codex provider configuration unavailable")?;
            let mut updated = providers
                .remove(&new)
                .context("Codex provider configuration unavailable")?;
            if let (Some(original), Some(updated)) = (
                before
                    .get("model_providers")
                    .and_then(|p| p.get(old))
                    .and_then(Item::as_table),
                updated.as_table_mut(),
            ) {
                // The canonical provider may already exist with another account's
                // retry limits/preferences. Preserve the active provider's own settings.
                for (key, value) in original {
                    if !matches!(
                        key,
                        "name"
                            | "base_url"
                            | "env_key"
                            | "env_key_instructions"
                            | "experimental_bearer_token"
                            | "auth"
                            | "requires_openai_auth"
                            | "wire_api"
                            | "supports_websockets"
                    ) {
                        updated.insert(key, value.clone());
                    }
                }
            }
            providers.insert(old, updated);
            // Retain an unrelated canonical provider that existed before the plan.
            if let Some(original) = before.get("model_providers").and_then(|v| v.get(&new)) {
                providers.insert(&new, original.clone());
            }
            restore_references(before.as_table(), after.as_table_mut(), old);
            after["model_provider"] = toml_edit::value(old);
        }
    }
    let selected = after
        .get("model_provider")
        .and_then(Item::as_str)
        .map(str::to_owned);
    if let Some(provider) = selected.and_then(|id| {
        after
            .get_mut("model_providers")
            .and_then(|p| p.get_mut(&id))
            .and_then(Item::as_table_mut)
    }) {
        for field in ["http_headers", "env_http_headers"] {
            if let Some(headers) = provider.get_mut(field).and_then(Item::as_table_like_mut) {
                let keys = headers
                    .iter()
                    .filter(|(key, _)| {
                        matches!(
                            key.to_ascii_lowercase().as_str(),
                            "authorization" | "x-api-key" | "api-key"
                        )
                    })
                    .map(|(key, _)| key.to_owned())
                    .collect::<Vec<_>>();
                for key in keys {
                    headers.remove(&key);
                }
            }
        }
    }
    after.remove("forced_login_method");
    after.remove("forced_chatgpt_workspace_id");
    Ok(after.to_string())
}
fn restore_references(before: &Table, after: &mut Table, old: &str) {
    for (key, value) in before.iter() {
        if key == "model_provider" && value.as_str() == Some(old) {
            after.insert(key, value.clone());
        } else if let (Some(before), Some(after)) = (
            value.as_table(),
            after.get_mut(key).and_then(Item::as_table_mut),
        ) {
            restore_references(before, after, old);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{preview_codex_plaintext_with_mode, CodexApiKeyMode, ToolEntry};
    use aipass_provider_registry::{AuthScheme, InterfaceType};
    #[test]
    fn native_switch_keeps_history_provider_and_removes_conflicting_auth_headers() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join(".codex")).unwrap();
        std::fs::write(home.path().join(".codex/config.toml"),
            "model_provider = 'legacy'\nforced_login_method = 'chatgpt'\n[model_providers.legacy]\nbase_url = 'https://old.test'\nrequest_max_retries = 23\n[model_providers.legacy.http_headers]\nAuthorization = 'fake-private-token'\nX-Trace = 'keep'\n[model_providers.aipass]\nrequest_max_retries = 99\n[profiles.work]\nmodel_provider = 'legacy'\n").unwrap();
        let entry = ToolEntry {
            id: uuid::Uuid::new_v4(),
            secret_id: None,
            title: "Fixture".into(),
            provider_id: None,
            endpoint: Some("https://new.test/v1".into()),
            interface_type: InterfaceType::OpenAiCompatible,
            auth_scheme: AuthScheme::Bearer,
            env_key: "KEY".into(),
            default_model: None,
            api_key: Some("fake-new-key".into()),
            supports_websockets: None,
        };
        let (mut plan, content) =
            preview_codex_plaintext_with_mode(home.path(), &entry, CodexApiKeyMode::AuthJson)
                .unwrap();
        let output = prepare_native_codex_plan(&mut plan, &content).unwrap();
        let doc: DocumentMut = output.parse().unwrap();
        assert_eq!(doc["model_provider"].as_str(), Some("legacy"));
        assert_eq!(
            doc["profiles"]["work"]["model_provider"].as_str(),
            Some("legacy")
        );
        assert!(output.contains("https://new.test/v1"));
        assert!(output.contains("keep"));
        assert!(!output.contains("fake-private-token"));
        assert!(!output.contains("forced_login_method"));
        assert_eq!(
            doc["model_providers"]["legacy"]["request_max_retries"].as_integer(),
            Some(23)
        );
        assert_eq!(
            doc["model_providers"]["aipass"]["request_max_retries"].as_integer(),
            Some(99)
        );
        assert!(plan.codex_provider_migration.is_none());
    }

    #[test]
    fn built_in_provider_ids_are_never_used_for_custom_api_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        for id in [
            "openai",
            "ollama",
            "lmstudio",
            "amazon_bedrock",
            "amazon_bedrock_runtime",
        ] {
            std::fs::write(&path, format!("model_provider = '{id}'\n")).unwrap();
            let mut plan = crate::utils::new_plan(
                crate::ToolId::Codex,
                path.clone(),
                "Fixture".into(),
                String::new(),
            );
            let out = prepare_native_codex_plan(&mut plan, "model_provider = 'aipass'\n[model_providers.aipass]\nbase_url = 'https://selected.test/v1'\n").unwrap();
            let doc: DocumentMut = out.parse().unwrap();
            assert_eq!(doc["model_provider"].as_str(), Some("aipass"));
            assert!(out.contains("https://selected.test/v1"));
        }
    }
}
