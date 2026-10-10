use super::*;
use aipass_config_writers::native_auth::{parse, Resource};
use sha2::{Digest, Sha256};

pub(super) fn active_store(home: &Path, tool: &ToolConfigTool) -> Result<NativeStore> {
    match tool {
        ToolConfigTool::Codex => NativeStore::codex(
            &std::env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".codex")),
        ),
        ToolConfigTool::ClaudeCode => NativeStore::claude(
            &std::env::var_os("CLAUDE_CONFIG_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".claude")),
            &home.join(".claude"),
        ),
        _ => bail!("unsupported native tool"),
    }
}
pub(super) fn reference(vault: &Vault, id: Uuid, tool: &ToolConfigTool) -> Result<Value> {
    let entry = vault.get_provider_summary(id)?;
    if entry.credential_kind != aipass_provider_registry::CredentialKind::OAuth {
        bail!("selected entry is not a subscription");
    }
    let raw = vault
        .provider_runtime_extension(
            id,
            if *tool == ToolConfigTool::Codex {
                "community_account_v1"
            } else {
                "claude_cli_home"
            },
        )?
        .context("native account unavailable; sign in again")?;
    let value = parse(raw.expose().as_bytes())?;
    let auth = if *tool == ToolConfigTool::Codex {
        parse(
            value.0["auth"]
                .as_str()
                .context("invalid native account reference")?
                .as_bytes(),
        )?
    } else {
        value
    };
    if auth.0["nativeDevice"].as_str()
        != Some(
            crate::subscriptions::cli_accounts::device()
                .map_err(anyhow::Error::msg)?
                .as_str(),
        )
    {
        bail!("connect this subscription on this computer");
    }
    if entry.account_identity.as_deref() != auth.0["accountId"].as_str() {
        bail!("native account identity changed");
    }
    Ok(auth.0.clone())
}
pub(super) fn referenced_store(
    vault: &Vault,
    request: &ToolConfigRequest,
    home: &Path,
) -> Result<NativeStore> {
    let auth = reference(vault, request.id, &request.tool)?;
    let path = PathBuf::from(
        auth["nativeHome"]
            .as_str()
            .context("native account home unavailable")?,
    );
    if !path.is_absolute() {
        bail!("native account home unavailable; sign in again");
    }
    let store = match request.tool {
        ToolConfigTool::Codex => NativeStore::codex(&path)?,
        ToolConfigTool::ClaudeCode => NativeStore::claude(&path, &home.join(".claude"))?,
        _ => bail!("unsupported native tool"),
    };
    Ok(store)
}
pub(super) fn source_store(
    vault: &Vault,
    request: &ToolConfigRequest,
    home: &Path,
) -> Result<NativeStore> {
    let store = referenced_store(vault, request, home)?;
    let expected = reference(vault, request.id, &request.tool)?;
    match store.identity()? {
        Some(identity) if Some(identity.as_str()) == expected["accountId"].as_str() => Ok(store),
        None => bail!("native sign-in missing; sign in again"),
        _ => bail!("native account changed; reconnect explicitly"),
    }
}
pub(super) fn renew(
    state: &Arc<AgentState>,
    request: &ToolConfigRequest,
) -> std::result::Result<(), String> {
    renew_inner(state, request).map_err(|error| {
        // Vendor stderr/RPC errors may echo private values. Only controlled
        // classifications cross IPC; never return the original error text.
        let e = error.to_ascii_lowercase();
        if e.contains("changed") || e.contains("workspace") {
            "Native account changed; reconnect explicitly"
        } else if e.contains("expired")
            || e.contains("sign in")
            || e.contains("sign-in")
            || e.contains("not signed")
            || e.contains("401")
            || e.contains("invalid_grant")
            || e.contains("revoked")
        {
            "Native sign-in expired; sign in again"
        } else if e.contains("credential store") || e.contains("keychain") {
            "System credential store unavailable"
        } else if e.contains("backup") || e.contains("transaction") {
            "Encrypted credential backup unavailable"
        } else if e.contains("429") || e.contains("quota") {
            "Subscription quota unavailable"
        } else if ["500", "502", "503", "504", "service unavailable"]
            .iter()
            .any(|v| e.contains(v))
        {
            "Official CLI service unavailable; retry later"
        } else if e.contains("busy") {
            "Account renewal in progress; retry"
        } else if e.contains("install")
            || e.contains("update the official cli")
            || e.contains("cannot start")
        {
            "Install or update the official CLI, then retry"
        } else {
            "Official CLI verification unavailable; check the network and retry"
        }
        .into()
    })
}
fn renew_inner(
    state: &Arc<AgentState>,
    request: &ToolConfigRequest,
) -> std::result::Result<(), String> {
    let lock = if request.tool == ToolConfigTool::ClaudeCode {
        Some(native_account_lock(state, request.id).map_err(|e| e.message)?)
    } else {
        None
    };
    let _guard = lock
        .as_ref()
        .map(|lock| {
            lock.try_lock()
                .map_err(|_| "Account renewal busy; retry".to_owned())
        })
        .transpose()?;
    let (store, expected, bridge, proxy, marker) = with_vault(state, false, |vault| {
        let home = crate::server::home_dir(vault)?;
        let store = source_store(vault, request, &home).map_err(safe_error)?;
        let expected = reference(vault, request.id, &request.tool).map_err(safe_error)?
            ["accountId"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        // Even official CLI renewals are preceded by an encrypted snapshot.
        let mut changes = Vec::new();
        for r in store.resources() {
            changes.push(Change::unchanged(r).map_err(safe_error)?);
        }
        let tx = Transaction {
            operation_id: Uuid::new_v4(),
            committed: true,
            changes,
            metadata: json!([]),
        };
        tx.save(&root(state), &vault.config_backup_key())
            .map_err(safe_error)?;
        let marker = Zeroizing::new(vault.reveal_secret(request.id).map_err(map_vault_error)?);
        let mut proxy = state
            .proxy
            .lock()
            .map_err(|_| safe_error(anyhow::anyhow!("proxy unavailable")))?;
        let config = proxy.load_config(vault)?;
        Ok((
            store,
            expected,
            proxy.community_bridge(),
            config.upstream_proxy,
            marker,
        ))
    })
    .map_err(|e| e.message)?;
    if request.tool == ToolConfigTool::Codex {
        bridge.verify_native(request.id, &marker, &proxy)?;
    } else {
        let account = crate::claude_cli::local_account(&store.home)?;
        if account.identity != expected {
            return Err("native account changed; reconnect explicitly".into());
        }
        if store
            .expires_at()
            .map_err(|e| e.to_string())?
            .is_some_and(|t| t <= time::OffsetDateTime::now_utc().unix_timestamp() + 300)
        {
            crate::claude_cli::usage_native(&store.home, &proxy)?;
        }
    }
    if store.identity().map_err(|e| e.to_string())?.as_deref() != Some(expected.as_str()) {
        return Err("native account changed during renewal".into());
    }
    if store
        .expires_at()
        .map_err(|e| e.to_string())?
        .is_some_and(|t| t <= time::OffsetDateTime::now_utc().unix_timestamp())
    {
        return Err("native sign-in expired; sign in again".into());
    }
    Ok(())
}
pub(super) fn replace_change(changes: &mut Vec<Change>, change: Change) {
    if let Some(index) = changes.iter().position(|c| c.resource == change.resource) {
        changes[index] = change;
    } else {
        changes.push(change)
    }
}
pub(super) fn dedup_changes(changes: &mut Vec<Change>) -> Result<()> {
    let mut i = 0;
    while i < changes.len() {
        if let Some(j) = (0..i).find(|j| changes[*j].resource == changes[i].resource) {
            if changes[j].before != changes[i].before || changes[j].after != changes[i].after {
                bail!("conflicting native credential writes");
            }
            changes.remove(i);
        } else {
            i += 1;
        }
    }
    Ok(())
}
pub(super) fn copy_store(
    source: &NativeStore,
    target: &NativeStore,
    changes: &mut Vec<Change>,
) -> Result<()> {
    if source.primary == target.primary {
        for r in target.resources() {
            replace_change(changes, Change::unchanged(r)?);
        }
        return Ok(());
    }
    let auth = source.credentials()?;
    let bytes = if source.tool == "claude-code" {
        let mut existing = target
            .primary
            .read()?
            .map(|v| parse(&v))
            .transpose()?
            .unwrap_or_else(|| PrivateJson(json!({})));
        existing.0["claudeAiOauth"] = auth.0["claudeAiOauth"].clone();
        serde_json::to_vec(&existing.0)?
    } else {
        serde_json::to_vec(&auth.0)?
    };
    replace_change(changes, Change::new(target.primary.clone(), Some(bytes))?);
    if let Some(fallback) = &target.fallback {
        replace_change(changes, Change::new(fallback.clone(), None)?);
    }
    if let (Some(from), Some(to)) = (&source.profile, &target.profile) {
        let raw = from.read()?.context("native account profile unavailable")?;
        let profile = parse(&raw)?;
        let mut existing = to
            .read()?
            .map(|v| parse(&v))
            .transpose()?
            .unwrap_or_else(|| PrivateJson(json!({})));
        // Account metadata changes only; MCP/trust/preferences remain user-owned.
        existing.0["oauthAccount"] = profile.0["oauthAccount"].clone();
        replace_change(
            changes,
            Change::new(to.clone(), Some(serde_json::to_vec(&existing.0)?))?,
        );
    }
    Ok(())
}
pub(super) fn archive_outgoing(
    vault: &Vault,
    request: &ToolConfigRequest,
    active: &NativeStore,
    home: &Path,
    changes: &mut Vec<Change>,
    refs: &mut Vec<RefUpdate>,
) -> Result<()> {
    let Some(identity) = active.identity()? else {
        return Ok(());
    };
    for entry in vault.list_provider_summaries()? {
        if entry.account_identity.as_deref() != Some(&identity)
            || entry.credential_kind != aipass_provider_registry::CredentialKind::OAuth
        {
            continue;
        }
        if request.mode == ToolConfigMode::Official && entry.id == request.id {
            continue;
        }
        let Ok(before) = reference(vault, entry.id, &request.tool) else {
            continue;
        };
        if before["nativeHome"].as_str() != Some(active.home.to_string_lossy().as_ref()) {
            continue;
        }
        let path = active
            .home
            .canonicalize()
            .context("native credential directory unavailable")?
            .join("aipass-accounts")
            .join(entry.id.to_string());
        let mut archived = NativeStore {
            tool: active.tool.clone(),
            home: path.clone(),
            primary: Resource::File(path.join(if active.tool == "codex" {
                "auth.json"
            } else {
                ".credentials.json"
            })),
            fallback: None,
            profile: active
                .profile
                .as_ref()
                .map(|_| Resource::File(path.join(".claude.json"))),
        };
        if matches!(active.primary, Resource::Keychain { .. }) {
            let account = std::env::var("USER").context("system account unavailable")?;
            archived.primary = Resource::Keychain {
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
                    account
                },
            };
        }
        if active.tool == "codex" {
            changes.push(Change::new(
                Resource::File(path.join("config.toml")),
                Some(
                    format!(
                        "cli_auth_credentials_store = \"{}\"\n",
                        if matches!(archived.primary, Resource::Keychain { .. }) {
                            "keyring"
                        } else {
                            "file"
                        }
                    )
                    .into_bytes(),
                ),
            )?);
        }
        // Ensure the persistent vendor directory exists even when its credentials use Keychain.
        if active.tool == "claude-code" && archived.profile.is_none() {
            bail!("native account profile unavailable");
        }
        copy_store(active, &archived, changes)?;
        let mut after = before.clone();
        after["nativeHome"] = json!(path);
        refs.push(RefUpdate {
            id: entry.id,
            provider: if request.tool == ToolConfigTool::Codex {
                "codex"
            } else {
                "anthropic"
            }
            .into(),
            before,
            after,
        });
    }
    let _ = home;
    Ok(())
}
pub(super) fn preserve_codex_backend(
    active: &NativeStore,
    plan: &ConfigPlan,
    changes: &mut [Change],
) -> Result<()> {
    if active.tool != "codex" || plan.target_path != active.home.join("config.toml") {
        return Ok(());
    }
    let mode = if matches!(active.primary, Resource::Keychain { .. }) {
        "keyring"
    } else {
        "file"
    };
    if let Some(c) = changes
        .iter_mut()
        .find(|c| c.resource == Resource::File(plan.target_path.clone()))
    {
        let raw = c
            .after
            .as_ref()
            .context("Codex configuration unavailable")?;
        c.after = Some(
            aipass_config_writers::native_auth::codex_storage_mode(
                std::str::from_utf8(raw)?,
                mode,
            )?
            .into_bytes(),
        );
    }
    Ok(())
}
pub(super) fn fingerprint_after(
    resources: &[Resource],
    changes: &[Change],
    key: &[u8; 32],
    context: &[u8],
) -> Result<String> {
    let mut hash = Sha256::new();
    hash.update(key);
    hash.update(context);
    for r in resources {
        hash.update(serde_json::to_vec(r)?);
        let before = r.read()?;
        let bytes = changes
            .iter()
            .find(|c| c.resource == *r)
            .map(|c| c.after.as_deref())
            .unwrap_or(before.as_deref().map(|v| v.as_slice()));
        if let Some(b) = bytes {
            hash.update([1]);
            hash.update((b.len() as u64).to_le_bytes());
            hash.update(b);
        } else {
            hash.update([0]);
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}
