//! Synchronized account descriptors refer to grants owned by this device/vault.
use super::*;
use crate::subscription_import::reader::PrivateAuth;

pub(super) const KEY: &str = "community_account_v3";
pub(super) const RECONNECT: &str =
    "Subscription requires local authorization; reconnect this account on this device";

pub(crate) fn supports_local_authorization(provider: &str) -> bool {
    crate::provider_plugins::is_plugin(provider)
        || matches!(
            provider,
            "factory"
                | "antigravity"
                | "minimax-code"
                | "minimax-code-global"
                | "cline"
                | "trae-cn"
                | "trae-global"
                | "qoder"
                | "qoder-cn"
                | "kiro"
                | "workbuddy"
                | "workbuddy-ai"
                | "cursor"
        )
}
pub(super) fn managed(provider: &str, auth: &Value) -> bool {
    crate::provider_plugins::is_plugin(provider)
        || supports_local_authorization(provider)
            && ["refresh", "deviceRefresh"].iter().any(|key| {
                auth[*key]
                    .as_str()
                    .is_some_and(|v| !v.is_empty() && v != "cursor-agent")
            })
}

fn auth(account: &Account) -> ServiceResult<PrivateAuth> {
    serde_json::from_str(account.auth.expose())
        .map(PrivateAuth)
        .map_err(ServiceError::internal)
}
pub(super) fn requires_local(account: &Account) -> ServiceResult<bool> {
    Ok(managed(&account.provider, &auth(account)?.0))
}
pub(super) fn descriptor(account: &Account) -> ServiceResult<Account> {
    let full = auth(account)?;
    let mut public = json!({"type":"oauth","deviceCredentials":1});
    for key in [
        "accountId",
        "userId",
        "uid",
        "profileArn",
        "orgId",
        "region",
    ] {
        if let Some(value) = full.0.get(key) {
            public[key] = value.clone();
        }
    }
    for key in ["email", "userId", "server", "region"] {
        if let Some(value) = full.0["metadata"].get(key) {
            public["metadata"][key] = value.clone();
        }
    }
    let mut next = account.clone();
    next.auth = SensitiveString::new(public.to_string());
    next.local_binding = None;
    next.revision = 0;
    next.feedback = Default::default();
    Ok(next)
}
pub(super) fn local(vault: &Vault, descriptor: &Account) -> ServiceResult<Option<Account>> {
    let owner = crate::subscriptions::cli_accounts::device().map_err(invalid)?;
    let Some(raw) = vault
        .read_local_subscription(descriptor.generation, &owner)
        .map_err(|_| invalid(RECONNECT))?
    else {
        return Ok(None);
    };
    let account = decode(raw.expose())?;
    if account.generation != descriptor.generation
        || account.provider != descriptor.provider
        || account.identity != descriptor.identity
        || account.local_binding.is_none()
    {
        return Err(invalid("local subscription binding changed; reconnect"));
    }
    ensure_owner(&auth(descriptor)?.0, &auth(&account)?.0).map_err(invalid)?;
    Ok(Some(account))
}
pub(super) fn save(vault: &Vault, account: &Account) -> ServiceResult<()> {
    if account.local_binding.is_none() {
        return Err(invalid("missing local binding"));
    }
    let raw = SecretString::new(serde_json::to_string(account).map_err(ServiceError::internal)?);
    vault
        .write_local_subscription(
            account.generation,
            &crate::subscriptions::cli_accounts::device().map_err(invalid)?,
            &raw,
        )
        .map_err(map_vault_error)
}

/// Write the local grant before publishing a descriptor. If the record write
/// fails, the previous synchronized authorization remains recoverable.
pub(super) fn prepare(vault: &Vault, account: &mut Account) -> ServiceResult<()> {
    if account.identity.trim().is_empty() {
        return Err(invalid("subscription returned no account identity"));
    }
    account.local_binding = Some(Uuid::new_v4());
    save(vault, account)
}

pub(super) fn bind(vault: &Vault, id: Uuid, mut next: Account) -> ServiceResult<()> {
    let raw = stored(vault, id)?.ok_or_else(|| invalid("subscription unavailable"))?;
    let previous = decode(raw.expose())?;
    if previous.provider != next.provider || previous.identity != next.identity {
        return Err(invalid(
            "subscription account changed; reconnect the original account",
        ));
    }
    let before = auth(&previous)?;
    let after = auth(&next)?;
    ensure_owner(&before.0, &after.0).map_err(invalid)?;
    for key in ["orgId", "region"] {
        if !before.0[key].is_null() && before.0[key] != after.0[key] {
            return Err(invalid(
                "subscription scope changed; reconnect the original account",
            ));
        }
    }
    if !managed(&next.provider, &after.0) {
        return Err(invalid("local authorization returned no refresh grant"));
    }
    if account::storage_key(vault, id)? == KEY {
        next.generation = previous.generation;
        next.revision = local(vault, &previous)
            .ok()
            .flatten()
            .map(|a| a.revision)
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| invalid("account revision overflow"))?;
        prepare(vault, &mut next)?;
        return Ok(());
    }
    if ["refresh", "deviceRefresh"].iter().any(|key| {
        before.0[*key].as_str().is_some_and(|v| !v.is_empty()) && before.0[*key] == after.0[*key]
    }) {
        return Err(invalid(
            "Fresh local authorization required; sign in again before reconnecting",
        ));
    }
    prepare(vault, &mut next)?;
    let public = descriptor(&next)?;
    vault
        .bind_cli_subscription(
            id,
            &next.provider,
            &next.identity,
            &format!("aipass:community:{}", next.generation),
            KEY,
            &SecretString::new(serde_json::to_string(&public).map_err(ServiceError::internal)?),
        )
        .map_err(map_vault_error)
}

#[cfg(test)]
mod tests;
