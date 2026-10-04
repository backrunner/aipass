//! Discovery and provider-owned usage refresh for official CLI accounts.
//!
//! The agent is the only component allowed to read these files or hold their
//! access tokens. The result contains no credential material; tokens are used
//! only for the short-lived refresh request and are immediately dropped.

use aipass_agent_protocol::OfficialAccountRefreshResult;
use aipass_provider_registry::{
    AuthScheme, CredentialKind, InterfaceType, ProviderEndpoint, ProviderKind,
    SubscriptionSnapshot, SubscriptionWindow,
};
use aipass_vault::{ProviderEntryInput, Vault};
use base64::Engine as _;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

const USAGE_HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const GROK_BILLING_ENDPOINT: &str =
    "https://grok.com/grok_api_v2.GrokBuildBilling/GetGrokCreditsConfig";
/// ChatGPT/Codex OAuth access tokens are rejected by api.openai.com; they
/// only work against the Codex backend's Responses API.
const CODEX_OAUTH_ENDPOINT: &str = "https://chatgpt.com/backend-api/codex";
/// `~/.grok/auth.json` holds xAI OIDC session tokens for the Grok CLI chat
/// proxy, not the public api.x.ai API. The proxy requires the
/// `X-XAI-Token-Auth` marker header documented in xai-org/grok-build.
const GROK_CLI_PROXY_ENDPOINT: &str = "https://cli-chat-proxy.grok.com/v1";

type ClaudeUsage = (
    Vec<SubscriptionWindow>,
    Option<String>,
    Option<String>,
    Option<String>,
);

#[derive(Clone)]
struct DiscoveredAccount {
    provider_id: &'static str,
    identity: Option<String>,
    /// Codex `tokens.account_id`; sent upstream as the `chatgpt-account-id`
    /// header required by the Codex OAuth backend.
    account_id: Option<String>,
    token: String,
    native_credentials: Option<String>,
    refresh_bundle: Option<crate::oauth::OAuthTokenBundle>,
    credential_expires_at: Option<String>,
    plan: Option<String>,
}

impl std::fmt::Debug for DiscoveredAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DiscoveredAccount([REDACTED])")
    }
}
impl Drop for DiscoveredAccount {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.token.zeroize();
        self.native_credentials.zeroize();
    }
}

/// A discovered account plus its freshly fetched usage snapshot.
///
/// Collected without touching the vault so the slow discovery and network
/// work never runs while the session lock is held.
pub(crate) struct CollectedAccount {
    account: DiscoveredAccount,
    snapshot: Option<SubscriptionSnapshot>,
}

/// Refresh only accounts already admitted to the vault. A background sweep
/// never imports a newly selected CLI identity or overwrites a concurrent login.
pub(crate) fn refresh_registered_accounts(
    state: &std::sync::Arc<crate::session::AgentState>,
) -> crate::session::ServiceResult<()> {
    use crate::session::{map_vault_error, with_vault, ServiceError};
    let accounts = with_vault(state, false, |vault| {
        let mut accounts = Vec::new();
        for entry in vault.list_provider_summaries().map_err(map_vault_error)? {
            if entry.provider_kind != ProviderKind::Official
                || entry.credential_kind != CredentialKind::OAuth
            {
                continue;
            }
            let preferences = crate::provider_runtime::load(vault, entry.id)?;
            if !preferences.quota_tracking {
                continue;
            }
            if entry
                .subscription
                .as_ref()
                .and_then(|s| OffsetDateTime::parse(&s.observed_at, &Rfc3339).ok())
                .is_some_and(|at| {
                    let age = (OffsetDateTime::now_utc() - at).whole_seconds();
                    age >= 0 && age < i64::from(preferences.quota_refresh_seconds)
                })
            {
                continue;
            }
            let provider_id = match entry.provider_id.as_deref() {
                Some("openai" | "codex") => "openai",
                Some("anthropic") => "anthropic",
                Some("xai") => "xai",
                Some("copilot") => "copilot",
                _ => continue,
            };
            let Some(secret) = aipass_provider_registry::primary_secret_ref(&entry.secret_refs)
            else {
                continue;
            };
            let credentials = vault
                .runtime_provider_credentials(entry.id, &secret.id)
                .map_err(map_vault_error)?;
            if credentials.secret.expose().starts_with("aipass:") {
                continue;
            }
            let account_id = credentials
                .headers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("chatgpt-account-id"))
                .map(|(_, v)| v.clone());
            let native_expiry = if provider_id == "anthropic" {
                vault
                    .provider_runtime_extension(entry.id, "claude_native")
                    .map_err(map_vault_error)?
                    .and_then(|v| serde_json::from_str::<Value>(v.expose()).ok())
                    .and_then(|v| find_timestamp(&v["claudeAiOauth"], &["expiresAt"]))
            } else {
                None
            };
            accounts.push((
                entry.id,
                vault.fingerprint_secret(credentials.secret.expose()),
                DiscoveredAccount {
                    native_credentials: None,
                    provider_id,
                    identity: entry.account_identity,
                    account_id,
                    token: credentials.secret.expose().to_owned(),
                    refresh_bundle: None,
                    credential_expires_at: native_expiry.or_else(|| {
                        entry
                            .subscription
                            .as_ref()
                            .and_then(|s| s.credential_expires_at.clone())
                    }),
                    plan: entry.subscription.as_ref().and_then(|s| s.plan.clone()),
                },
            ));
        }
        Ok(accounts)
    })?;
    let native_claude = if accounts
        .iter()
        .any(|(_, _, a)| a.provider_id == "anthropic")
    {
        discover_claude_accounts()
    } else {
        Vec::new()
    };
    for (id, generation, mut account) in accounts {
        // Claude Code owns its rotating grant. Reconcile only a proven identity;
        // a native account switch must never change a saved account's owner.
        if account.provider_id == "anthropic" {
            if let Some(native) = native_claude.iter().find(|native| {
                native.token == account.token
                    || account.identity.as_ref().is_some_and(|identity| {
                        !identity.starts_with("account:")
                            && native.identity.as_ref() == Some(identity)
                            && native
                                .credential_expires_at
                                .as_deref()
                                .and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok())
                                > account
                                    .credential_expires_at
                                    .as_deref()
                                    .and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok())
                    })
            }) {
                account = native.clone();
            }
        }
        let snapshot = refresh_snapshot(&account);
        with_vault(state, false, |vault| {
            let Ok(current) = vault.get_provider_summary(id) else {
                return Ok(());
            };
            let secret = primary_secret_ref(&current).map_err(ServiceError::internal)?;
            let credentials = vault
                .runtime_provider_credentials(id, &secret.id)
                .map_err(map_vault_error)?;
            if vault.fingerprint_secret(credentials.secret.expose()) != generation {
                return Ok(());
            }
            if credentials.secret.expose() != account.token {
                refresh_account_secret(vault, id, &account.token)
                    .map_err(ServiceError::internal)?;
            }
            if let Some(value) = account.native_credentials.as_ref() {
                vault
                    .set_provider_runtime_extension(
                        id,
                        "claude_native",
                        Some(&aipass_crypto::SecretString::new(value)),
                    )
                    .map_err(map_vault_error)?;
            }
            vault
                .update_provider_subscription(id, merge_snapshot(current.subscription, snapshot))
                .map_err(map_vault_error)?;
            state
                .proxy
                .lock()
                .map_err(|_| ServiceError::internal(anyhow::anyhow!("proxy lock poisoned")))?
                .refresh_provider_credentials(vault, id)?;
            Ok(())
        })?;
    }
    Ok(())
}

/// Discover local official CLI credentials and fetch their provider-owned
/// usage snapshots. Runs subprocesses and network I/O; call it outside of
/// any vault/session lock.
pub(crate) fn collect_official_accounts(provider_ids: &[String]) -> Vec<CollectedAccount> {
    let requested =
        |id: &str| provider_ids.is_empty() || provider_ids.iter().any(|item| item == id);
    let mut discovered = Vec::new();
    if requested("openai") {
        discovered.extend(discover_codex_accounts());
    }
    if requested("anthropic") {
        discovered.extend(discover_claude_accounts());
    }
    if requested("copilot") {
        discovered.extend(discover_copilot_accounts());
    }
    if requested("xai") {
        discovered.extend(discover_grok_accounts());
    }

    let mut seen_accounts = HashSet::new();
    discovered
        .into_iter()
        .filter(|account| seen_accounts.insert((account.provider_id, account.token.clone())))
        .map(|account| {
            let snapshot = refresh_snapshot(&account);
            CollectedAccount { account, snapshot }
        })
        .collect()
}

