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
    /// file records one (Grok), so the stored expiry matches the stored access
    /// token; `None` when the file has no expiry (Codex).
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
/// Backups contain live refresh tokens, so they must not accumulate forever.
const MAX_NATIVE_BACKUPS: usize = 5;

/// Back up an existing credential file into a sibling `.aipass-backups` dir
/// before overwriting it, so a bad write is always recoverable.
fn backup_existing(path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let dir = path.parent().unwrap_or(path).join(".aipass-backups");
    fs::create_dir_all(&dir)?;
    let stamp = ms_to_rfc3339(now_ms()).replace(':', "-");
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let backup = dir.join(format!("{name}.{stamp}.bak"));
    let bytes = fs::read(path)?;
    atomic_write_bytes(&backup, &bytes)?;
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

fn read_json(path: &Path) -> Option<Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
}

/// Reconcile our Codex tokens with `~/.codex/auth.json`.
///
/// `our_generation_ms` is the last_refresh generation we believe is current:
/// `None` on a fresh interactive login (always write our new token), `Some`
/// during a background refresh (adopt the CLI's token only if the native file
/// is strictly newer). `write_last_refresh_ms` is the timestamp written into
/// the file for our own bundle.
pub(crate) fn sync_codex_auth_json(
    access_token: &str,
    refresh_token: &str,
    id_token: Option<&str>,
    chatgpt_account_id: &str,
    our_generation_ms: Option<i64>,
    write_last_refresh_ms: i64,
) -> anyhow::Result<NativeSyncOutcome> {
    let path = codex_dir().join("auth.json");
    if let Some(existing) = read_json(&path) {
        // Only touch a file that already looks like a ChatGPT OAuth login.
        if existing.get("auth_mode").and_then(Value::as_str) != Some("chatgpt") {
            return Ok(NativeSyncOutcome::Skipped(
                "existing auth.json is not a chatgpt login".into(),
            ));
        }
        if let Some(generation) = our_generation_ms {
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
                    // The Codex native file records no token expiry.
                    expires_at_ms: None,
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
    backup_existing(&path)?;
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
    access_token: &str,
    refresh_token: &str,
    our_generation_expires_ms: Option<i64>,
    write_expires_ms: i64,
    identity: Option<&str>,
) -> anyhow::Result<NativeSyncOutcome> {
    const XAI_ISSUER_KEY: &str = "https://accounts.x.ai";
    let path = grok_dir().join("auth.json");
    let mut root = match read_json(&path) {
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
                .find(|key| key.contains("accounts.x.ai"))
                .cloned()
        })
        .unwrap_or_else(|| XAI_ISSUER_KEY.to_string());

    if let Some(generation) = our_generation_expires_ms {
        if let Some(existing_entry) = root.get(&issuer_key).cloned() {
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
    backup_existing(&path)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let encoded = format!("{}\n", serde_json::to_string_pretty(&root)?);
    atomic_write_bytes(&path, encoded.as_bytes())?;
    Ok(NativeSyncOutcome::Written)
}
