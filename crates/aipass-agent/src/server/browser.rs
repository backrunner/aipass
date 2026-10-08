//! Detected provider validation and browser save plans.
use super::*;

pub(super) fn is_origin_ignored(vault_dir: &Path, origin: &str) -> Result<bool> {
    let origin = normalize_origin(origin)?;
    Ok(load_native_host_settings(vault_dir)?
        .ignored_origins
        .iter()
        .any(|value| value == &origin))
}

pub(super) fn ignore_origin(vault_dir: &Path, origin: &str) -> Result<Vec<String>> {
    let origin = normalize_origin(origin)?;
    let mut settings = load_native_host_settings(vault_dir)?;
    if !settings
        .ignored_origins
        .iter()
        .any(|value| value == &origin)
    {
        settings.ignored_origins.push(origin);
        settings.ignored_origins.sort();
        settings.ignored_origins.dedup();
        save_native_host_settings(vault_dir, &settings)?;
    }
    Ok(settings.ignored_origins)
}

pub(super) fn load_native_host_settings(vault_dir: &Path) -> Result<NativeHostSettings> {
    let path = native_host_settings_path(vault_dir);
    if !path.exists() {
        return Ok(NativeHostSettings::default());
    }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

pub(super) fn save_native_host_settings(
    vault_dir: &Path,
    settings: &NativeHostSettings,
) -> Result<()> {
    let path = native_host_settings_path(vault_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    atomic_write_bytes(&path, &serde_json::to_vec_pretty(settings)?)?;
    Ok(())
}

pub(super) fn normalize_origin(origin: &str) -> Result<String> {
    let normalized = origin.trim().trim_end_matches('/').to_lowercase();
    if normalized.is_empty() {
        bail!("origin is required");
    }
    Ok(normalized)
}

pub(super) fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub(super) fn save_detected_secret(
    vault: &Vault,
    fields: BrowserDetectedSecretFields,
) -> ServiceResult<SaveDetectedResult> {
    let domain = host_from_origin(&fields.origin);
    let provider_guess = fields
        .provider_id
        .clone()
        .or_else(|| match_provider_by_domain(&domain).map(|provider| provider.id.to_string()));
    let provider_kind = provider_guess
        .as_deref()
        .map(|id| provider_kind_for_id(Some(id)))
        .unwrap_or(aipass_provider_registry::ProviderKind::Unknown);
    let preview = detected_secret_preview(vault, &fields);
    let secret_metadata = detected_secret_metadata(&preview);
    if let Some(existing_entry_id) = preview.existing_entry_id {
        // Deliberately no entry-level gateway write here: an entry can now hold
        // several groups, so stamping one of them onto the entry would be wrong.
        if preview
            .favicon_url
            .as_deref()
            .is_some_and(|favicon| favicon.starts_with("data:image/"))
        {
            vault
                .replace_provider_favicon_url(
                    existing_entry_id,
                    preview.favicon_url.clone().unwrap(),
                )
                .map_err(map_vault_error)?;
        }

        // The exact key is already stored: refresh its group / format / billing
        // rather than storing it twice.
        if let Some(secret_id) = preview.existing_secret_id.clone() {
            if !secret_metadata.is_empty() {
                vault
                    .set_secret_metadata(existing_entry_id, &secret_id, &secret_metadata)
                    .map_err(map_vault_error)?;
            }
            return Ok(SaveDetectedResult {
                entry_id: existing_entry_id,
                secret_id: Some(secret_id),
                merged_into_existing: true,
            });
        }

        // A different key for a site we already track — most often a second
        // gateway group. It belongs on the same entry as another key.
        let existing = vault
            .get_provider_summary(existing_entry_id)
            .map_err(map_vault_error)?;
        let label = unique_secret_label(
            &existing,
            preview
                .secret_label
                .as_deref()
                .or(preview.group.as_deref())
                .unwrap_or("key"),
        );
        let secret_id = vault
            .add_secret_with_metadata(
                existing_entry_id,
                label,
                fields.api_key.into_inner(),
                &secret_metadata,
            )
            .map_err(map_vault_error)?;
        return Ok(SaveDetectedResult {
            entry_id: existing_entry_id,
            secret_id: Some(secret_id),
            merged_into_existing: true,
        });
    }
    let api_key = fields.api_key.into_inner();
    let mut domains = vec![domain];
    for extra in &fields.domains {
        let cleaned = extra.trim();
        if !cleaned.is_empty() && !domains.iter().any(|item| item == cleaned) {
            domains.push(cleaned.to_string());
        }
    }
    let mut endpoints: Vec<ProviderEndpoint> = preview
        .endpoint
        .clone()
        .into_iter()
        .map(ProviderEndpoint::api)
        .collect();
    endpoints.extend(
        fields
            .console_endpoint
            .clone()
            .and_then(non_empty)
            .into_iter()
            .map(ProviderEndpoint::console),
    );
    let entry_id = vault
        .add_provider(ProviderEntryInput {
            max_concurrent_requests: None,
            supports_websockets: None,
            title: preview.title,
            provider_kind,
            provider_id: preview.provider_id,
            credential_kind: Default::default(),
            account_identity: None,
            domains,
            favicon_url: preview.favicon_url,
            endpoints,
            interface_type: preview.interface_type,
            auth_scheme: preview.auth_scheme,
            api_key: api_key.clone(),
            secret_label: preview.secret_label,
            default_model: fields.default_model.clone().and_then(non_empty),
            model_aliases: fields
                .model_aliases
                .iter()
                .filter_map(|(alias, model)| {
                    Some((non_empty(alias.clone())?, non_empty(model.clone())?))
                })
                .collect(),
            headers: fields.headers.clone(),
            quota: None,
            subscription: None,
            gateway: preview.gateway,
            tags: preview.tags,
            notes: fields.notes.clone().and_then(non_empty),
            secret_metadata,
        })
        .map_err(map_vault_error)?;
    let secret_id = vault
        .find_secret_id_by_value(entry_id, &api_key)
        .map_err(map_vault_error)?;
    Ok(SaveDetectedResult {
        entry_id,
        secret_id,
        merged_into_existing: false,
    })
}

/// The per-key attributes a detected draft carries. The wire format is always
/// recorded on the key: on a relay one group may speak Anthropic while another
/// speaks OpenAI, and the entry can only name one of them.
pub(super) fn detected_secret_metadata(
    preview: &BrowserDetectedSecretPreview,
) -> SecretMetadataInput {
    SecretMetadataInput {
        endpoint: None,
        default_model: None,
        group: preview.group.clone(),
        interface_type: Some(preview.interface_type.clone()),
        billing: preview.billing.clone().filter(|rule| !rule.is_empty()),
    }
}

/// Labels are unique within an entry, so a second key from the same group gets
/// a numeric suffix instead of failing the save.
pub(super) fn unique_secret_label(entry: &EntrySummary, preferred: &str) -> String {
    let preferred = preferred.trim();
    let base = if preferred.is_empty() {
        "key"
    } else {
        preferred
    };
    if !entry.secret_refs.iter().any(|secret| secret.label == base) {
        return base.to_string();
    }
    for suffix in 2..100 {
        let candidate = format!("{base} {suffix}");
        if !entry
            .secret_refs
            .iter()
            .any(|secret| secret.label == candidate)
        {
            return candidate;
        }
    }
    format!("{base} {}", Uuid::new_v4())
}

pub(super) fn detected_secret_preview(
    vault: &Vault,
    fields: &BrowserDetectedSecretFields,
) -> BrowserDetectedSecretPreview {
    let domain = host_from_origin(&fields.origin);
    let provider_guess = fields
        .provider_id
        .clone()
        .or_else(|| match_provider_by_domain(&domain).map(|provider| provider.id.to_string()));
    let provider_definition = provider_guess.as_deref().and_then(|id| {
        default_provider_definitions()
            .into_iter()
            .find(|provider| provider.id == id)
    });
    let endpoint = fields
        .endpoint
        .clone()
        .or_else(|| {
            provider_definition.as_ref().and_then(|provider| {
                provider
                    .endpoints
                    .iter()
                    .find(|(_, kind, _)| *kind == EndpointKind::Api)
                    .map(|(_, _, url)| (*url).to_string())
            })
        })
        .or_else(|| Some(fields.url.clone()));
    let interface_type = fields.interface_type.clone().unwrap_or_else(|| {
        endpoint
            .as_deref()
            .and_then(infer_interface_from_endpoint)
            .or_else(|| {
                provider_definition
                    .as_ref()
                    .and_then(|provider| provider.interfaces.first().cloned())
            })
            .or_else(|| provider_guess_interface(&fields.origin))
            .unwrap_or(InterfaceType::CustomHttp)
    });
    let auth_scheme = fields.auth_scheme.clone().unwrap_or_else(|| {
        provider_definition
            .as_ref()
            .and_then(|provider| provider.auth_schemes.first().cloned())
            .unwrap_or_else(|| default_auth_for_interface(&interface_type))
    });
    let title = fields
        .title
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            provider_definition
                .as_ref()
                .map(|provider| provider.display_name.to_string())
        })
        .unwrap_or_else(|| "Browser Provider".to_string());
    let tags = fields.tags.clone();

    // Resolve where this key belongs. An exact key match wins; otherwise any
    // entry already tracking this site adopts the key, so a relay with several
    // groups stays one entry with one key per group.
    let stored = vault
        .search(fields.api_key.expose())
        .ok()
        .and_then(|matches| matches.into_iter().next());
    let existing_secret_id = stored.as_ref().and_then(|entry| {
        vault
            .find_secret_id_by_value(entry.id, fields.api_key.expose())
            .ok()
            .flatten()
    });
    let is_saved = stored.is_some();
    let target = stored.or_else(|| {
        vault
            .lookup_by_origin(&fields.origin)
            .ok()
            .into_iter()
            .flatten()
            .next()
    });

    let gateway = clean_gateway(fields.gateway.clone());
    // Group is its own field now; accept it from the dedicated field and fall
    // back to the legacy entry-level gateway blob for older drafts.
    let group = clean_gateway_field(fields.group.as_deref().unwrap_or_default(), 48)
        .or_else(|| gateway.as_ref().and_then(|value| value.group.clone()));
    let billing = clean_billing(fields.billing.clone(), gateway.as_ref());

    BrowserDetectedSecretPreview {
        title,
        secret_label: clean_secret_label(fields.secret_label.as_deref()),
        favicon_url: clean_favicon_url(fields.favicon_url.as_deref()),
        provider_id: provider_guess,
        endpoint,
        interface_type,
        auth_scheme,
        masked_secret: mask_secret(fields.api_key.expose()),
        fingerprint: vault.fingerprint_secret(fields.api_key.expose()),
        existing_entry_id: target.as_ref().map(|entry| entry.id),
        existing_entry_title: target.as_ref().map(|entry| entry.title.clone()),
        existing_secret_id,
        existing_groups: target
            .as_ref()
            .map(|entry| {
                entry
                    .secret_refs
                    .iter()
                    .filter_map(|secret| secret.group.clone())
                    .collect()
            })
            .unwrap_or_default(),
        is_saved,
        tags,
        gateway,
        group,
        billing,
    }
}

