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
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// How often to sweep for tokens that are about to expire. Providers issue
/// hour-scale tokens, so a couple of minutes is plenty granular.
const REFRESH_LOOP_INTERVAL: Duration = Duration::from_secs(120);

/// Written into the mirrored entry secret when a refresh token is rejected, so
/// the proxy stops sending the dead access token.
const REQUIRES_REAUTH_PLACEHOLDER: &str = "aipass:requires-reauth";

pub(crate) fn spawn_token_refresh(state: Arc<AgentState>) {
    std::thread::spawn(move || loop {
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

fn refresh_due_accounts(state: &Arc<AgentState>) -> ServiceResult<()> {
    // Skip entirely while locked; with_vault would just error on every account.
    if session_status(state)?.locked {
        return Ok(());
    }
    let now = now_ms();
    let due: Vec<DueAccount> = with_vault(state, false, |vault| {
        let accounts = vault.list_oauth_accounts(None).map_err(map_vault_error)?;
        Ok(accounts
            .into_iter()
            .filter(|account| {
                !account.requires_reauth
                    && account.expires_at_ms > 0
                    && account.expires_at_ms - now < TOKEN_REFRESH_BUFFER_MS
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
        let result = match read_native() {
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
                if let Err(err) = persist_refreshed(
                    state,
                    account.id,
                    account.provider,
                    account.last_refresh_ms,
                    &account.refresh_token,
                    bundle,
                ) {
                    write_component_log(
                        AGENT_LOG,
                        "WARN",
                        &format!("oauth token refresh persist failed: {}", err.message),
                    );
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
    Ok(())
}

fn persist_refreshed(
    state: &Arc<AgentState>,
    id: Uuid,
    provider: OAuthProvider,
    expected_last_refresh_ms: i64,
    expected_refresh_token: &str,
    bundle: crate::oauth::OAuthTokenBundle,
) -> ServiceResult<()> {
    let now = now_ms();
    let expires_at_ms =
        now.saturating_add(clamp_expires_in(bundle.expires_in).saturating_mul(1000));
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

        let outcome = match provider {
            OAuthProvider::Codex => native_write::sync_codex_auth_json(
                &vault.config_backup_key(),
                &account.access_token,
                &account.refresh_token,
                account.id_token.as_deref(),
                account.chatgpt_account_id.as_deref().unwrap_or_default(),
                Some(prior_generation),
                now,
            ),
            OAuthProvider::Grok => native_write::sync_grok_auth_json(
                &vault.config_backup_key(),
                &account.access_token,
                &account.refresh_token,
                Some(prior_expires),
                expires_at_ms,
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
        });
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
        }

        // Commit the rotated refresh token before updating dependent mirrors.
        // A missing entry/secret must not discard a one-use token generation.
        vault
            .update_oauth_account(account.clone())
            .map_err(map_vault_error)?;
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
        vault
            .update_oauth_account(account)
            .map_err(map_vault_error)?;
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

    #[test]
    fn stale_refresh_success_and_failure_cannot_clobber_a_new_login() {
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
            initial_sync: Mutex::new(InitialSyncState::Done),
            sync_watcher: Mutex::new(None),
            shutdown: AtomicBool::new(false),
        });
        crate::session::set_session_vault(&state, creation.vault);
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
