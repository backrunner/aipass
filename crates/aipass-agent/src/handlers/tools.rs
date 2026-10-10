use super::*;

pub(super) fn handle(
    state: &Arc<AgentState>,
    request: AgentRequest,
) -> ServiceResult<AgentResponse> {
    match request {
        AgentRequest::ToolConfigStatus { tool } => {
            crate::tool_switch::status(state, tool).map(AgentResponse::success)
        }
        request @ (AgentRequest::ToolConfigLoginStart { .. }
        | AgentRequest::ToolConfigLoginPoll { .. }
        | AgentRequest::ToolConfigLoginCode { .. }
        | AgentRequest::ToolConfigLoginCancel { .. }) => {
            crate::tool_switch::handle_login(state, request)
        }
        AgentRequest::ToolConfigPreview { request } => {
            let started = Instant::now();
            let request_for_closure = request.clone();
            write_component_log(
                AGENT_LOG,
                "INFO",
                &format!(
                    "tool config preview started tool={:?} mode={:?} entry_id={}",
                    request.tool, request.mode, request.id
                ),
            );
            let result = with_vault(state, true, |vault| {
                let (entry, plan, content) = build_tool_config_plan(vault, &request_for_closure, true)?;
                let preview_id = if crate::tool_switch::supported(&request_for_closure.tool) {
                    Some(crate::tool_switch::preview_id(vault, state, &request_for_closure, &plan).map_err(ServiceError::internal)?)
                } else { None };
                let mut files = tool_config_preview_files(&plan, &content);
                files.extend(crate::tool_switch::credential_preview(vault, &request_for_closure)?);
                let preview = combined_tool_config_preview(&files);
                write_component_log(
                    AGENT_LOG,
                    "INFO",
                    &format!(
                        "tool config preview completed operation_id={} tool={:?} mode={:?} target={} files={} elapsed_ms={}",
                        plan.operation_id,
                        request.tool,
                        request.mode,
                        "redacted",
                        files.len(),
                        started.elapsed().as_millis()
                    ),
                );
                Ok(ToolConfigPreviewResponse {
                    preview_id,
                    tool: request_for_closure.tool,
                    mode: request_for_closure.mode,
                    entry_id: entry.id,
                    entry_title: entry.title,
                    target_path: plan.target_path.display().to_string(),
                    summary: plan.summary,
                    preview,
                    files,
                })
            })
            .map(AgentResponse::success);
            if let Err(err) = &result {
                write_component_log(
                    AGENT_LOG,
                    "ERROR",
                    &format!(
                        "tool config preview failed tool={:?} mode={:?} entry_id={} elapsed_ms={} code={:?}",
                        request.tool,
                        request.mode,
                        request.id,
                        started.elapsed().as_millis(),
                        err.code
                    ),
                );
            }
            result
        }
        AgentRequest::ToolConfigApply { request } => {
            if crate::tool_switch::supported(&request.tool) && request.mode != ToolConfigMode::Env {
                return crate::tool_switch::apply(state, request).map(AgentResponse::success);
            }
            let started = Instant::now();
            let request_for_closure = request.clone();
            write_component_log(
                AGENT_LOG,
                "INFO",
                &format!(
                    "tool config apply started tool={:?} mode={:?} entry_id={}",
                    request.tool, request.mode, request.id
                ),
            );
            let result = with_vault(state, false, |vault| {
                let (entry, plan, content) = build_tool_config_plan(vault, &request_for_closure, false)?;
                let operation_id = plan.operation_id;
                let target = "redacted";
                let applied = apply_plan_encrypted(&plan, &content, &vault.config_backup_key())
                    .map_err(|err| {
                        write_component_log(
                            AGENT_LOG,
                            "ERROR",
                            &format!(
                                "tool config apply failed operation_id={} tool={:?} mode={:?} target={} elapsed_ms={} error={}",
                                operation_id,
                                request.tool,
                                request.mode,
                                target,
                                started.elapsed().as_millis(),
                                "write_failed"
                            ),
                        );
                        ServiceError::internal(err)
                    })?;
                write_component_log(
                    AGENT_LOG,
                    "INFO",
                    &format!(
                        "tool config apply completed operation_id={} tool={:?} mode={:?} target={} backup={} elapsed_ms={}",
                        operation_id,
                        request.tool,
                        request.mode,
                        target,
                        "redacted",
                        started.elapsed().as_millis()
                    ),
                );
                Ok(tool_apply_response(request.clone(), entry, plan, applied))
            })
            .map(AgentResponse::success);
            if let Err(err) = &result {
                write_component_log(
                    AGENT_LOG,
                    "ERROR",
                    &format!(
                        "tool config apply rejected tool={:?} mode={:?} entry_id={} elapsed_ms={} code={:?}",
                        request.tool,
                        request.mode,
                        request.id,
                        started.elapsed().as_millis(),
                        err.code
                    ),
                );
            }
            result
        }
        AgentRequest::ToolConfigRollback { operation_id } => {
            if crate::tool_switch::has_backup(state, operation_id) {
                return crate::tool_switch::rollback(state, operation_id)
                    .map(AgentResponse::success);
            }
            let started = Instant::now();
            write_component_log(
                AGENT_LOG,
                "INFO",
                &format!("tool config rollback started operation_id={operation_id}"),
            );
            let result = with_vault(state, false, |vault| {
                let home = home_dir(vault)?;
                let backup = aipass_config_writers::find_backup_by_operation(&home, operation_id)
                    .map_err(ServiceError::internal)?;
                let backup_path = "redacted";
                rollback_encrypted(&backup, &vault.config_backup_key())
                    .map_err(|err| {
                        write_component_log(
                            AGENT_LOG,
                            "ERROR",
                            &format!(
                                "tool config rollback failed operation_id={operation_id} backup={} elapsed_ms={} error=rollback_failed",
                                backup_path,
                                started.elapsed().as_millis()
                            ),
                        );
                        ServiceError::internal(err)
                    })
            })
            .map(AgentResponse::success);
            if result.is_ok() {
                write_component_log(
                    AGENT_LOG,
                    "INFO",
                    &format!(
                        "tool config rollback completed operation_id={operation_id} elapsed_ms={}",
                        started.elapsed().as_millis()
                    ),
                );
            } else if let Err(err) = &result {
                write_component_log(
                    AGENT_LOG,
                    "ERROR",
                    &format!(
                        "tool config rollback rejected operation_id={operation_id} elapsed_ms={} code={:?}",
                        started.elapsed().as_millis(),
                        err.code
                    ),
                );
            }
            result
        }
        AgentRequest::ToolConfigProxyPreview { request } => {
            let started = Instant::now();
            let request_for_closure = request.clone();
            write_component_log(
                AGENT_LOG,
                "INFO",
                &format!(
                    "tool config proxy preview started tool={:?} route_id={}",
                    request.tool, request.route_id
                ),
            );
            let result = with_vault(state, true, |vault| {
                let (entry, plan, content) = build_tool_config_proxy_plan(vault, state, &request_for_closure, true)?;
                let files = tool_config_preview_files(&plan, &content);
                let preview = combined_tool_config_preview(&files);
                write_component_log(
                    AGENT_LOG,
                    "INFO",
                    &format!(
                        "tool config proxy preview completed operation_id={} tool={:?} route_id={} target={} files={} elapsed_ms={}",
                        plan.operation_id, request.tool, request.route_id,
                        "redacted", files.len(), started.elapsed().as_millis()
                    ),
                );
                Ok(ToolConfigPreviewResponse {
                    preview_id: None,
                    tool: tool_config_tool_for(&request.tool),
                    mode: ToolConfigMode::Plaintext,
                    entry_id: entry.id,
                    entry_title: entry.title,
                    target_path: plan.target_path.display().to_string(),
                    summary: plan.summary,
                    preview,
                    files,
                })
            })
            .map(AgentResponse::success);
            if let Err(err) = &result {
                write_component_log(
                    AGENT_LOG,
                    "ERROR",
                    &format!(
                        "tool config proxy preview failed tool={:?} route_id={} elapsed_ms={} code={:?}",
                        request.tool, request.route_id, started.elapsed().as_millis(), err.code
                    ),
                );
            }
            result
        }
        AgentRequest::ToolConfigProxyApply { request } => {
            if matches!(
                request.tool,
                aipass_config_writers::ToolId::Codex | aipass_config_writers::ToolId::ClaudeCode
            ) {
                return crate::tool_switch::apply_proxy(state, request).map(AgentResponse::success);
            }
            let started = Instant::now();
            let request_for_closure = request.clone();
            write_component_log(
                AGENT_LOG,
                "INFO",
                &format!(
                    "tool config proxy apply started tool={:?} route_id={}",
                    request.tool, request.route_id
                ),
            );
            let result = with_vault(state, false, |vault| {
                let (entry, plan, content) = build_tool_config_proxy_plan(vault, state, &request_for_closure, false)?;
                let operation_id = plan.operation_id;
                let target = "redacted";
                let applied = apply_plan_encrypted(&plan, &content, &vault.config_backup_key())
                    .map_err(|err| {
                        write_component_log(
                            AGENT_LOG,
                            "ERROR",
                            &format!(
                                "tool config proxy apply failed operation_id={} tool={:?} route_id={} target={} elapsed_ms={} error={}",
                                operation_id, request.tool, request.route_id, target,
                                started.elapsed().as_millis(), "write_failed"
                            ),
                        );
                        ServiceError::internal(err)
                    })?;
                write_component_log(
                    AGENT_LOG,
                    "INFO",
                    &format!(
                        "tool config proxy apply completed operation_id={} tool={:?} route_id={} target={} backup={} elapsed_ms={}",
                        operation_id, request.tool, request.route_id, target,
                        "redacted",
                        started.elapsed().as_millis()
                    ),
                );
                Ok(ToolConfigApplyResponse {
        outcome: aipass_agent_protocol::ToolConfigOutcome::Applied,
        message: None,
                    tool: tool_config_tool_for(&request.tool),
                    mode: ToolConfigMode::Plaintext,
                    entry_id: entry.id,
                    entry_title: entry.title,
                    operation_id: applied.operation_id,
                    target_path: applied.target_path.display().to_string(),
                    backup_path: applied.backup_path.display().to_string(),
                    summary: plan.summary,
                })
            })
            .map(AgentResponse::success);
            if let Err(err) = &result {
                write_component_log(
                    AGENT_LOG,
                    "ERROR",
                    &format!(
                        "tool config proxy apply rejected tool={:?} route_id={} elapsed_ms={} code={:?}",
                        request.tool, request.route_id, started.elapsed().as_millis(), err.code
                    ),
                );
            }
            result
        }
        _ => Err(ServiceError::new(
            AgentErrorCode::ValidationFailed,
            "unsupported tool operation",
        )),
    }
}
