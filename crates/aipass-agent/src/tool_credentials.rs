//! Read-only credential summaries and mandatory vault import before managed switches.
//! Secret material stays in Rust; imports never overwrite an existing vault key.
use crate::session::{map_vault_error, AgentState, ServiceError, ServiceResult};
use aipass_agent_protocol::{AgentErrorCode, ToolConfigStatus, ToolConfigTool};
use aipass_config_writers::native_auth::{NativeStore, Resource};
use aipass_provider_registry::{
    AuthScheme, CredentialKind, InterfaceType, ProviderEndpoint, ProviderKind,
};
use aipass_vault::{ProviderEntryInput, Vault};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use uuid::Uuid;
use zeroize::Zeroizing;
mod additional;
mod byok;
mod reader;
mod subscriptions;
#[cfg(test)]
mod tests;

struct Credential {
    key: Zeroizing<String>,
    endpoint: String,
    interface: InterfaceType,
    auth: AuthScheme,
    model: Option<String>,
}
pub(crate) fn error(_: impl std::fmt::Display) -> ServiceError {
    ServiceError::new(AgentErrorCode::ValidationFailed,
        "Existing Agent credentials could not be imported safely; no tool configuration was changed")
}
pub(crate) fn native(home: &Path, tool: &ToolConfigTool) -> anyhow::Result<Option<NativeStore>> {
    Ok(match tool {
        ToolConfigTool::Codex => Some(NativeStore::codex(
            &std::env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".codex")),
        )?),
        ToolConfigTool::ClaudeCode => Some(NativeStore::claude(
            &std::env::var_os("CLAUDE_CONFIG_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".claude")),
            &home.join(".claude"),
        )?),
        _ => None,
    })
}
pub(crate) fn resources(home: &Path, tool: &ToolConfigTool) -> anyhow::Result<Vec<Resource>> {
    let mut resources = reader::paths(home, tool)
        .into_iter()
        .map(Resource::File)
        .collect::<Vec<_>>();
    if let Some(store) = native(home, tool)? {
        resources.extend(store.resources());
    }
    resources.sort_by_key(Resource::label);
    resources.dedup();
    Ok(resources)
}
pub(crate) fn config_paths(home: &Path, tool: &ToolConfigTool) -> Vec<PathBuf> {
    let mut paths = reader::paths(home, tool);
    if *tool == ToolConfigTool::Codex {
        if let Some(config) = paths.first() {
            paths.push(config.with_file_name("auth.json"));
        }
    }
    paths
}
fn matching(vault: &Vault, c: &Credential) -> ServiceResult<Option<(Uuid, String)>> {
    for entry in vault.list_provider_summaries().map_err(map_vault_error)? {
        if entry.credential_kind != CredentialKind::Api || entry.archived_at.is_some() {
            continue;
        }
        for secret in &entry.secret_refs {
            let endpoint = secret.endpoint.as_deref().or_else(|| {
                entry
                    .endpoints
                    .iter()
                    .find(|e| e.kind == aipass_provider_registry::EndpointKind::Api)
                    .and_then(|e| e.url.as_deref())
            });
            if endpoint.map(|s| s.trim_end_matches('/')) != Some(c.endpoint.trim_end_matches('/'))
                || secret
                    .interface_type
                    .as_ref()
                    .unwrap_or(&entry.interface_type)
                    != &c.interface
                || secret.effective_auth(&entry.interface_type, &entry.auth_scheme) != c.auth
            {
                continue;
            }
            let value = Zeroizing::new(
                vault
                    .reveal_secret_by_id(entry.id, &secret.id)
                    .map_err(map_vault_error)?,
            );
            if value.as_str() == c.key.as_str() {
                return Ok(Some((entry.id, secret.id.clone())));
            }
        }
    }
    Ok(None)
}
pub(crate) fn preserve(
    vault: &Vault,
    state: &Arc<AgentState>,
    tool: &ToolConfigTool,
) -> ServiceResult<()> {
    let home = crate::server::home_dir(vault)?;
    // Include native stores that the config writer does not itself overwrite.
    // A failed encrypted snapshot must stop before any credential import/write.
    let changes = resources(&home, tool)
        .map_err(error)?
        .into_iter()
        .map(aipass_config_writers::transaction::Change::unchanged)
        .collect::<anyhow::Result<Vec<_>>>()
        .map_err(error)?;
    let snapshot = aipass_config_writers::transaction::Transaction {
        operation_id: Uuid::new_v4(),
        committed: true,
        changes,
        metadata: serde_json::json!([]),
    };
    snapshot
        .save(
            &state.vault_dir.join("agent-credential-imports"),
            &vault.config_backup_key(),
        )
        .map_err(error)?;
    // Read everything first. Malformed or inaccessible stores stop before any import/write.
    let credentials = reader::read(&home, tool).map_err(error)?;
    let store = native(&home, tool).map_err(error)?;
    let identity = store
        .as_ref()
        .map(|s| s.identity())
        .transpose()
        .map_err(error)?
        .flatten();
    for c in credentials {
        if matching(vault, &c)?.is_some() {
            continue;
        }
        vault
            .add_provider(ProviderEntryInput {
                title: format!("{} · imported", name(tool)),
                provider_kind: ProviderKind::Unknown,
                provider_id: None,
                credential_kind: CredentialKind::Api,
                account_identity: None,
                domains: vec![],
                favicon_url: None,
                endpoints: vec![ProviderEndpoint::api(c.endpoint)],
                interface_type: c.interface,
                auth_scheme: c.auth,
                api_key: c.key.to_string(),
                secret_label: Some("Imported credential".into()),
                default_model: c.model,
                max_concurrent_requests: None,
                supports_websockets: None,
                model_aliases: vec![],
                headers: vec![],
                quota: None,
                subscription: None,
                gateway: None,
                tags: vec!["agent-import".into()],
                notes: Some(format!("Saved before switching {} credentials", name(tool))),
                secret_metadata: Default::default(),
            })
            .map_err(map_vault_error)?;
    }
    if let (Some(store), Some(identity)) = (store, identity) {
        // Already-managed accounts must keep their source home and identity intact.
        let exists = vault
            .list_provider_summaries()
            .map_err(map_vault_error)?
            .iter()
            .any(|e| {
                e.credential_kind == CredentialKind::OAuth
                    && e.archived_at.is_none()
                    && e.account_identity.as_deref() == Some(&identity)
                    && match tool {
                        ToolConfigTool::Codex => {
                            matches!(e.provider_id.as_deref(), Some("openai" | "codex"))
                        }
                        _ => e.provider_id.as_deref() == Some("anthropic"),
                    }
            });
        if !exists {
            if *tool == ToolConfigTool::Codex {
                let reference = crate::subscriptions::cli_accounts::reference("codex", &store.home)
                    .map_err(error)?;
                crate::community::register_cli(vault, "codex", reference)?;
            } else {
                let account = crate::claude_cli::local_account(&store.home).map_err(error)?;
                if account.identity != identity {
                    return Err(error("native account changed"));
                }
                crate::official_accounts::persist_claude_login(vault, &account).map_err(error)?;
            }
        }
    }
    subscriptions::preserve(vault, state, &home, tool)?;
    Ok(())
}
pub(crate) fn status(
    state: &Arc<AgentState>,
    tool: ToolConfigTool,
) -> ServiceResult<ToolConfigStatus> {
    crate::session::with_vault(state, false, |vault| snapshot(vault, tool))
}
pub(crate) fn snapshot(vault: &Vault, tool: ToolConfigTool) -> ServiceResult<ToolConfigStatus> {
    let mut status = ToolConfigStatus {
        tool: tool.clone(),
        state: "unmanaged".into(),
        entry_title: None,
        entry_id: None,
        secret_id: None,
        mode: None,
        account_identity: None,
        operation_id: None,
        message: None,
        overrides: vec![],
    };
    let home = crate::server::home_dir(vault)?;
    if let Some(identity) = subscriptions::active_gemini_identity(&home, &tool)? {
        status.account_identity = Some(identity.clone());
        if let Some(entry) = vault
            .list_provider_summaries()
            .map_err(map_vault_error)?
            .into_iter()
            .find(|e| {
                e.provider_id.as_deref() == Some("gemini-cli")
                    && e.credential_kind == CredentialKind::OAuth
                    && e.account_identity.as_deref() == Some(&identity)
                    && e.archived_at.is_none()
            })
        {
            status.entry_title = Some(entry.title);
            status.entry_id = Some(entry.id);
            status.secret_id = entry.secret_refs.first().map(|s| s.id.clone());
            status.state = "ready".into();
        }
        return Ok(status);
    }
    let credentials = reader::read(&home, &tool).map_err(error)?;
    if let Some(c) = credentials.first() {
        if let Some((id, secret)) = matching(vault, c)? {
            let entry = vault.get_provider_summary(id).map_err(map_vault_error)?;
            status.entry_title = Some(entry.title);
            status.entry_id = Some(id);
            status.secret_id = Some(secret);
            status.state = "ready".into();
        }
    }
    Ok(status)
}
fn name(tool: &ToolConfigTool) -> &'static str {
    match tool {
        ToolConfigTool::Codex => "Codex",
        ToolConfigTool::ClaudeCode => "Claude Code",
        ToolConfigTool::GeminiCli => "Gemini CLI",
        ToolConfigTool::OpenCode => "OpenCode",
        ToolConfigTool::Grok => "Grok Build",
        ToolConfigTool::Pi => "Pi",
        ToolConfigTool::Cursor => "Cursor Agent Local",
        ToolConfigTool::KimiCode => "Kimi Code",
        ToolConfigTool::Zed => "Zed",
        ToolConfigTool::DeepseekHarness => "DeepSeek Harness",
        ToolConfigTool::AntigravityCli => "Antigravity CLI",
        ToolConfigTool::MuseCode => "Muse Code",
        ToolConfigTool::HermesAgent => "Hermes Agent",
        ToolConfigTool::CommandCode => "Command Code",
        ToolConfigTool::Codebuddy => "CodeBuddy",
        ToolConfigTool::Workbuddy => "WorkBuddy",
        ToolConfigTool::VscodeChat => "VS Code Chat",
        ToolConfigTool::Cline => "Cline",
        ToolConfigTool::Trae => "TRAE",
        ToolConfigTool::Qoder => "Qoder",
        ToolConfigTool::Zcode => "ZCode",
        ToolConfigTool::JetbrainsCopilot => "JetBrains Copilot",
    }
}