/// Persist collected accounts into the vault. A failure on one account is
/// reported in its result and never aborts the remaining accounts. Each
/// result carries the vault entry it imported or refreshed so callers can
/// propagate credential changes to a running proxy.
pub(crate) fn persist_official_accounts(
    vault: &Vault,
    collected: Vec<CollectedAccount>,
) -> anyhow::Result<Vec<(OfficialAccountRefreshResult, Option<uuid::Uuid>)>> {
    // Archived entries still belong to the user and must dedupe/refresh in
    // place; only trashed entries (deleted_at) are forgotten. Both listing
    // variants already skip trash.
    let mut existing = vault.list_provider_summaries()?;
    existing.extend(vault.list_archived_provider_summaries()?);
    let mut batch_imported = HashSet::new();
    let mut results = Vec::new();
    for item in collected {
        let provider_id = item.account.provider_id.to_string();
        let identity = item.account.identity.clone();
        match persist_account(vault, &mut existing, &mut batch_imported, item) {
            Ok(result) => results.push(result),
            Err(error) => results.push((
                OfficialAccountRefreshResult {
                    provider_id,
                    account_identity: identity,
                    credential_kind: CredentialKind::OAuth,
                    snapshot: None,
                    status: "error".to_string(),
                    error: Some(error.to_string()),
                },
                None,
            )),
        }
    }
    Ok(results)
}

/// Persist an in-app OAuth login through the same path as a CLI import, so a
/// logged-in account and a discovered one produce identical provider entries.
/// Does no network I/O (it runs inside the vault lock); the usage snapshot is
/// enriched later by the background refresh / usage-probe paths.
pub(crate) fn persist_login_account(
    vault: &Vault,
    provider_id: &'static str,
    identity: Option<String>,
    account_id: Option<String>,
    token: String,
    credential_expires_at: Option<String>,
) -> anyhow::Result<uuid::Uuid> {
    let account = DiscoveredAccount {
        native_credentials: None,
        refresh_bundle: None,
        provider_id,
        identity,
        account_id,
        token,
        credential_expires_at: credential_expires_at.clone(),
        plan: None,
    };
    let snapshot = Some(SubscriptionSnapshot {
        credential_expires_at,
        observed_at: now_rfc3339(),
        source: format!("{provider_id}-oauth-login"),
        ..Default::default()
    });
    let mut existing = vault.list_provider_summaries()?;
    existing.extend(vault.list_archived_provider_summaries()?);
    let mut batch_imported = HashSet::new();
    let (_result, entry_id) = persist_account(
        vault,
        &mut existing,
        &mut batch_imported,
        CollectedAccount { account, snapshot },
    )?;
    entry_id.ok_or_else(|| anyhow::anyhow!("login did not produce a provider entry"))
}

fn persist_account(
    vault: &Vault,
    existing: &mut Vec<aipass_vault::EntrySummary>,
    batch_imported: &mut HashSet<uuid::Uuid>,
    item: CollectedAccount,
) -> anyhow::Result<(OfficialAccountRefreshResult, Option<uuid::Uuid>)> {
    let CollectedAccount { account, snapshot } = item;
    let fingerprint = vault.fingerprint_secret(&account.token);
    let identity = account.identity.clone().or_else(|| {
        Some(format!(
            "account:{}",
            &fingerprint[..fingerprint.len().min(12)]
        ))
    });
    // One user can authorize multiple ChatGPT workspaces. Keep their tokens and
    // chatgpt-account-id headers together instead of deduplicating by email alone.
    let mut matching_workspaces = HashSet::new();
    if account.provider_id == "openai" {
        if let Some(account_id) = account.account_id.as_deref() {
            for entry in existing
                .iter()
                .filter(|entry| entry.provider_id.as_deref() == Some("openai"))
            {
                if vault
                    .reveal_provider_headers(entry.id)?
                    .iter()
                    .any(|(key, value)| {
                        key.eq_ignore_ascii_case("chatgpt-account-id") && value == account_id
                    })
                {
                    matching_workspaces.insert(entry.id);
                }
            }
        }
    }
    let existing_id = existing
        .iter()
        .find(|entry| {
            entry.provider_id.as_deref() == Some(account.provider_id)
                && (entry.credential_kind == CredentialKind::OAuth
                    || entry.tags.iter().any(|tag| tag == "oauth"))
                && (account.provider_id != "openai"
                    || account.account_id.is_none()
                    || matching_workspaces.contains(&entry.id))
                && (entry.account_identity == identity || entry.fingerprint == fingerprint)
        })
        .map(|entry| entry.id)
        .or_else(|| {
            // Identity-less accounts carry a synthetic `account:<fingerprint>`
            // identity that goes stale on every token rotation, so the direct
            // match above can never find them again after the CLI rotates its
            // access token.
            if account.identity.is_some() || account.account_id.is_some() {
                return None;
            }
            rotation_candidate(existing, batch_imported, account.provider_id)
        });
    let previous_snapshot = existing
        .iter()
        .find(|entry| Some(entry.id) == existing_id)
        .and_then(|entry| entry.subscription.clone());
    let snapshot = merge_snapshot(previous_snapshot, snapshot);
    let status = if existing_id.is_some() {
        "refreshed"
    } else {
        "imported"
    };
    let entry_id = if let Some(existing_id) = existing_id {
        // Refresh in place even when the match is archived: archiving must
        // not strand the entry with a stale token, and it stays archived.
        refresh_account_secret(vault, existing_id, &account.token)?;
        vault.update_provider_subscription(existing_id, snapshot.clone())?;
        existing_id
    } else {
        let (interface_type, auth_scheme, endpoint) = match account.provider_id {
            "anthropic" => (
                InterfaceType::AnthropicMessages,
                AuthScheme::Bearer,
                "https://api.anthropic.com",
            ),
            "copilot" => (
                InterfaceType::OpenAiCompatible,
                AuthScheme::Bearer,
                "https://api.githubcopilot.com",
            ),
            "xai" => (
                InterfaceType::OpenAiCompatible,
                AuthScheme::Bearer,
                GROK_CLI_PROXY_ENDPOINT,
            ),
            _ => (
                InterfaceType::OpenAiCompatible,
                AuthScheme::Bearer,
                CODEX_OAUTH_ENDPOINT,
            ),
        };
        let title = account
            .identity
            .as_deref()
            .map(|identity| format!("{} ({identity})", account.provider_id))
            .unwrap_or_else(|| account.provider_id.to_string());
        let headers = match account.provider_id {
            "anthropic" => vec![("anthropic-beta".to_string(), "oauth-2025-04-20".to_string())],
            "xai" => vec![("X-XAI-Token-Auth".to_string(), "xai-grok-cli".to_string())],
            "openai" => account
                .account_id
                .as_ref()
                .map(|account_id| vec![("chatgpt-account-id".to_string(), account_id.clone())])
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let new_id = vault.add_provider(ProviderEntryInput {
            max_concurrent_requests: None,
            supports_websockets: None,
            title,
            provider_kind: ProviderKind::Official,
            provider_id: Some(account.provider_id.to_string()),
            credential_kind: CredentialKind::OAuth,
            account_identity: identity.clone(),
            domains: Vec::new(),
            favicon_url: None,
            endpoints: vec![ProviderEndpoint::api(endpoint)],
            interface_type,
            auth_scheme,
            api_key: account.token.clone(),
            secret_label: Some("oauth".to_string()),
            default_model: None,
            model_aliases: Vec::new(),
            headers,
            quota: None,
            subscription: snapshot.clone(),
            gateway: None,
            tags: vec!["official".to_string(), "oauth".to_string()],
            notes: Some("Imported from the provider's official CLI credential store".to_string()),
            secret_metadata: Default::default(),
        })?;
        // Make the freshly imported entry visible to later accounts in this
        // batch so a duplicate identity matches instead of importing twice.
        batch_imported.insert(new_id);
        if let Ok(summary) = vault.get_provider_summary(new_id) {
            existing.push(summary);
        }
        new_id
    };
    if account.provider_id == "copilot" {
        vault.set_provider_runtime_extension(
            entry_id,
            "copilot_auth_v1",
            account
                .native_credentials
                .as_ref()
                .map(aipass_crypto::SecretString::new)
                .as_ref(),
        )?;
    }
    if let Some(bundle) = &account.refresh_bundle {
        persist_imported_bundle(vault, entry_id, &account, bundle)?;
    }
    let refresh_error = snapshot.as_ref().and_then(|item| item.error.clone());
    Ok((
        OfficialAccountRefreshResult {
            provider_id: account.provider_id.to_string(),
            account_identity: identity,
            credential_kind: CredentialKind::OAuth,
            snapshot,
            status: status.to_string(),
            error: refresh_error,
        },
        Some(entry_id),
    ))
}

fn persist_imported_bundle(
    vault: &Vault,
    entry_id: uuid::Uuid,
    account: &DiscoveredAccount,
    bundle: &crate::oauth::OAuthTokenBundle,
) -> anyhow::Result<()> {
    if account.provider_id == "anthropic" {
        if let Some(previous) = vault.provider_runtime_extension(entry_id, "claude_native")? {
            if let Ok(previous) = serde_json::from_str::<Value>(previous.expose()) {
                let expires = previous["claudeAiOauth"]["expiresAt"].as_i64().unwrap_or(0);
                let incoming = account
                    .credential_expires_at
                    .as_deref()
                    .and_then(|v| OffsetDateTime::parse(v, &Rfc3339).ok())
                    .map(|t| t.unix_timestamp() * 1000)
                    .unwrap_or(0);
                if expires > incoming {
                    if let Some(token) = previous["claudeAiOauth"]["accessToken"].as_str() {
                        refresh_account_secret(vault, entry_id, token)?;
                        return Ok(());
                    }
                }
            }
        }
        if let Some(value) = account.native_credentials.as_ref() {
            vault.set_provider_runtime_extension(
                entry_id,
                "claude_native",
                Some(&aipass_crypto::SecretString::new(value)),
            )?;
        }
        return Ok(());
    }
    use aipass_provider_registry::OAuthProvider;
    let provider = match account.provider_id {
        "openai" => OAuthProvider::Codex,
        "xai" => OAuthProvider::Grok,
        _ => return Ok(()),
    };
    let now = crate::oauth::now_ms();
    let existing = vault
        .list_oauth_accounts(Some(provider))?
        .into_iter()
        .find(|a| a.entry_id == Some(entry_id));
    let expires_at_ms = account
        .credential_expires_at
        .as_deref()
        .and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok())
        .map(|t| t.unix_timestamp().saturating_mul(1000))
        .unwrap_or(now);
    if let Some(current) = existing.as_ref() {
        if current.expires_at_ms > expires_at_ms && current.access_token != account.token {
            // Collection raced with a managed refresh. Restore the newer mirror.
            refresh_account_secret(vault, entry_id, &current.access_token)?;
            return Ok(());
        }
        if current.access_token == account.token && current.refresh_token == bundle.refresh_token {
            return Ok(());
        }
    }
    let record = aipass_vault::ManagedOAuthAccount {
        id: existing
            .as_ref()
            .map(|a| a.id)
            .unwrap_or_else(uuid::Uuid::new_v4),
        provider,
        account_identity: account.identity.clone(),
        chatgpt_account_id: account.account_id.clone(),
        access_token: bundle.access_token.clone(),
        refresh_token: bundle.refresh_token.clone(),
        id_token: bundle.id_token.clone(),
        expires_at_ms,
        last_refresh_ms: now,
        entry_id: Some(entry_id),
        is_default: existing.as_ref().is_some_and(|a| a.is_default),
        requires_reauth: false,
        authenticated_at: existing
            .as_ref()
            .map(|a| a.authenticated_at)
            .unwrap_or_else(OffsetDateTime::now_utc),
    };
    if existing.is_some() {
        vault.update_oauth_account(record)?;
    } else {
        vault.add_oauth_account(record)?;
    }
    Ok(())
}