/// Billing details are scraped from a console page, so they get the same
/// length and secret-shaped sanitising as the other gateway fields. The legacy
/// entry-level `gateway.rate` seeds the rate when no rule was sent.
pub(super) fn clean_billing(
    billing: Option<aipass_provider_registry::BillingRule>,
    gateway: Option<&aipass_provider_registry::GatewayMetadata>,
) -> Option<aipass_provider_registry::BillingRule> {
    const MAX_LEN: usize = 48;
    const MAX_NOTE_LEN: usize = 160;
    let mut rule = billing.unwrap_or_default();
    rule.rate = rule
        .rate
        .and_then(|value| clean_gateway_field(&value, MAX_LEN))
        .or_else(|| gateway.and_then(|value| value.rate.clone()));
    rule.currency = rule
        .currency
        .and_then(|value| clean_gateway_field(&value, MAX_LEN));
    rule.unit_price = rule
        .unit_price
        .and_then(|value| clean_gateway_field(&value, MAX_LEN));
    rule.note = rule
        .note
        .and_then(|value| clean_gateway_field(&value, MAX_NOTE_LEN));
    (!rule.is_empty()).then_some(rule)
}

pub(super) fn clean_secret_label(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty()
        || value.len() > 64
        || value.chars().any(char::is_control)
        || value.eq_ignore_ascii_case("api key")
        || value.eq_ignore_ascii_case("token")
        || value.eq_ignore_ascii_case("secret")
        || value == "密钥"
        || value == "令牌"
        || looks_like_secret_or_masked(value)
    {
        None
    } else {
        Some(value.to_string())
    }
}

