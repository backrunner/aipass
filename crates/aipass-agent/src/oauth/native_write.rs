//! Write managed OAuth tokens back into the official CLIs' native credential
//! files, and adopt tokens the CLI rotated on its own.
//!
//! Codex (`~/.codex/auth.json`) and Grok (`~/.grok/auth.json`) both refresh
//! their own tokens at runtime. If we blindly overwrote those files we would
//! clobber a newer refresh_token the CLI just obtained, and the CLI would in
//! turn clobber ours. So before writing we compare a "generation" marker
//! (last_refresh timestamp + refresh_token) and adopt the newer side instead of
//! overwriting. Writes are atomic and the previous file is backed up first.
//! Anything whose shape we do not recognize is left untouched.

use crate::official_accounts::home;
use aipass_storage::atomic_write_bytes;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

/// What happened when we tried to reconcile our tokens with the native file.
#[derive(Clone, Debug)]
pub(crate) enum NativeSyncOutcome {
    /// We wrote our bundle into the native file.
    Written,
    /// The CLI had a newer generation; the caller must adopt these tokens.
    /// `expires_at_ms` carries the adopted token's real expiry when the native
    /// file or access JWT records one, so the stored expiry describes the
    /// adopted access token.
    Adopted {
        access_token: String,
        refresh_token: String,
        id_token: Option<String>,
        last_refresh_ms: i64,
        expires_at_ms: Option<i64>,
    },
    /// The native file existed in a shape we do not understand; left untouched.
    Skipped(String),
}

fn now_ms() -> i64 {
    super::now_ms()
}

fn rfc3339_to_ms(value: &str) -> Option<i64> {
    OffsetDateTime::parse(value, &Rfc3339)
        .ok()
        .map(|dt| (dt.unix_timestamp_nanos() / 1_000_000) as i64)
}

pub(crate) fn ms_to_rfc3339(ms: i64) -> String {
    OffsetDateTime::from_unix_timestamp_nanos((ms as i128) * 1_000_000)
        .ok()
        .and_then(|dt| dt.format(&Rfc3339).ok())
        .unwrap_or_default()
}

fn codex_dir() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".codex"))
}

fn grok_dir() -> PathBuf {
    home().join(".grok")
}

/// How many generations of each credential file to keep in `.aipass-backups`.
/// Encrypted backups contain refresh tokens, so retention is bounded.
const MAX_NATIVE_BACKUPS: usize = 5;

/// Back up an existing credential file into a sibling `.aipass-backups` dir
/// before overwriting it, so a bad write is always recoverable.
fn backup_existing(path: &Path, backup_key: &[u8; 32]) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let dir = path.parent().unwrap_or(path).join(".aipass-backups");
    fs::create_dir_all(&dir)?;
    let stamp = ms_to_rfc3339(now_ms()).replace(':', "-");
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let backup = dir.join(format!("{name}.{stamp}.encrypted.bak"));
    let bytes = zeroize::Zeroizing::new(fs::read(path)?);
    let encrypted =
        aipass_crypto::encrypt_bytes(backup_key, b"aipass oauth native backup v1", &bytes)?;
    atomic_write_bytes(&backup, &serde_json::to_vec(&encrypted)?)?;
    prune_backups(&dir, &name);
    Ok(())
}

/// Keep only the newest `MAX_NATIVE_BACKUPS` backups of a file. Best-effort:
/// a pruning failure must not fail the credential write that triggered it.
fn prune_backups(dir: &Path, name: &str) {
    let prefix = format!("{name}.");
    let mut backups: Vec<PathBuf> = match fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|file| file.starts_with(&prefix) && file.ends_with(".bak"))
            .map(|file| dir.join(file))
            .collect(),
        Err(_) => return,
    };
    // The RFC3339 stamp in the file name sorts chronologically.
    backups.sort();
    while backups.len() > MAX_NATIVE_BACKUPS {
        let _ = fs::remove_file(backups.remove(0));
    }
}

