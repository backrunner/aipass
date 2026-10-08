//! CLI account policy and compatibility IPC; credentials stay in the vendor store.
use super::*;

pub(super) fn handle(
    state: &Arc<AgentState>,
    request: AgentRequest,
) -> ServiceResult<AgentResponse> {
    match request {
        AgentRequest::SubscriptionCliStatus { provider } => {
            with_vault(state, false, |_| Ok(()))?;
            Ok(AgentResponse::success(
                crate::subscriptions::cli_accounts::status(&provider),
            ))
        }
        AgentRequest::ClaudeCliStatus => {
            with_vault(state, false, |_| Ok(()))?;
            Ok(AgentResponse::success(crate::claude_cli::status()))
        }
        AgentRequest::ClaudeLoginStart => {
            let epoch = with_vault(state, true, |_| Ok(crate::claude_cli::logins().epoch()))?;
            let result = crate::claude_cli::logins()
                .start(epoch)
                .map_err(|e| ServiceError::new(AgentErrorCode::ValidationFailed, e))?;
            if let Err(error) = with_vault(state, false, |_| Ok(())) {
                crate::claude_cli::logins().cancel(result.ticket);
                return Err(error);
            }
            Ok(AgentResponse::success(result))
        }
        AgentRequest::ClaudeLoginPoll { ticket } => {
            with_vault(state, false, |_| Ok(()))?;
            let mut result = crate::claude_cli::logins()
                .poll(ticket)
                .map_err(|e| ServiceError::new(AgentErrorCode::ValidationFailed, e))?;
            if result.status == "authorized" && result.entry_id.is_none() {
                result.entry_id = Some(with_vault(state, false, |vault| {
                    crate::claude_cli::logins().commit(ticket, |account| {
                        let id = crate::official_accounts::persist_claude_login(vault, account)
                            .map_err(ServiceError::internal)?;
                        // Persistence is authoritative; a later reload failure must not replay login.
                        if refresh_proxy_provider_credentials(state, vault, id).is_err() {
                            crate::logging::write_component_log(
                                crate::logging::AGENT_LOG,
                                "WARN",
                                "event=claude.login.proxy_refresh_failed",
                            );
                        }
                        Ok(id)
                    })
                })?);
            }
            Ok(AgentResponse::success(result))
        }
        AgentRequest::ClaudeLoginCode { ticket, code } => {
            with_vault(state, false, |_| Ok(()))?;
            crate::claude_cli::logins()
                .code(ticket, code.expose())
                .map_err(|e| ServiceError::new(AgentErrorCode::ValidationFailed, e))?;
            Ok(AgentResponse::empty())
        }
        AgentRequest::ClaudeLoginCancel { ticket } => Ok(AgentResponse::success(
            crate::claude_cli::logins().cancel(ticket),
        )),
        AgentRequest::CommunityRefresh { entry_id } => {
            let claude = with_vault(state, false, |vault| {
                Ok(crate::official_accounts::claude_reference(vault, entry_id)?.is_some())
            })?;
            if claude {
                crate::official_accounts::refresh_claude(state, entry_id)
                    .map(AgentResponse::success)
            } else {
                crate::community::refresh(state, entry_id).map(AgentResponse::success)
            }
        }
        AgentRequest::ClaudeNativeRead {
            entry_id,
            access_token,
        } => with_vault(state, false, |vault| {
            crate::claude_bridge::validate_account(vault, entry_id, access_token.expose())?;
            let reference = vault
                .provider_runtime_extension(entry_id, "claude_cli_home")
                .map_err(map_vault_error)?;
            if reference.is_none() {
                return Err(ServiceError::new(
                    AgentErrorCode::ValidationFailed,
                    "Reconnect Claude through its official CLI",
                ));
            }
            Ok(AgentResponse::success(
                reference.map(|v| SensitiveString::new(v.expose())),
            ))
        }),
        AgentRequest::ClaudeNativeWrite { .. } => Err(ServiceError::new(
            AgentErrorCode::ValidationFailed,
            "Claude Code owns credential renewal; vault grant writes have been retired",
        )),
        AgentRequest::OAuthLoginStart { .. }
        | AgentRequest::OAuthLoginPoll { .. }
        | AgentRequest::OAuthLoginCancel { .. } => Err(ServiceError::new(
            AgentErrorCode::ValidationFailed,
            "Connect this subscription through its official CLI; managed OAuth login has been retired",
        )),
        _ => Err(ServiceError::internal(anyhow::anyhow!(
            "unsupported subscription request"
        ))),
    }
}