/// Defense against page-scraped metadata that is actually the API key itself,
/// either raw or elided (e.g. `sk-abc…xyz`, `sk-abc...xyz`, `sk-abc***xyz`).
pub(super) fn looks_like_secret_or_masked(value: &str) -> bool {
    let has_mask_run = value.contains('…')
        || value.contains("...")
        || value.contains("***")
        || value.contains("•••");
    if has_mask_run
        && value.len() >= 8
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '…' | '.' | '*' | '•'))
    {
        return true;
    }
    const SECRET_PREFIXES: &[&str] = &[
        "sk-", "r8_", "gsk_", "fw_", "xai-", "pplx-", "csk", "nvapi-", "hf_", "AIza",
    ];
    value.len() >= 16
        && SECRET_PREFIXES
            .iter()
            .any(|prefix| value.starts_with(prefix))
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

pub(super) fn clean_favicon_url(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() || value.len() > 512 * 1024 || value.chars().any(char::is_control) {
        return None;
    }
    let lower = value.to_lowercase();
    if lower.starts_with("https://")
        || lower.starts_with("http://")
        || (lower.starts_with("data:image/") && lower.contains(";base64,"))
    {
        Some(value.to_string())
    } else {
        None
    }
}

pub(super) fn clean_gateway(
    gateway: Option<aipass_provider_registry::GatewayMetadata>,
) -> Option<aipass_provider_registry::GatewayMetadata> {
    const MAX_GROUP_LEN: usize = 48;
    const MAX_RATE_LEN: usize = 24;
    let mut gateway = gateway?;
    gateway.group = gateway
        .group
        .and_then(|value| clean_gateway_field(&value, MAX_GROUP_LEN));
    gateway.rate = gateway
        .rate
        .and_then(|value| clean_gateway_field(&value, MAX_RATE_LEN));
    if gateway.group.is_none() && gateway.rate.is_none() {
        None
    } else {
        Some(gateway)
    }
}

