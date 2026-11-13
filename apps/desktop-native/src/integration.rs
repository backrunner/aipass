//! Quick Integration uses existing Agent plans and encrypted backup/rollback.
use aipass_agent::{AgentClient, AgentCommandError};
use aipass_agent_protocol::{
    AgentRequest, SensitiveString, ToolConfigApplyResponse, ToolConfigLoginStatus,
    ToolConfigPreviewResponse, ToolConfigRequest, ToolConfigStatus, ToolConfigTool,
};
use aipass_config_writers::ToolDetection;
use uuid::Uuid;
pub mod catalog;
pub mod diff;
pub mod syntax;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scope {
    pub entry: Uuid,
    pub generation: u64,
}

pub enum Operation {
    Scan,
    Preview(ToolConfigRequest),
    Apply(ToolConfigRequest),
    Rollback(Uuid),
    LoginStart(ToolConfigRequest),
    LoginPoll(Uuid),
    LoginCode(Uuid, SensitiveString),
    LoginCancel(Uuid),
}
pub struct Request {
    pub scope: Scope,
    pub operation: Operation,
}
pub enum Outcome {
    Scanned {
        detections: Vec<ToolDetection>,
        statuses: Vec<ToolConfigStatus>,
    },
    Preview(ToolConfigPreviewResponse),
    Applied(ToolConfigApplyResponse),
    Login(ToolConfigLoginStatus),
    CodeSent,
    Cancelled,
}
pub struct Completion {
    pub scope: Scope,
    pub result: Result<Outcome, AgentCommandError>,
}

impl Request {
    pub(crate) fn execute(self, client: &AgentClient) -> Completion {
        let result = match self.operation {
            Operation::Scan => {
                let detections = aipass_config_writers::detect_tools();
                let mut statuses = Vec::new();
                for tool in [ToolConfigTool::Codex, ToolConfigTool::ClaudeCode] {
                    match client.request(&AgentRequest::ToolConfigStatus { tool: tool.clone() }) {
                        Ok(status) => statuses.push(status),
                        Err(error)
                            if error.code
                                == Some(aipass_agent_protocol::AgentErrorCode::Locked) =>
                        {
                            return Completion {
                                scope: self.scope,
                                result: Err(error),
                            }
                        }
                        Err(_) => statuses.push(ToolConfigStatus {
                            tool,
                            state: "unavailable".into(),
                            entry_title: None,
                            entry_id: None,
                            secret_id: None,
                            mode: None,
                            account_identity: None,
                            operation_id: None,
                            message: None,
                            overrides: vec![],
                        }),
                    }
                }
                Ok(Outcome::Scanned {
                    detections,
                    statuses,
                })
            }
            Operation::Preview(request) => client
                .request(&AgentRequest::ToolConfigPreview { request })
                .map(Outcome::Preview),
            Operation::Apply(request) => client
                .request(&AgentRequest::ToolConfigApply { request })
                .map(Outcome::Applied),
            Operation::Rollback(operation_id) => client
                .request(&AgentRequest::ToolConfigRollback { operation_id })
                .map(Outcome::Applied),
            Operation::LoginStart(request) => client
                .request(&AgentRequest::ToolConfigLoginStart { request })
                .map(Outcome::Login),
            Operation::LoginPoll(ticket) => client
                .request(&AgentRequest::ToolConfigLoginPoll { ticket })
                .map(Outcome::Login),
            Operation::LoginCode(ticket, code) => client
                .request::<serde_json::Value>(&AgentRequest::ToolConfigLoginCode { ticket, code })
                .map(|_| Outcome::CodeSent),
            Operation::LoginCancel(ticket) => client
                .request::<serde_json::Value>(&AgentRequest::ToolConfigLoginCancel { ticket })
                .map(|_| Outcome::Cancelled),
        };
        Completion {
            scope: self.scope,
            result,
        }
    }
}
