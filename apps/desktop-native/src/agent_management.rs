//! Agent management projects metadata; all configuration writes remain in Agent.
use aipass_agent::{AgentClient, AgentCommandError};
use aipass_agent_protocol::{
    AgentRequest, ServerUsageSummary, ToolConfigApplyResponse, ToolConfigPreviewResponse,
    ToolConfigRequest, ToolConfigStatus,
};
use aipass_config_writers::ToolDetection;
use uuid::Uuid;
pub mod choices;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Scan,
    Usage,
    Preview,
    Apply,
    Restore,
    Copy,
}
pub enum Operation {
    Scan,
    Usage { timezone_offset_minutes: i32 },
    Preview(ToolConfigRequest),
    Apply(ToolConfigRequest),
    Restore(Uuid),
    Copy(crate::credentials::Request),
}
impl Operation {
    pub fn kind(&self) -> Kind {
        match self {
            Self::Scan => Kind::Scan,
            Self::Usage { .. } => Kind::Usage,
            Self::Preview(_) => Kind::Preview,
            Self::Apply(_) => Kind::Apply,
            Self::Restore(_) => Kind::Restore,
            Self::Copy(_) => Kind::Copy,
        }
    }
}
pub struct Request {
    pub generation: u64,
    pub operation: Operation,
}
pub struct Completion {
    pub generation: u64,
    pub kind: Kind,
    pub result: Result<Outcome, AgentCommandError>,
}
pub enum Outcome {
    Scanned {
        detections: Vec<ToolDetection>,
        statuses: Vec<ToolConfigStatus>,
    },
    Usage(ServerUsageSummary),
    Preview(ToolConfigPreviewResponse),
    Applied(ToolConfigApplyResponse),
    Copied(bool),
}
impl Request {
    pub(crate) fn execute(self, client: &AgentClient) -> Completion {
        let kind = self.operation.kind();
        let result = match self.operation {
            Operation::Scan => scan(client),
            Operation::Usage {
                timezone_offset_minutes,
            } => client
                .request(&AgentRequest::ServerUsageSummary {
                    days: Some(7),
                    timezone_offset_minutes,
                    granularity: Default::default(),
                })
                .map(Outcome::Usage),
            Operation::Preview(request) => client
                .request(&AgentRequest::ToolConfigPreview { request })
                .map(Outcome::Preview),
            Operation::Apply(request) => client
                .request(&AgentRequest::ToolConfigManagedApply { request })
                .map(Outcome::Applied),
            Operation::Restore(operation_id) => client
                .request(&AgentRequest::ToolConfigRollback { operation_id })
                .map(Outcome::Applied),
            Operation::Copy(request) => {
                request
                    .execute(client)
                    .result
                    .and_then(|result| match result {
                        crate::credentials::Outcome::Copied(copied) => Ok(Outcome::Copied(copied)),
                        _ => Err(AgentCommandError {
                            code: None,
                            message: "Unexpected copy response".into(),
                        }),
                    })
            }
        };
        Completion {
            generation: self.generation,
            kind,
            result,
        }
    }
}
fn scan(client: &AgentClient) -> Result<Outcome, AgentCommandError> {
    let detections = aipass_config_writers::detect_tools();
    let mut statuses = vec![];
    for tool in choices::tools(&detections) {
        let status = if choices::guide(&tool).is_some() {
            choices::empty_status(tool, "unmanaged")
        } else {
            match client.request(&AgentRequest::ToolConfigStatus { tool: tool.clone() }) {
                Ok(status) => status,
                Err(error) if error.code == Some(aipass_agent_protocol::AgentErrorCode::Locked) => {
                    return Err(error)
                }
                Err(_) => choices::empty_status(tool, "unavailable"),
            }
        };
        statuses.push(status);
    }
    Ok(Outcome::Scanned {
        detections,
        statuses,
    })
}