/// Find the single vault entry an identity-less rotated token must belong to.
///
/// Only an unambiguous official OAuth entry for the provider qualifies; when
/// several candidates exist they may be distinct accounts and must be left
/// alone. Entries imported earlier in this batch are excluded because a token
/// rotation cannot happen mid-batch, so those are by construction different
/// accounts.
fn rotation_candidate(
    existing: &[aipass_vault::EntrySummary],
    batch_imported: &HashSet<uuid::Uuid>,
    provider_id: &str,
) -> Option<uuid::Uuid> {
    let mut candidates = existing.iter().filter(|entry| {
        entry.provider_id.as_deref() == Some(provider_id)
            && entry.provider_kind == ProviderKind::Official
            && entry.credential_kind == CredentialKind::OAuth
            && !batch_imported.contains(&entry.id)
            && entry
                .account_identity
                .as_deref()
                .is_none_or(|identity| identity.starts_with("account:"))
    });
    let candidate = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    Some(candidate.id)
}

pub(crate) fn refresh_account_secret(
    vault: &Vault,
    id: uuid::Uuid,
    token: &str,
) -> anyhow::Result<()> {
    if vault.find_secret_id_by_value(id, token)?.is_some() {
        return Ok(());
    }
    let summary = vault.get_provider_summary(id)?;
    let primary = primary_secret_ref(&summary)?;
    vault.update_secret(id, &primary.id, &primary.label, Some(token.to_string()))?;
    Ok(())
}

/// The primary secret a rotated token is written into. An entry without any
/// stored secret is a desync between the vault and the discovered credential
/// store; report it instead of silently pretending the refresh happened.
fn primary_secret_ref(
    summary: &aipass_vault::EntrySummary,
) -> anyhow::Result<&aipass_provider_registry::SecretRef> {
    aipass_provider_registry::primary_secret_ref(&summary.secret_refs).ok_or_else(|| {
        anyhow::anyhow!(
            "provider entry {} has no stored secret to refresh",
            summary.id
        )
    })
}

fn refresh_snapshot(account: &DiscoveredAccount) -> Option<SubscriptionSnapshot> {
    let observed_at = now_rfc3339();
    let mut snapshot = SubscriptionSnapshot {
        plan: account.plan.clone(),
        credential_expires_at: account.credential_expires_at.clone(),
        observed_at,
        source: format!("{}-official-cli", account.provider_id),
        ..Default::default()
    };

    match account.provider_id {
        "openai" => match codex_usage(&account.token, account.account_id.as_deref()) {
            Ok((windows, credits, plan)) => {
                snapshot.windows = windows;
                snapshot.credits_remaining = credits;
                snapshot.plan = plan.or(snapshot.plan);
                snapshot.status = Some("active".to_string());
            }
            Err(error) => snapshot.error = Some(error.to_string()),
        },
        "anthropic" => match claude_usage(&account.token) {
            Ok((windows, extra, currency, plan)) => {
                snapshot.windows = windows;
                snapshot.credits_remaining = extra;
                snapshot.credits_currency = currency;
                snapshot.plan = plan.or(snapshot.plan);
                snapshot.status = Some("active".to_string());
            }
            Err(error) => snapshot.error = Some(error.to_string()),
        },
        "copilot" => match copilot_usage(&account.token) {
            Ok((windows, plan)) => {
                snapshot.windows = windows;
                snapshot.plan = plan;
                snapshot.status = Some("active".into());
            }
            Err(error) => snapshot.error = Some(error.to_string()),
        },
        "xai" => match grok_usage(&account.token) {
            Ok(windows) => {
                snapshot.windows = windows;
                snapshot.status = Some("active".to_string());
            }
            Err(error) => snapshot.error = Some(error.to_string()),
        },
        _ => {}
    }
    Some(snapshot)
}

