//! Validate an unmanaged historical grant in a private, same-backend CLI home.
use super::*;
use sha2::{Digest, Sha256};

pub(super) fn validate(
    state: &Arc<AgentState>,
    vault: &Vault,
    active: &NativeStore,
    tx: &Transaction,
) -> ServiceResult<Vec<(Resource, Zeroizing<Vec<u8>>)>> {
    let Some(old) = tx
        .changes
        .iter()
        .find(|c| c.resource == active.primary)
        .and_then(|c| c.before.as_ref())
    else {
        return Ok(Vec::new());
    };
    let auth = aipass_config_writers::native_auth::parse(old).map_err(safe_error)?;
    if auth.0.get("tokens").is_none() && auth.0.get("claudeAiOauth").is_none() {
        return Ok(Vec::new());
    }
    if aipass_config_writers::native_auth::credential_expiry(&active.tool, &auth.0)
        .is_none_or(|t| t <= time::OffsetDateTime::now_utc().unix_timestamp())
    {
        return Err(ServiceError::new(AgentErrorCode::ValidationFailed,
            "Original sign-in expired; reconnect that account using its official CLI before restoring. Current credentials are unchanged."));
    }
    let directory = tempfile::Builder::new()
        .prefix("verify-original-")
        .tempdir_in(root(state))
        .map_err(safe_error)?;
    let path = directory.path().canonicalize().map_err(safe_error)?;
    let file = Resource::File(path.join(if active.tool == "codex" {
        "auth.json"
    } else {
        ".credentials.json"
    }));
    let primary = if let Resource::Keychain { account, .. } = &active.primary {
        Resource::Keychain {
            service: if active.tool == "codex" {
                "Codex Auth".into()
            } else {
                format!(
                    "Claude Code-credentials-{}",
                    &format!("{:x}", Sha256::digest(path.to_string_lossy().as_bytes()))[..8]
                )
            },
            account: if active.tool == "codex" {
                format!(
                    "cli|{}",
                    &format!("{:x}", Sha256::digest(path.to_string_lossy().as_bytes()))[..16]
                )
            } else {
                account.clone()
            },
        }
    } else {
        file
    };
    // Cleanup runs on every failure as well as success; a Keychain grant must not
    // survive after its isolated vendor home has been removed.
    struct Cleanup(Resource);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = self.0.write(None);
        }
    }
    let cleanup = Cleanup(primary.clone());
    primary.write(Some(old)).map_err(safe_error)?;
    let profile = active
        .profile
        .as_ref()
        .map(|_| Resource::File(path.join(".claude.json")));
    if let (Some(source), Some(destination)) = (&active.profile, &profile) {
        let bytes = tx
            .changes
            .iter()
            .find(|c| &c.resource == source)
            .and_then(|c| c.before.as_ref());
        destination
            .write(bytes.map(Vec::as_slice))
            .map_err(safe_error)?;
    }
    if active.tool == "codex" {
        Resource::File(path.join("config.toml"))
            .write(Some(if matches!(primary, Resource::Keychain { .. }) {
                b"cli_auth_credentials_store = \"keyring\"\n"
            } else {
                b"cli_auth_credentials_store = \"file\"\n"
            }))
            .map_err(safe_error)?;
    }
    let isolated = NativeStore {
        tool: active.tool.clone(),
        home: path,
        primary,
        fallback: None,
        profile,
    };
    let expected = isolated.identity().map_err(safe_error)?.ok_or_else(|| {
        ServiceError::new(
            AgentErrorCode::ValidationFailed,
            "Original account identity unavailable; reconnect that account before restoring",
        )
    })?;
    let proxy = state
        .proxy
        .lock()
        .map_err(|_| safe_error(anyhow::anyhow!("proxy unavailable")))?
        .load_config(vault)?
        .upstream_proxy;
    let checked = if active.tool == "codex" {
        let mut worker = crate::subscriptions::Adapter::start(Some(&proxy))
            .map_err(|_| safe_error(anyhow::anyhow!("official verification unavailable")))?;
        let reference = crate::subscriptions::cli_accounts::reference("codex", &isolated.home)
            .map_err(|_| safe_error(anyhow::anyhow!("original account unavailable")))?;
        worker
            .process
            .send(&json!({"op":"native_verify","provider":"codex","auth":reference,"models":{}}))
            .and_then(|_| worker.next())
            .map(|v| v["value"]["verified"] == true)
    } else {
        crate::claude_cli::local_account(&isolated.home).and_then(|a| {
            if a.identity == expected {
                crate::claude_cli::usage_native(&isolated.home, &proxy).map(|_| true)
            } else {
                Err("original account changed".into())
            }
        })
    };
    if !matches!(checked, Ok(true))
        || isolated.identity().map_err(safe_error)?.as_deref() != Some(expected.as_str())
    {
        return Err(ServiceError::new(AgentErrorCode::ValidationFailed,
            "Original account verification failed; reconnect that account in its official CLI before restoring. Current credentials are unchanged."));
    }
    if isolated
        .expires_at()
        .map_err(safe_error)?
        .is_none_or(|t| t <= time::OffsetDateTime::now_utc().unix_timestamp())
    {
        return Err(ServiceError::new(
            AgentErrorCode::ValidationFailed,
            "Original sign-in expired; reconnect that account before restoring",
        ));
    }
    let result = isolated
        .primary
        .read()
        .map_err(safe_error)?
        .ok_or_else(|| safe_error(anyhow::anyhow!("original credential unavailable")))?;
    cleanup.0.write(None).map_err(safe_error)?;
    Ok(vec![(active.primary.clone(), result)])
}