fn read_json(path: &Path) -> anyhow::Result<Option<Value>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(serde_json::from_str(&text)?)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn codex_account_matches(existing: &Value, account_id: &str, id_token: Option<&str>) -> bool {
    if account_id.is_empty()
        || existing
            .pointer("/tokens/account_id")
            .and_then(Value::as_str)
            != Some(account_id)
    {
        return false;
    }
    // A workspace may contain multiple users. Match both the workspace and
    // the OIDC subject before adopting or overwriting native credentials.
    let subject = |token: &str| {
        super::jwt_claims(token).and_then(|claims| {
            claims
                .get("sub")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
    };
    let ours = id_token.and_then(subject);
    let theirs = existing
        .pointer("/tokens/id_token")
        .and_then(Value::as_str)
        .and_then(subject);
    ours.is_some() && ours == theirs
}

fn grok_account_matches(existing: &Value, identity: Option<&str>) -> bool {
    identity
        .filter(|value| !value.trim().is_empty())
        .is_some_and(|identity| existing.get("email").and_then(Value::as_str) == Some(identity))
}

/// Reconcile our Codex tokens with `~/.codex/auth.json`.
///
/// `our_generation_ms` is the last_refresh generation we believe is current:
/// `None` on a fresh interactive login (always write our new token), `Some`
/// during a background refresh (adopt the CLI's token only if the native file
/// is strictly newer). `write_last_refresh_ms` is the timestamp written into
/// the file for our own bundle.
pub(crate) fn sync_codex_auth_json(
    backup_key: &[u8; 32],
    access_token: &str,
    refresh_token: &str,
    id_token: Option<&str>,
    chatgpt_account_id: &str,
    our_generation_ms: Option<i64>,
    write_last_refresh_ms: i64,
) -> anyhow::Result<NativeSyncOutcome> {
    let path = codex_dir().join("auth.json");
    if let Some(existing) = read_json(&path)? {
        // Only touch a file that already looks like a ChatGPT OAuth login.
        if existing.get("auth_mode").and_then(Value::as_str) != Some("chatgpt") {
            return Ok(NativeSyncOutcome::Skipped(
                "existing auth.json is not a chatgpt login".into(),
            ));
        }
        if let Some(generation) = our_generation_ms {
            if !codex_account_matches(&existing, chatgpt_account_id, id_token) {
                return Ok(NativeSyncOutcome::Skipped(
                    "native Codex account differs from refreshed account".into(),
                ));
            }
            let native_refresh = existing
                .pointer("/tokens/refresh_token")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let native_last_refresh_ms = existing
                .get("last_refresh")
                .and_then(Value::as_str)
                .and_then(rfc3339_to_ms)
                .unwrap_or(0);
            // The CLI rotated its refresh_token after ours: adopt it rather than
            // overwrite, so the next refresh uses the live generation.
            if !native_refresh.is_empty()
                && native_refresh != refresh_token
                && existing
                    .pointer("/tokens/access_token")
                    .and_then(Value::as_str)
                    .is_some_and(|v| !v.trim().is_empty())
                && native_last_refresh_ms > generation
            {
                return Ok(NativeSyncOutcome::Adopted {
                    access_token: existing
                        .pointer("/tokens/access_token")
                        .and_then(Value::as_str)
                        .unwrap_or(access_token)
                        .to_string(),
                    refresh_token: native_refresh.to_string(),
                    id_token: existing
                        .pointer("/tokens/id_token")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    last_refresh_ms: native_last_refresh_ms,
                    expires_at_ms: existing
                        .pointer("/tokens/access_token")
                        .and_then(Value::as_str)
                        .and_then(super::jwt_claims)
                        .and_then(|claims| claims.get("exp").and_then(Value::as_i64))
                        .map(|exp| exp.saturating_mul(1000))
                        .or(Some(native_last_refresh_ms.saturating_add(3_600_000))),
                });
            }
        }
    }
    let mut tokens = json!({
        "access_token": access_token,
        "refresh_token": refresh_token,
        "account_id": chatgpt_account_id,
    });
    if let Some(id_token) = id_token {
        tokens["id_token"] = json!(id_token);
    }
    let content = json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": tokens,
        "last_refresh": ms_to_rfc3339(write_last_refresh_ms),
    });
    backup_existing(&path, backup_key)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let encoded = format!("{}\n", serde_json::to_string_pretty(&content)?);
    atomic_write_bytes(&path, encoded.as_bytes())?;
    Ok(NativeSyncOutcome::Written)
}

