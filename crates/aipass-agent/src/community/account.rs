//! Atomic subscription records, identity checks and CLI rebinding.
use super::*;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Account {
    pub(super) provider: String,
    pub(super) generation: Uuid,
    pub(super) auth: SensitiveString,
    pub(super) models: Value,
    pub(super) revision: u64,
    pub(super) native_method: Option<usize>,
    pub(super) identity: String,
}
pub(super) fn invalid(message: impl Into<String>) -> ServiceError {
    ServiceError::new(AgentErrorCode::ValidationFailed, message)
}
pub(super) fn decode(raw: &str) -> ServiceResult<Account> {
    if raw.len() > MAX_FRAME as usize {
        return Err(invalid("community account exceeds limit"));
    }
    let account: Account =
        serde_json::from_str(raw).map_err(|_| invalid("invalid community account"))?;
    if !PROVIDERS.contains(&account.provider.as_str())
        || !account.models.is_object()
        || account.models.as_object().is_some_and(|m| m.len() > 4096)
    {
        return Err(invalid("invalid community provider or catalog"));
    }
    let auth: Value = serde_json::from_str(account.auth.expose())
        .map_err(|_| invalid("invalid community credentials"))?;
    if !matches!(auth["type"].as_str(), Some("api" | "oauth")) {
        return Err(invalid("invalid community credential type"));
    }
    Ok(account)
}
pub(crate) fn read(vault: &Vault, id: Uuid, marker: &str) -> ServiceResult<SensitiveString> {
    let entry = vault.get_provider_summary(id).map_err(map_vault_error)?;
    if entry.provider_kind != ProviderKind::Official
        || entry.credential_kind != CredentialKind::OAuth
        || !PROVIDERS.contains(&entry.provider_id.as_deref().unwrap_or(""))
        || entry.deleted_at.is_some()
        || entry.archived_at.is_some()
        || !marker.starts_with("aipass:community:")
        || vault.reveal_secret(id).map_err(map_vault_error)? != marker
    {
        return Err(invalid("community account changed or is unavailable"));
    }
    let raw = vault
        .provider_runtime_extension(id, KEY)
        .map_err(map_vault_error)?
        .ok_or_else(|| invalid("community account is missing; reconnect it"))?;
    let account = decode(raw.expose())?;
    if marker != format!("aipass:community:{}", account.generation)
        || Some(account.provider.as_str()) != entry.provider_id.as_deref()
    {
        return Err(invalid("community provider changed; reconnect it"));
    }
    Ok(SensitiveString::new(raw.expose()))
}
pub(crate) fn write(
    vault: &Vault,
    id: Uuid,
    marker: &str,
    revision: u64,
    raw: &str,
) -> ServiceResult<()> {
    let current = decode(read(vault, id, marker)?.expose())?;
    let next = decode(raw)?;
    if current.revision != revision
        || next.revision
            != revision
                .checked_add(1)
                .ok_or_else(|| invalid("account revision overflow"))?
        || next.provider != current.provider
        || next.generation != current.generation
        || next.identity != current.identity
        || next.native_method != current.native_method
    {
        return Err(invalid("stale community credential update refused"));
    }
    let before: Value =
        serde_json::from_str(current.auth.expose()).map_err(ServiceError::internal)?;
    let after: Value = serde_json::from_str(next.auth.expose()).map_err(ServiceError::internal)?;
    ensure_owner(&before, &after).map_err(invalid)?;
    vault
        .set_provider_runtime_extension(id, KEY, Some(&SecretString::new(raw)))
        .map_err(map_vault_error)
}
pub(crate) fn identity(auth: &Value) -> Option<String> {
    [
        auth.get("accountId"),
        auth.pointer("/metadata/email"),
        auth.pointer("/metadata/userId"),
        auth.get("userId"),
        auth.get("uid"),
        auth.get("profileArn"),
    ]
    .into_iter()
    .flatten()
    .find_map(identity_value)
    .or_else(|| {
        let token = auth["access"].as_str().or(auth["key"].as_str())?;
        let data = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(token.split('.').nth(1)?)
            .ok()?;
        let value: Value = serde_json::from_slice(&data).ok()?;
        value["sub"]
            .as_str()
            .or(value["email"].as_str())
            .map(str::to_owned)
    })
}
pub(super) fn identity_value(v: &Value) -> Option<String> {
    v.as_str()
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .or_else(|| v.as_u64().map(|n| n.to_string()))
}
pub(crate) fn ensure_owner(before: &Value, after: &Value) -> Result<(), String> {
    let mut anchored = false;
    for path in [
        "/accountId",
        "/userId",
        "/uid",
        "/profileArn",
        "/metadata/email",
        "/metadata/userId",
    ] {
        if let Some(owner) = before.pointer(path).and_then(identity_value) {
            anchored = true;
            if after.pointer(path).and_then(identity_value).as_ref() != Some(&owner) {
                return Err("subscription account ownership changed; reconnect explicitly".into());
            }
        }
    }
    // Compare the same token claim on both sides, independently of cached
    // account metadata. Adding a profile/email is not a change of JWT subject;
    // keeping stale accountId metadata must not hide an actual subject change.
    for field in ["access", "key"] {
        let claims = |auth: &Value| {
            auth[field]
                .as_str()
                .map(crate::subscriptions::claims)
                .unwrap_or(Value::Null)
        };
        let a = claims(before);
        let b = claims(after);
        for key in ["sub", "email"] {
            if let Some(a) = a.get(key).and_then(identity_value) {
                let b = b.get(key).and_then(identity_value);
                if b.as_ref().is_some_and(|b| b != &a) || (b.is_none() && !anchored) {
                    return Err("subscription token ownership changed; reconnect explicitly".into());
                }
            }
        }
    }
    Ok(())
}
pub(crate) fn is_account(vault: &Vault, id: Uuid) -> bool {
    vault
        .provider_runtime_extension(id, KEY)
        .ok()
        .flatten()
        .is_some()
}

