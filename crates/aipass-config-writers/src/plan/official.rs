use super::*;
use std::path::PathBuf;

/// Configure Codex to use the credentials managed by the official CLI.
/// Existing auth.json is deliberately left untouched so an API-key switch can
/// be reversed without destroying the user's OAuth session.
pub fn plan_codex_official(home: &Path, entry: &ToolEntry) -> Result<(ConfigPlan, String)> {
    plan_codex_official_with_history(home, entry, true)
}

pub fn preview_codex_official(home: &Path, entry: &ToolEntry) -> Result<(ConfigPlan, String)> {
    plan_codex_official_with_history(home, entry, false)
}

fn plan_codex_official_with_history(
    home: &Path,
    entry: &ToolEntry,
    scan_history: bool,
) -> Result<(ConfigPlan, String)> {
    ensure_codex_entry(entry)?;
    let codex_dir = resolve_codex_dir(home);
    let target = codex_dir.join("config.toml");
    let before = fs::read_to_string(&target).unwrap_or_default();
    let mut doc = read_toml(&target)?;
    let (provider_name, provider_migration) = codex_provider_selection(&doc);
    if let Some(from_provider) = provider_migration.as_deref() {
        rename_codex_provider_block(&mut doc, from_provider, &provider_name);
    }
    doc.remove("forced_login_method");
    doc.remove("forced_chatgpt_workspace_id");
    update_codex_provider(
        &mut doc,
        &provider_name,
        entry,
        None,
        Some(CodexAuthMode::Official),
    )?;
    if let Some(from_provider) = provider_migration.as_deref() {
        replace_codex_provider_references(&mut doc, from_provider, &provider_name);
    }
    doc.remove("model");
    let content = doc.to_string();
    let secret_redactions = codex_secret_values(&before);
    let secret_redaction_refs = secret_redactions
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let mut plan = new_plan(
        ToolId::Codex,
        target.clone(),
        format!(
            "Configure Codex official OAuth subscription for {}",
            entry.title
        ),
        redacted_diff_preview(
            &diff_preview_from(&before, &content),
            &secret_redaction_refs,
        ),
    );
    append_codex_migration(
        &codex_dir,
        &mut plan,
        provider_migration.as_deref(),
        &provider_name,
        scan_history,
    )?;
    Ok((plan, content))
}

pub fn plan_claude_code(home: &Path, entry: &ToolEntry) -> Result<(ConfigPlan, String)> {
    ensure_claude_code_entry(entry)?;
    let target = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".claude"))
        .join("settings.json");
    let mut json = read_json_object(&target)?;
    json.insert(
        "apiKeyHelper".to_string(),
        Value::String(entry.credential_command()),
    );
    json.remove("anthropicBaseUrl");
    json.remove("forceLoginMethod");
    let env = ensure_json_object(&mut json, "env")?;
    for key in [
        "CLAUDE_CODE_OAUTH_TOKEN",
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX",
        "CLAUDE_CODE_USE_FOUNDRY",
    ] {
        env.remove(key);
    }
    env.remove("ANTHROPIC_API_KEY");
    env.remove("ANTHROPIC_AUTH_TOKEN");
    if let Some(endpoint) = &entry.endpoint {
        env.insert(
            "ANTHROPIC_BASE_URL".to_string(),
            Value::String(claude_code_base_url(endpoint)),
        );
    } else {
        env.remove("ANTHROPIC_BASE_URL");
    }
    if let Some(model) = &entry.default_model {
        env.insert("ANTHROPIC_MODEL".to_string(), Value::String(model.clone()));
    } else {
        env.remove("ANTHROPIC_MODEL");
    }
    let content = serde_json::to_string_pretty(&json)?;
    let plan = new_plan(
        ToolId::ClaudeCode,
        target.clone(),
        format!("Configure Claude Code to use {}", entry.title),
        redacted_diff_preview(&diff_preview_for_path(&target, &content), &[]),
    );
    Ok((plan, content))
}

