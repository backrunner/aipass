use super::*;

pub(super) fn handle(
    state: &Arc<AgentState>,
    request: AgentRequest,
) -> ServiceResult<AgentResponse> {
    let managed = matches!(&request, AgentRequest::ToolConfigManagedApply { .. });
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
                let preview_id = Some(crate::tool_switch::preview_id(vault, state, &request_for_closure, &plan).map_err(ServiceError::internal)?);
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
        AgentRequest::ToolConfigApply { request }
        | AgentRequest::ToolConfigManagedApply { request } => {
            if managed && request.preview_id.is_none() {
                return Err(ServiceError::new(
                    AgentErrorCode::Conflict,
                    "Preview the Agent switch before applying",
                ));
            }
            require_installed(state, &crate::server::tool_id_for(&request.tool))?;
            if crate::tool_switch::supported(&request.tool) && request.mode != ToolConfigMode::Env {
                return crate::tool_switch::apply_managed(state, request, managed)
                    .map(AgentResponse::success);
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
                if managed {
                    let stamp = crate::tool_switch::preview_id(vault, state, &request_for_closure, &plan).map_err(crate::tool_credentials::error)?;
                    if request_for_closure.preview_id.as_deref() != Some(stamp.as_str()) {
                        return Err(ServiceError::new(AgentErrorCode::Conflict, "Agent credentials changed; preview again"));
                    }
                    crate::tool_credentials::preserve(vault, state, &request_for_closure.tool)?;
                    let after = crate::tool_switch::preview_id(vault, state, &request_for_closure, &plan).map_err(crate::tool_credentials::error)?;
                    if after != stamp {
                        return Err(ServiceError::new(AgentErrorCode::Conflict, "Agent credentials changed during import; preview again"));
                    }
                }
                let operation_id = plan.operation_id;
                let target = "redacted";
                let mut recovery = crate::proxy_recovery::WriteGuard::begin(&home_dir(vault)?, false).map_err(ServiceError::internal)?;
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
                let retirement_failed = recovery.as_mut().is_some_and(|r| r.retire(&plan, &[]).is_err());
                let mut response = tool_apply_response(request.clone(), entry, plan, applied);
                if retirement_failed {
                    write_component_log(AGENT_LOG, "WARN", "event=proxy.uninstall.retirement_pending");
                    response.message = Some("Configuration applied; proxy restoration needs attention. Remove remaining proxy entries or retry this configuration before uninstalling.".into());
                }
                Ok(response)
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
                let mut recovery = crate::proxy_recovery::WriteGuard::begin(&home, false).map_err(ServiceError::internal)?;
                let backup = aipass_config_writers::find_backup_by_operation(&home, operation_id)
                    .map_err(ServiceError::internal)?;
                let prepared = aipass_config_writers::prepare_rollback_encrypted(&backup, &vault.config_backup_key()).map_err(ServiceError::internal)?;
                if let Some(recovery) = &mut recovery { recovery.reactivate(&state.vault_dir, operation_id).map_err(ServiceError::internal)?; }
                let backup_path = "redacted";
                prepared.apply()
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
            require_installed(state, &request.tool)?;
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
                let mut recovery = crate::proxy_recovery::WriteGuard::begin(&home_dir(vault)?, true).map_err(ServiceError::internal)?;
                if let Some(recovery) = &mut recovery { recovery.record_plan(&state.vault_dir, &plan, &content, &entry).map_err(ServiceError::internal)?; }
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
                if let Some(recovery) = &mut recovery { recovery.commit(); }
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

fn require_installed(
    state: &Arc<AgentState>,
    tool: &aipass_config_writers::ToolId,
) -> ServiceResult<()> {
    with_vault(state, true, |vault| {
        let home = home_dir(vault)?;
        let dirs = aipass_config_writers::cli_discovery::search_dirs();
        #[cfg(test)]
        let dirs = if crate::server::TOOL_HOME_OVERRIDES
            .lock()
            .unwrap()
            .contains_key(&vault.vault_id())
        {
            vec![home.join(".local/bin"), home.join(".grok/bin")]
        } else {
            dirs
        };
        if aipass_config_writers::detect_tool_in(Some(&home), &dirs, tool).is_installed() {
            Ok(())
        } else {
            Err(ServiceError::new(
                AgentErrorCode::ValidationFailed,
                "Agent is no longer installed; scan installed Agents again",
            ))
        }
    })
}