fn merge_snapshot(
    previous: Option<SubscriptionSnapshot>,
    current: Option<SubscriptionSnapshot>,
) -> Option<SubscriptionSnapshot> {
    let Some(mut current) = current else {
        return previous;
    };
    if current.error.is_some() {
        current.stale = true;
        if let Some(previous) = previous {
            current.plan = current.plan.or(previous.plan);
            current.status = current.status.or(previous.status);
            current.subscription_expires_at = current
                .subscription_expires_at
                .or(previous.subscription_expires_at);
            current.subscription_renews_at = current
                .subscription_renews_at
                .or(previous.subscription_renews_at);
            current.billing_period_ends_at = current
                .billing_period_ends_at
                .or(previous.billing_period_ends_at);
            current.credential_expires_at = current
                .credential_expires_at
                .or(previous.credential_expires_at);
            current.credits_remaining = current.credits_remaining.or(previous.credits_remaining);
            current.credits_currency = current.credits_currency.or(previous.credits_currency);
            if current.windows.is_empty() {
                current.windows = previous.windows;
            }
        }
    }
    Some(current)
}

fn copilot_usage(token: &str) -> anyhow::Result<(Vec<SubscriptionWindow>, Option<String>)> {
    let client = reqwest::blocking::Client::builder()
        .timeout(USAGE_HTTP_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let response = client
        .get("https://api.github.com/copilot_internal/user")
        .header("authorization", format!("token {token}"))
        .header("accept", "application/json")
        .header("user-agent", "AIPass")
        .send()?;
    anyhow::ensure!(
        response.status().is_success(),
        "Copilot usage endpoint returned HTTP {}",
        response.status()
    );
    let value = crate::oauth::read_json_response(response)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    Ok(parse_copilot_usage(&value))
}
fn parse_copilot_usage(value: &Value) -> (Vec<SubscriptionWindow>, Option<String>) {
    let resets = value["quota_reset_date_utc"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| {
            value["quota_reset_date"]
                .as_str()
                .map(|v| format!("{v}T00:00:00Z"))
        });
    let mut windows = Vec::new();
    for (id, label) in [
        ("chat", "Chat requests"),
        ("completions", "Completions"),
        ("premium_interactions", "Premium requests"),
    ] {
        let w = &value["quota_snapshots"][id];
        if w["unlimited"] == true {
            windows.push(SubscriptionWindow {
                id: format!("copilot_{id}"),
                label: format!("{label} (unlimited)"),
                used_percent: None,
                resets_at: resets.clone(),
                window_minutes: None,
                source: Some("copilot-account-usage".into()),
            });
            continue;
        }
        if w["has_quota"] != true {
            continue;
        }
        let Some(total) = w["entitlement"]
            .as_f64()
            .filter(|n| n.is_finite() && *n > 0.)
        else {
            continue;
        };
        let Some(remaining) = w["quota_remaining"].as_f64().filter(|n| n.is_finite()) else {
            continue;
        };
        windows.push(SubscriptionWindow {
            id: format!("copilot_{id}"),
            label: label.into(),
            used_percent: Some((100. * (total - remaining) / total).clamp(0., 100.)),
            resets_at: resets.clone(),
            window_minutes: Some(30 * 24 * 60),
            source: Some("copilot-account-usage".into()),
        });
    }
    (windows, value["copilot_plan"].as_str().map(str::to_owned))
}

/// Query the exact token/workspace being imported. Asking the ambient Codex
/// process would use whichever account its native credential file holds now.
fn codex_usage(
    token: &str,
    account_id: Option<&str>,
) -> anyhow::Result<(Vec<SubscriptionWindow>, Option<String>, Option<String>)> {
    let client = reqwest::blocking::Client::builder()
        .timeout(USAGE_HTTP_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    codex_usage_at(
        &client,
        "https://chatgpt.com/backend-api/wham/usage",
        token,
        account_id,
    )
}

fn codex_usage_at(
    client: &reqwest::blocking::Client,
    endpoint: &str,
    token: &str,
    account_id: Option<&str>,
) -> anyhow::Result<(Vec<SubscriptionWindow>, Option<String>, Option<String>)> {
    let mut request = client.get(endpoint).bearer_auth(token);
    if let Some(account_id) = account_id {
        request = request.header("chatgpt-account-id", account_id);
    }
    let response = request.send()?;
    if !response.status().is_success() {
        anyhow::bail!("Codex usage endpoint returned HTTP {}", response.status());
    }
    let value: Value = crate::oauth::read_json_response(response)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    parse_codex_usage(&value)
}

fn parse_codex_usage(
    value: &Value,
) -> anyhow::Result<(Vec<SubscriptionWindow>, Option<String>, Option<String>)> {
    let mut windows = Vec::new();
    for (key, id) in [
        ("primary_window", "primary"),
        ("secondary_window", "secondary"),
    ] {
        let Some(window) = value.get("rate_limit").and_then(|limit| limit.get(key)) else {
            continue;
        };
        let used_percent = window.get("used_percent").and_then(number_value);
        let resets_at = window
            .get("reset_at")
            .and_then(Value::as_i64)
            .and_then(unix_timestamp);
        let window_minutes = window
            .get("limit_window_seconds")
            .and_then(Value::as_u64)
            .map(|seconds| seconds / 60);
        if used_percent.is_some() || resets_at.is_some() {
            windows.push(SubscriptionWindow {
                id: id.into(),
                label: id.into(),
                used_percent,
                resets_at,
                window_minutes,
                source: Some("codex-account-usage".into()),
            });
        }
    }
    let credits = value
        .pointer("/credits/balance")
        .and_then(|value| match value {
            Value::String(value) => Some(value.clone()),
            Value::Number(value) => Some(value.to_string()),
            _ => None,
        });
    let plan = find_string(value, &["plan_type"]);
    if windows.is_empty() && credits.is_none() && plan.is_none() {
        anyhow::bail!("Codex usage endpoint did not return account usage");
    }
    Ok((windows, credits, plan))
}

fn unix_timestamp(value: i64) -> Option<String> {
    OffsetDateTime::from_unix_timestamp(value)
        .ok()
        .and_then(|date| date.format(&Rfc3339).ok())
}

fn discover_codex_accounts() -> Vec<DiscoveredAccount> {
    let dir = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".codex"));
    let path = dir.join("auth.json");
    let Ok(value) = read_json(&path) else {
        return Vec::new();
    };
    let token = find_string(&value, &["access_token", "accessToken", "token"]);
    let Some(token) = token else {
        return Vec::new();
    };
    let identity = find_string(
        &value,
        &["email", "email_address", "account_id", "accountId"],
    );
    let account_id = value
        .get("tokens")
        .and_then(|tokens| find_string(tokens, &["account_id", "accountId"]));
    // Codex auth.json carries no explicit expiry; the access token's own JWT
    // `exp` claim is the only expiry signal available for display.
    let expiry =
        find_timestamp(&value, &["expires_at", "expiresAt"]).or_else(|| jwt_expiry(&token));
    let oauth = value
        .get("auth_mode")
        .and_then(Value::as_str)
        .is_some_and(|mode| mode.eq_ignore_ascii_case("oauth"))
        || value.get("tokens").is_some();
    if !oauth {
        return Vec::new();
    }
    let refresh_bundle = discovered_bundle(
        &value,
        &token,
        identity.clone(),
        account_id.clone(),
        expiry.as_deref(),
    );
    vec![DiscoveredAccount {
        native_credentials: None,
        refresh_bundle,
        provider_id: "openai",
        identity,
        account_id,
        token,
        credential_expires_at: expiry,
        plan: None,
    }]
}

fn discovered_bundle(
    value: &Value,
    access_token: &str,
    identity: Option<String>,
    account_id: Option<String>,
    expiry: Option<&str>,
) -> Option<crate::oauth::OAuthTokenBundle> {
    let refresh_token = find_string(value, &["refresh_token", "refreshToken"])?;
    let expires = expiry
        .and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok())?
        .unix_timestamp();
    Some(crate::oauth::OAuthTokenBundle {
        access_token: access_token.to_owned(),
        refresh_token,
        id_token: find_string(value, &["id_token", "idToken"]),
        chatgpt_account_id: account_id,
        account_identity: identity,
        expires_in: (expires - OffsetDateTime::now_utc().unix_timestamp()).max(1),
    })
}

