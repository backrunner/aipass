use crate::*;
use aipass_agent_protocol::{ToolConfigLoginStatus, ToolConfigStatus};
pub(crate) fn handle(
    json: bool,
    vault: Option<PathBuf>,
    password: Option<String>,
    command: ToolCommand,
) -> Result<()> {
    let agent = CliAgent::from_parts(vault, password)?;
    match command {
        ToolCommand::Status { tool } => {
            let status: ToolConfigStatus =
                agent.request(AgentRequest::ToolConfigStatus { tool: tool.into() })?;
            output(
                json,
                serde_json::to_value(&status)?,
                &format!(
                    "{}: {}",
                    status
                        .account_identity
                        .or(status.entry_title)
                        .unwrap_or_else(|| "Original CLI credentials".into()),
                    status.state
                ),
            )
        }
        ToolCommand::Login {
            tool,
            id,
            secret_id,
        } => {
            let id = resolve_entry_id(&agent, &id)?;
            let request = ToolConfigRequest {
                tool: tool.into(),
                id,
                secret_id,
                mode: aipass_agent_protocol::ToolConfigMode::Official,
                codex_api_key_mode: None,
                preview_id: None,
            };
            let result: ToolConfigLoginStatus =
                agent.request(AgentRequest::ToolConfigLoginStart { request })?;
            output(
                json,
                serde_json::to_value(&result)?,
                &format!(
                    "Sign-in started: {}. Poll with aipass tool login-poll {}",
                    result.ticket, result.ticket
                ),
            )
        }
        ToolCommand::LoginPoll { ticket } => {
            let result: ToolConfigLoginStatus =
                agent.request(AgentRequest::ToolConfigLoginPoll { ticket })?;
            output(
                json,
                serde_json::to_value(&result)?,
                &format!(
                    "{}{}",
                    result.status,
                    result.url.map(|u| format!(": {u}")).unwrap_or_default()
                ),
            )
        }
        ToolCommand::LoginCode { ticket, code } => {
            agent.request::<()>(AgentRequest::ToolConfigLoginCode {
                ticket,
                code: code.into(),
            })?;
            output(json, serde_json::json!({"ok":true}), "Code submitted")
        }
        ToolCommand::LoginCancel { ticket } => {
            agent.request_no_unlock::<()>(AgentRequest::ToolConfigLoginCancel { ticket })?;
            output(json, serde_json::json!({"ok":true}), "Sign-in cancelled")
        }
    }
}
