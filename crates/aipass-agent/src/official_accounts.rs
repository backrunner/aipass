//! CLI-owned subscriptions: discovery saves references, never OAuth grants.
use crate::session::{map_vault_error, with_vault, AgentState, ServiceError, ServiceResult};
use crate::subscriptions::cli_accounts;
use aipass_crypto::SecretString;
use aipass_provider_registry::{
    AuthScheme, CredentialKind, InterfaceType, ProviderEndpoint, ProviderKind, SubscriptionSnapshot,
};
use aipass_vault::{ProviderEntryInput, Vault};
use serde_json::{json, Value};
use std::path::PathBuf;
use uuid::Uuid;

pub(crate) fn persist_claude_login(
    vault: &Vault,
    native: &crate::claude_cli::NativeAccount,
) -> anyhow::Result<Uuid> {
    let reference = SecretString::new(
        json!({"nativeHome":native.home,"accountId":native.identity,"nativeDevice":cli_accounts::device().map_err(anyhow::Error::msg)?}).to_string(),
    );
    let existing = vault.list_provider_summaries()?.into_iter().find(|e| {
        e.provider_id.as_deref() == Some("anthropic")
            && e.credential_kind == CredentialKind::OAuth
            && e.account_identity.as_deref() == Some(&native.identity)
    });
    let id = if let Some(entry) = existing {
        entry.id
    } else {
        vault.add_provider_with_runtime_extension(
            ProviderEntryInput {
                title: format!("Claude · {}", native.identity),
                provider_kind: ProviderKind::Official,
                provider_id: Some("anthropic".into()),
                credential_kind: CredentialKind::OAuth,
                account_identity: Some(native.identity.clone()),
                domains: vec![],
                favicon_url: None,
                endpoints: vec![ProviderEndpoint::api("https://api.anthropic.com")],
                interface_type: InterfaceType::AnthropicMessages,
                max_concurrent_requests: Some(1),
                supports_websockets: Some(false),
                auth_scheme: AuthScheme::Bearer,
                api_key: format!("aipass:claude-cli:{}", Uuid::new_v4()),
                secret_label: Some("Subscription".into()),
                default_model: None,
                model_aliases: vec![],
                headers: vec![],
                quota: None,
                subscription: Some(SubscriptionSnapshot {
                    plan: native.plan.clone(),
                    source: "claude-cli".into(),
                    observed_at: now(),
                    ..Default::default()
                }),
                gateway: None,
                tags: vec!["subscription".into()],
                notes: None,
                secret_metadata: Default::default(),
            },
            "claude_cli_home",
            &reference,
        )?
    };
    if vault
        .provider_runtime_extension(id, "claude_cli_home")?
        .as_ref()
        .is_some_and(|v| v.expose() == reference.expose())
    {
        return Ok(id);
    }
    vault.bind_cli_subscription(
        id,
        "anthropic",
        &native.identity,
        &format!("aipass:claude-cli:{}", Uuid::new_v4()),
        "claude_cli_home",
        &reference,
    )?;
    Ok(id)
}
pub(crate) fn claude_home() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".claude"))
}
pub(crate) fn claude_reference(vault: &Vault, id: Uuid) -> ServiceResult<Option<Value>> {
    vault
        .provider_runtime_extension(id, "claude_cli_home")
        .map_err(map_vault_error)?
        .map(|v| serde_json::from_str(v.expose()).map_err(ServiceError::internal))
        .transpose()
}
pub(crate) fn refresh_registered_accounts(state: &std::sync::Arc<AgentState>) -> ServiceResult<()> {
    let due = with_vault(state, false, |vault| {
        if migrate(vault).is_err() {
            crate::logging::write_component_log(
                crate::logging::AGENT_LOG,
                "WARN",
                "event=subscription.cli_handoff.failed",
            );
        }
        let mut due = Vec::new();
        for entry in vault.list_provider_summaries().map_err(map_vault_error)? {
            if claude_reference(vault, entry.id)?.is_none() {
                continue;
            }
            let prefs = crate::provider_runtime::load(vault, entry.id)?;
            if !prefs.quota_tracking {
                continue;
            }
            let recent = entry
                .subscription
                .as_ref()
                .and_then(|s| {
                    time::OffsetDateTime::parse(
                        &s.observed_at,
                        &time::format_description::well_known::Rfc3339,
                    )
                    .ok()
                })
                .is_some_and(|t| {
                    (time::OffsetDateTime::now_utc() - t).whole_seconds()
                        < i64::from(prefs.quota_refresh_seconds)
                });
            if !recent {
                due.push(entry.id);
            }
        }
        Ok(due)
    })?;
    for id in due {
        let _ = refresh_claude(state, id);
    }
    Ok(())
}