/// Read the `exp` claim from a JWT without verifying the signature. Used only
/// to display credential expiry, never for an authorization decision.
fn jwt_expiry(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let value = serde_json::from_slice::<Value>(&bytes).ok()?;
    unix_timestamp(value.get("exp")?.as_i64()?)
}

fn discover_claude_accounts() -> Vec<DiscoveredAccount> {
    let value = read_claude_credentials().ok();
    let Some(value) = value else {
        return Vec::new();
    };
    let oauth = value
        .get("claudeAiOauth")
        .or_else(|| value.get("claude_ai_oauth"));
    let Some(oauth) = oauth else {
        return Vec::new();
    };
    let Some(token) = find_string(oauth, &["accessToken", "access_token"]) else {
        return Vec::new();
    };
    let profile = read_json(&home().join(".claude.json")).ok();
    let identity = find_string(&value, &["email", "emailAddress", "email_address"])
        .or_else(|| find_string(oauth, &["email", "emailAddress", "email_address"]))
        .or_else(|| {
            profile
                .as_ref()
                .and_then(|p| p.get("oauthAccount"))
                .and_then(|p| find_string(p, &["emailAddress", "accountUuid"]))
        });
    let expiry = find_timestamp(oauth, &["expiresAt", "expires_at"]);
    let plan = find_string(oauth, &["subscriptionType", "rateLimitTier"]);
    vec![DiscoveredAccount {
        native_credentials: Some(value.to_string()),
        refresh_bundle: find_string(oauth, &["refreshToken", "refresh_token"]).map(
            |refresh_token| crate::oauth::OAuthTokenBundle {
                access_token: token.clone(),
                refresh_token,
                id_token: None,
                expires_in: 0,
                account_identity: identity.clone(),
                chatgpt_account_id: None,
            },
        ),
        provider_id: "anthropic",
        identity,
        account_id: None,
        token,
        credential_expires_at: expiry,
        plan,
    }]
}

