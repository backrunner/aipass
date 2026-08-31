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
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

const CODEX_APP_SERVER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
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

#[derive(Clone, Debug)]
struct DiscoveredAccount {
    provider_id: &'static str,
    identity: Option<String>,
    /// Codex `tokens.account_id`; sent upstream as the `chatgpt-account-id`
    /// header required by the Codex OAuth backend.
    account_id: Option<String>,
    token: String,
    credential_expires_at: Option<String>,
    plan: Option<String>,
}

/// A discovered account plus its freshly fetched usage snapshot.
///
/// Collected without touching the vault so the slow discovery and network
/// work never runs while the session lock is held.
pub(crate) struct CollectedAccount {
    account: DiscoveredAccount,
    snapshot: Option<SubscriptionSnapshot>,
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
    if requested("xai") {
        discovered.extend(discover_grok_accounts());
    }

    let mut seen_accounts = HashSet::new();
    discovered
        .into_iter()
        .filter(|account| {
            seen_accounts.insert((account.provider_id, account.token.clone()))
        })
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
    let existing_id = existing
        .iter()
        .find(|entry| {
            entry.provider_id.as_deref() == Some(account.provider_id)
                && (entry.credential_kind == CredentialKind::OAuth
                    || entry.tags.iter().any(|tag| tag == "oauth"))
                && (entry.account_identity == identity || entry.fingerprint == fingerprint)
        })
        .map(|entry| entry.id)
        .or_else(|| {
            // Identity-less accounts carry a synthetic `account:<fingerprint>`
            // identity that goes stale on every token rotation, so the direct
            // match above can never find them again after the CLI rotates its
            // access token.
            if account.identity.is_some() {
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
    summary.secret_refs.first().ok_or_else(|| {
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
        "openai" => match codex_usage() {
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

fn codex_usage() -> anyhow::Result<(Vec<SubscriptionWindow>, Option<String>, Option<String>)> {
    let mut child = Command::new("codex")
        .args(["app-server", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    // Always reap the app-server, including on timeout and parse errors.
    let result = read_codex_usage(&mut child);
    let _ = child.kill();
    let _ = child.wait();
    result
}

fn read_codex_usage(
    child: &mut std::process::Child,
) -> anyhow::Result<(Vec<SubscriptionWindow>, Option<String>, Option<String>)> {
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("codex app-server stdin unavailable"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("codex app-server stdout unavailable"))?;
    for request in [
        serde_json::json!({"id": 1, "method": "initialize", "params": {"clientInfo": {"name": "aipass", "version": env!("CARGO_PKG_VERSION")}}}),
        serde_json::json!({"method": "initialized", "params": {}}),
        serde_json::json!({"id": 2, "method": "account/read", "params": {"refreshToken": false}}),
        serde_json::json!({"id": 3, "method": "account/rateLimits/read", "params": null}),
    ] {
        writeln!(stdin, "{request}")?;
    }
    stdin.flush()?;
    drop(stdin);

    let (lines_tx, lines_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            if lines_tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut account_response = None;
    let mut limits_response = None;
    while account_response.is_none() || limits_response.is_none() {
        let line = lines_rx
            .recv_timeout(CODEX_APP_SERVER_TIMEOUT)
            .map_err(|error| anyhow::anyhow!("Codex app-server timed out: {error}"))??;
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match value.get("id").and_then(Value::as_i64) {
            Some(2) => account_response = Some(value),
            Some(3) => limits_response = Some(value),
            _ => {}
        }
    }
    let account = account_response
        .and_then(|value| value.get("result").cloned())
        .unwrap_or_default();
    let limits = limits_response
        .and_then(|value| value.get("result").cloned())
        .unwrap_or_default();
    let account_data = account.get("account").unwrap_or(&account);
    let plan = find_string(account_data, &["planType", "plan_type"]);
    let limit_data = limits.get("rateLimits").unwrap_or(&limits);
    let mut windows = Vec::new();
    for (key, label) in [("primary", "primary"), ("secondary", "secondary")] {
        let Some(window) = limit_data.get(key).and_then(Value::as_object) else {
            continue;
        };
        let used_percent = window.get("usedPercent").and_then(Value::as_f64);
        let resets_at = window
            .get("resetsAt")
            .and_then(Value::as_i64)
            .and_then(unix_timestamp);
        let window_minutes = window.get("windowDurationMins").and_then(Value::as_u64);
        if used_percent.is_some() || resets_at.is_some() {
            windows.push(SubscriptionWindow {
                id: key.to_string(),
                label: label.to_string(),
                used_percent,
                resets_at,
                window_minutes,
                source: Some("codex-app-server".to_string()),
            });
        }
    }
    let credits = limit_data
        .get("credits")
        .and_then(|value| value.get("balance"))
        .and_then(Value::as_str)
        .map(str::to_string);
    if windows.is_empty() && credits.is_none() && plan.is_none() {
        anyhow::bail!("Codex app-server did not return account usage")
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
    vec![DiscoveredAccount {
        provider_id: "openai",
        identity,
        account_id,
        token,
        credential_expires_at: expiry,
        plan: None,
    }]
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
    let identity = find_string(&value, &["email", "emailAddress", "email_address"])
        .or_else(|| find_string(oauth, &["email", "emailAddress", "email_address"]));
    let expiry = find_timestamp(oauth, &["expiresAt", "expires_at"]);
    let plan = find_string(oauth, &["subscriptionType", "rateLimitTier"]);
    vec![DiscoveredAccount {
        provider_id: "anthropic",
        identity,
        account_id: None,
        token,
        credential_expires_at: expiry,
        plan,
    }]
}

fn read_claude_credentials() -> anyhow::Result<Value> {
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("security")
            .args([
                "find-generic-password",
                "-s",
                "Claude Code-credentials",
                "-w",
            ])
            .output();
        if let Ok(output) = output {
            if output.status.success() {
                if let Ok(value) = serde_json::from_slice::<Value>(&output.stdout) {
                    return Ok(value);
                }
            }
        }
    }
    read_json(&home().join(".claude").join(".credentials.json"))
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
            Some(DiscoveredAccount {
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
        .timeout(CODEX_APP_SERVER_TIMEOUT)
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
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
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

    fn test_vault(temp: &tempfile::TempDir) -> Vault {
        Vault::create(temp.path(), &SecretString::new("correct horse battery staple"))
            .expect("create vault")
            .vault
    }

    fn identity_less_account(token: &str) -> CollectedAccount {
        CollectedAccount {
            account: DiscoveredAccount {
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
}