pub(crate) fn refresh_claude(
    state: &std::sync::Arc<AgentState>,
    id: Uuid,
) -> ServiceResult<SubscriptionSnapshot> {
    let (auth, previous, outbound) = with_vault(state, false, |vault| {
        let auth = claude_reference(vault, id)?.ok_or_else(|| {
            ServiceError::internal(anyhow::anyhow!("Connect Claude with its official CLI"))
        })?;
        let entry = vault.get_provider_summary(id).map_err(map_vault_error)?;
        let prefs = crate::provider_runtime::load(vault, id)?;
        let global = state
            .proxy
            .lock()
            .map_err(|_| ServiceError::internal(anyhow::anyhow!("proxy unavailable")))?
            .load_config(vault)?
            .upstream_proxy;
        let outbound = crate::provider_runtime::outbound(&prefs)
            .map_err(|e| ServiceError::internal(anyhow::anyhow!(e)))?
            .unwrap_or(global);
        Ok((auth, entry.subscription, outbound))
    })?;
    let path = PathBuf::from(auth["nativeHome"].as_str().unwrap_or(""));
    let result = cli_accounts::check_device(&auth)
        .and_then(|_| crate::claude_cli::local_account(&path))
        .and_then(|a| {
            if a.identity == auth["accountId"] {
                crate::claude_cli::usage_native(&path, &outbound)
            } else {
                Err("Claude CLI account changed; reconnect it".into())
            }
        });
    let mut snapshot = previous.unwrap_or_default();
    snapshot.source = "claude-cli".into();
    snapshot.observed_at = now();
    match result {
        Ok(w) => {
            snapshot.windows = w;
            snapshot.error = None;
            snapshot.stale = false;
        }
        Err(e) => {
            snapshot.error = Some(e);
            snapshot.stale = true;
        }
    }
    with_vault(state, false, |vault| {
        if claude_reference(vault, id)? != Some(auth.clone()) {
            return Err(ServiceError::internal(anyhow::anyhow!(
                "Claude account changed during quota read"
            )));
        }
        vault
            .update_provider_subscription(id, Some(snapshot.clone()))
            .map_err(map_vault_error)?;
        state
            .proxy
            .lock()
            .map_err(|_| ServiceError::internal(anyhow::anyhow!("proxy unavailable")))?
            .refresh_provider_credentials(vault, id)?;
        Ok(())
    })?;
    state
        .sync_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(snapshot)
}