pub(super) fn clean_gateway_field(value: &str, max_len: usize) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > max_len
        || value.chars().any(char::is_control)
        || looks_like_secret_or_masked(value)
    {
        None
    } else {
        Some(value.to_string())
    }
}

pub(super) fn host_from_origin(value: &str) -> String {
    let trimmed = value.trim().to_lowercase();
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(&trimmed);
    without_scheme
        .split('/')
        .next()
        .unwrap_or(without_scheme)
        .split('@')
        .next_back()
        .unwrap_or(without_scheme)
        .split(':')
        .next()
        .unwrap_or(without_scheme)
        .to_string()
}

pub(super) fn provider_guess_interface(origin: &str) -> Option<InterfaceType> {
    match_provider_by_domain(origin).and_then(|provider| provider.interfaces.first().cloned())
}

pub(super) fn infer_interface_from_endpoint(endpoint: &str) -> Option<InterfaceType> {
    let endpoint = endpoint.to_lowercase();
    // Keep in sync with OPENAI_COMPATIBLE_ENDPOINT_PATTERN in
    // packages/schemas/src/index.ts. `/v\d` covers versioned API paths such
    // as /v2, /api/paas/v4, and /v1beta.
    const OPENAI_COMPATIBLE_EVIDENCE: &[&str] = &[
        "openai",
        "chat/completions",
        "messages",
        "gateway",
        "one-api",
        "one_api",
        "one api",
        "oneapi",
        "new-api",
        "new_api",
        "new api",
        "newapi",
        "litellm",
        "sub2api",
        "openrouter",
        "veloera",
        "omniroute",
        "metapi",
        "onehub",
        "donehub",
        "anyrouter",
        "siliconflow",
        "deepseek",
        "moonshot",
        "dashscope",
        "qwen",
        "bigmodel",
        "zhipu",
        "volcengine",
        "volces",
        "ark",
        "together",
        "fireworks",
        "groq",
        "x.ai",
        "mistral",
        "perplexity",
        "cerebras",
        "nvidia",
        "nim",
        "novita",
        "huggingface",
        "hugging face",
    ];
    let has_versioned_api_path = endpoint
        .as_bytes()
        .windows(3)
        .any(|window| window[0] == b'/' && window[1] == b'v' && window[2].is_ascii_digit());
    if endpoint.contains("generativelanguage") || endpoint.contains("gemini") {
        Some(InterfaceType::Gemini)
    } else if endpoint.contains("anthropic") || endpoint.contains("claude") {
        Some(InterfaceType::AnthropicMessages)
    } else if has_versioned_api_path
        || OPENAI_COMPATIBLE_EVIDENCE
            .iter()
            .any(|keyword| endpoint.contains(keyword))
    {
        Some(InterfaceType::OpenAiCompatible)
    } else {
        None
    }
}

pub(super) fn default_auth_for_interface(interface_type: &InterfaceType) -> AuthScheme {
    match interface_type {
        InterfaceType::AnthropicMessages => AuthScheme::XApiKey,
        InterfaceType::Gemini => AuthScheme::GoogleApiKey,
        InterfaceType::AzureOpenAi => AuthScheme::AzureApiKey,
        InterfaceType::Bedrock => AuthScheme::AwsProfile,
        InterfaceType::OpenAiCompatible => AuthScheme::Bearer,
        InterfaceType::CustomHttp => AuthScheme::CustomHeader,
    }
}
