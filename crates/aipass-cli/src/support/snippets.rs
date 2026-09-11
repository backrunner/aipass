use crate::*;

pub(crate) fn field_value(item: &aipass_vault::EntrySummary, field: &str) -> Result<String> {
    match field {
        "api_key" | "secret" => Ok(item.masked_secret.clone()),
        "title" => Ok(item.title.clone()),
        "provider" | "provider_id" => Ok(item.provider_id.clone().unwrap_or_default()),
        "provider_kind" => Ok(format!("{:?}", item.provider_kind)),
        "domain" | "domains" => Ok(item.domains.join(",")),
        "endpoint" | "base_url" => Ok(endpoint_url(&item.endpoints).unwrap_or_default()),
        "console_url" | "console" => Ok(console_url(&item.endpoints).unwrap_or_default()),
        "interface" => Ok(format!("{:?}", item.interface_type)),
        "auth" => Ok(format!("{:?}", item.auth_scheme)),
        "default_model" => Ok(item.default_model.clone().unwrap_or_default()),
        "curl" | "curl_snippet" => Ok(curl_snippet_for_entry(item)),
        "env" | "env_export" => Ok(env_export_for_entry(item)),
        "config" | "config_snippet" => config_snippet_for_entry(item),
        "tags" => Ok(item.tags.join(",")),
        "notes" => Ok(item.notes.clone().unwrap_or_default()),
        "fingerprint" => Ok(item.fingerprint.clone()),
        other => anyhow::bail!("unsupported field: {other}"),
    }
}

pub(crate) fn console_url(endpoints: &[ProviderEndpoint]) -> Option<String> {
    endpoints
        .iter()
        .find(|endpoint| endpoint.kind == aipass_provider_registry::EndpointKind::Console)
        .and_then(|endpoint| endpoint.url.clone())
}

pub(crate) fn curl_snippet_for_entry(item: &aipass_vault::EntrySummary) -> String {
    let key = env_key_for_entry(item);
    let endpoint =
        endpoint_url(&item.endpoints).unwrap_or_else(|| "https://api.example.com".to_string());
    if matches!(item.interface_type, InterfaceType::Bedrock)
        || matches!(item.auth_scheme, AuthScheme::AwsProfile)
    {
        let region = item
            .endpoints
            .iter()
            .find_map(|endpoint| endpoint.region.as_deref())
            .unwrap_or("${AWS_REGION:-us-east-1}");
        return format!(
            "AWS_PROFILE=${{{key}:-default}} aws bedrock list-foundation-models --region {region}"
        );
    }
    match item.interface_type {
        InterfaceType::AnthropicMessages => format!(
            "curl -sS {}/v1/models -H 'x-api-key: ${}' -H 'anthropic-version: 2023-06-01'",
            endpoint.trim_end_matches('/'),
            key
        ),
        InterfaceType::Gemini => format!(
            "curl -sS '{}/v1beta/models?key=${}'",
            endpoint.trim_end_matches('/'),
            key
        ),
        InterfaceType::AzureOpenAi => format!(
            "curl -sS {}/models -H 'api-key: ${}'",
            endpoint.trim_end_matches('/'),
            key
        ),
        InterfaceType::OpenAiCompatible | InterfaceType::CustomHttp | InterfaceType::Bedrock => {
            let auth = auth_header_snippet(&item.auth_scheme, &key);
            format!("curl -sS {}/models {auth}", endpoint.trim_end_matches('/'))
        }
    }
}

pub(crate) fn auth_header_snippet(auth_scheme: &AuthScheme, key: &str) -> String {
    match auth_scheme {
        AuthScheme::Bearer => format!("-H 'Authorization: Bearer ${key}'"),
        AuthScheme::XApiKey => format!("-H 'x-api-key: ${key}'"),
        AuthScheme::GoogleApiKey | AuthScheme::AwsProfile => String::new(),
        AuthScheme::AzureApiKey => format!("-H 'api-key: ${key}'"),
        AuthScheme::CustomHeader => format!("-H 'Authorization: ${key}'"),
    }
}