pub(super) fn persist(vault: &Vault, account: Account) -> ServiceResult<Uuid> {
    let raw = SecretString::new(serde_json::to_string(&account).map_err(ServiceError::internal)?);
    decode(raw.expose())?;
    let marker = format!("aipass:community:{}", account.generation);
    let entry = vault
        .add_provider_with_runtime_extension(
            ProviderEntryInput {
                title: format!(
                    "{}{}",
                    account.provider,
                    if account.identity.is_empty() {
                        String::new()
                    } else {
                        format!(" · {}", account.identity)
                    }
                ),
                provider_kind: ProviderKind::Official,
                provider_id: Some(account.provider.clone()),
                credential_kind: CredentialKind::OAuth,
                account_identity: (!account.identity.is_empty()).then_some(account.identity),
                domains: Vec::new(),
                favicon_url: None,
                endpoints: vec![ProviderEndpoint::api("https://community.aipass.invalid/v1")],
                interface_type: InterfaceType::OpenAiCompatible,
                max_concurrent_requests: Some(1),
                supports_websockets: Some(false),
                auth_scheme: AuthScheme::Bearer,
                api_key: marker,
                secret_label: Some("Subscription".into()),
                default_model: account
                    .models
                    .as_object()
                    .and_then(|m| m.keys().next())
                    .cloned(),
                model_aliases: Vec::new(),
                headers: Vec::new(),
                quota: None,
                subscription: Some(SubscriptionSnapshot {
                    source: format!("community:{}", account.provider),
                    observed_at: now(),
                    ..Default::default()
                }),
                gateway: None,
                tags: vec!["subscription".into()],
                notes: None,
                secret_metadata: Default::default(),
            },
            KEY,
            &raw,
        )
        .map_err(map_vault_error)?;
    Ok(entry)
}

