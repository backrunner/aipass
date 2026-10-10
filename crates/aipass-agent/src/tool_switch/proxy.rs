//! Existing local proxy integration uses the same native credential transaction.
use super::*;
use aipass_config_writers::ToolId;
pub(crate) fn apply_proxy(
    state: &Arc<AgentState>,
    request: ToolConfigProxyRequest,
) -> ServiceResult<ToolConfigApplyResponse> {
    let tool = match request.tool {
        ToolId::Codex => ToolConfigTool::Codex,
        ToolId::ClaudeCode => ToolConfigTool::ClaudeCode,
        _ => return Err(safe_error(anyhow::anyhow!("unsupported native tool"))),
    };
    let _guard = SWITCH_LOCK
        .lock()
        .map_err(|_| safe_error(anyhow::anyhow!("tool coordinator unavailable")))?;
    apply_proxy_locked(state, request, tool)
}
pub(super) fn apply_proxy_locked(
    state: &Arc<AgentState>,
    request: ToolConfigProxyRequest,
    tool: ToolConfigTool,
) -> ServiceResult<ToolConfigApplyResponse> {
    if status_snapshot(state, tool.clone())?.state == "conflict" {
        return Err(ServiceError::new(
            AgentErrorCode::Conflict,
            "tool configuration changed externally",
        ));
    }
    let locks = account_locks(state)?;
    let mut guards = Vec::new();
    for lock in &locks {
        guards.push(lock.try_lock().map_err(|_| {
            ServiceError::new(
                AgentErrorCode::Conflict,
                "account renewal is in progress; retry",
            )
        })?);
    }
    with_vault(state, false, |vault| {
        if !aipass_config_writers::transaction::pending(&root(state), &vault.config_backup_key())
            .map_err(safe_error)?
            .is_empty()
        {
            return Err(ServiceError::new(
                AgentErrorCode::Conflict,
                "pending tool recovery",
            ));
        }
        let (entry, plan, content) =
            crate::server::build_tool_config_proxy_plan(vault, state, &request, true)?;
        let direct = ToolConfigRequest {
            tool,
            id: request.route_id,
            secret_id: None,
            mode: ToolConfigMode::Plaintext,
            codex_api_key_mode: None,
            preview_id: None,
        };
        apply_prepared(
            state,
            vault,
            &direct,
            Prepared {
                title: entry.title,
                identity: None,
                selected: None,
                route_id: Some(request.route_id),
                plan,
                content,
            },
        )
    })
}