/// Idempotent handoff of older encrypted grants. The CLI file is verified before
/// the reference replaces the mirror; a failed handoff keeps recoverable input.
pub(crate) fn migrate(vault: &Vault) -> ServiceResult<()> {
    let mut failure = None;
    for account in vault.list_oauth_accounts(None).map_err(map_vault_error)? {
        let Some(id) = account.entry_id else {
            continue;
        };
        let result = if crate::community::is_account(vault, id) {
            vault
                .remove_oauth_account(account.id)
                .map_err(map_vault_error)
        } else {
            let provider = match account.provider {
                aipass_provider_registry::OAuthProvider::Codex => "codex",
                aipass_provider_registry::OAuthProvider::Grok => "grok",
            };
            cli_accounts::handoff_home(provider, id)
                .map_err(|e| ServiceError::internal(anyhow::anyhow!(e)))
                .and_then(|path| {
                    migrate_managed_at(
                        vault,
                        &account,
                        cli_accounts::default_home(provider).ok(),
                        path,
                    )
                })
        };
        if let Err(e) = result {
            failure = Some(e);
        }
    }
    // Older imported mirrors can reconnect to the current official CLI without
    // copying their grants. Never substitute another account from that CLI.
    for entry in vault.list_provider_summaries().map_err(map_vault_error)? {
        if entry.credential_kind != CredentialKind::OAuth
            || entry.provider_kind != ProviderKind::Official
        {
            continue;
        }
        if crate::community::is_account(vault, entry.id)
            || claude_reference(vault, entry.id)?.is_some()
        {
            continue;
        }
        if entry.provider_id.as_deref() == Some("anthropic") {
            if let Ok(native) = crate::claude_cli::local_account(&claude_home()) {
                if entry.account_identity.as_deref() == Some(&native.identity) {
                    if let Err(e) = persist_claude_login(vault, &native) {
                        failure = Some(ServiceError::internal(e));
                    }
                }
            }
        } else {
            let provider = match entry.provider_id.as_deref() {
                Some("openai" | "codex") => "codex",
                Some("xai" | "grok") => "grok",
                Some("copilot") => "copilot",
                Some("gemini-cli") => "gemini-cli",
                _ => continue,
            };
            if let Ok(auth) = cli_accounts::default_home(provider)
                .and_then(|p| cli_accounts::reference(provider, &p))
            {
                if entry.account_identity.as_deref() == auth["accountId"].as_str() {
                    if let Err(e) = crate::community::bind_cli(vault, entry.id, provider, auth) {
                        failure = Some(e);
                    }
                }
            }
        }
    }
    failure.map_or(Ok(()), Err)
}

