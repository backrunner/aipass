//! Trusted native tool switching. All persistent material is encrypted or CLI-owned.
use crate::session::{map_vault_error, with_vault, AgentState, ServiceError, ServiceResult};
use aipass_agent_protocol::*;
use aipass_config_writers::{
    native_auth::{NativeStore, PrivateJson, Resource},
    transaction::{fingerprint, Change, Transaction},
    ConfigPlan,
};
use aipass_crypto::{decrypt_bytes, encrypt_bytes, Ciphertext, SecretString};
use aipass_vault::Vault;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use uuid::Uuid;
use zeroize::Zeroizing;

mod health;
mod historical;
mod login;
pub(crate) use health::remember as remember_probe;
mod native;
mod proxy;
mod restore;
mod safety;
pub(crate) use login::{bind_codex as bind_codex_login, clear as cancel_logins, handle_login};
use native::*;
pub(crate) use proxy::apply_proxy;
pub(crate) use restore::{has_backup, rollback};
static SWITCH_LOCK: Mutex<()> = Mutex::new(());

#[derive(Serialize, Deserialize, Clone)]
struct Binding {
    #[serde(default)]
    route_id: Option<Uuid>,
    #[serde(default)]
    title: String,
    request: ToolConfigRequest,
    operation_id: Uuid,
    identity: Option<String>,
    config_resources: Vec<Resource>,
    config_fingerprint: String,
}
#[derive(Serialize, Deserialize)]
struct RefUpdate {
    id: Uuid,
    provider: String,
    before: Value,
    after: Value,
}

