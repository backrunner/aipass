//! Bounded discovery of explicit vendor stores and device-local vault bindings.
use super::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub(super) const PROVIDERS: &[&str] = &[
    "anthropic",
    "codex",
    "grok",
    "copilot",
    "gemini-cli",
    "zcode",
    "devin",
    "commandcode-plan",
    "cursor",
    "kiro",
    "workbuddy",
    "workbuddy-ai",
];
pub(super) fn canonical_provider(id: &str) -> Option<&'static str> {
    let id = match id {
        "claude" => "anthropic",
        "openai" => "codex",
        "xai" => "grok",
        "gemini" => "gemini-cli",
        _ => id,
    };
    PROVIDERS.iter().copied().find(|p| *p == id)
}
pub(super) fn normalize(
    mut source: SubscriptionImportSource,
) -> ServiceResult<SubscriptionImportSource> {
    source.provider = canonical_provider(&source.provider)
        .ok_or_else(|| validation("unsupported import provider"))?
        .into();
    let selector_ok = match source.provider.as_str() {
        "cursor" => matches!(source.selector.as_str(), "" | "file" | "keychain"),
        "kiro" => matches!(
            source.selector.as_str(),
            "" | "ide" | "cli:social" | "cli:odic" | "cli:external-idp"
        ),
        "zcode" => {
            source.selector.is_empty()
                || matches!(source.selector.as_str(), "team:zai" | "team:bigmodel")
                || source.selector.strip_prefix("key:").is_some_and(|name| {
                    name.contains(":coding-plan:") && name.ends_with(":api-key")
                })
        }
        _ => source.selector.is_empty(),
    };
    if !selector_ok {
        return Err(validation("unsupported account source selector"));
    }
    if !source.root.is_absolute()
        || source
            .root
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(validation(
            "account directory must be an absolute path without parent traversal",
        ));
    }
    source.root = source.root.canonicalize().unwrap_or(source.root);
    Ok(source)
}

pub(super) struct Snapshot {
    pub sources: Vec<SubscriptionImportSource>,
    pub revisions: HashMap<Uuid, time::OffsetDateTime>,
}

pub(super) fn snapshot(vault: &aipass_vault::Vault) -> ServiceResult<Snapshot> {
    let mut sources = Vec::new();
    let mut revisions = HashMap::new();
    let device = crate::subscriptions::cli_accounts::device().map_err(validation)?;
    for entry in vault.list_provider_summaries().map_err(map_vault_error)? {
        revisions.insert(entry.id, entry.updated_at);
        if entry.archived_at.is_some() {
            continue;
        }
        for key in ["claude_cli_home", "community_account_v1"] {
            if let Some(raw) = vault
                .provider_runtime_extension(entry.id, key)
                .map_err(map_vault_error)?
            {
                let mut value = reader::PrivateAuth(
                    serde_json::from_str(raw.expose())
                        .map_err(|_| validation("invalid account binding"))?,
                );
                let auth = reader::PrivateAuth(if key == "community_account_v1" {
                    serde_json::from_str(value.0["auth"].as_str().unwrap_or("null"))
                        .map_err(|_| validation("invalid account reference"))?
                } else {
                    value.0.take()
                });
                if auth.0["nativeDevice"] == device {
                    if let Ok(source) = serde_json::from_value::<SubscriptionImportSource>(
                        auth.0["nativeSource"].clone(),
                    ) {
                        // Canonicalize filesystem paths during discovery, outside the vault lock.
                        sources.push(source);
                    } else if let Some(root) = auth.0["nativeHome"].as_str() {
                        let provider = entry.provider_id.as_deref().and_then(canonical_provider);
                        if let Some(provider) = provider {
                            sources.push(SubscriptionImportSource {
                                provider: provider.into(),
                                root: root.into(),
                                selector: String::new(),
                            });
                        }
                    }
                }
            }
        }
    }
    Ok(Snapshot { sources, revisions })
}