fn migrate_managed_at(
    vault: &Vault,
    account: &aipass_vault::ManagedOAuthAccount,
    ambient: Option<PathBuf>,
    handoff: PathBuf,
) -> ServiceResult<()> {
    let id = account
        .entry_id
        .ok_or_else(|| ServiceError::internal(anyhow::anyhow!("legacy account has no provider")))?;
    let provider = match account.provider {
        aipass_provider_registry::OAuthProvider::Codex => "codex",
        aipass_provider_registry::OAuthProvider::Grok => "grok",
    };
    let expected = account.account_identity.clone().unwrap_or_default();
    let matches = |auth: &Value| {
        let identity = auth["accountId"].as_str().unwrap_or("");
        if provider == "codex" {
            account
                .chatgpt_account_id
                .as_deref()
                .filter(|s| !s.is_empty())
                .is_some_and(|workspace| {
                    !expected.is_empty() && identity == format!("{expected}:{workspace}")
                })
        } else {
            !expected.is_empty() && identity == expected
        }
    };
    let ambient = ambient
        .filter(|path| cli_accounts::reference(provider, path).is_ok_and(|auth| matches(&auth)));
    let path = ambient.unwrap_or(handoff);
    let file = path.join("auth.json");
    if !file.exists() {
        let raw = if provider == "codex" {
            json!({"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{"access_token":account.access_token,"refresh_token":account.refresh_token,"id_token":account.id_token,"account_id":account.chatgpt_account_id}})
        } else {
            json!({"https://auth.x.ai":{"key":account.access_token,"refresh_token":account.refresh_token,"email":account.account_identity,"expires_at":time::OffsetDateTime::from_unix_timestamp(account.expires_at_ms/1000).ok().and_then(|t|t.format(&time::format_description::well_known::Rfc3339).ok())}})
        };
        let serialized = zeroize::Zeroizing::new(raw.to_string());
        crate::claude_bridge::write_private(&file, serialized.as_bytes())
            .map_err(|e| ServiceError::internal(anyhow::anyhow!(e)))?;
    }
    let auth = cli_accounts::reference(provider, &path)
        .map_err(|e| ServiceError::internal(anyhow::anyhow!(e)))?;
    if !matches(&auth) {
        return Err(ServiceError::internal(anyhow::anyhow!(
            "CLI handoff identity mismatch; reconnect the subscription"
        )));
    }
    crate::community::bind_cli(vault, id, provider, auth)?;
    vault
        .remove_oauth_account(account.id)
        .map_err(map_vault_error)
}

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}
pub(crate) fn home() -> PathBuf {
    directories::BaseDirs::new()
        .map(|d| d.home_dir().to_owned())
        .unwrap_or_else(|| PathBuf::from("."))
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

#[cfg(target_os = "macos")]
pub(crate) fn read_keychain_bytes(
    service: &str,
    account: Option<&str>,
) -> anyhow::Result<zeroize::Zeroizing<Vec<u8>>> {
    use std::io::Read;
    use std::process::{Command, Stdio};
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

#[cfg(test)]
pub(crate) fn persist_login_account(
    vault: &Vault,
    provider: &str,
    identity: Option<String>,
    _: Option<String>,
    token: String,
    _: Option<String>,
) -> anyhow::Result<Uuid> {
    Ok(vault.add_provider(ProviderEntryInput {
        title: provider.into(),
        provider_kind: ProviderKind::Official,
        provider_id: Some(provider.into()),
        credential_kind: CredentialKind::OAuth,
        account_identity: identity,
        domains: vec![],
        favicon_url: None,
        endpoints: vec![ProviderEndpoint::api("https://api.anthropic.com")],
        interface_type: InterfaceType::AnthropicMessages,
        max_concurrent_requests: None,
        supports_websockets: None,
        auth_scheme: AuthScheme::Bearer,
        api_key: token,
        secret_label: None,
        default_model: None,
        model_aliases: vec![],
        headers: vec![],
        quota: None,
        subscription: None,
        gateway: None,
        tags: vec![],
        notes: None,
        secret_metadata: Default::default(),
    })?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aipass_vault::ManagedOAuthAccount;
    fn setup() -> (tempfile::TempDir, Vault, ManagedOAuthAccount) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(
            dir.path().join("vault"),
            &SecretString::new("test password"),
        )
        .unwrap()
        .vault;
        let id = persist_login_account(
            &vault,
            "openai",
            Some("alice@example.test".into()),
            None,
            "legacy-access".into(),
            None,
        )
        .unwrap();
        let claims = base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            json!({"email":"alice@example.test","sub":"alice"}).to_string(),
        );
        let account = ManagedOAuthAccount {
            id: Uuid::new_v4(),
            provider: aipass_provider_registry::OAuthProvider::Codex,
            account_identity: Some("alice@example.test".into()),
            chatgpt_account_id: Some("workspace-a".into()),
            access_token: "legacy-access".into(),
            refresh_token: "legacy-refresh".into(),
            id_token: Some(format!("x.{claims}.x")),
            expires_at_ms: 2000000000000,
            last_refresh_ms: 0,
            entry_id: Some(id),
            is_default: true,
            requires_reauth: false,
            authenticated_at: time::OffsetDateTime::now_utc(),
        };
        vault.add_oauth_account(account.clone()).unwrap();
        (dir, vault, account)
    }
    fn native(path: &std::path::Path, a: &ManagedOAuthAccount, workspace: &str, access: &str) {
        std::fs::create_dir_all(path).unwrap();
        std::fs::write(path.join("auth.json"),json!({"tokens":{"id_token":a.id_token,"account_id":workspace,"access_token":access,"refresh_token":"cli-refresh"}}).to_string()).unwrap();
    }
    #[test]
    fn handoff_preserves_entry_and_secret_ids_without_a_vault_grant() {
        let (dir, vault, a) = setup();
        let id = a.entry_id.unwrap();
        let secret = vault.get_provider_summary(id).unwrap().secret_refs[0]
            .id
            .clone();
        let path = dir.path().join("native");
        std::fs::create_dir(&path).unwrap();
        migrate_managed_at(&vault, &a, None, path.clone()).unwrap();
        assert_eq!(
            vault.get_provider_summary(id).unwrap().secret_refs[0].id,
            secret
        );
        assert_eq!(
            vault
                .get_provider_summary(id)
                .unwrap()
                .provider_id
                .as_deref(),
            Some("codex")
        );
        assert!(vault.list_oauth_accounts(None).unwrap().is_empty());
        let reference =
            crate::community::read(&vault, id, &vault.reveal_secret(id).unwrap()).unwrap();
        assert!(!reference.expose().contains("legacy-access"));
        assert!(!reference.expose().contains("legacy-refresh"));
        assert!(reference.expose().contains("workspace-a"));
        // An already handed-off grant is an idempotent retry, even after a lost removal ACK.
        vault.add_oauth_account(a.clone()).unwrap();
        migrate_managed_at(&vault, &a, None, path).unwrap();
        assert!(vault.list_oauth_accounts(None).unwrap().is_empty());
    }
    #[test]
    fn matching_existing_cli_is_reused_and_foreign_workspaces_are_never_overwritten() {
        let (dir, vault, a) = setup();
        let ambient = dir.path().join("ambient");
        let handoff = dir.path().join("handoff");
        std::fs::create_dir(&handoff).unwrap();
        native(&ambient, &a, "workspace-a", "cli-current-access");
        migrate_managed_at(&vault, &a, Some(ambient.clone()), handoff.clone()).unwrap();
        assert!(!handoff.join("auth.json").exists());
        assert_eq!(
            cli_accounts::reference("codex", &ambient).unwrap()["accountId"],
            "alice@example.test:workspace-a"
        );
        let reference = crate::community::read(
            &vault,
            a.entry_id.unwrap(),
            &vault.reveal_secret(a.entry_id.unwrap()).unwrap(),
        )
        .unwrap();
        assert!(reference
            .expose()
            .contains(&ambient.to_string_lossy().to_string()));
        let (dir, vault, a) = setup();
        let handoff = dir.path().join("foreign");
        native(&handoff, &a, "workspace-b", "foreign-access");
        let before = std::fs::read(handoff.join("auth.json")).unwrap();
        assert!(migrate_managed_at(&vault, &a, None, handoff.clone()).is_err());
        assert_eq!(std::fs::read(handoff.join("auth.json")).unwrap(), before);
        assert_eq!(
            vault.reveal_secret(a.entry_id.unwrap()).unwrap(),
            "legacy-access"
        );
        assert_eq!(vault.list_oauth_accounts(None).unwrap().len(), 1);
    }
    #[test]
    fn reconnecting_a_legacy_workspace_reuses_its_entry_and_replaces_its_grant() {
        let (dir, vault, a) = setup();
        let path = dir.path().join("native");
        native(&path, &a, "workspace-a", "cli-access");
        let auth = cli_accounts::reference("codex", &path).unwrap();
        let id = crate::community::register_cli(&vault, "codex", auth.clone()).unwrap();
        assert_eq!(id, a.entry_id.unwrap());
        assert!(vault.list_oauth_accounts(None).unwrap().is_empty());
        let marker = vault.reveal_secret(id).unwrap();
        assert_eq!(
            crate::community::register_cli(&vault, "codex", auth).unwrap(),
            id
        );
        assert_eq!(vault.reveal_secret(id).unwrap(), marker);
    }
    #[test]
    fn claude_reconnect_rotates_the_binding_but_keeps_route_secret_ids() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(
            dir.path().join("vault"),
            &SecretString::new("test password"),
        )
        .unwrap()
        .vault;
        let mut account = crate::claude_cli::NativeAccount {
            home: dir.path().join("a"),
            identity: "alice@example.test".into(),
            plan: Some("max".into()),
        };
        let id = persist_claude_login(&vault, &account).unwrap();
        let secret = vault.get_provider_summary(id).unwrap().secret_refs[0]
            .id
            .clone();
        let first = vault.reveal_secret(id).unwrap();
        assert_eq!(persist_claude_login(&vault, &account).unwrap(), id);
        assert_eq!(vault.reveal_secret(id).unwrap(), first);
        let reference = vault
            .provider_runtime_extension(id, "claude_cli_home")
            .unwrap()
            .unwrap();
        assert!(!reference.expose().contains("fake-private"));
        assert!(vault
            .provider_runtime_extension(id, "claude_native")
            .unwrap()
            .is_none());
        account.home = dir.path().join("b");
        assert_eq!(persist_claude_login(&vault, &account).unwrap(), id);
        assert_ne!(vault.reveal_secret(id).unwrap(), first);
        assert_eq!(
            vault.get_provider_summary(id).unwrap().secret_refs[0].id,
            secret
        );
    }
}