pub(crate) fn validate_reference(
    vault: &Vault,
    id: Uuid,
    tool: &ToolConfigTool,
) -> ServiceResult<()> {
    native::reference(vault, id, tool)
        .map(|_| ())
        .map_err(safe_error)
}
pub(crate) fn supported(tool: &ToolConfigTool) -> bool {
    matches!(tool, ToolConfigTool::Codex | ToolConfigTool::ClaudeCode)
}
fn root(state: &AgentState) -> PathBuf {
    state.vault_dir.join("local-tool-transactions")
}
fn binding_path(state: &AgentState, tool: &ToolConfigTool) -> PathBuf {
    root(state).join(format!("{}.aipbinding", tool_name(tool)))
}
fn tool_name(tool: &ToolConfigTool) -> &'static str {
    match tool {
        ToolConfigTool::Codex => "codex",
        ToolConfigTool::ClaudeCode => "claude-code",
        _ => "unsupported",
    }
}
fn context(request: &ToolConfigRequest) -> Result<Vec<u8>> {
    let mut r = request.clone();
    r.preview_id = None;
    Ok(serde_json::to_vec(&r)?)
}
fn encode_binding(binding: &Binding, key: &[u8; 32]) -> Result<Vec<u8>> {
    let raw = Zeroizing::new(serde_json::to_vec(binding)?);
    Ok(serde_json::to_vec(&encrypt_bytes(
        key,
        b"aipass-tool-binding;v=1",
        &raw,
    )?)?)
}
fn load_binding(
    state: &AgentState,
    tool: &ToolConfigTool,
    key: &[u8; 32],
) -> Result<Option<Binding>> {
    let Some(raw) = Resource::File(binding_path(state, tool)).read()? else {
        return Ok(None);
    };
    let envelope: Ciphertext = serde_json::from_slice(&raw)?;
    let raw = Zeroizing::new(decrypt_bytes(key, b"aipass-tool-binding;v=1", &envelope)?);
    Ok(Some(serde_json::from_slice(&raw)?))
}
fn preview_resources(
    vault: &Vault,
    state: &AgentState,
    request: &ToolConfigRequest,
    plan: &ConfigPlan,
) -> Result<Vec<Resource>> {
    let home = crate::server::home_dir(vault).map_err(|e| anyhow::anyhow!(e.message))?;
    let store = active_store(&home, &request.tool)?;
    let mut resources = store.resources();
    resources.push(Resource::File(plan.target_path.clone()));
    resources.extend(
        plan.extra_writes
            .iter()
            .map(|w| Resource::File(w.target_path.clone())),
    );
    resources.push(Resource::File(binding_path(state, &request.tool)));
    if request.mode == ToolConfigMode::Official {
        resources.extend(referenced_store(vault, request, &home)?.resources());
    }
    resources.sort_by_key(|r| r.label());
    resources.dedup();
    Ok(resources)
}
pub(crate) fn preview_id(
    vault: &Vault,
    state: &AgentState,
    request: &ToolConfigRequest,
    plan: &ConfigPlan,
) -> Result<String> {
    let mut input = Zeroizing::new(context(request)?);
    input.extend(selection_context(vault, request.id)?);
    if request.mode == ToolConfigMode::Official {
        input.extend(serde_json::to_vec(&reference(
            vault,
            request.id,
            &request.tool,
        )?)?);
    }
    fingerprint(
        &preview_resources(vault, state, request, plan)?,
        &vault.config_backup_key(),
        &input,
    )
}
fn selection_context(vault: &Vault, id: Uuid) -> Result<Vec<u8>> {
    let e = vault.get_provider_summary(id)?;
    Ok(serde_json::to_vec(&json!({
        "id":e.id,"title":e.title,"providerId":e.provider_id,"providerKind":e.provider_kind,
        "credentialKind":e.credential_kind,"identity":e.account_identity,"secretRefs":e.secret_refs,
        "endpoints":e.endpoints,"interfaceType":e.interface_type,"authScheme":e.auth_scheme,
        "defaultModel":e.default_model,"supportsWebsockets":e.supports_websockets,
        "archivedAt":e.archived_at
    }))?)
}
fn set_references(vault: &Vault, updates: &[RefUpdate], forward: bool) -> ServiceResult<()> {
    for update in updates {
        let tool = if update.provider == "codex" {
            ToolConfigTool::Codex
        } else {
            ToolConfigTool::ClaudeCode
        };
        let current = reference(vault, update.id, &tool).map_err(safe_error)?;
        if current != update.before && current != update.after {
            return Err(ServiceError::new(
                AgentErrorCode::Conflict,
                "native account binding changed",
            ));
        }
        let next = if forward {
            &update.after
        } else {
            &update.before
        };
        if current == *next {
            continue;
        }
        if update.provider == "codex" {
            crate::community::bind_cli(vault, update.id, "codex", next.clone())?;
        } else {
            let raw = SecretString::new(next.to_string());
            vault
                .bind_cli_subscription(
                    update.id,
                    "anthropic",
                    next["accountId"].as_str().unwrap_or(""),
                    &format!("aipass:claude-cli:{}", Uuid::new_v4()),
                    "claude_cli_home",
                    &raw,
                )
                .map_err(map_vault_error)?;
        }
    }
    Ok(())
}
fn safe_error(e: impl Into<anyhow::Error>) -> ServiceError {
    let e = e.into();
    let message = e.to_string();
    let code = if message.contains("changed") {
        AgentErrorCode::Conflict
    } else {
        AgentErrorCode::ValidationFailed
    };
    ServiceError::new(code, message)
}
fn refresh_cache(
    state: &Arc<AgentState>,
    vault: &Vault,
    updates: &[RefUpdate],
) -> ServiceResult<()> {
    let mut proxy = state
        .proxy
        .lock()
        .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "proxy unavailable"))?;
    for update in updates {
        proxy.refresh_provider_credentials(vault, update.id)?;
    }
    Ok(())
}
pub(crate) fn recover(state: &Arc<AgentState>) -> ServiceResult<()> {
    let _guard = SWITCH_LOCK
        .lock()
        .map_err(|_| safe_error(anyhow::anyhow!("tool coordinator unavailable")))?;
    let has_pending = with_vault(state, false, |vault| {
        Ok(
            !aipass_config_writers::transaction::pending(&root(state), &vault.config_backup_key())
                .map_err(safe_error)?
                .is_empty(),
        )
    })?;
    if !has_pending {
        return Ok(());
    }
    // A proxy refresh may still be finishing while the vault is unlocked again.
    let locks = account_locks(state)?;
    let mut guards = Vec::new();
    for lock in &locks {
        guards.push(lock.try_lock().map_err(|_| {
            ServiceError::new(
                AgentErrorCode::Conflict,
                "Account renewal is in progress; retry recovery",
            )
        })?);
    }
    with_vault(state, false, |vault| {
        let key = Zeroizing::new(vault.config_backup_key());
        for tx in
            aipass_config_writers::transaction::pending(&root(state), &key).map_err(safe_error)?
        {
            let refs: Vec<RefUpdate> =
                serde_json::from_value(tx.metadata.clone()).map_err(safe_error)?;
            tx.restore().map_err(safe_error)?;
            set_references(vault, &refs, false)?;
            refresh_cache(state, vault, &refs)?;
            std::fs::remove_file(Transaction::path(&root(state), tx.operation_id))
                .map_err(safe_error)?;
        }
        Ok(())
    })
}
fn outcome(
    request: &ToolConfigRequest,
    result: ToolConfigOutcome,
    message: Option<String>,
) -> ToolConfigApplyResponse {
    ToolConfigApplyResponse {
        tool: request.tool.clone(),
        mode: request.mode.clone(),
        entry_id: request.id,
        entry_title: String::new(),
        operation_id: Uuid::nil(),
        target_path: String::new(),
        backup_path: String::new(),
        summary: String::new(),
        outcome: result,
        message,
    }
}
pub(crate) fn apply(
    state: &Arc<AgentState>,
    request: ToolConfigRequest,
) -> ServiceResult<ToolConfigApplyResponse> {
    apply_coordinated(state, request.clone()).or_else(|error| safety::apply_error(&request, error))
}
fn apply_coordinated(
    state: &Arc<AgentState>,
    request: ToolConfigRequest,
) -> ServiceResult<ToolConfigApplyResponse> {
    let _guard = SWITCH_LOCK
        .lock()
        .map_err(|_| safe_error(anyhow::anyhow!("tool coordinator unavailable")))?;
    apply_locked(state, request)
}
fn apply_locked(
    state: &Arc<AgentState>,
    request: ToolConfigRequest,
) -> ServiceResult<ToolConfigApplyResponse> {
    // Existing conflicts require explicit restoration/reconciliation, not an
    // account switch that silently replaces an external writer's state.
    if status_snapshot(state, request.tool.clone())?.state == "conflict" {
        return Ok(outcome(
            &request,
            ToolConfigOutcome::Conflict,
            Some(
                "Tool configuration changed externally; resolve the conflict before switching"
                    .into(),
            ),
        ));
    }
    // Compare the displayed preview before a renewal writes anything.
    let check = with_vault(state, false, |vault| {
        let (_, plan, _) = crate::server::build_tool_config_plan(vault, &request, true)?;
        if let Some(id) = &request.preview_id {
            if *id != preview_id(vault, state, &request, &plan).map_err(safe_error)? {
                return Err(ServiceError::new(
                    AgentErrorCode::Conflict,
                    "tool credentials or selected account changed; preview again",
                ));
            }
        }
        if !aipass_config_writers::transaction::pending(&root(state), &vault.config_backup_key())
            .map_err(safe_error)?
            .is_empty()
        {
            return Err(ServiceError::new(
                AgentErrorCode::Conflict,
                "pending tool recovery; unlock and retry",
            ));
        }
        let mut resources = preview_resources(vault, state, &request, &plan).map_err(safe_error)?;
        if request.mode == ToolConfigMode::Official {
            let home = crate::server::home_dir(vault)?;
            let source = referenced_store(vault, &request, &home).map_err(safe_error)?;
            // Only the official target's grants may rotate during verification.
            // Configuration, outgoing account and account metadata stay bound.
            resources.retain(|r| *r != source.primary && source.fallback.as_ref() != Some(r));
        }
        let selected = selection_context(vault, request.id).map_err(safe_error)?;
        let stamp =
            fingerprint(&resources, &vault.config_backup_key(), &selected).map_err(safe_error)?;
        Ok((resources, stamp))
    });
    let (resources, stamp) = match check {
        Ok(check) => check,
        Err(e) if e.code == AgentErrorCode::Conflict => {
            return Ok(outcome(
                &request,
                ToolConfigOutcome::Conflict,
                Some(e.message),
            ))
        }
        Err(e) => return Err(e),
    };
    if request.mode == ToolConfigMode::Official {
        if let Err(e) = renew(state, &request) {
            let kind = if e.contains("expired")
                || e.contains("sign in")
                || e.contains("sign-in")
                || e.contains("not signed")
            {
                ToolConfigOutcome::LoginRequired
            } else if e.contains("changed") {
                ToolConfigOutcome::Conflict
            } else {
                ToolConfigOutcome::StorageUnavailable
            };
            return Ok(outcome(&request, kind, Some(e)));
        }
    }
    // Acquire account locks outside the vault mutex. An in-flight refresh may itself
    // need the vault, so waiting under that mutex would deadlock.
    let locks = account_locks(state)?;
    let mut guards = Vec::new();
    for lock in &locks {
        match lock.try_lock() {
            Ok(guard) => guards.push(guard),
            Err(_) => {
                return Ok(outcome(
                    &request,
                    ToolConfigOutcome::Conflict,
                    Some("Account renewal is in progress; retry the switch".into()),
                ))
            }
        }
    }
    with_vault(state, false, |vault| {
        let selected = selection_context(vault, request.id).map_err(safe_error)?;
        if fingerprint(&resources, &vault.config_backup_key(), &selected).map_err(safe_error)?
            != stamp
        {
            return Err(ServiceError::new(
                AgentErrorCode::Conflict,
                "tool configuration or selected account changed during verification; preview again",
            ));
        }
        apply_inner(state, vault, &request)
    })
    .or_else(|e| {
        if e.code == AgentErrorCode::Conflict {
            Ok(outcome(
                &request,
                ToolConfigOutcome::Conflict,
                Some(e.message),
            ))
        } else {
            Err(e)
        }
    })
}
fn apply_inner(
    state: &Arc<AgentState>,
    vault: &Vault,
    request: &ToolConfigRequest,
) -> ServiceResult<ToolConfigApplyResponse> {
    let (entry, plan, content) = crate::server::build_tool_config_plan(vault, request, true)?;
    let selected = request
        .secret_id
        .clone()
        .or_else(|| entry.secret_refs.first().map(|s| s.id.clone()));
    apply_prepared(
        state,
        vault,
        request,
        Prepared {
            title: entry.title,
            identity: entry.account_identity,
            selected,
            route_id: None,
            plan,
            content,
        },
    )
}
struct Prepared {
    title: String,
    identity: Option<String>,
    selected: Option<String>,
    route_id: Option<Uuid>,
    plan: ConfigPlan,
    content: String,
}
fn apply_prepared(
    state: &Arc<AgentState>,
    vault: &Vault,
    request: &ToolConfigRequest,
    prepared: Prepared,
) -> ServiceResult<ToolConfigApplyResponse> {
    let Prepared {
        title,
        identity,
        selected,
        route_id,
        plan,
        content,
    } = prepared;
    let content = Zeroizing::new(content);
    let home = crate::server::home_dir(vault)?;
    let active = active_store(&home, &request.tool).map_err(safe_error)?;
    let key = Zeroizing::new(vault.config_backup_key());
    let mut refs = Vec::new();
    let mut changes = Vec::new();
    archive_outgoing(vault, request, &active, &home, &mut changes, &mut refs)
        .map_err(safe_error)?;
    let source = if request.mode == ToolConfigMode::Official {
        Some(source_store(vault, request, &home).map_err(safe_error)?)
    } else {
        None
    };
    if let Some(source) = &source {
        let expected = identity
            .as_deref()
            .ok_or_else(|| safe_error(anyhow::anyhow!("native account identity unavailable")))?;
        if source.identity().map_err(safe_error)?.as_deref() != Some(expected) {
            return Err(ServiceError::new(
                AgentErrorCode::Conflict,
                "native account changed",
            ));
        }
        copy_store(source, &active, &mut changes).map_err(safe_error)?;
        let before = reference(vault, request.id, &request.tool).map_err(safe_error)?;
        let mut after = before.clone();
        after["nativeHome"] = json!(active.home);
        refs.push(RefUpdate {
            id: request.id,
            provider: if request.tool == ToolConfigTool::Codex {
                "codex"
            } else {
                "anthropic"
            }
            .into(),
            before,
            after,
        });
    } else {
        for resource in active.resources() {
            changes.push(Change::unchanged(resource).map_err(safe_error)?);
        }
    }
    changes.push(
        Change::new(
            Resource::File(plan.target_path.clone()),
            Some(content.as_bytes().to_vec()),
        )
        .map_err(safe_error)?,
    );
    for write in &plan.extra_writes {
        // For keyring Codex API auth, use its existing secure backend.
        let resource = if write.target_path == active.home.join("auth.json") {
            active.primary.clone()
        } else {
            Resource::File(write.target_path.clone())
        };
        replace_change(
            &mut changes,
            Change::new(resource, Some(write.content.as_bytes().to_vec())).map_err(safe_error)?,
        );
    }
    // Native/auto backend selection must stay consistent with actual writes.
    preserve_codex_backend(&active, &plan, &mut changes).map_err(safe_error)?;
    let mut config_resources = std::iter::once(Resource::File(plan.target_path.clone()))
        .chain(
            plan.extra_writes
                .iter()
                .filter(|w| w.target_path != active.home.join("auth.json"))
                .map(|w| Resource::File(w.target_path.clone())),
        )
        .collect::<Vec<_>>();
    if request.mode != ToolConfigMode::Official {
        config_resources.extend(active.resources());
        config_resources.sort_by_key(|r| r.label());
        config_resources.dedup();
    }
    // Save binding in the same encrypted transaction; calculate postimage fingerprint in memory.
    let mut saved_request = request.clone();
    saved_request.preview_id = None;
    if saved_request.secret_id.is_none() {
        saved_request.secret_id = selected;
    }
    let config_fingerprint = fingerprint_after(
        &config_resources,
        &changes,
        &key,
        &context(&saved_request).map_err(safe_error)?,
    )
    .map_err(safe_error)?;
    let binding = Binding {
        route_id,
        title: title.clone(),
        request: saved_request,
        operation_id: plan.operation_id,
        identity: if source.is_some() {
            identity.clone()
        } else {
            None
        },
        config_resources,
        config_fingerprint,
    };
    changes.push(
        Change::new(
            Resource::File(binding_path(state, &request.tool)),
            Some(encode_binding(&binding, &key).map_err(safe_error)?),
        )
        .map_err(safe_error)?,
    );
    dedup_changes(&mut changes).map_err(safe_error)?;
    let mut tx = Transaction {
        operation_id: plan.operation_id,
        committed: false,
        changes,
        metadata: serde_json::to_value(&refs).map_err(safe_error)?,
    };
    let backup = tx.save(&root(state), &key).map_err(safe_error)?;
    if let Err(error) = tx
        .apply()
        .map_err(safe_error)
        .and_then(|_| set_references(vault, &refs, true))
    {
        tx.restore().map_err(safe_error)?;
        set_references(vault, &refs, false)?;
        std::fs::remove_file(&backup).map_err(safe_error)?;
        return Err(error);
    }
    tx.committed = true;
    if let Err(e) = tx.save(&root(state), &key) {
        tx.restore().map_err(safe_error)?;
        set_references(vault, &refs, false)?;
        return Err(safe_error(e));
    }
    // A cache error cannot turn committed credentials into a replayable failure.
    if refresh_cache(state, vault, &refs).is_err() {
        crate::logging::write_component_log(
            crate::logging::AGENT_LOG,
            "WARN",
            "event=tool.switch.proxy_refresh_failed",
        );
    }
    crate::logging::write_component_log(
        crate::logging::AGENT_LOG,
        "INFO",
        &format!(
            "event=tool.switch.committed operation_id={} tool={} entry_id={}",
            tx.operation_id,
            tool_name(&request.tool),
            request.id
        ),
    );
    Ok(ToolConfigApplyResponse {
        tool: request.tool.clone(),
        mode: request.mode.clone(),
        entry_id: request.id,
        entry_title: title,
        operation_id: tx.operation_id,
        target_path: plan.target_path.display().to_string(),
        backup_path: backup.display().to_string(),
        summary: plan.summary,
        outcome: ToolConfigOutcome::Applied,
        message: Some("Restart the CLI to use the selected credentials".into()),
    })
}
pub(crate) fn status(
    state: &Arc<AgentState>,
    tool: ToolConfigTool,
) -> ServiceResult<ToolConfigStatus> {
    status_checked(state, tool.clone()).or_else(|error| safety::status_error(tool, error))
}
fn status_checked(
    state: &Arc<AgentState>,
    tool: ToolConfigTool,
) -> ServiceResult<ToolConfigStatus> {
    // Snapshot and any renewal must refer to the same completed switch.
    let _guard = SWITCH_LOCK
        .lock()
        .map_err(|_| safe_error(anyhow::anyhow!("tool coordinator unavailable")))?;
    let mut result = status_snapshot(state, tool.clone())?;
    if result.state == "renewal_required"
        || (result.state == "ready"
            && result.mode == Some(ToolConfigMode::Official)
            && result.entry_id.is_some())
    {
        let request = with_vault(state, false, |vault| {
            load_binding(state, &tool, &vault.config_backup_key()).map_err(safe_error)
        })?
        .map(|b| b.request)
        .or_else(|| {
            result.entry_id.map(|id| ToolConfigRequest {
                tool: tool.clone(),
                id,
                secret_id: result.secret_id.clone(),
                mode: ToolConfigMode::Official,
                codex_api_key_mode: None,
                preview_id: None,
            })
        });
        if let Some(request) = request {
            match renew(state, &request) {
                Ok(()) => return status_snapshot(state, tool),
                Err(message) => {
                    result.state = if message.contains("expired")
                        || message.contains("sign in")
                        || message.contains("sign-in")
                        || message.contains("401")
                    {
                        "login_required"
                    } else if message.contains("changed") {
                        "conflict"
                    } else if message.contains("quota") || message.contains("429") {
                        "quota_exhausted"
                    } else if message.contains("service unavailable") {
                        "service_unavailable"
                    } else if message.contains("credential store") || message.contains("backup") {
                        "storage_unavailable"
                    } else {
                        "network_error"
                    }
                    .into();
                    result.message = Some(message);
                }
            }
        }
    }
    Ok(result)
}
fn status_snapshot(
    state: &Arc<AgentState>,
    tool: ToolConfigTool,
) -> ServiceResult<ToolConfigStatus> {
    with_vault(state, false, |vault| {
        let mut result = ToolConfigStatus {
            tool: tool.clone(),
            state: "unmanaged".into(),
            entry_title: None,
            entry_id: None,
            secret_id: None,
            mode: None,
            account_identity: None,
            operation_id: None,
            message: None,
            overrides: environment_overrides(&tool),
        };
        let Some(binding) =
            load_binding(state, &tool, &vault.config_backup_key()).map_err(safe_error)?
        else {
            let home = crate::server::home_dir(vault)?;
            match active_store(&home, &tool) {
                Ok(store) => {
                    result.account_identity = store.identity().map_err(safe_error)?;
                    if let Some(identity) = &result.account_identity {
                        if let Some(entry) = vault
                            .list_provider_summaries()
                            .map_err(map_vault_error)?
                            .into_iter()
                            .find(|e| {
                                e.account_identity.as_deref() == Some(identity)
                                    && reference(vault, e.id, &tool).is_ok_and(|r| {
                                        r["nativeHome"].as_str()
                                            == Some(store.home.to_string_lossy().as_ref())
                                    })
                            })
                        {
                            result.entry_id = Some(entry.id);
                            result.entry_title = Some(entry.title);
                            result.secret_id = entry.secret_refs.first().map(|s| s.id.clone());
                            result.mode = Some(ToolConfigMode::Official);
                            if store.expires_at().map_err(safe_error)?.is_some_and(|t| {
                                t <= time::OffsetDateTime::now_utc().unix_timestamp()
                            }) {
                                result.state = "renewal_required".into();
                            }
                        }
                    }
                }
                Err(_) => result.state = "storage_unavailable".into(),
            }
            return Ok(result);
        };
        result.entry_title = vault
            .get_provider_summary(binding.request.id)
            .ok()
            .map(|e| e.title)
            .or_else(|| Some(binding.title.clone()));
        result.entry_id = Some(binding.request.id);
        result.secret_id = binding.request.secret_id.clone();
        result.mode = Some(binding.request.mode.clone());
        result.operation_id = Some(binding.operation_id);
        result.account_identity = binding.identity.clone();
        let actual = fingerprint(
            &binding.config_resources,
            &vault.config_backup_key(),
            &context(&binding.request).map_err(safe_error)?,
        )
        .map_err(safe_error)?;
        if actual != binding.config_fingerprint {
            result.state = "conflict".into();
            return Ok(result);
        }
        let home = crate::server::home_dir(vault)?;
        let store = match active_store(&home, &tool) {
            Ok(s) => s,
            Err(_) => {
                result.state = "storage_unavailable".into();
                return Ok(result);
            }
        };
        if binding.request.mode == ToolConfigMode::Official {
            match store.identity() {
                Ok(id) if id == binding.identity => {}
                Ok(None) => {
                    result.state = "login_required".into();
                    return Ok(result);
                }
                Ok(_) => {
                    result.state = "conflict".into();
                    return Ok(result);
                }
                Err(_) => {
                    result.state = "storage_unavailable".into();
                    return Ok(result);
                }
            }
            if store
                .expires_at()
                .ok()
                .flatten()
                .is_some_and(|t| t <= time::OffsetDateTime::now_utc().unix_timestamp())
            {
                result.state = "renewal_required".into();
            } else {
                result.state = "ready".into();
            }
        } else {
            // auth.json/keychain API keys are compared against the exact selected vault key.
            if binding.request.tool == ToolConfigTool::Codex
                && binding.route_id.is_none()
                && binding.request.mode == ToolConfigMode::Plaintext
                && binding.request.codex_api_key_mode
                    != Some(CodexApiKeyMode::ExperimentalBearerToken)
            {
                let selected = binding.request.secret_id.as_deref().ok_or_else(|| {
                    safe_error(anyhow::anyhow!("selected credential unavailable"))
                })?;
                let key = Zeroizing::new(
                    vault
                        .reveal_secret_by_id(binding.request.id, selected)
                        .map_err(map_vault_error)?,
                );
                if store
                    .credentials()
                    .ok()
                    .is_none_or(|v| v.0["OPENAI_API_KEY"].as_str() != Some(key.as_str()))
                {
                    result.state = "api_key_changed".into();
                    return Ok(result);
                }
            }
            result.state = health::observed(state, vault, &binding.request)
                .unwrap_or("ready")
                .into();
        }
        Ok(result)
    })
}
pub(crate) fn redact_config(content: &str) -> String {
    aipass_config_writers::redact_config(content)
}
fn environment_overrides(tool: &ToolConfigTool) -> Vec<String> {
    let keys: &[&str] = if *tool == ToolConfigTool::Codex {
        &["OPENAI_API_KEY", "CODEX_API_KEY", "OPENAI_BASE_URL"]
    } else {
        &[
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_BASE_URL",
            "CLAUDE_CODE_OAUTH_TOKEN",
        ]
    };
    keys.iter()
        .filter(|k| std::env::var(k).is_ok_and(|v| !v.is_empty()))
        .map(|k| k.to_string())
        .collect()
}

