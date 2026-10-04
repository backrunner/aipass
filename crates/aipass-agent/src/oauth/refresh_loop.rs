//! Background refresh of managed OAuth access tokens.
//!
//! Mirrors the idle-lock watcher: a periodic thread that skips while the vault
//! is locked, reads the due accounts under a short lock, performs the network
//! refresh outside any lock, then re-acquires the lock to persist the rotated
//! tokens to the vault, the native CLI file, and the running proxy. A rejected
//! refresh token flips the account to `requires_reauth` so the UI can prompt an
//! interactive re-login instead of silently failing.

use crate::logging::{write_component_log, AGENT_LOG};
use crate::oauth::native_write::{self, NativeSyncOutcome};
use crate::oauth::{clamp_expires_in, now_ms, OAuthError, OAuthManager, TOKEN_REFRESH_BUFFER_MS};
use crate::session::{
    map_vault_error, session_status, with_vault, AgentState, ServiceError, ServiceResult,
};
use aipass_provider_registry::{primary_secret_ref, OAuthProvider};
use aipass_vault::{ManagedOAuthAccount, Vault};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// How often to sweep for tokens that are about to expire. Providers issue
/// hour-scale tokens, so a couple of minutes is plenty granular.
const REFRESH_LOOP_INTERVAL: Duration = Duration::from_secs(30);

/// Written into the mirrored entry secret when a refresh token is rejected, so
/// the proxy stops sending the dead access token.
const REQUIRES_REAUTH_PLACEHOLDER: &str = "aipass:requires-reauth";

pub(crate) fn spawn_token_refresh(state: Arc<AgentState>) {
    std::thread::spawn(move || {
        let mut provider_cache = crate::provider_runtime::Background::default();
        loop {
            if state.shutdown.load(Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(REFRESH_LOOP_INTERVAL);
            if state.shutdown.load(Ordering::Relaxed) {
                break;
            }
            if let Err(err) = refresh_due_accounts(&state) {
                write_component_log(
                    AGENT_LOG,
                    "WARN",
                    &format!("oauth token refresh pass failed: {}", err.message),
                );
            }
            if !session_status(&state).map(|s| s.locked).unwrap_or(true) {
                if let Err(err) = crate::official_accounts::refresh_registered_accounts(&state) {
                    write_component_log(
                        AGENT_LOG,
                        "WARN",
                        &format!("account usage refresh failed: {}", err.message),
                    );
                }
                crate::community::refresh_due(&state);
                let _ = crate::provider_runtime::refresh(&state, &mut provider_cache);
            }
        }
    });
}

/// Minimal clone of an account so the network refresh runs without holding the
/// vault lock (which would also keep the secrets in memory longer than needed).
struct DueAccount {
    id: Uuid,
    provider: OAuthProvider,
    refresh_token: String,
    /// Generation marker captured at read time; a concurrent re-login bumps
    /// it, which tells the persist step to discard our now-stale bundle.
    last_refresh_ms: i64,
}
impl Drop for DueAccount {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.refresh_token.zeroize();
    }
}

