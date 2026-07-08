use crate::models::{ConfigPlan, PlannedWrite, ToolEntry, ToolId};
use crate::utils::{
    config_backup_path, diff_preview, dotenv_quote, ensure_json_object, ensure_table, new_plan,
    read_json_object, read_json_value, read_toml, redacted_diff_preview, resolve_codex_dir, slug,
};
use aipass_provider_registry::{AuthScheme, InterfaceType};
use anyhow::{Context, Result};
use serde_json::{Map, Value};
use std::path::Path;
use toml_edit::{value, Item, Table};

pub fn plan_codex(home: &Path, entry: &ToolEntry) -> Result<(ConfigPlan, String)> {
    ensure_codex_entry(entry)?;
    let codex_dir = resolve_codex_dir(home);
    let target = codex_dir.join("config.toml");
    let mut doc = read_toml(&target)?;
    let provider_name = codex_provider_name(entry);
    let mut provider = Table::new();
    provider["name"] = value(entry.title.clone());
    provider["env_key"] = value(entry.env_key.clone());
    if let Some(endpoint) = codex_base_url(entry) {
        provider["base_url"] = value(endpoint);
    }
    provider["wire_api"] = value("responses");
    provider["requires_openai_auth"] = value(false);
    ensure_table(&mut doc, "model_providers")?[&provider_name] = Item::Table(provider);
    doc["model_provider"] = value(provider_name);
    if let Some(model) = &entry.default_model {
        doc["model"] = value(model.clone());
    }
    let content = doc.to_string();
    let plan = new_plan(
        ToolId::Codex,
        target,
        format!("Configure Codex env-based config to use {}", entry.title),
        diff_preview(&content),
    );
    Ok((plan, content))
}

pub fn plan_codex_plaintext(home: &Path, entry: &ToolEntry) -> Result<(ConfigPlan, String)> {
    ensure_codex_entry(entry)?;
    let api_key = entry
        .api_key
        .as_deref()
        .context("plaintext Codex config requires an API key")?;
    let codex_dir = resolve_codex_dir(home);
    let target = codex_dir.join("config.toml");
    let mut doc = read_toml(&target)?;
    let provider_name = codex_provider_name(entry);
    let mut provider = Table::new();
    provider["name"] = value(entry.title.clone());
    if let Some(endpoint) = codex_base_url(entry) {
        provider["base_url"] = value(endpoint);
    }
    provider["wire_api"] = value("responses");
    provider["requires_openai_auth"] = value(true);
    ensure_table(&mut doc, "model_providers")?[&provider_name] = Item::Table(provider);
    doc["model_provider"] = value(provider_name.clone());
    if let Some(model) = &entry.default_model {
        doc["model"] = value(model.clone());
    }
    let content = doc.to_string();
    let auth_target = codex_dir.join("auth.json");
    let auth_content = serde_json::to_string_pretty(&serde_json::json!({
        "OPENAI_API_KEY": api_key
    }))?;
    let mut plan = new_plan(
        ToolId::Codex,
        target.clone(),
        format!(
            "Configure Codex live config (config.toml + auth.json) to use {}",
            entry.title
        ),
        combine_previews([
            (&target, diff_preview(&content)),
            (
                &auth_target,
                redacted_diff_preview(&auth_content, &[api_key]),
            ),
        ]),
    );
    plan.extra_writes.push(PlannedWrite {
        backup_path: config_backup_path(&auth_target),
        target_path: auth_target,
        content: auth_content,
    });
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
    }
    if let Some(model) = &entry.default_model {
        env.insert("ANTHROPIC_MODEL".to_string(), Value::String(model.clone()));
    }
    let content = serde_json::to_string_pretty(&json)?;
    let plan = new_plan(
        ToolId::ClaudeCode,
        target,
        format!("Configure Claude Code to use {}", entry.title),
        diff_preview(&content),
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
    env.insert(
        "ANTHROPIC_API_KEY".to_string(),
        Value::String(api_key.to_string()),
    );
    env.remove("ANTHROPIC_AUTH_TOKEN");
    if let Some(endpoint) = &entry.endpoint {
        env.insert(
            "ANTHROPIC_BASE_URL".to_string(),
            Value::String(endpoint.clone()),
        );
    }
    if let Some(model) = &entry.default_model {
        env.insert("ANTHROPIC_MODEL".to_string(), Value::String(model.clone()));
    }
    let content = serde_json::to_string_pretty(&json)?;
    let plan = new_plan(
        ToolId::ClaudeCode,
        target,
        format!(
            "Configure Claude Code plaintext credentials for {}",
            entry.title
        ),
        redacted_diff_preview(&content, &[api_key]),
    );
    Ok((plan, content))
}

pub fn plan_gemini_cli(home: &Path, entry: &ToolEntry) -> Result<(ConfigPlan, String)> {
    ensure_gemini_entry(entry)?;
    let target = home.join(".gemini").join("aipass.env");
    let key = match entry.auth_scheme {
        AuthScheme::GoogleApiKey => "GEMINI_API_KEY",
        _ => &entry.env_key,
    };
    let mut content = format!(
        "# Generated by AIPass. Gemini CLI reads ~/.gemini/.env directly, so this helper file keeps secrets out of the tool config.\n# Run Gemini with `aipass exec {id} -- gemini` or export this value in your shell.\n{key}=\"$(aipass get {id} --field api_key --reveal)\"\n",
        id = entry.id,
    );
    if let Some(endpoint) = &entry.endpoint {
        content.push_str(&format!(
            "GOOGLE_GEMINI_BASE_URL={}\n",
            dotenv_quote(endpoint)
        ));
    }
    if let Some(model) = &entry.default_model {
        content.push_str(&format!("GEMINI_MODEL={}\n", dotenv_quote(model)));
    }
    let plan = new_plan(
        ToolId::GeminiCli,
        target,
        format!("Create Gemini CLI helper env for {}", entry.title),
        diff_preview(&content),
    );
    Ok((plan, content))
}