/// Reconcile our Grok tokens with `~/.grok/auth.json`.
///
/// The Grok CLI stores OIDC sessions in a top-level object keyed by issuer URL
/// (an entry carrying `refresh_token`/`expires_at`, per `grok_accounts_from`).
/// We update an existing issuer entry in place when present; otherwise we create
/// one under the xAI issuer. An unrecognized shape is left untouched.
pub(crate) fn sync_grok_auth_json(
    backup_key: &[u8; 32],
    access_token: &str,
    refresh_token: &str,
    our_generation_expires_ms: Option<i64>,
    write_expires_ms: i64,
    identity: Option<&str>,
) -> anyhow::Result<NativeSyncOutcome> {
    const XAI_ISSUER_KEY: &str = "https://accounts.x.ai";
    let path = grok_dir().join("auth.json");
    let mut root = match read_json(&path)? {
        Some(Value::Object(map)) => Value::Object(map),
        Some(_) => {
            return Ok(NativeSyncOutcome::Skipped(
                "existing grok auth.json is not an object".into(),
            ))
        }
        None => json!({}),
    };

    // Find the issuer key that already holds an OAuth session, if any.
    let issuer_key = root
        .as_object()
        .and_then(|map| {
            map.keys()
                .find(|key| {
                    url::Url::parse(key).ok().is_some_and(|url| {
                        url.scheme() == "https"
                            && matches!(url.host_str(), Some("accounts.x.ai" | "auth.x.ai"))
                    })
                })
                .cloned()
        })
        .unwrap_or_else(|| XAI_ISSUER_KEY.to_string());

    if let Some(generation) = our_generation_expires_ms {
        if let Some(existing_entry) = root.get(&issuer_key).cloned() {
            if !grok_account_matches(&existing_entry, identity) {
                return Ok(NativeSyncOutcome::Skipped(
                    "native Grok account differs from refreshed account".into(),
                ));
            }
            let native_refresh = existing_entry
                .get("refresh_token")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let native_expires_ms = existing_entry
                .get("expires_at")
                .and_then(Value::as_str)
                .and_then(rfc3339_to_ms)
                .unwrap_or(0);
            if !native_refresh.is_empty()
                && native_refresh != refresh_token
                && existing_entry
                    .get("access_token")
                    .and_then(Value::as_str)
                    .is_some_and(|v| !v.trim().is_empty())
                && native_expires_ms > generation
            {
                return Ok(NativeSyncOutcome::Adopted {
                    access_token: existing_entry
                        .get("access_token")
                        .and_then(Value::as_str)
                        .unwrap_or(access_token)
                        .to_string(),
                    refresh_token: native_refresh.to_string(),
                    id_token: None,
                    last_refresh_ms: now_ms(),
                    expires_at_ms: Some(native_expires_ms),
                });
            }
        }
    }

    let mut entry = json!({
        "access_token": access_token,
        "refresh_token": refresh_token,
        "expires_at": ms_to_rfc3339(write_expires_ms),
    });
    if let Some(identity) = identity {
        entry["email"] = json!(identity);
    }
    root[&issuer_key] = entry;
    backup_existing(&path, backup_key)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let encoded = format!("{}\n", serde_json::to_string_pretty(&root)?);
    atomic_write_bytes(&path, encoded.as_bytes())?;
    Ok(NativeSyncOutcome::Written)
}

/// Read a CLI rotation before spending the manager's potentially stale refresh
/// token. Never read an unrelated account into a managed account.
pub(crate) fn newer_native_bundle(
    account: &aipass_vault::ManagedOAuthAccount,
) -> anyhow::Result<Option<super::OAuthTokenBundle>> {
    use aipass_provider_registry::OAuthProvider;
    let path = match account.provider {
        OAuthProvider::Codex => codex_dir().join("auth.json"),
        OAuthProvider::Grok => grok_dir().join("auth.json"),
    };
    let Some(root) = read_json(&path)? else {
        return Ok(None);
    };
    Ok(newer_native_bundle_from(account, &root))
}

