//! Pricing requests carry metadata only; the authenticated Agent owns prices,
//! credential assignments, history recalculation and persistence.
use aipass_agent::{AgentClient, AgentCommandError};
use aipass_agent_protocol::{AgentRequest, PricingApplyScope, PricingConfig, PricingGroup};
use uuid::Uuid;

pub mod draft;
pub mod presentation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scope {
    pub entry: Uuid,
    pub generation: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Assignment {
    pub entry_id: Uuid,
    pub secret_id: String,
    pub group_id: Option<Uuid>,
    pub multiplier: f64,
}

#[derive(Clone)]
pub enum Operation {
    Load,
    Assign(Assignment),
    SaveGroup {
        group: PricingGroup,
        apply_scope: PricingApplyScope,
    },
    DeleteGroup(Uuid),
    DeleteVersion {
        group_id: Uuid,
        effective_from: i64,
    },
}

pub struct Request {
    pub scope: Scope,
    pub operation: Operation,
}
pub struct Completion {
    pub scope: Scope,
    pub result: Result<PricingConfig, AgentCommandError>,
}

impl Request {
    pub fn execute(self, client: &AgentClient) -> Completion {
        let request = match self.operation {
            Operation::Load => AgentRequest::ServerPricingConfigGet,
            Operation::Assign(a) => AgentRequest::ServerPricingAssignmentSet {
                entry_id: a.entry_id,
                secret_id: a.secret_id,
                group_id: a.group_id,
                multiplier: a.multiplier,
            },
            Operation::SaveGroup { group, apply_scope } => {
                AgentRequest::ServerPricingGroupUpsert { group, apply_scope }
            }
            Operation::DeleteGroup(group_id) => AgentRequest::ServerPricingGroupDelete { group_id },
            Operation::DeleteVersion {
                group_id,
                effective_from,
            } => AgentRequest::ServerPricingGroupVersionDelete {
                group_id,
                effective_from,
            },
        };
        Completion {
            scope: self.scope,
            result: client.request(&request),
        }
    }
}
