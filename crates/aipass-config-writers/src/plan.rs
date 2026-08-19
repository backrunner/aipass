use crate::models::{
    CodexApiKeyMode, CodexProviderMigration, ConfigPlan, PlannedWrite, ToolEntry, ToolId,
};
use crate::utils::{
    config_backup_path, diff_preview_for_path, diff_preview_from, dotenv_quote, ensure_json_object,
    ensure_table, new_plan, read_json_object, read_json_value, read_toml, redacted_diff_preview,
    resolve_codex_dir, slug,
};
use aipass_provider_registry::{AuthScheme, InterfaceType};
use anyhow::{Context, Result};
use serde_json::{Map, Value};
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;
use toml_edit::{value, DocumentMut, Item, Table};

pub fn plan_codex(home: &Path, entry: &ToolEntry) -> Result<(ConfigPlan, String)> {
    ensure_codex_entry(entry)?;
    let codex_dir = resolve_codex_dir(home);
    let target = codex_dir.join("config.toml");
    let before = fs::read_to_string(&target).unwrap_or_default();
    let mut doc = read_toml(&target)?;
    let (provider_name, provider_migration) = codex_provider_selection(&doc, entry);
    update_codex_provider(&mut doc, &provider_name, entry, None, None)?;
    if let Some(from_provider) = provider_migration.as_deref() {
        replace_codex_provider_references(&mut doc, from_provider, &provider_name);
    }
    let content = doc.to_string();
    let mut plan = new_plan(
        ToolId::Codex,
        target.clone(),
        format!("Configure Codex env-based config to use {}", entry.title),
        redacted_diff_preview(&diff_preview_from(&before, &content), &[]),
    );
    append_codex_migration(
        &codex_dir,
        &mut plan,
        provider_migration.as_deref(),
        &provider_name,
    )?;
    Ok((plan, content))
}

pub fn plan_codex_plaintext(home: &Path, entry: &ToolEntry) -> Result<(ConfigPlan, String)> {
    plan_codex_plaintext_with_mode(home, entry, CodexApiKeyMode::AuthJson)
}

pub fn plan_codex_plaintext_with_mode(
    home: &Path,
    entry: &ToolEntry,
    api_key_mode: CodexApiKeyMode,
) -> Result<(ConfigPlan, String)> {
    ensure_codex_entry(entry)?;
    let api_key = entry
        .api_key
        .as_deref()
        .context("plaintext Codex config requires an API key")?;
    let codex_dir = resolve_codex_dir(home);
    let target = codex_dir.join("config.toml");
    let before = fs::read_to_string(&target).unwrap_or_default();
    let mut doc = read_toml(&target)?;
    let (provider_name, provider_migration) = codex_provider_selection(&doc, entry);
    let auth_mode = update_codex_provider(
        &mut doc,
        &provider_name,
        entry,
        Some(api_key),
        Some(match api_key_mode {
            CodexApiKeyMode::ExperimentalBearerToken => CodexAuthMode::ExperimentalBearer,
            CodexApiKeyMode::AuthJson => CodexAuthMode::AuthJson,
        }),
    )?;
    if let Some(from_provider) = provider_migration.as_deref() {
        replace_codex_provider_references(&mut doc, from_provider, &provider_name);
    }
    let content = doc.to_string();
    let config_preview = redacted_diff_preview(&diff_preview_from(&before, &content), &[api_key]);
    let mut plan = new_plan(
        ToolId::Codex,
        target.clone(),
        if matches!(auth_mode, CodexAuthMode::AuthJson) {
            format!(
                "Configure Codex live config (config.toml + auth.json) to use {}",
                entry.title
            )
        } else if matches!(auth_mode, CodexAuthMode::ExperimentalBearer) {
            format!(
                "Configure Codex config.toml with experimental_bearer_token for {}",
                entry.title
            )
        } else {
            format!(
                "Configure Codex live config (config.toml; preserving provider auth) to use {}",
                entry.title
            )
        },
        config_preview,
    );
    if matches!(auth_mode, CodexAuthMode::AuthJson) {
        let auth_target = codex_dir.join("auth.json");
        let auth_before = fs::read_to_string(&auth_target).unwrap_or_default();
        let auth_before_value = read_json_value(&auth_target)?;
        auth_before_value
            .as_object()
            .context("Codex auth.json must contain a JSON object")?;
        let auth_content = format!(
            "{}\n",
            serde_json::to_string_pretty(&serde_json::json!({
                "auth_mode": "apikey",
                "OPENAI_API_KEY": api_key,
            }))?
        );
        let mut auth_redactions = Vec::new();
        collect_json_strings(&auth_before_value, &mut auth_redactions);
        auth_redactions.push(api_key.to_string());
        let auth_redaction_refs = auth_redactions
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        plan.preview = combine_previews([
            (&target, plan.preview),
            (
                &auth_target,
                redacted_diff_preview(
                    &diff_preview_from(&auth_before, &auth_content),
                    &auth_redaction_refs,
                ),
            ),
        ]);
        plan.extra_writes.push(PlannedWrite {
            backup_path: config_backup_path(&auth_target),
            target_path: auth_target,
            content: auth_content,
        });
    }
    append_codex_migration(
        &codex_dir,
        &mut plan,
        provider_migration.as_deref(),
        &provider_name,
    )?;
    Ok((plan, content))
}