fn newer_native_bundle_from(
    account: &aipass_vault::ManagedOAuthAccount,
    root: &Value,
) -> Option<super::OAuthTokenBundle> {
    use aipass_provider_registry::OAuthProvider;
    let (tokens, expires_at_ms) = match account.provider {
        OAuthProvider::Codex => {
            if root.get("auth_mode").and_then(Value::as_str) != Some("chatgpt")
                || !codex_account_matches(
                    root,
                    account.chatgpt_account_id.as_deref().unwrap_or_default(),
                    account.id_token.as_deref(),
                )
            {
                return None;
            }
            let generation = root
                .get("last_refresh")
                .and_then(Value::as_str)
                .and_then(rfc3339_to_ms)
                .unwrap_or(0);
            if generation <= account.last_refresh_ms {
                return None;
            }
            let tokens = root.get("tokens").cloned().unwrap_or(Value::Null);
            let expires = tokens
                .get("access_token")
                .and_then(Value::as_str)
                .and_then(super::jwt_claims)
                .and_then(|claims| claims.get("exp").and_then(Value::as_i64))
                .map(|exp| exp.saturating_mul(1000))
                .unwrap_or(generation.saturating_add(3_600_000));
            (tokens, expires)
        }
        OAuthProvider::Grok => {
            let tokens = root.as_object().and_then(|map| {
                map.iter().find_map(|(issuer, tokens)| {
                    let url = url::Url::parse(issuer).ok()?;
                    (url.scheme() == "https"
                        && matches!(url.host_str(), Some("accounts.x.ai" | "auth.x.ai"))
                        && grok_account_matches(tokens, account.account_identity.as_deref()))
                    .then(|| tokens.clone())
                })
            })?;
            let expires = tokens
                .get("expires_at")
                .and_then(Value::as_str)
                .and_then(rfc3339_to_ms)
                .unwrap_or(0);
            if expires <= account.expires_at_ms {
                return None;
            }
            (tokens, expires)
        }
    };
    let access = tokens
        .get("access_token")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let refresh = tokens
        .get("refresh_token")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if access.trim().is_empty() || refresh.trim().is_empty() || refresh == account.refresh_token {
        return None;
    }
    Some(super::OAuthTokenBundle {
        access_token: access.into(),
        refresh_token: refresh.into(),
        id_token: tokens
            .get("id_token")
            .and_then(Value::as_str)
            .map(str::to_string),
        chatgpt_account_id: account.chatgpt_account_id.clone(),
        account_identity: account.account_identity.clone(),
        expires_in: (expires_at_ms - now_ms()) / 1000,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    fn jwt(subject: &str) -> String {
        format!(
            "header.{}.signature",
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(serde_json::to_vec(&json!({"sub": subject})).unwrap())
        )
    }

    #[test]
    fn native_accounts_must_match_workspace_and_subject() {
        let root = json!({"tokens": {"account_id": "workspace", "id_token": jwt("alice")}});
        assert!(codex_account_matches(
            &root,
            "workspace",
            Some(&jwt("alice"))
        ));
        assert!(!codex_account_matches(
            &root,
            "other-workspace",
            Some(&jwt("alice"))
        ));
        assert!(!codex_account_matches(
            &root,
            "workspace",
            Some(&jwt("bob"))
        ));
        assert!(!codex_account_matches(&root, "workspace", None));
        assert!(!grok_account_matches(
            &json!({"email":"alice"}),
            Some("bob")
        ));
        assert!(!grok_account_matches(&json!({}), None));
    }

    #[test]
    fn malformed_native_files_are_not_treated_as_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        assert!(read_json(&path).unwrap().is_none());
        fs::write(&path, "broken-json").unwrap();
        assert!(read_json(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "broken-json");
    }

    #[test]
    fn native_backups_are_encrypted_and_recoverable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        let original = br#"{"refresh_token":"fake-refresh-secret"}"#;
        fs::write(&path, original).unwrap();
        backup_existing(&path, &[7; 32]).unwrap();
        let backup = fs::read_dir(dir.path().join(".aipass-backups"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let bytes = fs::read(backup).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("fake-refresh-secret"));
        let ciphertext = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            aipass_crypto::decrypt_bytes(&[7; 32], b"aipass oauth native backup v1", &ciphertext)
                .unwrap(),
            original
        );
    }
    #[test]
    fn cli_rotations_require_a_newer_complete_bundle_for_the_same_account() {
        let account = aipass_vault::ManagedOAuthAccount {
            id: uuid::Uuid::new_v4(),
            provider: aipass_provider_registry::OAuthProvider::Codex,
            account_identity: Some("alice".into()),
            chatgpt_account_id: Some("workspace".into()),
            access_token: "old-access".into(),
            refresh_token: "old-refresh".into(),
            id_token: Some(jwt("alice")),
            expires_at_ms: 1000,
            last_refresh_ms: 1000,
            entry_id: None,
            is_default: true,
            requires_reauth: false,
            authenticated_at: OffsetDateTime::now_utc(),
        };
        let mut root = json!({"auth_mode":"chatgpt", "last_refresh": ms_to_rfc3339(2000), "tokens": {
            "account_id":"workspace", "id_token":jwt("alice"), "access_token":"cli-access", "refresh_token":"cli-refresh"
        }});
        assert_eq!(
            newer_native_bundle_from(&account, &root)
                .unwrap()
                .refresh_token,
            "cli-refresh"
        );
        root["tokens"]["account_id"] = json!("other-workspace");
        assert!(newer_native_bundle_from(&account, &root).is_none());
        root["tokens"]["account_id"] = json!("workspace");
        root["tokens"]["access_token"] = json!("");
        assert!(newer_native_bundle_from(&account, &root).is_none());
        root["tokens"]["access_token"] = json!("cli-access");
        root["last_refresh"] = json!(ms_to_rfc3339(500));
        assert!(newer_native_bundle_from(&account, &root).is_none());
    }
}