#[cfg(test)]
mod tests;

pub(crate) fn credential_preview(
    vault: &Vault,
    request: &ToolConfigRequest,
) -> ServiceResult<Vec<ToolConfigPreviewFile>> {
    if request.mode != ToolConfigMode::Official || !supported(&request.tool) {
        return Ok(Vec::new());
    }
    let home = crate::server::home_dir(vault)?;
    let active = active_store(&home, &request.tool).map_err(safe_error)?;
    let identity = vault
        .get_provider_summary(request.id)
        .map_err(map_vault_error)?
        .account_identity
        .unwrap_or_default();
    Ok(active.resources().iter().map(|resource| ToolConfigPreviewFile {
        path: resource.label(),
        content: format!("Native account: {identity}\nCredential material: [redacted]\nEncrypted backup before replacement"),
        diff: "+ Replace native account credentials [redacted]".into(),
    }).collect())
}

fn account_locks(state: &Arc<AgentState>) -> ServiceResult<Vec<Arc<Mutex<()>>>> {
    with_vault(state, false, |vault| {
        let bridge = state
            .proxy
            .lock()
            .map_err(|_| safe_error(anyhow::anyhow!("proxy unavailable")))?
            .community_bridge();
        let mut ids = vault
            .list_provider_summaries()
            .map_err(map_vault_error)?
            .into_iter()
            .filter(|e| {
                matches!(e.provider_id.as_deref(), Some("codex" | "openai"))
                    || (e.provider_id.as_deref() == Some("anthropic")
                        && e.credential_kind == aipass_provider_registry::CredentialKind::OAuth)
            })
            .map(|e| e.id)
            .collect::<Vec<_>>();
        ids.sort();
        ids.into_iter()
            .map(|id| {
                bridge
                    .account_lock(id)
                    .map_err(|e| safe_error(anyhow::anyhow!(e)))
            })
            .collect::<ServiceResult<Vec<_>>>()
    })
}

pub(crate) fn native_account_lock(
    state: &Arc<AgentState>,
    id: Uuid,
) -> ServiceResult<Arc<Mutex<()>>> {
    state
        .proxy
        .lock()
        .map_err(|_| safe_error(anyhow::anyhow!("proxy unavailable")))?
        .community_bridge()
        .account_lock(id)
        .map_err(|e| safe_error(anyhow::anyhow!(e)))
}