pub fn plan_gemini_cli_plaintext(home: &Path, entry: &ToolEntry) -> Result<(ConfigPlan, String)> {
    ensure_gemini_entry(entry)?;
    let target = home.join(".gemini").join(".env");
    let api_key = entry
        .api_key
        .as_deref()
        .context("plaintext Gemini CLI config requires an API key")?;
    let mut content = format!("GEMINI_API_KEY={}\n", dotenv_quote(api_key));
    if let Some(endpoint) = &entry.endpoint {
        content.push_str(&format!(
            "GOOGLE_GEMINI_BASE_URL={}\n",
            dotenv_quote(endpoint)
        ));
    }
    if let Some(model) = &entry.default_model {
        content.push_str(&format!("GEMINI_MODEL={}\n", dotenv_quote(model)));
    }
    let plan = new_plan(
        ToolId::GeminiCli,
        target,
        format!("Configure Gemini CLI plaintext env for {}", entry.title),
        redacted_diff_preview(&content, &[api_key]),
    );
    Ok((plan, content))
}

pub fn plan_opencode(home: &Path, entry: &ToolEntry) -> Result<(ConfigPlan, String)> {
    let target = home.join(".config").join("opencode").join("opencode.json");
    let mut root = read_root_object(&target)?;
    let provider_id = opencode_provider_name(entry);
    if !root.contains_key("$schema") {
        root.insert(
            "$schema".to_string(),
            Value::String("https://opencode.ai/config.json".to_string()),
        );
    }
    let provider =
        build_opencode_provider(entry, Value::String(format!("{{env:{}}}", entry.env_key)));
    ensure_json_object(&mut root, "provider")?.insert(provider_id.clone(), provider);
    if let Some(model) = &entry.default_model {
        root.insert(
            "model".to_string(),
            Value::String(format!("{provider_id}/{model}")),
        );
    }
    let content = serde_json::to_string_pretty(&root)?;
    let plan = new_plan(
        ToolId::OpenCode,
        target,
        format!("Configure OpenCode to use {}", entry.title),
        diff_preview(&content),
    );
    Ok((plan, content))
}

pub fn plan_opencode_plaintext(home: &Path, entry: &ToolEntry) -> Result<(ConfigPlan, String)> {
    let target = home.join(".config").join("opencode").join("opencode.json");
    let api_key = entry
        .api_key
        .as_deref()
        .context("plaintext OpenCode config requires an API key")?;
    let mut root = read_root_object(&target)?;
    let provider_id = opencode_provider_name(entry);
    if !root.contains_key("$schema") {
        root.insert(
            "$schema".to_string(),
            Value::String("https://opencode.ai/config.json".to_string()),
        );
    }
    let provider = build_opencode_provider(entry, Value::String(api_key.to_string()));
    ensure_json_object(&mut root, "provider")?.insert(provider_id.clone(), provider);
    if let Some(model) = &entry.default_model {
        root.insert(
            "model".to_string(),
            Value::String(format!("{provider_id}/{model}")),
        );
    }
    let content = serde_json::to_string_pretty(&root)?;
    let plan = new_plan(
        ToolId::OpenCode,
        target,
        format!(
            "Configure OpenCode plaintext credentials for {}",
            entry.title
        ),
        redacted_diff_preview(&content, &[api_key]),
    );
    Ok((plan, content))
}

fn codex_provider_name(entry: &ToolEntry) -> String {
    format!("aipass_{}", slug(&entry.title))
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
    let value = read_json_value(path)?;
    Ok(value.as_object().cloned().unwrap_or_default())
}

fn opencode_provider_name(entry: &ToolEntry) -> String {
    format!("aipass_{}", slug(&entry.title))
}

fn build_opencode_provider(entry: &ToolEntry, api_key: Value) -> Value {
    let mut provider = Map::new();
    provider.insert(
        "npm".to_string(),
        Value::String(opencode_npm_package(entry.interface_type.clone()).to_string()),
    );
    provider.insert("name".to_string(), Value::String(entry.title.clone()));

    let mut options = Map::new();
    if let Some(endpoint) = &entry.endpoint {
        options.insert("baseURL".to_string(), Value::String(endpoint.clone()));
    }
    options.insert("apiKey".to_string(), api_key);
    provider.insert("options".to_string(), Value::Object(options));

    if let Some(model) = &entry.default_model {
        let mut models = Map::new();
        let mut model_config = Map::new();
        model_config.insert("name".to_string(), Value::String(model.clone()));
        models.insert(model.clone(), Value::Object(model_config));
        provider.insert("models".to_string(), Value::Object(models));
    }

    Value::Object(provider)
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
    if !matches!(entry.auth_scheme, AuthScheme::XApiKey) {
        anyhow::bail!("Claude Code config requires x-api-key authentication");
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
