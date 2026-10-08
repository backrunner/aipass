use super::*;
use reader::{Account, PrivateAuth};

pub(super) fn commit(
    state: &Arc<AgentState>,
    session: Uuid,
    source: SubscriptionImportSource,
    account: Account,
    revisions: &mut HashMap<Uuid, time::OffsetDateTime>,
    seen: &mut HashMap<(String, String), Uuid>,
) -> SubscriptionImportResult {
    let identity = match &account {
        Account::Claude(a) => Some(a.identity.clone()),
        Account::Auth(a) => serde_json::from_str::<Value>(a.expose())
            .ok()
            .and_then(|v| {
                let v = PrivateAuth(v);
                crate::community::identity(&v.0)
            }),
    };
    let Some(identity) = identity else {
        return failure(
            source,
            SubscriptionImportStatus::Failed,
            "identity_missing",
            "login",
        );
    };
    let key = (source.provider.clone(), identity.clone());
    let result = (|| -> ServiceResult<(Uuid, SubscriptionImportStatus)> {
        let guard = state
            .session
            .lock()
            .map_err(|_| validation("session unavailable"))?;
        let info = match &*guard {
            SessionState::Unlocked(info) if info.id == session => info,
            _ => return Err(ServiceError::new(AgentErrorCode::Locked, "session changed")),
        };
        crate::control_panel::check_session(info)?;
        info.vault.ensure_sync_ready().map_err(map_vault_error)?;
        let entries = info
            .vault
            .list_provider_summaries()
            .map_err(map_vault_error)?;
        let existing = entries.iter().find(|e| {
            sources::canonical_provider(e.provider_id.as_deref().unwrap_or(""))
                == Some(source.provider.as_str())
                && e.credential_kind == CredentialKind::OAuth
                && (e.account_identity.as_deref() == Some(&identity)
                    || match &account {
                        Account::Auth(a) => serde_json::from_str::<Value>(a.expose())
                            .ok()
                            .is_some_and(|auth| {
                                let auth = PrivateAuth(auth);
                                crate::community::legacy_scope_matches(&info.vault, e, &auth.0)
                                    .unwrap_or(false)
                            }),
                        _ => false,
                    })
        });
        if let Some(id) = seen.get(&key) {
            return Ok((*id, SubscriptionImportStatus::Existing));
        }
        if let Some(entry) = existing {
            if revisions.get(&entry.id) != Some(&entry.updated_at) {
                return Err(ServiceError::new(
                    AgentErrorCode::Conflict,
                    "account changed during discovery",
                ));
            }
            if let Account::Auth(auth) = &account {
                if same_binding(&info.vault, entry.id, &source, auth)? {
                    seen.insert(key, entry.id);
                    return Ok((entry.id, SubscriptionImportStatus::Existing));
                }
            }
        }
        let id = match account {
            Account::Claude(native) => {
                crate::official_accounts::persist_claude_login(&info.vault, &native)
                    .map_err(ServiceError::internal)?
            }
            Account::Auth(auth) => {
                let mut parsed = PrivateAuth(
                    serde_json::from_str(auth.expose())
                        .map_err(|_| validation("invalid account"))?,
                );
                crate::community::register_cli(&info.vault, &source.provider, parsed.0.take())?
            }
        };
        let updated = info
            .vault
            .list_provider_summaries()
            .map_err(map_vault_error)?
            .into_iter()
            .find(|e| e.id == id)
            .ok_or_else(|| validation("account commit missing"))?;
        let status = match existing {
            None => SubscriptionImportStatus::Imported,
            Some(old) if old.updated_at == updated.updated_at => SubscriptionImportStatus::Existing,
            _ => SubscriptionImportStatus::Updated,
        };
        revisions.insert(id, updated.updated_at);
        seen.insert(key, id);
        let mut proxy = state.proxy.lock().unwrap_or_else(|p| p.into_inner());
        if proxy.refresh_provider_credentials(&info.vault, id).is_err() {
            // A committed record must not be replayed as a new login.
            if proxy.reload_if_running(&info.vault).is_err() {
                return Err(ServiceError::new(
                    AgentErrorCode::Internal,
                    "proxy_refresh_failed",
                ));
            }
        }
        Ok((id, status))
    })();
    match result {
        Ok((id, status)) => SubscriptionImportResult {
            source_id: Uuid::new_v4(),
            source,
            account_identity: Some(identity),
            status,
            entry_id: Some(id),
            error_code: None,
            action: None,
        },
        Err(e) => {
            let (status, code) = if e.code == AgentErrorCode::Locked {
                (SubscriptionImportStatus::Cancelled, "session_changed")
            } else if e.code == AgentErrorCode::Conflict {
                (SubscriptionImportStatus::Failed, "account_changed")
            } else {
                (SubscriptionImportStatus::Failed, "commit_failed")
            };
            let mut r = failure(source, status, code, "retry");
            r.account_identity = Some(identity);
            r
        }
    }
}

fn same_binding(
    vault: &aipass_vault::Vault,
    id: Uuid,
    source: &SubscriptionImportSource,
    incoming: &SensitiveString,
) -> ServiceResult<bool> {
    let Some(raw) = vault
        .provider_runtime_extension(id, "community_account_v1")
        .map_err(map_vault_error)?
    else {
        return Ok(false);
    };
    let bundle =
        PrivateAuth(serde_json::from_str(raw.expose()).map_err(|_| validation("invalid binding"))?);
    let old = PrivateAuth(
        serde_json::from_str(bundle.0["auth"].as_str().unwrap_or("null"))
            .map_err(|_| validation("invalid binding"))?,
    );
    let next = PrivateAuth(
        serde_json::from_str(incoming.expose()).map_err(|_| validation("invalid account"))?,
    );
    let bound = serde_json::from_value::<SubscriptionImportSource>(old.0["nativeSource"].clone())
        .ok()
        .and_then(|s| sources::normalize(s).ok());
    if bound != Some(sources::normalize(source.clone())?)
        || old.0["nativeDevice"] != next.0["nativeDevice"]
    {
        return Ok(false);
    }
    crate::community::ensure_owner(&old.0, &next.0).map_err(validation)?;
    Ok(true)
}