/// Remove API-key overrides and let Claude Code use its native OAuth store.
pub fn plan_claude_code_official(home: &Path, entry: &ToolEntry) -> Result<(ConfigPlan, String)> {
    ensure_claude_code_entry(entry)?;
    let target = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".claude"))
        .join("settings.json");
    let mut json = read_json_object(&target)?;
    json.remove("apiKeyHelper");
    json.remove("anthropicBaseUrl");
    json.remove("forceLoginMethod");
    let env = ensure_json_object(&mut json, "env")?;
    for key in [
        "CLAUDE_CODE_OAUTH_TOKEN",
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX",
        "CLAUDE_CODE_USE_FOUNDRY",
    ] {
        env.remove(key);
    }
    env.remove("ANTHROPIC_API_KEY");
    env.remove("ANTHROPIC_AUTH_TOKEN");
    env.remove("ANTHROPIC_BASE_URL");
    for key in [
        "ANTHROPIC_MODEL",
        "ANTHROPIC_SMALL_FAST_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "CLAUDE_CODE_OAUTH_TOKEN",
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX",
        "CLAUDE_CODE_USE_FOUNDRY",
    ] {
        env.remove(key);
    }
    json.remove("model");
    json.remove("forceLoginMethod");
    let content = serde_json::to_string_pretty(&json)?;
    let plan = new_plan(
        ToolId::ClaudeCode,
        target.clone(),
        format!(
            "Configure Claude Code official OAuth subscription for {}",
            entry.title
        ),
        redacted_diff_preview(&diff_preview_for_path(&target, &content), &[]),
    );
    Ok((plan, content))
}

pub fn plan_claude_code_plaintext(home: &Path, entry: &ToolEntry) -> Result<(ConfigPlan, String)> {
    ensure_claude_code_entry(entry)?;
    let target = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".claude"))
        .join("settings.json");
    let mut json = read_json_object(&target)?;
    let api_key = entry
        .api_key
        .as_deref()
        .context("plaintext Claude Code config requires an API key")?;
    json.remove("apiKeyHelper");
    json.remove("anthropicBaseUrl");
    json.remove("forceLoginMethod");
    let env = ensure_json_object(&mut json, "env")?;
    for key in [
        "CLAUDE_CODE_OAUTH_TOKEN",
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX",
        "CLAUDE_CODE_USE_FOUNDRY",
    ] {
        env.remove(key);
    }
    let auth_key = match entry.auth_scheme {
        AuthScheme::XApiKey => "ANTHROPIC_API_KEY",
        AuthScheme::Bearer => "ANTHROPIC_AUTH_TOKEN",
        _ => unreachable!("ensure_claude_code_entry validates the auth scheme"),
    };
    env.insert(auth_key.to_string(), Value::String(api_key.to_string()));
    if auth_key != "ANTHROPIC_API_KEY" {
        env.remove("ANTHROPIC_API_KEY");
    }
    if auth_key != "ANTHROPIC_AUTH_TOKEN" {
        env.remove("ANTHROPIC_AUTH_TOKEN");
    }
    if let Some(endpoint) = &entry.endpoint {
        env.insert(
            "ANTHROPIC_BASE_URL".to_string(),
            Value::String(claude_code_base_url(endpoint)),
        );
    } else {
        env.remove("ANTHROPIC_BASE_URL");
    }
    if let Some(model) = &entry.default_model {
        env.insert("ANTHROPIC_MODEL".to_string(), Value::String(model.clone()));
    } else {
        env.remove("ANTHROPIC_MODEL");
    }
    let content = serde_json::to_string_pretty(&json)?;
    let plan = new_plan(
        ToolId::ClaudeCode,
        target.clone(),
        format!(
            "Configure Claude Code plaintext credentials for {}",
            entry.title
        ),
        redacted_diff_preview(&diff_preview_for_path(&target, &content), &[api_key]),
    );
    Ok((plan, content))
}