#[cfg(target_os = "macos")]
pub(crate) fn read_keychain(service: &str) -> anyhow::Result<Value> {
    let bytes = read_keychain_bytes(service, None)?;
    Ok(serde_json::from_slice(&bytes)?)
}
#[cfg(target_os = "macos")]
fn read_keychain_bytes(
    service: &str,
    account: Option<&str>,
) -> anyhow::Result<zeroize::Zeroizing<Vec<u8>>> {
    use std::io::Read;
    use std::process::Stdio;
    let mut command = Command::new("security");
    command.args(["find-generic-password", "-s", service, "-w"]);
    if let Some(account) = account {
        command.args(["-a", account]);
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("missing credential pipe"))?;
    let reader = std::thread::spawn(move || {
        let mut bytes = zeroize::Zeroizing::new(Vec::new());
        stdout
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let bytes = reader
        .join()
        .map_err(|_| anyhow::anyhow!("credential read failed"))??;
    anyhow::ensure!(
        status.is_some_and(|s| s.success()) && bytes.len() <= 1024 * 1024,
        "credential unavailable"
    );
    Ok(bytes)
}
fn read_claude_credentials() -> anyhow::Result<Value> {
    #[cfg(target_os = "macos")]
    if let Ok(value) = read_keychain("Claude Code-credentials") {
        return Ok(value);
    }
    read_json(&home().join(".claude").join(".credentials.json"))
}

fn discover_copilot_accounts() -> Vec<DiscoveredAccount> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"));
    let mut accounts = Vec::new();
    for name in ["apps.json", "hosts.json"] {
        let Ok(value) = read_json(&config.join("github-copilot").join(name)) else {
            continue;
        };
        for (host, entry) in value.as_object().into_iter().flatten() {
            if host != "github.com" && !host.starts_with("github.com:") {
                continue;
            }
            let Some(token) = entry
                .get("oauth_token")
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
            else {
                continue;
            };
            accounts.push(DiscoveredAccount {
                native_credentials: None,
                provider_id: "copilot",
                identity: entry.get("user").and_then(Value::as_str).map(str::to_owned),
                account_id: None,
                token: token.to_owned(),
                refresh_bundle: None,
                credential_expires_at: None,
                plan: None,
            });
        }
    }
    let cli_home = std::env::var_os("COPILOT_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".copilot"));
    if let Ok(raw) = std::fs::read_to_string(cli_home.join("config.json")) {
        let raw = zeroize::Zeroizing::new(raw);
        if let Ok(config) = json5::from_str::<Value>(&raw) {
            accounts.extend(copilot_cli_accounts_from(&config, |key| {
                #[cfg(target_os = "macos")]
                if let Ok(bytes) = read_keychain_bytes("copilot-cli", Some(key)) {
                    return String::from_utf8(bytes.to_vec())
                        .ok()
                        .map(|v| v.trim().to_owned());
                }
                #[cfg(not(target_os = "macos"))]
                let _ = key;
                None
            }));
        }
    }
    accounts
}

fn copilot_cli_accounts_from(
    config: &Value,
    mut secret: impl FnMut(&str) -> Option<String>,
) -> Vec<DiscoveredAccount> {
    let mut seen = HashSet::new();
    config
        .get("lastLoggedInUser")
        .into_iter()
        .chain(config["loggedInUsers"].as_array().into_iter().flatten())
        .filter_map(|user| {
            let login = user["login"].as_str().filter(|v| !v.is_empty())?;
            let host = user["host"].as_str().unwrap_or("");
            if (!host.is_empty() && host != "https://github.com") || !seen.insert(login.to_owned())
            {
                return None;
            }
            let key = format!("https://github.com:{login}");
            let token = config["copilotTokens"][&key]
                .as_str()
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
                .or_else(|| secret(&key))?;
            Some(DiscoveredAccount {
                provider_id: "copilot",
                identity: Some(login.to_owned()),
                account_id: None,
                token,
                native_credentials: Some(r#"{"client":"cli"}"#.into()),
                refresh_bundle: None,
                credential_expires_at: None,
                plan: None,
            })
        })
        .collect()
}

fn discover_grok_accounts() -> Vec<DiscoveredAccount> {
    let path = home().join(".grok").join("auth.json");
    let Ok(value) = read_json(&path) else {
        return Vec::new();
    };
    grok_accounts_from(&value)
}

fn grok_accounts_from(value: &Value) -> Vec<DiscoveredAccount> {
    let Some(root) = value.as_object() else {
        return Vec::new();
    };
    root.iter()
        .filter(|(issuer, entry)| grok_entry_is_oauth_session(issuer, entry))
        .filter_map(|(_, entry)| {
            let token = find_string(entry, &["key", "access_token", "accessToken"])?;
            let identity = find_string(entry, &["email", "user_id", "userId", "principal_id"]);
            let expiry = find_timestamp(entry, &["expires_at", "expiresAt"]);
            let refresh_bundle =
                discovered_bundle(entry, &token, identity.clone(), None, expiry.as_deref());
            Some(DiscoveredAccount {
                native_credentials: None,
                refresh_bundle,
                provider_id: "xai",
                identity,
                account_id: None,
                token,
                credential_expires_at: expiry,
                plan: None,
            })
        })
        .collect()
}

/// `~/.grok/auth.json` mixes xAI OIDC sessions (keyed by issuer URL, carrying
/// refresh/expiry material) with plain API-key entries; only the sessions are
/// official-account imports, so anything that is clearly a bare key is left
/// alone.
fn grok_entry_is_oauth_session(issuer: &str, entry: &Value) -> bool {
    issuer.contains("accounts.x.ai")
        || find_string(entry, &["refresh_token", "refreshToken"]).is_some()
        || find_timestamp(entry, &["expires_at", "expiresAt"]).is_some()
}

fn claude_usage(token: &str) -> anyhow::Result<ClaudeUsage> {
    let response = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?
        .get("https://api.anthropic.com/api/oauth/usage")
        .bearer_auth(token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("user-agent", "claude-code/2.1.0")
        .send()?;
    if !response.status().is_success() {
        anyhow::bail!("Claude usage endpoint returned HTTP {}", response.status());
    }
    let value: Value = response.json()?;
    let windows = claude_usage_windows(&value);
    let (extra, currency) = value
        .get("extra_usage")
        .and_then(Value::as_object)
        .map(|extra| {
            let limit = extra.get("monthly_limit").and_then(number_value);
            let used = extra.get("used_credits").and_then(number_value);
            let remaining = match (limit, used) {
                (Some(limit), Some(used)) => Some((limit - used).max(0.0).to_string()),
                (Some(limit), None) => Some(limit.to_string()),
                _ => None,
            };
            let currency = extra
                .get("currency")
                .and_then(Value::as_str)
                .map(str::to_string);
            (remaining, currency)
        })
        .unwrap_or((None, None));
    Ok((windows, extra, currency, None))
}

/// Parse the rate-limit windows from the `/api/oauth/usage` response. The
/// endpoint reports `utilization` as a 0-100 percentage, so the value is
/// used as-is.
fn claude_usage_windows(value: &Value) -> Vec<SubscriptionWindow> {
    let mut windows = Vec::new();
    for (key, label, minutes) in [
        ("five_hour", "5h", Some(300)),
        ("seven_day", "7d", Some(10080)),
        ("seven_day_opus", "7d Opus", Some(10080)),
        ("seven_day_sonnet", "7d Sonnet", Some(10080)),
    ] {
        let Some(item) = value.get(key).and_then(Value::as_object) else {
            continue;
        };
        let used_percent = item.get("utilization").and_then(Value::as_f64);
        let resets_at = item
            .get("resets_at")
            .and_then(Value::as_str)
            .map(str::to_string);
        if used_percent.is_some() || resets_at.is_some() {
            windows.push(SubscriptionWindow {
                id: key.to_string(),
                label: label.to_string(),
                used_percent,
                resets_at,
                window_minutes: minutes,
                source: Some("anthropic-oauth-usage".to_string()),
            });
        }
    }
    windows
}

fn number_value(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.parse::<f64>().ok()))
}

fn grok_usage(token: &str) -> anyhow::Result<Vec<SubscriptionWindow>> {
    let response = reqwest::blocking::Client::builder()
        .timeout(USAGE_HTTP_TIMEOUT)
        .build()?
        .post(GROK_BILLING_ENDPOINT)
        .bearer_auth(token)
        .header("Origin", "https://grok.com")
        .header("Referer", "https://grok.com/?_s=usage")
        .header("Accept", "*/*")
        .header("Content-Type", "application/grpc-web+proto")
        .header("x-grpc-web", "1")
        .header("x-user-agent", "connect-es/2.1.1")
        .body(vec![0_u8; 5])
        .send()?;
    if !response.status().is_success() {
        anyhow::bail!("Grok billing endpoint returned HTTP {}", response.status());
    }
    let body = response.bytes()?.to_vec();
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let (used_percent, resets_at) = parse_grok_billing(&body, now)
        .ok_or_else(|| anyhow::anyhow!("Grok billing response did not contain usage"))?;
    Ok(vec![SubscriptionWindow {
        id: "credits".to_string(),
        label: "credits".to_string(),
        used_percent: Some(used_percent),
        resets_at: resets_at.and_then(unix_timestamp),
        window_minutes: None,
        source: Some("grok-billing-grpc-web".to_string()),
    }])
}

#[derive(Default)]
struct ProtobufScan {
    fixed32: Vec<(Vec<u64>, f32, usize)>,
    varints: Vec<(Vec<u64>, u64)>,
}

fn read_varint(bytes: &[u8], index: &mut usize) -> Option<u64> {
    let mut value = 0_u64;
    let mut shift = 0_u32;
    while *index < bytes.len() && shift < 64 {
        let byte = bytes[*index];
        *index += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
    }
    None
}

fn scan_protobuf(
    bytes: &[u8],
    depth: usize,
    path: &[u64],
    order: usize,
    scan: &mut ProtobufScan,
) -> usize {
    let mut index = 0;
    let mut next_order = order;
    while index < bytes.len() {
        let start = index;
        let Some(key) = read_varint(bytes, &mut index) else {
            break;
        };
        let field = key >> 3;
        let wire = key & 7;
        if field == 0 {
            index = start + 1;
            continue;
        }
        let mut field_path = path.to_vec();
        field_path.push(field);
        match wire {
            0 => {
                if let Some(value) = read_varint(bytes, &mut index) {
                    scan.varints.push((field_path, value));
                } else {
                    index = start + 1;
                }
            }
            1 => index = (index + 8).min(bytes.len()),
            2 => {
                let Some(length) = read_varint(bytes, &mut index)
                    .and_then(|length| usize::try_from(length).ok())
                    .filter(|length| *length <= bytes.len().saturating_sub(index))
                else {
                    index = start + 1;
                    continue;
                };
                let end = index + length;
                if depth < 4 {
                    next_order =
                        scan_protobuf(&bytes[index..end], depth + 1, &field_path, next_order, scan);
                }
                index = end;
            }
            5 => {
                if index + 4 > bytes.len() {
                    break;
                }
                let bits = u32::from_le_bytes(bytes[index..index + 4].try_into().unwrap());
                scan.fixed32
                    .push((field_path, f32::from_bits(bits), next_order));
                next_order += 1;
                index += 4;
            }
            _ => index = start + 1,
        }
    }
    next_order
}

fn parse_grok_billing(bytes: &[u8], now: i64) -> Option<(f64, Option<i64>)> {
    let mut payloads = Vec::new();
    let mut index = 0;
    while index + 5 <= bytes.len() {
        let flags = bytes[index];
        let length = u32::from_be_bytes(bytes[index + 1..index + 5].try_into().ok()?) as usize;
        let start = index + 5;
        let end = start.checked_add(length)?;
        if end > bytes.len() {
            payloads.clear();
            break;
        }
        if flags & 0x80 == 0 {
            payloads.push(&bytes[start..end]);
        }
        index = end;
    }
    if payloads.is_empty()
        && bytes.first().is_some_and(|byte| {
            let field = byte >> 3;
            let wire = byte & 7;
            field > 0 && matches!(wire, 0 | 1 | 2 | 5)
        })
    {
        payloads.push(bytes);
    }
    if payloads.is_empty() {
        return None;
    }
    let mut scan = ProtobufScan::default();
    for payload in payloads {
        scan_protobuf(payload, 0, &[], 0, &mut scan);
    }
    let used = scan
        .fixed32
        .iter()
        .filter(|(path, value, _)| {
            path.last() == Some(&1) && value.is_finite() && (0.0..=100.0).contains(value)
        })
        .min_by_key(|(path, _, order)| (path.len(), *order))
        .map(|(_, value, _)| f64::from(*value))
        .or_else(|| {
            scan.varints
                .iter()
                .find(|(path, value)| path.last() == Some(&1) && *value <= 100)
                .map(|(_, value)| *value as f64)
        })?;
    let reset = scan
        .varints
        .iter()
        .filter_map(|(_, value)| i64::try_from(*value).ok())
        .filter(|value| (1_700_000_000..=2_100_000_000).contains(value) && *value > now)
        .min();
    Some((used.clamp(0.0, 100.0), reset))
}

fn read_json(path: &Path) -> anyhow::Result<Value> {
    use std::io::Read;
    let mut bytes = zeroize::Zeroizing::new(Vec::new());
    std::fs::File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= 1024 * 1024, "credential file exceeds limit");
    Ok(serde_json::from_slice(&bytes)?)
}

fn find_string(value: &Value, keys: &[&str]) -> Option<String> {
    if let Some(found) = keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    }) {
        return Some(found);
    }
    match value {
        Value::Object(object) => object.values().find_map(|child| find_string(child, keys)),
        Value::Array(array) => array.iter().find_map(|child| find_string(child, keys)),
        _ => None,
    }
}

fn find_timestamp(value: &Value, keys: &[&str]) -> Option<String> {
    if let Some(found) = keys
        .iter()
        .find_map(|key| value.get(*key).and_then(timestamp_value))
    {
        return Some(found);
    }
    match value {
        Value::Object(object) => object
            .values()
            .find_map(|child| find_timestamp(child, keys)),
        Value::Array(array) => array.iter().find_map(|child| find_timestamp(child, keys)),
        _ => None,
    }
}