pub(super) fn discover(
    input: &SubscriptionImportInput,
    bound: &[SubscriptionImportSource],
) -> ServiceResult<Vec<SubscriptionImportSource>> {
    let filters = input
        .provider_ids
        .iter()
        .map(|p| canonical_provider(p).ok_or_else(|| validation("unsupported import provider")))
        .collect::<ServiceResult<Vec<_>>>()?;
    let allowed = |p: &str| filters.is_empty() || filters.contains(&p);
    let mut list = bound
        .iter()
        .filter(|s| allowed(&s.provider))
        .cloned()
        .collect::<Vec<_>>();
    for source in &input.sources {
        let source = normalize(source.clone())?;
        if allowed(&source.provider) {
            list.extend(expand(source)?);
        }
    }
    for provider in PROVIDERS.iter().copied().filter(|p| allowed(p)) {
        if native_cli(provider) {
            let home = default_root(provider)?;
            let vendor_default = vendor_root(provider)?;
            list.push(SubscriptionImportSource {
                provider: provider.into(),
                root: home.clone(),
                selector: String::new(),
            });
            if vendor_default != home {
                list.push(SubscriptionImportSource {
                    provider: provider.into(),
                    root: vendor_default.clone(),
                    selector: String::new(),
                });
            }
            for root in [home, vendor_default] {
                let base = if provider == "gemini-cli" {
                    root.join(".gemini/aipass-accounts")
                } else {
                    root.join("aipass-accounts")
                };
                for child in children(&base) {
                    list.push(SubscriptionImportSource {
                        provider: provider.into(),
                        root: child,
                        selector: String::new(),
                    });
                }
            }
        } else {
            list.extend(
                crate::subscriptions::native_import::defaults(provider).map_err(validation)?,
            );
        }
    }
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for source in list {
        // A broken environment/bound directory must not abort other providers.
        // The reader rejects invalid sources before opening any files.
        let source = normalize(source.clone()).unwrap_or(source);
        if seen.insert(source.clone()) {
            out.push(source);
        }
        if out.len() > 256 {
            return Err(validation("too many account sources; filter by provider"));
        }
    }
    Ok(out)
}
fn expand(source: SubscriptionImportSource) -> ServiceResult<Vec<SubscriptionImportSource>> {
    if native_cli(&source.provider) {
        Ok(vec![source])
    } else {
        crate::subscriptions::native_import::expand(source).map_err(validation)
    }
}
pub(super) fn native_cli(provider: &str) -> bool {
    matches!(
        provider,
        "anthropic" | "codex" | "grok" | "copilot" | "gemini-cli"
    )
}
pub(super) fn retry_sources(
    sources: Vec<SubscriptionImportSource>,
) -> ServiceResult<Vec<SubscriptionImportSource>> {
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    for source in sources {
        // A repaired multi-account store may now contain several logins.
        // Re-enumerate only the failed root, without scanning other locations.
        let expanded = if source.provider == "zcode" && source.selector.is_empty() {
            expand(source)?
        } else {
            vec![source]
        };
        for source in expanded {
            let source = normalize(source.clone()).unwrap_or(source);
            if seen.insert(source.clone()) {
                found.push(source);
            }
            if found.len() > 256 {
                return Err(validation("too many retry account sources"));
            }
        }
    }
    Ok(found)
}
fn default_root(provider: &str) -> ServiceResult<PathBuf> {
    if provider == "anthropic" {
        Ok(crate::official_accounts::claude_home())
    } else {
        crate::subscriptions::cli_accounts::default_home(provider).map_err(validation)
    }
}
fn vendor_root(provider: &str) -> ServiceResult<PathBuf> {
    let h = directories::BaseDirs::new()
        .ok_or_else(|| validation("home unavailable"))?
        .home_dir()
        .to_owned();
    Ok(match provider {
        "anthropic" => h.join(".claude"),
        "codex" => h.join(".codex"),
        "grok" => h.join(".grok"),
        "copilot" => h.join(".copilot"),
        "gemini-cli" => h,
        _ => unreachable!(),
    })
}
fn children(base: &Path) -> Vec<PathBuf> {
    if !base.is_absolute()
        || base
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Vec::new();
    }
    let mut dirs = std::fs::read_dir(base)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type()
                .is_ok_and(|t| t.is_dir() || (t.is_symlink() && e.path().is_dir()))
        })
        .map(|e| e.path())
        .take(257)
        .collect::<Vec<_>>();
    dirs.sort();
    dirs
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn known_roots_follow_direct_account_links_and_deduplicate_without_recursing() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().join("aipass-accounts");
        let external = temp.path().join("external-account");
        std::fs::create_dir_all(&external).unwrap();
        std::fs::create_dir_all(base.join("account/nested")).unwrap();
        for name in ["alias-a", "alias-b"] {
            std::os::unix::fs::symlink(&external, base.join(name)).unwrap();
        }
        let found = children(&base);
        assert_eq!(found.len(), 3);
        let unique = found
            .into_iter()
            .map(|p| p.canonicalize().unwrap())
            .collect::<HashSet<_>>();
        assert_eq!(unique.len(), 2);
        assert!(!unique.contains(&base.join("account/nested").canonicalize().unwrap()));
    }
}