pub fn plan_claude_code(home: &Path, entry: &ToolEntry) -> Result<(ConfigPlan, String)> {
    ensure_claude_code_entry(entry)?;
    let target = home.join(".claude").join("settings.json");
    let mut json = read_json_object(&target)?;
    json.insert(
        "apiKeyHelper".to_string(),
        Value::String(format!("aipass get {} --field api_key --reveal", entry.id)),
    );
    json.remove("anthropicBaseUrl");
    let env = ensure_json_object(&mut json, "env")?;
    env.remove("ANTHROPIC_API_KEY");
    env.remove("ANTHROPIC_AUTH_TOKEN");
    if let Some(endpoint) = &entry.endpoint {
        env.insert(
            "ANTHROPIC_BASE_URL".to_string(),
            Value::String(endpoint.clone()),
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

pub fn plan_claude_code_plaintext(home: &Path, entry: &ToolEntry) -> Result<(ConfigPlan, String)> {
    ensure_claude_code_entry(entry)?;
    let target = home.join(".claude").join("settings.json");
    let mut json = read_json_object(&target)?;
    let api_key = entry
        .api_key
        .as_deref()
        .context("plaintext Claude Code config requires an API key")?;
    json.remove("apiKeyHelper");
    json.remove("anthropicBaseUrl");
    let env = ensure_json_object(&mut json, "env")?;
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
            Value::String(endpoint.clone()),
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

pub fn plan_gemini_cli(home: &Path, entry: &ToolEntry) -> Result<(ConfigPlan, String)> {
    ensure_gemini_entry(entry)?;
    let target = home.join(".aipass").join("tools").join("gemini-cli.env");
    let mut content = format!(
        "# Generated by AIPass. Source this file before running Gemini CLI, or use `aipass exec {id} -- gemini`.\nexport GEMINI_API_KEY=\"$(aipass get {id} --field api_key --reveal)\"\n",
        id = entry.id,
    );
    if let Some(endpoint) = &entry.endpoint {
        content.push_str(&format!(
            "export GOOGLE_GEMINI_BASE_URL={}\n",
            dotenv_quote(endpoint)
        ));
    }
    if let Some(model) = &entry.default_model {
        content.push_str(&format!("export GEMINI_MODEL={}\n", dotenv_quote(model)));
    }
    let plan = new_plan(
        ToolId::GeminiCli,
        target.clone(),
        format!("Create Gemini CLI helper env for {}", entry.title),
        redacted_diff_preview(&diff_preview_for_path(&target, &content), &[]),
    );
    Ok((plan, content))
}

pub fn plan_gemini_cli_plaintext(home: &Path, entry: &ToolEntry) -> Result<(ConfigPlan, String)> {
    ensure_gemini_entry(entry)?;
    let target = home.join(".gemini").join(".env");
    let before = fs::read_to_string(&target).unwrap_or_default();
    let api_key = entry
        .api_key
        .as_deref()
        .context("plaintext Gemini CLI config requires an API key")?;
    let mut updates = vec![("GEMINI_API_KEY", dotenv_quote(api_key))];
    let mut removals = Vec::new();
    if let Some(endpoint) = &entry.endpoint {
        updates.push(("GOOGLE_GEMINI_BASE_URL", dotenv_quote(endpoint)));
    } else {
        removals.push("GOOGLE_GEMINI_BASE_URL");
    }
    if let Some(model) = &entry.default_model {
        updates.push(("GEMINI_MODEL", dotenv_quote(model)));
    } else {
        removals.push("GEMINI_MODEL");
    }
    let content = update_dotenv_content(&before, &updates, &removals);
    let plan = new_plan(
        ToolId::GeminiCli,
        target.clone(),
        format!("Configure Gemini CLI plaintext env for {}", entry.title),
        redacted_diff_preview(&diff_preview_for_path(&target, &content), &[api_key]),
    );
    Ok((plan, content))
}

/// Configure Cursor's separately distributed local runtime.
///
/// The regular `cursor-agent` binary intentionally ignores model API settings;
/// `cursor-agent-local` reads these two variables instead. Keep this writer
/// separate from the normal Cursor integration so the generated instructions
/// cannot accidentally make a standard Cursor install appear local-aware.
pub fn plan_cursor_local(home: &Path, entry: &ToolEntry) -> Result<(ConfigPlan, String)> {
    plan_cursor_local_with_api_key(home, entry, None)
}

pub fn plan_cursor_local_plaintext(home: &Path, entry: &ToolEntry) -> Result<(ConfigPlan, String)> {
    let api_key = entry
        .api_key
        .as_deref()
        .context("plaintext Cursor local config requires an API key")?;
    plan_cursor_local_with_api_key(home, entry, Some(api_key))
}

fn plan_cursor_local_with_api_key(
    home: &Path,
    entry: &ToolEntry,
    api_key: Option<&str>,
) -> Result<(ConfigPlan, String)> {
    ensure_cursor_local_entry(entry)?;
    let endpoint_owned = cursor_local_endpoint(entry);
    let endpoint = endpoint_owned
        .as_deref()
        .context("Cursor local config requires an API endpoint")?;
    let target = home
        .join(".aipass")
        .join("tools")
        .join("cursor-agent-local.env");
    let mut content = String::from(
        "# Generated by AIPass. Use this file with Cursor's separate local runtime:\n# source ~/.aipass/tools/cursor-agent-local.env\n# cursor-agent-local --authless --model <model>\n",
    );
    content.push_str(&format!(
        "export CURSOR_LOCAL_AGENT_BASE_URL={}\n",
        dotenv_quote(endpoint)
    ));
    match api_key {
        Some(api_key) => content.push_str(&format!(
            "export CURSOR_LOCAL_AGENT_API_KEY={}\n",
            dotenv_quote(api_key)
        )),
        None => content.push_str(&format!(
            "export CURSOR_LOCAL_AGENT_API_KEY=\"$(aipass get {} --field api_key --reveal)\"\n",
            entry.id
        )),
    }
    content.push_str("export CURSOR_ENABLE_AUTHLESS=1\n");
    let redactions = api_key.into_iter().collect::<Vec<_>>();
    let plan = new_plan(
        ToolId::Cursor,
        target.clone(),
        format!(
            "Configure Cursor Agent Local env helper for {}",
            entry.title
        ),
        redacted_diff_preview(&diff_preview_for_path(&target, &content), &redactions),
    );
    Ok((plan, content))
}

fn cursor_local_endpoint(entry: &ToolEntry) -> Option<String> {
    let endpoint = entry.endpoint.as_deref()?.trim_end_matches('/');
    if matches!(entry.interface_type, InterfaceType::AnthropicMessages) {
        if endpoint.ends_with("/messages") {
            Some(endpoint.to_string())
        } else if endpoint.ends_with("/v1") {
            Some(format!("{endpoint}/messages"))
        } else {
            Some(format!("{endpoint}/v1/messages"))
        }
    } else {
        Some(endpoint.to_string())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenCodeApi {
    OpenAiResponses,
    OpenAiChatCompletions,
    AnthropicMessages,
}

impl OpenCodeApi {
    fn npm_package(self) -> &'static str {
        match self {
            Self::OpenAiResponses => "@ai-sdk/openai",
            Self::OpenAiChatCompletions => "@ai-sdk/openai-compatible",
            Self::AnthropicMessages => "@ai-sdk/anthropic",
        }
    }
}

pub fn plan_opencode(home: &Path, entry: &ToolEntry) -> Result<(ConfigPlan, String)> {
    plan_opencode_with_api(home, entry, None)
}

fn plan_opencode_with_api(
    home: &Path,
    entry: &ToolEntry,
    api: Option<OpenCodeApi>,
) -> Result<(ConfigPlan, String)> {
    let target = opencode_config_path(home);
    let mut root = read_root_object(&target)?;
    let provider_id = opencode_provider_selection(&root, entry);
    if !root.contains_key("$schema") {
        root.insert(
            "$schema".to_string(),
            Value::String("https://opencode.ai/config.json".to_string()),
        );
    }
    update_opencode_provider(
        &mut root,
        &provider_id,
        entry,
        Value::String(format!("{{env:{}}}", entry.env_key)),
        api,
    )?;
    if let Some(model) = &entry.default_model {
        root.insert(
            "model".to_string(),
            Value::String(format!("{provider_id}/{model}")),
        );
    }
    let content = serde_json::to_string_pretty(&root)?;
    let plan = new_plan(
        ToolId::OpenCode,
        target.clone(),
        format!("Configure OpenCode to use {}", entry.title),
        redacted_diff_preview(&diff_preview_for_path(&target, &content), &[]),
    );
    Ok((plan, content))
}

pub fn plan_opencode_plaintext(home: &Path, entry: &ToolEntry) -> Result<(ConfigPlan, String)> {
    plan_opencode_plaintext_with_optional_api(home, entry, None)
}

pub fn plan_opencode_plaintext_with_api(
    home: &Path,
    entry: &ToolEntry,
    api: OpenCodeApi,
) -> Result<(ConfigPlan, String)> {
    plan_opencode_plaintext_with_optional_api(home, entry, Some(api))
}

fn plan_opencode_plaintext_with_optional_api(
    home: &Path,
    entry: &ToolEntry,
    api: Option<OpenCodeApi>,
) -> Result<(ConfigPlan, String)> {
    let target = opencode_config_path(home);
    let api_key = entry
        .api_key
        .as_deref()
        .context("plaintext OpenCode config requires an API key")?;
    let mut root = read_root_object(&target)?;
    let provider_id = opencode_provider_selection(&root, entry);
    if !root.contains_key("$schema") {
        root.insert(
            "$schema".to_string(),
            Value::String("https://opencode.ai/config.json".to_string()),
        );
    }
    update_opencode_provider(
        &mut root,
        &provider_id,
        entry,
        Value::String(api_key.to_string()),
        api,
    )?;
    if let Some(model) = &entry.default_model {
        root.insert(
            "model".to_string(),
            Value::String(format!("{provider_id}/{model}")),
        );
    }
    let content = serde_json::to_string_pretty(&root)?;
    let plan = new_plan(
        ToolId::OpenCode,
        target.clone(),
        format!(
            "Configure OpenCode plaintext credentials for {}",
            entry.title
        ),
        redacted_diff_preview(&diff_preview_for_path(&target, &content), &[api_key]),
    );
    Ok((plan, content))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrokApiBackend {
    ChatCompletions,
    Responses,
    Messages,
}

impl GrokApiBackend {
    fn config_value(self) -> &'static str {
        match self {
            Self::ChatCompletions => "chat_completions",
            Self::Responses => "responses",
            Self::Messages => "messages",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PiApi {
    OpenAiCompletions,
    OpenAiResponses,
    AnthropicMessages,
}

impl PiApi {
    fn config_value(self) -> &'static str {
        match self {
            Self::OpenAiCompletions => "openai-completions",
            Self::OpenAiResponses => "openai-responses",
            Self::AnthropicMessages => "anthropic-messages",
        }
    }
}

pub fn plan_grok(home: &Path, entry: &ToolEntry) -> Result<(ConfigPlan, String)> {
    plan_grok_with_backend(home, entry, grok_backend_for_entry(entry), None)
}

pub fn plan_grok_plaintext(home: &Path, entry: &ToolEntry) -> Result<(ConfigPlan, String)> {
    plan_grok_with_backend(
        home,
        entry,
        grok_backend_for_entry(entry),
        Some(
            entry
                .api_key
                .as_deref()
                .context("plaintext Grok config requires an API key")?,
        ),
    )
}

pub fn plan_grok_plaintext_with_backend(
    home: &Path,
    entry: &ToolEntry,
    backend: GrokApiBackend,
) -> Result<(ConfigPlan, String)> {
    plan_grok_with_backend(
        home,
        entry,
        backend,
        Some(
            entry
                .api_key
                .as_deref()
                .context("plaintext Grok config requires an API key")?,
        ),
    )
}

fn plan_grok_with_backend(
    home: &Path,
    entry: &ToolEntry,
    backend: GrokApiBackend,
    api_key: Option<&str>,
) -> Result<(ConfigPlan, String)> {
    let endpoint = entry
        .endpoint
        .as_deref()
        .context("Grok config requires an API endpoint")?;
    let model = entry
        .default_model
        .as_deref()
        .context("Grok config requires a default model")?;
    let target = home.join(".grok").join("config.toml");
    let before = fs::read_to_string(&target).unwrap_or_default();
    let mut doc = read_toml(&target)?;
    let config_id = aipass_config_id(entry);
    let mut redactions = Vec::new();
    let model_configs = ensure_table(&mut doc, "model")?;
    let config = model_configs
        .entry(&config_id)
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_mut()
        .with_context(|| format!("model.{config_id} must be a TOML table"))?;
    if let Some(existing) = config.get("api_key").and_then(Item::as_str) {
        redactions.push(existing.to_string());
    }
    config.insert("model", value(model));
    config.insert("base_url", value(endpoint));
    config.insert("name", value(entry.title.clone()));
    config.insert("api_backend", value(backend.config_value()));
    if let Some(api_key) = api_key {
        config.insert("api_key", value(api_key));
        config.remove("env_key");
        redactions.push(api_key.to_string());
    } else {
        config.insert("env_key", value(entry.env_key.clone()));
        config.remove("api_key");
    }
    ensure_table(&mut doc, "models")?.insert("default", value(config_id));
    let content = doc.to_string();
    let redaction_refs = redactions.iter().map(String::as_str).collect::<Vec<_>>();
    let plan = new_plan(
        ToolId::Grok,
        target,
        format!("Configure Grok to use {}", entry.title),
        redacted_diff_preview(&diff_preview_from(&before, &content), &redaction_refs),
    );
    Ok((plan, content))
}

pub fn plan_pi(home: &Path, entry: &ToolEntry) -> Result<(ConfigPlan, String)> {
    plan_pi_with_api(home, entry, pi_api_for_entry(entry), None)
}

pub fn plan_pi_plaintext(home: &Path, entry: &ToolEntry) -> Result<(ConfigPlan, String)> {
    plan_pi_with_api(
        home,
        entry,
        pi_api_for_entry(entry),
        Some(
            entry
                .api_key
                .as_deref()
                .context("plaintext Pi config requires an API key")?,
        ),
    )
}

pub fn plan_pi_plaintext_with_api(
    home: &Path,
    entry: &ToolEntry,
    api: PiApi,
) -> Result<(ConfigPlan, String)> {
    plan_pi_with_api(
        home,
        entry,
        api,
        Some(
            entry
                .api_key
                .as_deref()
                .context("plaintext Pi config requires an API key")?,
        ),
    )
}

fn plan_pi_with_api(
    home: &Path,
    entry: &ToolEntry,
    api: PiApi,
    api_key: Option<&str>,
) -> Result<(ConfigPlan, String)> {
    let endpoint = entry
        .endpoint
        .as_deref()
        .context("Pi config requires an API endpoint")?;
    let model = entry
        .default_model
        .as_deref()
        .context("Pi config requires a default model")?;
    let agent_dir = home.join(".pi").join("agent");
    let target = agent_dir.join("models.json");
    let mut root = read_json_object(&target)?;
    let provider_id = aipass_config_id(entry);
    let providers = ensure_json_object(&mut root, "providers")?;
    let provider = providers
        .entry(provider_id.clone())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .with_context(|| format!("providers.{provider_id} must be a JSON object"))?;
    let mut redactions = provider
        .get("apiKey")
        .and_then(Value::as_str)
        .map(str::to_string)
        .into_iter()
        .collect::<Vec<_>>();
    provider.insert("name".to_string(), Value::String(entry.title.clone()));
    provider.insert("baseUrl".to_string(), Value::String(endpoint.to_string()));
    provider.insert(
        "api".to_string(),
        Value::String(api.config_value().to_string()),
    );
    if let Some(api_key) = api_key {
        provider.insert("apiKey".to_string(), Value::String(api_key.to_string()));
        redactions.push(api_key.to_string());
    } else {
        provider.insert(
            "apiKey".to_string(),
            Value::String(format!("${}", entry.env_key)),
        );
    }
    let models = provider
        .entry("models".to_string())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .with_context(|| format!("providers.{provider_id}.models must be a JSON array"))?;
    let model_config = if let Some(index) = models.iter().position(|item| {
        item.as_object()
            .and_then(|item| item.get("id"))
            .and_then(Value::as_str)
            == Some(model)
    }) {
        models[index]
            .as_object_mut()
            .with_context(|| format!("providers.{provider_id}.models[{index}] must be an object"))?
    } else {
        models.push(Value::Object(Map::new()));
        models
            .last_mut()
            .and_then(Value::as_object_mut)
            .context("new Pi model must be an object")?
    };
    model_config.insert("id".to_string(), Value::String(model.to_string()));
    model_config
        .entry("name".to_string())
        .or_insert_with(|| Value::String(model.to_string()));

    let content = format!("{}\n", serde_json::to_string_pretty(&root)?);
    let settings_target = agent_dir.join("settings.json");
    let mut settings = read_json_object(&settings_target)?;
    settings.insert(
        "defaultProvider".to_string(),
        Value::String(provider_id.clone()),
    );
    settings.insert("defaultModel".to_string(), Value::String(model.to_string()));
    let settings_content = format!("{}\n", serde_json::to_string_pretty(&settings)?);
    let redaction_refs = redactions.iter().map(String::as_str).collect::<Vec<_>>();
    let mut plan = new_plan(
        ToolId::Pi,
        target.clone(),
        format!("Configure Pi to use {}", entry.title),
        combine_previews([
            (
                target.as_path(),
                redacted_diff_preview(&diff_preview_for_path(&target, &content), &redaction_refs),
            ),
            (
                settings_target.as_path(),
                diff_preview_for_path(&settings_target, &settings_content),
            ),
        ]),
    );
    plan.extra_writes.push(PlannedWrite {
        backup_path: config_backup_path(&settings_target),
        target_path: settings_target,
        content: settings_content,
    });
    Ok((plan, content))
}

fn grok_backend_for_entry(entry: &ToolEntry) -> GrokApiBackend {
    if matches!(entry.interface_type, InterfaceType::AnthropicMessages) {
        GrokApiBackend::Messages
    } else {
        GrokApiBackend::ChatCompletions
    }
}

fn pi_api_for_entry(entry: &ToolEntry) -> PiApi {
    if matches!(entry.interface_type, InterfaceType::AnthropicMessages) {
        PiApi::AnthropicMessages
    } else {
        PiApi::OpenAiCompletions
    }
}

fn aipass_config_id(entry: &ToolEntry) -> String {
    let name = slug(&entry.title);
    if name.is_empty() {
        format!("aipass_{}", entry.id.simple())
    } else {
        format!("aipass_{name}")
    }
}

fn codex_provider_selection(doc: &DocumentMut, entry: &ToolEntry) -> (String, Option<String>) {
    let providers = doc.get("model_providers").and_then(Item::as_table);
    let active = doc
        .get("model_provider")
        .and_then(Item::as_str)
        .map(str::to_string);

    // Codex resolves conversations by this key, so an existing active provider
    // is always the safest provider to reuse. This also preserves custom fields
    // and avoids orphaning existing conversation history.
    if let Some(active_name) = active.as_deref() {
        if providers.is_some_and(|table| table.contains_key(active_name)) {
            return (active_name.to_string(), None);
        }
    }

    if let Some(provider_id) = entry.provider_id.as_deref() {
        if providers.is_some_and(|table| table.contains_key(provider_id)) {
            let migration = active.filter(|old| old != provider_id);
            return (provider_id.to_string(), migration);
        }
    }

    if let Some(table) = providers {
        if let Some((name, _)) = table.iter().find(|(_, item)| {
            item.as_table()
                .and_then(|provider| provider.get("name"))
                .and_then(Item::as_str)
                .is_some_and(|title| title == entry.title)
        }) {
            let migration = active.filter(|old| old != name);
            return (name.to_string(), migration);
        }
    }

    let generated = format!("aipass_{}", slug(&entry.title));
    let migration = active.filter(|old| old != &generated);
    (generated, migration)
}

fn update_codex_provider(
    doc: &mut DocumentMut,
    provider_name: &str,
    entry: &ToolEntry,
    plaintext_api_key: Option<&str>,
    requested_auth_mode: Option<CodexAuthMode>,
) -> Result<CodexAuthMode> {
    let providers = ensure_table(doc, "model_providers")?;
    let is_new = !providers.contains_key(provider_name);
    let item = providers
        .entry(provider_name)
        .or_insert_with(|| Item::Table(Table::new()));
    let provider = item
        .as_table_mut()
        .with_context(|| format!("model_providers.{provider_name} is not a TOML table"))?;

    let auth_mode = requested_auth_mode.unwrap_or_else(|| {
        if provider.contains_key("experimental_bearer_token") {
            CodexAuthMode::ExperimentalBearer
        } else if provider.contains_key("auth") {
            CodexAuthMode::Command
        } else if plaintext_api_key.is_some() {
            CodexAuthMode::AuthJson
        } else {
            CodexAuthMode::Env
        }
    });

    // Only API routing/auth fields are managed for an existing provider. Any
    // user-owned Codex options remain untouched. In particular, preserve
    // command-backed and experimental bearer authentication instead of adding
    // env_key, which would take precedence over those modes.
    if is_new {
        provider["name"] = value(entry.title.clone());
    }
    if let Some(endpoint) = codex_base_url(entry) {
        provider["base_url"] = value(endpoint);
    } else {
        provider.remove("base_url");
    }
    provider["wire_api"] = value("responses");
    match auth_mode {
        CodexAuthMode::ExperimentalBearer => {
            provider.remove("env_key");
            provider.remove("auth");
            provider["requires_openai_auth"] = value(false);
            if let Some(api_key) = plaintext_api_key {
                provider["experimental_bearer_token"] = value(api_key.to_string());
            }
        }
        CodexAuthMode::Command => {
            provider.remove("env_key");
            provider["requires_openai_auth"] = value(false);
        }
        CodexAuthMode::AuthJson => {
            provider.remove("env_key");
            provider.remove("auth");
            provider.remove("experimental_bearer_token");
            provider["requires_openai_auth"] = value(true);
        }
        CodexAuthMode::Env => {
            provider["env_key"] = value(entry.env_key.clone());
            provider["requires_openai_auth"] = value(false);
        }
    }
    doc["model_provider"] = value(provider_name.to_string());
    if matches!(auth_mode, CodexAuthMode::AuthJson) {
        doc["cli_auth_credentials_store"] = value("file");
    }
    if let Some(model) = &entry.default_model {
        doc["model"] = value(model.clone());
    }
    Ok(auth_mode)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CodexAuthMode {
    Env,
    ExperimentalBearer,
    Command,
    AuthJson,
}

fn collect_json_strings(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::String(value) => output.push(value.clone()),
        Value::Array(values) => values
            .iter()
            .for_each(|value| collect_json_strings(value, output)),
        Value::Object(values) => values
            .values()
            .for_each(|value| collect_json_strings(value, output)),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn replace_codex_provider_references(
    doc: &mut DocumentMut,
    from_provider: &str,
    to_provider: &str,
) {
    for (_, item) in doc.iter_mut() {
        replace_codex_provider_references_in_item(item, from_provider, to_provider);
    }
}

fn replace_codex_provider_references_in_item(
    item: &mut Item,
    from_provider: &str,
    to_provider: &str,
) {
    if let Some(table) = item.as_table_mut() {
        for (key, child) in table.iter_mut() {
            if key == "model_provider" && child.as_str() == Some(from_provider) {
                *child = value(to_provider.to_string());
            } else {
                replace_codex_provider_references_in_item(child, from_provider, to_provider);
            }
        }
    } else if let Some(value) = item.as_value_mut() {
        replace_codex_provider_references_in_value(value, from_provider, to_provider);
    }
}

fn replace_codex_provider_references_in_value(
    item: &mut toml_edit::Value,
    from_provider: &str,
    to_provider: &str,
) {
    if let Some(table) = item.as_inline_table_mut() {
        for (key, child) in table.iter_mut() {
            if key == "model_provider" && child.as_str() == Some(from_provider) {
                *child = toml_edit::Value::from(to_provider.to_string());
            } else {
                replace_codex_provider_references_in_value(child, from_provider, to_provider);
            }
        }
    }
}

fn append_codex_migration(
    codex_dir: &Path,
    plan: &mut ConfigPlan,
    from_provider: Option<&str>,
    to_provider: &str,
) -> Result<()> {
    let Some(from_provider) = from_provider else {
        return Ok(());
    };

    let mut session_files = Vec::new();
    for root in [
        codex_dir.join("sessions"),
        codex_dir.join("archived_sessions"),
    ] {
        collect_jsonl_files(&root, &mut session_files)?;
    }

    let backup_root = codex_dir.join(".aipass-backups");
    let mut changed_files = 0;
    let mut changed_records = 0;
    for path in session_files {
        let original = fs::read_to_string(&path)?;
        let mut changed = false;
        let mut next_lines = Vec::new();
        for line in original.split_inclusive('\n') {
            let has_newline = line.ends_with('\n');
            let body = line.strip_suffix('\n').unwrap_or(line);
            let mut parsed = match serde_json::from_str::<Value>(body) {
                Ok(value) => value,
                Err(_) => {
                    next_lines.push(line.to_string());
                    continue;
                }
            };
            let mut updated_record = false;
            let should_update = parsed.get("type").and_then(Value::as_str) == Some("session_meta")
                && parsed
                    .get("payload")
                    .and_then(Value::as_object)
                    .and_then(|payload| payload.get("model_provider"))
                    .and_then(Value::as_str)
                    == Some(from_provider);
            if should_update {
                if let Some(payload) = parsed.get_mut("payload").and_then(Value::as_object_mut) {
                    payload.insert(
                        "model_provider".to_string(),
                        Value::String(to_provider.to_string()),
                    );
                    changed = true;
                    updated_record = true;
                    changed_records += 1;
                }
            }
            let serialized = if updated_record {
                serde_json::to_string(&parsed)?
            } else {
                body.to_string()
            };
            next_lines.push(if has_newline {
                format!("{serialized}\n")
            } else {
                serialized
            });
        }

        if !changed {
            continue;
        }
        changed_files += 1;
        let backup_path = backup_root.join(format!("session-{}.aipbackup", path_hash(&path)));
        plan.extra_writes.push(PlannedWrite {
            target_path: path,
            backup_path,
            content: next_lines.concat(),
        });
    }

    let migration_preview = if changed_files == 0 {
        format!(
            "~ no conversation records found for provider {from_provider}; provider name migration not required"
        )
    } else {
        format!(
            "~ migrate {changed_records} session metadata records in {changed_files} JSONL files: {from_provider} -> {to_provider}"
        )
    };
    let sqlite_catalog_files = [
        codex_dir.join("state_5.sqlite"),
        codex_dir.join("sqlite").join("state_5.sqlite"),
        codex_dir.join("codex-dev.db"),
        codex_dir.join("sqlite").join("codex-dev.db"),
    ]
    .into_iter()
    .filter(|path| path.exists())
    .count();
    let sqlite_preview = if sqlite_catalog_files == 0 {
        "~ no Codex SQLite catalog files found".to_string()
    } else {
        format!(
            "~ migrate matching model_provider rows in {sqlite_catalog_files} Codex SQLite catalog files"
        )
    };
    plan.preview = format!(
        "{}\n\n# Codex conversation migration\n{}\n{}",
        plan.preview, migration_preview, sqlite_preview
    );
    plan.codex_provider_migration = Some(CodexProviderMigration {
        from_provider: from_provider.to_string(),
        to_provider: to_provider.to_string(),
    });
    Ok(())
}

fn collect_jsonl_files(root: &Path, files: &mut Vec<std::path::PathBuf>) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_jsonl_files(&path, files)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
    Ok(())
}

fn path_hash(path: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.to_string_lossy().hash(&mut hasher);
    hasher.finish()
}

fn codex_base_url(entry: &ToolEntry) -> Option<String> {
    let endpoint = entry.endpoint.as_deref()?.trim_end_matches('/');
    if matches!(
        entry.interface_type,
        InterfaceType::OpenAiCompatible | InterfaceType::AzureOpenAi
    ) {
        let origin_only = match endpoint.split_once("://") {
            Some((_scheme, rest)) => !rest.contains('/'),
            None => !endpoint.contains('/'),
        };
        if endpoint.ends_with("/v1") || !origin_only {
            Some(endpoint.to_string())
        } else {
            Some(format!("{endpoint}/v1"))
        }
    } else {
        Some(endpoint.to_string())
    }
}

fn read_root_object(path: &Path) -> Result<Map<String, Value>> {
    read_json_object(path)
}

fn opencode_config_path(home: &Path) -> std::path::PathBuf {
    let dir = home.join(".config").join("opencode");
    ["opencode.jsonc", "opencode.json", "config.json"]
        .into_iter()
        .map(|name| dir.join(name))
        .find(|path| path.exists())
        .unwrap_or_else(|| dir.join("opencode.json"))
}

fn opencode_provider_selection(root: &Map<String, Value>, entry: &ToolEntry) -> String {
    let providers = root.get("provider").and_then(Value::as_object);
    if let Some(provider) = entry.provider_id.as_deref() {
        if providers.is_some_and(|map| map.contains_key(provider)) {
            return provider.to_string();
        }
    }
    if let Some((name, _)) = providers.and_then(|map| {
        map.iter().find(|(_, value)| {
            value
                .as_object()
                .and_then(|provider| provider.get("name"))
                .and_then(Value::as_str)
                .is_some_and(|name| name == entry.title)
        })
    }) {
        return name.to_string();
    }
    let generated = format!("aipass_{}", slug(&entry.title));
    if providers.is_some_and(|map| map.contains_key(&generated)) {
        return generated;
    }
    generated
}

fn update_opencode_provider(
    root: &mut Map<String, Value>,
    provider_id: &str,
    entry: &ToolEntry,
    api_key: Value,
    api: Option<OpenCodeApi>,
) -> Result<()> {
    let providers = ensure_json_object(root, "provider")?;
    let provider = providers
        .entry(provider_id.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let provider = provider
        .as_object_mut()
        .with_context(|| format!("provider.{provider_id} must be a JSON object"))?;

    provider.insert(
        "npm".to_string(),
        Value::String(
            api.map(OpenCodeApi::npm_package)
                .unwrap_or_else(|| opencode_npm_package(entry.interface_type.clone()))
                .to_string(),
        ),
    );
    if !provider.contains_key("name") {
        provider.insert("name".to_string(), Value::String(entry.title.clone()));
    }
    let options = provider
        .entry("options".to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .with_context(|| format!("provider.{provider_id}.options must be a JSON object"))?;
    if let Some(endpoint) = &entry.endpoint {
        options.insert("baseURL".to_string(), Value::String(endpoint.clone()));
    } else {
        options.remove("baseURL");
    }
    options.insert("apiKey".to_string(), api_key);

    if let Some(model) = &entry.default_model {
        let models = provider
            .entry("models".to_string())
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .with_context(|| format!("provider.{provider_id}.models must be a JSON object"))?;
        let model_config = models
            .entry(model.clone())
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .with_context(|| {
                format!("provider.{provider_id}.models.{model} must be a JSON object")
            })?;
        model_config.insert("name".to_string(), Value::String(model.clone()));
    }
    Ok(())
}

fn opencode_npm_package(interface: InterfaceType) -> &'static str {
    match interface {
        InterfaceType::AnthropicMessages => "@ai-sdk/anthropic",
        InterfaceType::Gemini => "@ai-sdk/google",
        InterfaceType::Bedrock => "@ai-sdk/amazon-bedrock",
        InterfaceType::OpenAiCompatible
        | InterfaceType::AzureOpenAi
        | InterfaceType::CustomHttp => "@ai-sdk/openai-compatible",
    }
}

fn ensure_codex_entry(entry: &ToolEntry) -> Result<()> {
    if !matches!(entry.interface_type, InterfaceType::OpenAiCompatible) {
        anyhow::bail!("Codex live config requires an OpenAI-compatible provider entry");
    }
    if !matches!(entry.auth_scheme, AuthScheme::Bearer) {
        anyhow::bail!("Codex live config requires bearer-token authentication");
    }
    Ok(())
}

fn ensure_claude_code_entry(entry: &ToolEntry) -> Result<()> {
    if !matches!(entry.interface_type, InterfaceType::AnthropicMessages) {
        anyhow::bail!(
            "Claude Code config requires an Anthropic Messages-compatible provider entry"
        );
    }
    if !matches!(entry.auth_scheme, AuthScheme::XApiKey | AuthScheme::Bearer) {
        anyhow::bail!("Claude Code config requires x-api-key or bearer authentication");
    }
    Ok(())
}

fn ensure_cursor_local_entry(entry: &ToolEntry) -> Result<()> {
    match entry.interface_type {
        InterfaceType::OpenAiCompatible => {
            if !matches!(entry.auth_scheme, AuthScheme::Bearer) {
                anyhow::bail!("Cursor Agent Local requires bearer authentication for OpenAI-compatible providers");
            }
        }
        InterfaceType::AnthropicMessages => {
            if !matches!(entry.auth_scheme, AuthScheme::XApiKey | AuthScheme::Bearer) {
                anyhow::bail!("Cursor Agent Local requires x-api-key or bearer authentication for Anthropic providers");
            }
        }
        _ => anyhow::bail!(
            "Cursor Agent Local requires an OpenAI-compatible or Anthropic Messages provider"
        ),
    }
    Ok(())
}

fn ensure_gemini_entry(entry: &ToolEntry) -> Result<()> {
    if !matches!(entry.interface_type, InterfaceType::Gemini) {
        anyhow::bail!("Gemini CLI config requires a Gemini-native provider entry");
    }
    if !matches!(entry.auth_scheme, AuthScheme::GoogleApiKey) {
        anyhow::bail!("Gemini CLI config requires a Google API key");
    }
    Ok(())
}

fn combine_previews<const N: usize>(sections: [(&Path, String); N]) -> String {
    sections
        .into_iter()
        .map(|(path, preview)| format!("# {}\n{}", path.display(), preview))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn update_dotenv_content(before: &str, updates: &[(&str, String)], removals: &[&str]) -> String {
    let mut lines = before.lines().map(str::to_string).collect::<Vec<_>>();
    let mut seen = std::collections::HashSet::new();
    for line in &mut lines {
        let Some((key, _)) = dotenv_assignment(line) else {
            continue;
        };
        let key = key.to_string();
        if removals.iter().any(|name| *name == key) {
            *line = String::new();
            continue;
        }
        if let Some((_, value)) = updates.iter().find(|(name, _)| *name == key) {
            *line = format!("{key}={value}");
            seen.insert(key);
        }
    }
    for (key, value) in updates {
        if !seen.contains(*key) {
            lines.push(format!("{key}={value}"));
        }
    }
    let mut content = lines.join("\n");
    if !content.is_empty() {
        content.push('\n');
    }
    content
}

fn dotenv_assignment(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim_start();
    let assignment = trimmed.strip_prefix("export ").unwrap_or(trimmed);
    let (key, value) = assignment.split_once('=')?;
    let key = key.trim();
    if key.is_empty()
        || !key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return None;
    }
    Some((key, value))
}