fn timestamp_value(value: &Value) -> Option<String> {
    if let Some(number) = value.as_i64() {
        let seconds = if number.unsigned_abs() > 1_000_000_000_000 {
            number / 1_000
        } else {
            number
        };
        return unix_timestamp(seconds);
    }
    let text = value.as_str()?.trim();
    if text.is_empty() {
        return None;
    }
    if let Ok(number) = text.parse::<i64>() {
        return timestamp_value(&Value::from(number));
    }
    OffsetDateTime::parse(text, &Rfc3339)
        .ok()
        .and_then(|date| date.format(&Rfc3339).ok())
}

pub(crate) fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| OffsetDateTime::now_utc().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aipass_crypto::SecretString;

    #[test]
    fn copilot_usage_preserves_unlimited_and_separate_allowances() {
        let (windows, plan) = parse_copilot_usage(
            &serde_json::json!({"copilot_plan":"individual_pro","quota_reset_date":"2026-11-01","quota_snapshots":{"chat":{"unlimited":true},"completions":{"has_quota":true,"entitlement":2000,"quota_remaining":0},"premium_interactions":{"has_quota":true,"entitlement":300,"quota_remaining":75}}}),
        );
        assert_eq!(plan.as_deref(), Some("individual_pro"));
        assert_eq!(windows.len(), 3);
        assert_eq!(windows[0].used_percent, None);
        assert_eq!(windows[1].used_percent, Some(100.));
        assert_eq!(windows[2].used_percent, Some(75.));
        assert_eq!(
            windows[2].resets_at.as_deref(),
            Some("2026-11-01T00:00:00Z")
        );
    }

    #[test]
    fn copilot_cli_import_uses_matching_native_account_and_persists_client_kind() {
        let config = serde_json::json!({"lastLoggedInUser":{"login":"alice","host":"https://github.com"},"loggedInUsers":[{"login":"alice"},{"login":"bob"},{"login":"enterprise","host":"https://company.example"}],"copilotTokens":{"https://github.com:alice":"synthetic-cli-token"}});
        let accounts = copilot_cli_accounts_from(&config, |key| {
            assert_eq!(key, "https://github.com:bob");
            Some("synthetic-keychain-token".into())
        });
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0].identity.as_deref(), Some("alice"));
        assert_eq!(accounts[1].token, "synthetic-keychain-token");
        let directory = tempfile::tempdir().unwrap();
        let vault = Vault::create(
            directory.path(),
            &aipass_crypto::SecretString::new("fixture password"),
        )
        .unwrap()
        .vault;
        let (_, id) = persist_account(
            &vault,
            &mut Vec::new(),
            &mut HashSet::new(),
            CollectedAccount {
                account: accounts[0].clone(),
                snapshot: None,
            },
        )
        .unwrap();
        let raw = vault
            .provider_runtime_extension(id.unwrap(), "copilot_auth_v1")
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(raw.expose()).unwrap()["client"],
            "cli"
        );
        assert!(
            !serde_json::to_string(&vault.get_provider_summary(id.unwrap()).unwrap())
                .unwrap()
                .contains("synthetic-cli-token")
        );
    }

    #[test]
    fn codex_usage_is_bound_to_the_requested_workspace() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut bytes = Vec::new();
            while !bytes.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                bytes.push(byte[0]);
            }
            let headers = String::from_utf8(bytes).unwrap().to_ascii_lowercase();
            assert!(headers.contains("authorization: bearer fake-workspace-token"));
            assert!(headers.contains("chatgpt-account-id: workspace-b"));
            let body = serde_json::json!({"plan_type":"team", "rate_limit":{"primary_window":{"used_percent":42.5, "limit_window_seconds":18000, "reset_at":1791000000}}, "credits":{"balance":0}}).to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .build()
            .unwrap();
        let (windows, credits, plan) = codex_usage_at(
            &client,
            &format!("http://{addr}/usage"),
            "fake-workspace-token",
            Some("workspace-b"),
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(plan.as_deref(), Some("team"));
        assert_eq!(credits.as_deref(), Some("0"));
        assert_eq!(windows[0].used_percent, Some(42.5));
        assert_eq!(windows[0].window_minutes, Some(300));
        assert!(parse_codex_usage(&serde_json::json!({"error":"not usage"})).is_err());
    }

    fn test_vault(temp: &tempfile::TempDir) -> Vault {
        Vault::create(
            temp.path(),
            &SecretString::new("correct horse battery staple"),
        )
        .expect("create vault")
        .vault
    }

    fn identity_less_account(token: &str) -> CollectedAccount {
        CollectedAccount {
            account: DiscoveredAccount {
                native_credentials: None,
                refresh_bundle: None,
                provider_id: "anthropic",
                identity: None,
                account_id: None,
                token: token.to_string(),
                credential_expires_at: None,
                plan: None,
            },
            snapshot: None,
        }
    }

    #[test]
    fn token_rotation_without_identity_refreshes_the_existing_entry() {
        let temp = tempfile::tempdir().expect("tempdir");
        let vault = test_vault(&temp);

        let first = persist_official_accounts(&vault, vec![identity_less_account("token-v1")])
            .expect("persist first");
        assert_eq!(first[0].0.status, "imported");

        let second = persist_official_accounts(&vault, vec![identity_less_account("token-v2")])
            .expect("persist rotated");
        assert_eq!(second[0].0.status, "refreshed");

        let entries = vault.list_provider_summaries().expect("summaries");
        assert_eq!(entries.len(), 1);
        // The vault entry keeps its original synthetic identity...
        assert_eq!(entries[0].account_identity, first[0].0.account_identity);
        // ...while its secret is rotated to the new token.
        assert_eq!(entries[0].fingerprint, vault.fingerprint_secret("token-v2"));
    }

    #[test]
    fn ambiguous_identity_less_entries_are_not_merged_on_rotation() {
        let temp = tempfile::tempdir().expect("tempdir");
        let vault = test_vault(&temp);

        let batch = persist_official_accounts(
            &vault,
            vec![
                identity_less_account("token-a"),
                identity_less_account("token-b"),
            ],
        )
        .expect("persist batch");
        assert_eq!(batch[0].0.status, "imported");
        assert_eq!(batch[1].0.status, "imported");
        assert_eq!(vault.list_provider_summaries().expect("summaries").len(), 2);

        // Two identity-less candidates exist, so a rotated token cannot be
        // attributed to either one; keep importing instead of merging.
        let rotated = persist_official_accounts(&vault, vec![identity_less_account("token-a-v2")])
            .expect("persist rotated");
        assert_eq!(rotated[0].0.status, "imported");
        assert_eq!(vault.list_provider_summaries().expect("summaries").len(), 3);
    }

    #[test]
    fn claude_utilization_is_reported_as_a_zero_to_hundred_percentage() {
        let value = serde_json::json!({
            "five_hour": {"utilization": 33.0, "resets_at": "2026-08-30T12:00:00Z"},
            "seven_day": {"utilization": 0.4}
        });
        let windows = claude_usage_windows(&value);
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].used_percent, Some(33.0));
        assert_eq!(windows[1].used_percent, Some(0.4));
    }

    #[test]
    fn finds_nested_oauth_tokens_without_returning_refresh_material() {
        let value = serde_json::json!({
            "tokens": {"access_token": "access", "refresh_token": "refresh"}
        });
        assert_eq!(
            find_string(&value, &["access_token"]),
            Some("access".into())
        );
        assert_eq!(find_string(&value, &["missing"]), None);
    }

    #[test]
    fn subscription_snapshot_is_marked_with_automatic_source() {
        let account = DiscoveredAccount {
            native_credentials: None,
            refresh_bundle: None,
            provider_id: "xai",
            identity: Some("user@example.test".into()),
            account_id: None,
            token: "token".into(),
            credential_expires_at: None,
            plan: Some("SuperGrok".into()),
        };
        let snapshot = refresh_snapshot(&account).expect("snapshot");
        assert_eq!(snapshot.source, "xai-official-cli");
        assert_eq!(snapshot.plan.as_deref(), Some("SuperGrok"));
        assert!(!snapshot.observed_at.is_empty());
    }

    #[test]
    fn parses_grok_billing_percent_and_reset_from_raw_protobuf() {
        let used = 37.5_f32.to_bits().to_le_bytes();
        let mut nested = vec![0x0d]; // field 1, fixed32
        nested.extend(used);
        nested.push(0x10); // field 2, varint reset timestamp
        nested.extend(encode_varint(1_900_000_000));
        let mut payload = vec![0x0a]; // field 1, length-delimited
        payload.extend(encode_varint(nested.len() as u64));
        payload.extend(nested);

        let parsed = parse_grok_billing(&payload, 1_800_000_000).expect("billing payload");
        assert!((parsed.0 - 37.5).abs() < f64::EPSILON);
        assert_eq!(parsed.1, Some(1_900_000_000));
    }

    #[test]
    fn failed_refresh_keeps_last_good_usage_as_stale() {
        let previous = SubscriptionSnapshot {
            plan: Some("pro".into()),
            credits_remaining: Some("12".into()),
            windows: vec![SubscriptionWindow {
                id: "five_hour".into(),
                label: "5h".into(),
                used_percent: Some(25.0),
                resets_at: None,
                window_minutes: Some(300),
                source: Some("test".into()),
            }],
            observed_at: "2026-01-01T00:00:00Z".into(),
            source: "test".into(),
            ..Default::default()
        };
        let failed = SubscriptionSnapshot {
            observed_at: "2026-01-02T00:00:00Z".into(),
            source: "official-cli".into(),
            error: Some("offline".into()),
            ..Default::default()
        };
        let merged = merge_snapshot(Some(previous), Some(failed)).expect("snapshot");
        assert!(merged.stale);
        assert_eq!(merged.credits_remaining.as_deref(), Some("12"));
        assert_eq!(merged.windows.len(), 1);
    }

    #[test]
    fn openai_import_targets_codex_backend_with_account_header() {
        let temp = tempfile::tempdir().expect("tempdir");
        let vault = test_vault(&temp);
        let account = CollectedAccount {
            account: DiscoveredAccount {
                native_credentials: None,
                refresh_bundle: None,
                provider_id: "openai",
                identity: Some("user@example.test".into()),
                account_id: Some("acct-123".into()),
                token: "codex-token".into(),
                credential_expires_at: None,
                plan: None,
            },
            snapshot: None,
        };

        let results = persist_official_accounts(&vault, vec![account]).expect("persist");
        assert_eq!(results[0].0.status, "imported");

        let entries = vault.list_provider_summaries().expect("summaries");
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].endpoints[0].url.as_deref(),
            Some("https://chatgpt.com/backend-api/codex")
        );
        assert_eq!(entries[0].interface_type, InterfaceType::OpenAiCompatible);
        assert_eq!(entries[0].auth_scheme, AuthScheme::Bearer);
        assert_eq!(entries[0].header_names, vec!["chatgpt-account-id"]);
    }

    #[test]
    fn grok_discovery_skips_plain_api_key_entries() {
        let value = serde_json::json!({
            "https://accounts.x.ai/sign-in": {
                "key": "session-token",
                "refresh_token": "refresh",
                "expires_at": 1_893_456_000
            },
            "api-key": {"key": "xai-plain-api-key"}
        });
        let accounts = grok_accounts_from(&value);
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].token, "session-token");
        assert_eq!(
            accounts[0].credential_expires_at.as_deref(),
            Some("2030-01-01T00:00:00Z")
        );

        // An issuer under accounts.x.ai counts even without parsed refresh or
        // expiry material; a bare key entry never does.
        let value = serde_json::json!({
            "https://accounts.x.ai/sign-in": {"key": "session-token"},
            "ci": {"key": "xai-ci-key"}
        });
        let accounts = grok_accounts_from(&value);
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].token, "session-token");
    }

    #[test]
    fn secret_less_entry_reports_an_error_instead_of_a_false_refresh() {
        let summary = aipass_vault::EntrySummary {
            max_concurrent_requests: None,
            supports_websockets: None,
            websocket_warning: None,
            id: uuid::Uuid::new_v4(),
            title: "desynced".to_string(),
            favorite: false,
            provider_id: Some("openai".to_string()),
            provider_kind: ProviderKind::Official,
            credential_kind: CredentialKind::OAuth,
            account_identity: None,
            domains: Vec::new(),
            favicon_url: None,
            endpoints: Vec::new(),
            interface_type: InterfaceType::OpenAiCompatible,
            auth_scheme: AuthScheme::Bearer,
            masked_secret: String::new(),
            fingerprint: String::new(),
            secret_refs: Vec::new(),
            default_model: None,
            model_aliases: Vec::new(),
            quota: None,
            subscription: None,
            gateway: None,
            usage_source: None,
            tags: Vec::new(),
            notes: None,
            header_names: Vec::new(),
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
            last_used_at: None,
            archived_at: None,
            deleted_at: None,
        };
        let error = primary_secret_ref(&summary).expect_err("no secrets must error");
        assert!(error.to_string().contains("no stored secret"));
    }

    #[test]
    fn jwt_exp_claim_is_decoded_without_signature_verification() {
        let payload =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"exp":1893456000}"#);
        let token = format!("header.{payload}.signature");
        assert_eq!(jwt_expiry(&token).as_deref(), Some("2030-01-01T00:00:00Z"));
        assert_eq!(jwt_expiry("not-a-jwt"), None);
        let no_exp = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"sub":"x"}"#);
        assert_eq!(jwt_expiry(&format!("header.{no_exp}.signature")), None);
    }

    #[test]
    fn archived_entry_is_refreshed_in_place_without_unarchiving() {
        let temp = tempfile::tempdir().expect("tempdir");
        let vault = test_vault(&temp);
        let first = persist_official_accounts(&vault, vec![identity_less_account("token-v1")])
            .expect("persist first");
        assert_eq!(first[0].0.status, "imported");
        let id = first[0].1.expect("entry id");
        vault.archive_provider(id).expect("archive");

        let second = persist_official_accounts(&vault, vec![identity_less_account("token-v2")])
            .expect("persist rotated");
        assert_eq!(second[0].0.status, "refreshed");
        assert_eq!(second[0].1, Some(id));

        // The match refreshed the archived entry in place: still archived,
        // still a single entry, holding the rotated token.
        assert!(vault.list_provider_summaries().expect("active").is_empty());
        let archived = vault.list_archived_provider_summaries().expect("archived");
        assert_eq!(archived.len(), 1);
        assert_eq!(
            archived[0].fingerprint,
            vault.fingerprint_secret("token-v2")
        );
    }

    fn encode_varint(mut value: u64) -> Vec<u8> {
        let mut bytes = Vec::new();
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            bytes.push(byte);
            if value == 0 {
                return bytes;
            }
        }
    }
    #[test]
    fn oauth_workspaces_with_the_same_email_keep_separate_tokens_and_headers() {
        let dir = tempfile::tempdir().unwrap();
        let creation = Vault::create(
            dir.path().join("vault"),
            &aipass_crypto::SecretString::new("test password"),
        )
        .unwrap();
        let vault = creation.vault;
        let save = |workspace: &str, token: &str| {
            persist_login_account(
                &vault,
                "openai",
                Some("alice@example.com".into()),
                Some(workspace.into()),
                token.into(),
                None,
            )
            .unwrap()
        };
        let first = save("personal", "personal-token");
        let second = save("team", "team-token");
        assert_ne!(first, second);
        assert_eq!(save("personal", "rotated-personal-token"), first);
        assert_eq!(
            vault.reveal_secret(first).unwrap(),
            "rotated-personal-token"
        );
        assert_eq!(vault.reveal_secret(second).unwrap(), "team-token");
        assert!(vault
            .reveal_provider_headers(first)
            .unwrap()
            .contains(&("chatgpt-account-id".into(), "personal".into())));
        assert!(vault
            .reveal_provider_headers(second)
            .unwrap()
            .contains(&("chatgpt-account-id".into(), "team".into())));
    }
}