pub(crate) fn bind_cli(vault: &Vault, id: Uuid, provider: &str, auth: Value) -> ServiceResult<()> {
    bind_cli_with_models(vault, id, provider, auth, None)
}
pub(super) fn bind_cli_with_models(
    vault: &Vault,
    id: Uuid,
    provider: &str,
    auth: Value,
    live_models: Option<Value>,
) -> ServiceResult<()> {
    let catalog: Value = serde_json::from_str(include_str!("../subscriptions/catalog.json"))
        .map_err(ServiceError::internal)?;
    let models = catalog
        .as_array()
        .and_then(|v| v.iter().find(|p| p["id"] == provider))
        .ok_or_else(|| invalid("unknown CLI subscription"))?["models"]
        .clone();
    let models = live_models.unwrap_or(models);
    let identity = identity(&auth).ok_or_else(|| invalid("CLI account has no identity"))?;
    let account = Account {
        provider: provider.into(),
        generation: Uuid::new_v4(),
        auth: SensitiveString::new(auth.to_string()),
        models,
        revision: 0,
        native_method: Some(1),
        identity: identity.clone(),
    };
    let marker = format!("aipass:community:{}", account.generation);
    let raw = SecretString::new(serde_json::to_string(&account).map_err(ServiceError::internal)?);
    decode(raw.expose())?;
    vault
        .bind_cli_subscription(id, provider, &identity, &marker, KEY, &raw)
        .map_err(map_vault_error)
}

pub(crate) fn register_cli(vault: &Vault, provider: &str, auth: Value) -> ServiceResult<Uuid> {
    register_cli_with_models(vault, provider, auth, None)
}
pub(super) fn register_cli_with_models(
    vault: &Vault,
    provider: &str,
    auth: Value,
    live_models: Option<Value>,
) -> ServiceResult<Uuid> {
    let identity = identity(&auth).ok_or_else(|| invalid("CLI account has no identity"))?;
    for entry in vault.list_provider_summaries().map_err(map_vault_error)? {
        let provider_matches = entry.provider_id.as_deref() == Some(provider)
            || (provider == "codex" && entry.provider_id.as_deref() == Some("openai"))
            || (provider == "grok" && entry.provider_id.as_deref() == Some("xai"));
        let legacy_matches = if provider == "codex" && provider_matches {
            vault
                .list_oauth_accounts(None)
                .map_err(map_vault_error)?
                .iter()
                .any(|a| {
                    a.entry_id == Some(entry.id)
                        && a.account_identity
                            .as_deref()
                            .zip(a.chatgpt_account_id.as_deref())
                            .is_some_and(|(user, workspace)| {
                                identity == format!("{user}:{workspace}")
                            })
                })
        } else {
            false
        };
        if provider_matches
            && entry.provider_kind == ProviderKind::Official
            && entry.credential_kind == CredentialKind::OAuth
            && (entry.account_identity.as_deref() == Some(&identity) || legacy_matches)
        {
            if is_account(vault, entry.id) {
                if let Some(raw) = vault
                    .provider_runtime_extension(entry.id, KEY)
                    .map_err(map_vault_error)?
                {
                    let mut account = decode(raw.expose())?;
                    if serde_json::from_str::<Value>(account.auth.expose())
                        .map_err(ServiceError::internal)?
                        == auth
                    {
                        if let Some(models) = live_models {
                            if models != account.models {
                                let revision = account.revision;
                                account.revision += 1;
                                account.models = models;
                                let marker =
                                    vault.reveal_secret(entry.id).map_err(map_vault_error)?;
                                write(
                                    vault,
                                    entry.id,
                                    &marker,
                                    revision,
                                    &serde_json::to_string(&account)
                                        .map_err(ServiceError::internal)?,
                                )?;
                            }
                        }
                        return Ok(entry.id);
                    }
                }
            }
            bind_cli_with_models(vault, entry.id, provider, auth, live_models)?;
            for legacy in vault.list_oauth_accounts(None).map_err(map_vault_error)? {
                if legacy.entry_id == Some(entry.id) {
                    vault
                        .remove_oauth_account(legacy.id)
                        .map_err(map_vault_error)?;
                }
            }
            return Ok(entry.id);
        }
    }
    let catalog: Value = serde_json::from_str(include_str!("../subscriptions/catalog.json"))
        .map_err(ServiceError::internal)?;
    let models = catalog
        .as_array()
        .and_then(|v| v.iter().find(|p| p["id"] == provider))
        .ok_or_else(|| invalid("unknown CLI subscription"))?["models"]
        .clone();
    let models = live_models.unwrap_or(models);
    persist(
        vault,
        Account {
            provider: provider.into(),
            generation: Uuid::new_v4(),
            auth: SensitiveString::new(auth.to_string()),
            models,
            revision: 0,
            native_method: Some(1),
            identity,
        },
    )
}