pub(crate) fn env_export_for_entry(item: &aipass_vault::EntrySummary) -> String {
    let mut lines = vec![format!(
        "export {}=\"$(aipass get {} --field api_key --reveal)\"",
        env_key_for_entry(item),
        item.id
    )];
    if let Some(endpoint) = endpoint_url(&item.endpoints) {
        lines.push(format!("export AIPASS_BASE_URL={}", shell_quote(&endpoint)));
    }
    if let Some(model) = &item.default_model {
        lines.push(format!("export AIPASS_MODEL={}", shell_quote(model)));
    }
    lines.join("\n")
}

pub(crate) fn config_snippet_for_entry(item: &aipass_vault::EntrySummary) -> Result<String> {
    Ok(serde_json::to_string_pretty(&serde_json::json!({
        "provider": item.provider_id,
        "title": item.title,
        "interfaceType": item.interface_type,
        "authScheme": item.auth_scheme,
        "baseUrl": endpoint_url(&item.endpoints),
        "consoleUrl": console_url(&item.endpoints),
        "envKey": env_key_for_entry(item),
        "defaultModel": item.default_model,
        "modelAliases": item.model_aliases,
    }))?)
}

pub(crate) fn is_secret_field(field: &str) -> bool {
    matches!(field, "api_key" | "secret") || item_label(field).is_some()
}

pub(crate) fn secret_label_for_field(field: &str) -> &str {
    item_label(field).unwrap_or("primary")
}

pub(crate) fn item_label(field: &str) -> Option<&str> {
    field
        .strip_prefix("secret:")
        .or_else(|| field.strip_prefix("key:"))
        .filter(|label| !label.is_empty())
}

pub(crate) fn env_key_for_entry(item: &aipass_vault::EntrySummary) -> String {
    match item.provider_id.as_deref() {
        Some("anthropic") => "ANTHROPIC_API_KEY".to_string(),
        Some("gemini") => "GEMINI_API_KEY".to_string(),
        Some("openrouter") => "OPENROUTER_API_KEY".to_string(),
        Some("deepseek") => "DEEPSEEK_API_KEY".to_string(),
        Some("moonshot") => "MOONSHOT_API_KEY".to_string(),
        Some("qwen") => "DASHSCOPE_API_KEY".to_string(),
        Some("zhipu") => "ZHIPUAI_API_KEY".to_string(),
        Some("volcengine") => "ARK_API_KEY".to_string(),
        Some("groq") => "GROQ_API_KEY".to_string(),
        Some("replicate") => "REPLICATE_API_TOKEN".to_string(),
        Some("together") => "TOGETHER_API_KEY".to_string(),
        Some("fireworks") => "FIREWORKS_API_KEY".to_string(),
        Some("bedrock") => "AWS_PROFILE".to_string(),
        _ => match item.auth_scheme {
            AuthScheme::GoogleApiKey => "GEMINI_API_KEY".to_string(),
            AuthScheme::AzureApiKey => "AZURE_OPENAI_API_KEY".to_string(),
            AuthScheme::AwsProfile => "AWS_PROFILE".to_string(),
            _ => "AIPASS_API_KEY".to_string(),
        },
    }
}

pub(crate) fn copy_to_clipboard(secret: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let mut child = ProcessCommand::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("pbcopy unavailable")?;

    #[cfg(target_os = "windows")]
    let mut child = ProcessCommand::new("clip")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("clip unavailable")?;

    #[cfg(all(unix, not(target_os = "macos")))]
    let mut child = ProcessCommand::new("sh")
        .arg("-c")
        .arg("command -v wl-copy >/dev/null && wl-copy || xclip -selection clipboard")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("wl-copy/xclip unavailable")?;

    if let Some(stdin) = child.stdin.as_mut() {
        use std::io::Write;
        stdin.write_all(secret.as_bytes())?;
    }
    let status = child.wait()?;
    if !status.success() {
        anyhow::bail!("clipboard command failed");
    }
    Ok(())
}

pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