fn refresh_due_accounts(state: &Arc<AgentState>) -> ServiceResult<()> {
    // Skip entirely while locked; with_vault would just error on every account.
    if session_status(state)?.locked {
        return Ok(());
    }
    let now = now_ms();
    let due: Vec<DueAccount> = with_vault(state, false, |vault| {
        let accounts = vault.list_oauth_accounts(None).map_err(map_vault_error)?;
        crate::oauth::oauth_manager().retain_refreshes(&accounts);
        Ok(accounts
            .into_iter()
            .filter(|account| {
                !account.requires_reauth
                    && account.expires_at_ms > 0
                    && account.expires_at_ms.saturating_sub(now) < TOKEN_REFRESH_BUFFER_MS
            })
            .map(|account| DueAccount {
                id: account.id,
                provider: account.provider,
                refresh_token: account.refresh_token.clone(),
                last_refresh_ms: account.last_refresh_ms,
            })
            .collect())
    })?;

    for account in due {
        let manager = crate::oauth::oauth_manager();
        let epoch = manager.refresh_epoch();
        let read_native = || {
            with_vault(state, false, |vault| {
                let current = vault
                    .get_oauth_account(account.id)
                    .map_err(map_vault_error)?;
                native_write::newer_native_bundle(&current).map_err(ServiceError::internal)
            })
            .ok()
            .flatten()
        };
        let result = match manager
            .recovered_refresh(account.id, account.last_refresh_ms, &account.refresh_token)
            .or_else(read_native)
        {
            Some(bundle) => Ok(bundle),
            None => match OAuthManager::refresh(account.provider, &account.refresh_token) {
                Err(OAuthError::RefreshTokenInvalid) => {
                    read_native().ok_or(OAuthError::RefreshTokenInvalid)
                }
                result => result,
            },
        };
        match result {
            Ok(bundle) => {
                // Retain a one-use grant across a transient vault write failure;
                // retry persistence, not the already-spent token endpoint.
                if !manager.remember_refresh(
                    epoch,
                    account.id,
                    account.last_refresh_ms,
                    &account.refresh_token,
                    &bundle,
                ) {
                    continue; // lock/replacement revoked this session's work
                }
                match persist_refreshed(
                    state,
                    account.id,
                    account.provider,
                    account.last_refresh_ms,
                    &account.refresh_token,
                    bundle,
                ) {
                    Ok(()) => {
                        manager.forget_refresh(account.id);
                        state.sync_revision.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(err) => {
                        if err.code == aipass_agent_protocol::AgentErrorCode::ValidationFailed {
                            manager.forget_refresh(account.id);
                            let _ = mark_requires_reauth(
                                state,
                                account.id,
                                account.last_refresh_ms,
                                &account.refresh_token,
                            );
                        }
                        write_component_log(
                            AGENT_LOG,
                            "WARN",
                            &format!("oauth token refresh persist failed: {}", err.message),
                        );
                    }
                }
            }
            Err(OAuthError::RefreshTokenInvalid) => {
                if let Err(err) = mark_requires_reauth(
                    state,
                    account.id,
                    account.last_refresh_ms,
                    &account.refresh_token,
                ) {
                    write_component_log(
                        AGENT_LOG,
                        "WARN",
                        &format!("oauth reauth flag failed: {}", err.message),
                    );
                }
            }
            Err(err) => {
                write_component_log(
                    AGENT_LOG,
                    "WARN",
                    &format!("oauth token refresh failed: {err}"),
                );
            }
        }
    }
    reconcile_managed_credentials(state)
}

/// Recover secondary mirrors after an IO failure, and remove expired/rejected
/// credentials from a running snapshot even when the issuer is unavailable.
fn reconcile_managed_credentials(state: &Arc<AgentState>) -> ServiceResult<()> {
    with_vault(state, false, |vault| {
        for account in vault.list_oauth_accounts(None).map_err(map_vault_error)? {
            let Some(id) = account.entry_id else {
                continue;
            };
            let Ok(entry) = vault.get_provider_summary(id) else {
                continue;
            };
            if entry.provider_kind != aipass_provider_registry::ProviderKind::Official
                || entry.credential_kind != aipass_provider_registry::CredentialKind::OAuth
            {
                continue;
            }
            let expected_provider = match account.provider {
                OAuthProvider::Codex => "openai",
                OAuthProvider::Grok => "xai",
            };
            if entry.provider_id.as_deref() != Some(expected_provider) {
                continue;
            }
            let Some(secret) = primary_secret_ref(&entry.secret_refs) else {
                continue;
            };
            let Ok(current) = vault.runtime_provider_credentials(id, &secret.id) else {
                continue;
            };
            let desired = if account.requires_reauth {
                REQUIRES_REAUTH_PLACEHOLDER
            } else {
                &account.access_token
            };
            if current.secret.expose() != desired {
                match vault.update_secret(id, &secret.id, &secret.label, Some(desired.into())) {
                    Ok(_) => {
                        state.sync_revision.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(_) => write_component_log(
                        AGENT_LOG,
                        "WARN",
                        "OAuth credential mirror repair failed",
                    ),
                }
            }
        }
        state
            .proxy
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .reload_if_running(vault)
    })
}

fn persist_refreshed(
    state: &Arc<AgentState>,
    id: Uuid,
    provider: OAuthProvider,
    expected_last_refresh_ms: i64,
    expected_refresh_token: &str,
    bundle: crate::oauth::OAuthTokenBundle,
) -> ServiceResult<()> {
    persist_refreshed_with(
        state,
        id,
        expected_last_refresh_ms,
        expected_refresh_token,
        bundle,
        crate::oauth::oauth_manager().refresh_expiry(
            id,
            expected_last_refresh_ms,
            expected_refresh_token,
        ),
        |vault, account, prior_generation, prior_expires| {
            match provider {
                OAuthProvider::Codex => native_write::sync_codex_auth_json(
                    &vault.config_backup_key(),
                    &account.access_token,
                    &account.refresh_token,
                    account.id_token.as_deref(),
                    account.chatgpt_account_id.as_deref().unwrap_or_default(),
                    Some(prior_generation),
                    account.last_refresh_ms,
                ),
                OAuthProvider::Grok => native_write::sync_grok_auth_json(
                    &vault.config_backup_key(),
                    &account.access_token,
                    &account.refresh_token,
                    Some(prior_expires),
                    account.expires_at_ms,
                    account.account_identity.as_deref(),
                ),
            }
            .unwrap_or_else(|err| {
                write_component_log(
                    AGENT_LOG,
                    "WARN",
                    &format!("oauth native write-back failed: {err}"),
                );
                NativeSyncOutcome::Skipped("native credential file could not be updated".into())
            })
        },
    )
}

fn persist_refreshed_with(
    state: &Arc<AgentState>,
    id: Uuid,
    expected_last_refresh_ms: i64,
    expected_refresh_token: &str,
    bundle: crate::oauth::OAuthTokenBundle,
    received_expiry: Option<i64>,
    sync_native: impl FnOnce(&Vault, &ManagedOAuthAccount, i64, i64) -> NativeSyncOutcome,
) -> ServiceResult<()> {
    let now = now_ms();
    let expires_at_ms = received_expiry.unwrap_or_else(|| {
        now.saturating_add(clamp_expires_in(bundle.expires_in).saturating_mul(1000))
    });
    let expires_at_ms = crate::oauth::jwt_claims(&bundle.access_token)
        .and_then(|c| c["exp"].as_i64())
        .filter(|exp| *exp > 0)
        .map(|exp| expires_at_ms.min(exp.saturating_mul(1000)))
        .unwrap_or(expires_at_ms);
    with_vault(state, false, |vault| {
        let mut account = vault.get_oauth_account(id).map_err(map_vault_error)?;
        // A concurrent re-login while we were doing network I/O bumps
        // last_refresh_ms; discard our stale bundle instead of clobbering the
        // newer tokens (or tripping refresh_token_reused on the next pass).
        if account.last_refresh_ms != expected_last_refresh_ms
            || account.refresh_token != expected_refresh_token
        {
            write_component_log(
                AGENT_LOG,
                "INFO",
                "oauth token refresh discarded: account changed during refresh",
            );
            return Ok(());
        }
        validate_refresh_owner(&account, &bundle)?;
        // Capture our last-known generation BEFORE overwriting it, so the native
        // reconciliation can adopt a CLI-rotated token newer than this.
        let prior_generation = account.last_refresh_ms;
        let prior_expires = account.expires_at_ms;
        account.access_token = bundle.access_token.clone();
        if !bundle.refresh_token.is_empty() {
            account.refresh_token = bundle.refresh_token.clone();
        }
        if bundle.id_token.is_some() {
            account.id_token = bundle.id_token.clone();
        }
        account.expires_at_ms = expires_at_ms;
        account.last_refresh_ms = now;
        account.requires_reauth = false;

        // Persist the issuer's new one-use grant before touching optional CLI
        // mirrors. Even a later adoption/write failure leaves this grant safe.
        vault
            .update_oauth_account(account.clone())
            .map_err(map_vault_error)?;
        let outcome = sync_native(vault, &account, prior_generation, prior_expires);
        if let NativeSyncOutcome::Adopted {
            access_token,
            refresh_token,
            id_token,
            last_refresh_ms,
            expires_at_ms: adopted_expires_at_ms,
        } = outcome
        {
            account.access_token = access_token;
            account.refresh_token = refresh_token;
            if id_token.is_some() {
                account.id_token = id_token;
            }
            account.last_refresh_ms = last_refresh_ms;
            // Keep the stored expiry describing the stored (adopted) token.
            if let Some(adopted_expires_at_ms) = adopted_expires_at_ms {
                account.expires_at_ms = adopted_expires_at_ms;
            }
            vault
                .update_oauth_account(account.clone())
                .map_err(map_vault_error)?;
        }
        // Mirror the live access token into the entry secret the proxy reads.
        if let Some(entry_id) = account.entry_id {
            if let Ok(summary) = vault.get_provider_summary(entry_id) {
                if let Some(secret) = primary_secret_ref(&summary.secret_refs) {
                    vault
                        .update_secret(
                            entry_id,
                            &secret.id,
                            &secret.label,
                            Some(account.access_token.clone()),
                        )
                        .map_err(map_vault_error)?;
                }
            }
        }
        if let Some(entry_id) = account.entry_id {
            // Push the rotated token into a running proxy; recover a poisoned
            // lock rather than panicking the background thread.
            let mut proxy = state.proxy.lock().unwrap_or_else(|err| err.into_inner());
            let _ = proxy.refresh_provider_credentials(vault, entry_id);
        }
        Ok(())
    })
}

fn validate_refresh_owner(
    account: &ManagedOAuthAccount,
    bundle: &crate::oauth::OAuthTokenBundle,
) -> ServiceResult<()> {
    if bundle.access_token.trim().is_empty()
        || matches!((&account.chatgpt_account_id, &bundle.chatgpt_account_id), (Some(a), Some(b)) if a != b)
    {
        return Err(ServiceError::new(
            aipass_agent_protocol::AgentErrorCode::ValidationFailed,
            "OAuth refresh changed account or returned no token",
        ));
    }
    for (before, after) in [
        (account.id_token.as_deref(), bundle.id_token.as_deref()),
        (
            Some(account.access_token.as_str()),
            Some(bundle.access_token.as_str()),
        ),
    ] {
        if let (Some(before), Some(after)) = (
            before.and_then(crate::oauth::jwt_claims),
            after.and_then(crate::oauth::jwt_claims),
        ) {
            for key in ["sub", "email"] {
                if matches!((before.get(key).and_then(serde_json::Value::as_str), after.get(key).and_then(serde_json::Value::as_str)), (Some(a), Some(b)) if a != b)
                {
                    return Err(ServiceError::new(
                        aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                        "OAuth refresh changed account identity",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn mark_requires_reauth(
    state: &Arc<AgentState>,
    id: Uuid,
    expected_last_refresh_ms: i64,
    expected_refresh_token: &str,
) -> ServiceResult<()> {
    with_vault(state, false, |vault| {
        let mut account = vault.get_oauth_account(id).map_err(map_vault_error)?;
        if account.last_refresh_ms != expected_last_refresh_ms
            || account.refresh_token != expected_refresh_token
        {
            return Ok(());
        }
        account.requires_reauth = true;
        // The grant is dead, so the mirrored access token will never be
        // rotated again: quarantine it so the proxy stops sending it.
        account.access_token.clear();
        let entry_id = account.entry_id;
        // Quarantine authority first; a missing/failed secret mirror must not
        // leave the account eligible to spend this dead grant again.
        vault
            .update_oauth_account(account)
            .map_err(map_vault_error)?;
        state.sync_revision.fetch_add(1, Ordering::Relaxed);
        if let Some(entry_id) = entry_id {
            if let Ok(summary) = vault.get_provider_summary(entry_id) {
                if let Some(secret) = primary_secret_ref(&summary.secret_refs) {
                    vault
                        .update_secret(
                            entry_id,
                            &secret.id,
                            &secret.label,
                            Some(REQUIRES_REAUTH_PLACEHOLDER.to_string()),
                        )
                        .map_err(map_vault_error)?;
                }
            }
        }
        if let Some(entry_id) = entry_id {
            // Propagate the quarantined secret to a running proxy, same as the
            // refresh path does for rotated tokens.
            let mut proxy = state.proxy.lock().unwrap_or_else(|err| err.into_inner());
            let _ = proxy.refresh_provider_credentials(vault, entry_id);
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{InitialSyncState, SessionState};
    use aipass_agent_protocol::{SensitiveString, SessionPolicy};
    use std::sync::{atomic::AtomicBool, Condvar, Mutex};

    fn fixture() -> (
        tempfile::TempDir,
        Arc<AgentState>,
        ManagedOAuthAccount,
        Uuid,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let vault_dir = dir.path().join("vault");
        let creation = aipass_vault::Vault::create(
            &vault_dir,
            &aipass_crypto::SecretString::new("test password"),
        )
        .unwrap();
        let entry_id = crate::official_accounts::persist_login_account(
            &creation.vault,
            "openai",
            Some("alice".into()),
            Some("workspace".into()),
            "new-access".into(),
            None,
        )
        .unwrap();
        let account = aipass_vault::ManagedOAuthAccount {
            id: Uuid::new_v4(),
            provider: OAuthProvider::Codex,
            account_identity: Some("alice".into()),
            chatgpt_account_id: Some("workspace".into()),
            access_token: "new-access".into(),
            refresh_token: "new-refresh".into(),
            id_token: None,
            expires_at_ms: now_ms() + 3_600_000,
            last_refresh_ms: 42,
            entry_id: Some(entry_id),
            is_default: true,
            requires_reauth: false,
            authenticated_at: time::OffsetDateTime::now_utc(),
        };
        creation.vault.add_oauth_account(account.clone()).unwrap();
        let state = Arc::new(AgentState {
            control_panel: Default::default(),
            policy: Mutex::new(SessionPolicy::default()),
            vault_dir: vault_dir.clone(),
            namespace: "test".into(),
            auth_token: SensitiveString::from("test"),
            session: Mutex::new(SessionState::Locked),
            session_changed: Condvar::new(),
            last_lock_reason: Mutex::new(None),
            proxy: Mutex::new(crate::proxy_service::ProxyService::new(&vault_dir).unwrap()),
            favicon_backfill: Mutex::new(()),
            sync_lock: Mutex::new(()),
            cloudkit: Default::default(),
            webdav_transport: Mutex::new(None),
            sync_wake: std::sync::atomic::AtomicU64::new(0),
            initial_sync: Mutex::new(InitialSyncState::Done),
            sync_revision: std::sync::atomic::AtomicU64::new(0),
            sync_status: Mutex::new(None),
            sync_watcher: Mutex::new(None),
            shutdown: AtomicBool::new(false),
        });
        crate::session::set_session_vault(&state, creation.vault);
        (dir, state, account, entry_id)
    }

    fn bundle() -> crate::oauth::OAuthTokenBundle {
        crate::oauth::OAuthTokenBundle {
            access_token: "rotated-access".into(),
            refresh_token: "rotated-refresh".into(),
            id_token: None,
            chatgpt_account_id: None,
            account_identity: None,
            expires_in: 3600,
        }
    }

    #[test]
    fn rotated_grant_is_durable_before_native_mirrors_and_missing_secrets() {
        let (_dir, state, account, entry_id) = fixture();
        persist_refreshed_with(
            &state,
            account.id,
            42,
            "new-refresh",
            bundle(),
            None,
            |vault, saved, _, _| {
                let durable = vault.get_oauth_account(saved.id).unwrap();
                assert_eq!(durable.refresh_token, "rotated-refresh");
                assert_eq!(durable.access_token, "rotated-access");
                let secret = vault.get_provider_summary(entry_id).unwrap().secret_refs[0]
                    .id
                    .clone();
                vault.remove_secret(entry_id, &secret).unwrap();
                NativeSyncOutcome::Skipped("test mirror failure".into())
            },
        )
        .unwrap();
        with_vault(&state, false, |vault| {
            assert_eq!(
                vault.get_oauth_account(account.id).unwrap().refresh_token,
                "rotated-refresh"
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn omitted_refresh_and_id_tokens_preserve_the_prior_generation_fields() {
        let (_dir, state, account, _) = fixture();
        let mut next = bundle();
        next.refresh_token.clear();
        persist_refreshed_with(
            &state,
            account.id,
            42,
            "new-refresh",
            next,
            None,
            |_, _, _, _| NativeSyncOutcome::Written,
        )
        .unwrap();
        with_vault(&state, false, |vault| {
            let saved = vault.get_oauth_account(account.id).unwrap();
            assert_eq!(saved.access_token, "rotated-access");
            assert_eq!(saved.refresh_token, "new-refresh");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn retry_persistence_does_not_extend_an_opaque_tokens_original_expiry() {
        let (_dir, state, account, _) = fixture();
        persist_refreshed_with(
            &state,
            account.id,
            42,
            "new-refresh",
            bundle(),
            Some(1),
            |_, _, _, _| NativeSyncOutcome::Written,
        )
        .unwrap();
        with_vault(&state, false, |vault| {
            assert_eq!(
                vault.get_oauth_account(account.id).unwrap().expires_at_ms,
                1
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn credential_mirror_repairs_are_idempotent() {
        let (_dir, state, _, entry_id) = fixture();
        with_vault(&state, false, |vault| {
            let secret = vault.get_provider_summary(entry_id).unwrap().secret_refs[0].clone();
            vault
                .update_secret(
                    entry_id,
                    &secret.id,
                    &secret.label,
                    Some("stale-mirror".into()),
                )
                .unwrap();
            Ok(())
        })
        .unwrap();
        reconcile_managed_credentials(&state).unwrap();
        let revision = state.sync_revision.load(Ordering::Relaxed);
        assert_eq!(revision, 1);
        reconcile_managed_credentials(&state).unwrap();
        assert_eq!(state.sync_revision.load(Ordering::Relaxed), revision);
        with_vault(&state, false, |vault| {
            let secret = vault.get_provider_summary(entry_id).unwrap().secret_refs[0].clone();
            assert_eq!(
                vault
                    .runtime_provider_credentials(entry_id, &secret.id)
                    .unwrap()
                    .secret
                    .expose(),
                "new-access"
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn refresh_cannot_replace_the_workspace_or_token_subject() {
        use base64::Engine;
        let (_dir, state, mut account, _) = fixture();
        let jwt = |sub: &str| {
            format!(
                "header.{}.signature",
                base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .encode(serde_json::json!({"sub":sub}).to_string())
            )
        };
        account.id_token = Some(jwt("alice"));
        let mut next = bundle();
        next.chatgpt_account_id = Some("other-workspace".into());
        assert!(validate_refresh_owner(&account, &next).is_err());
        next.chatgpt_account_id = None;
        next.id_token = Some(jwt("bob"));
        assert!(validate_refresh_owner(&account, &next).is_err());
        next.id_token = Some(jwt("alice"));
        validate_refresh_owner(&account, &next).unwrap();
        drop(state);
    }

    #[test]
    fn stale_refresh_success_and_failure_cannot_clobber_a_new_login() {
        let (_dir, state, account, entry_id) = fixture();
        // Same timestamp deliberately covers two rotations within one millisecond.
        mark_requires_reauth(&state, account.id, 42, "old-refresh").unwrap();
        persist_refreshed(
            &state,
            account.id,
            account.provider,
            42,
            "old-refresh",
            crate::oauth::OAuthTokenBundle {
                access_token: "stale-access".into(),
                refresh_token: "stale-refresh".into(),
                id_token: None,
                chatgpt_account_id: None,
                account_identity: None,
                expires_in: 3600,
            },
        )
        .unwrap();
        with_vault(&state, false, |vault| {
            let saved = vault.get_oauth_account(account.id).unwrap();
            assert!(!saved.requires_reauth);
            assert_eq!(saved.refresh_token, "new-refresh");
            assert_eq!(vault.reveal_secret(entry_id).unwrap(), "new-access");
            Ok(())
        })
        .unwrap();
        mark_requires_reauth(&state, account.id, 42, "new-refresh").unwrap();
        with_vault(&state, false, |vault| {
            assert!(vault.get_oauth_account(account.id).unwrap().requires_reauth);
            assert_eq!(
                vault.reveal_secret(entry_id).unwrap(),
                REQUIRES_REAUTH_PLACEHOLDER
            );
            Ok(())
        })
        .unwrap();
    }
}
