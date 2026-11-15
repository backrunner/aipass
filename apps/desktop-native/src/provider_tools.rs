//! Provider maintenance reuses authenticated Agent operations. The UI never
//! reads credentials to probe an endpoint or writes the vault directly.
pub use crate::credentials::Scope;
use aipass_agent::{AgentClient, AgentCommandError};
use aipass_agent_protocol::{
    AgentErrorCode, AgentRequest, ProbeResult, SensitiveString, UsageProbeMode, UsageProbeResult,
    UsageProbeSource,
};
use aipass_provider_registry::{GatewayMetadata, QuotaInfo, SecretRef};
use aipass_vault::EntrySummary;

pub struct Request {
    pub scope: Scope,
    pub operation: Operation,
}
pub enum Operation {
    Probe,
    Usage {
        secret_id: Option<String>,
        mode: UsageProbeMode,
        base_url: Option<String>,
        access_token: Option<SensitiveString>,
        user_id: Option<String>,
    },
    Apply {
        entry: Box<EntrySummary>,
        result: Box<UsageProbeResult>,
    },
    Reset {
        secrets: Vec<SecretRef>,
        archived: bool,
    },
}
pub enum Outcome {
    Probe(ProbeResult),
    Usage(Box<UsageProbeResult>),
    Applied,
    Reset(Box<EntrySummary>),
}
pub struct Completion {
    pub scope: Scope,
    /// Also true after a partial reset. Refresh metadata even on failure.
    pub mutated: bool,
    pub result: Result<Outcome, AgentCommandError>,
}

impl Request {
    pub(crate) fn execute(self, client: &AgentClient) -> Completion {
        self.execute_with(|request| client.request::<serde_json::Value>(&request))
    }
    fn execute_with(
        self,
        mut call: impl FnMut(AgentRequest) -> Result<serde_json::Value, AgentCommandError>,
    ) -> Completion {
        let id = self.scope.entry;
        let mut mutated = false;
        let result = (|| match self.operation {
            Operation::Probe => decode(call(AgentRequest::ProviderProbe {
                id,
                timeout_seconds: 15,
            })?)
            .map(Outcome::Probe),
            Operation::Usage {
                secret_id,
                mode,
                base_url,
                access_token,
                user_id,
            } => decode(call(AgentRequest::ProviderUsageProbe {
                id,
                secret_id,
                mode,
                timeout_seconds: 15,
                base_url,
                access_token,
                user_id,
            })?)
            .map(|result| Outcome::Usage(Box::new(result))),
            Operation::Apply { entry, result } => {
                if entry.id != id || !can_apply(&result) {
                    return Err(conflict());
                }
                let current = current_entry(id, entry.archived_at.is_some(), &mut call)?;
                if current.secret_refs != entry.secret_refs
                    || current.endpoints != entry.endpoints
                    || current.provider_id != entry.provider_id
                    || current.interface_type != entry.interface_type
                {
                    return Err(conflict());
                }
                call(apply_request(&entry, &result))?;
                mutated = true;
                Ok(Outcome::Applied)
            }
            Operation::Reset { secrets, archived } => {
                // Keep the confirmation snapshot across retries. Missing IDs
                // were already removed; newly added keys are never included.
                for secret in &secrets {
                    let entry = current_entry(id, archived, &mut call)?;
                    if let Some(current) = entry.secret_refs.iter().find(|s| s.id == secret.id) {
                        if current.fingerprint != secret.fingerprint {
                            return Err(conflict());
                        }
                        call(AgentRequest::SecretRemove {
                            id,
                            label: secret.id.clone(),
                        })?;
                        mutated = true;
                    }
                }
                let entry = current_entry(id, archived, &mut call)?;
                if !entry.secret_refs.is_empty() {
                    return Err(conflict());
                }
                Ok(Outcome::Reset(Box::new(entry)))
            }
        })();
        Completion {
            scope: self.scope,
            mutated,
            result,
        }
    }
}
fn current_entry(
    id: uuid::Uuid,
    archived: bool,
    call: &mut impl FnMut(AgentRequest) -> Result<serde_json::Value, AgentCommandError>,
) -> Result<EntrySummary, AgentCommandError> {
    let entries: Vec<EntrySummary> = decode(call(AgentRequest::EntriesList { archived })?)?;
    entries
        .into_iter()
        .find(|entry| entry.id == id)
        .ok_or_else(conflict)
}
fn decode<T: serde::de::DeserializeOwned>(
    value: serde_json::Value,
) -> Result<T, AgentCommandError> {
    serde_json::from_value(value).map_err(|_| AgentCommandError {
        code: Some(AgentErrorCode::Internal),
        message: "Invalid Agent response".into(),
    })
}
fn conflict() -> AgentCommandError {
    AgentCommandError {
        code: Some(AgentErrorCode::Conflict),
        message: "Provider changed; reopen this action to review its current state".into(),
    }
}
pub fn can_apply(result: &UsageProbeResult) -> bool {
    result.ok
        && (result.quota.as_ref().is_some_and(|q| {
            [&q.label, &q.limit, &q.used, &q.remaining, &q.reset_at]
                .iter()
                .any(|v| v.as_ref().is_some_and(|v| !v.is_empty()))
        }) || result.gateway.as_ref().is_some_and(|g| {
            [&g.group, &g.rate]
                .iter()
                .any(|v| v.as_ref().is_some_and(|v| !v.is_empty()))
        }) || result.subscription.is_some())
}
pub fn source_label(source: UsageProbeSource) -> &'static str {
    match source {
        UsageProbeSource::NewApiTokenUsage => "providerDetail.usageSourceNewApiToken",
        UsageProbeSource::NewApiUserSelf => "providerDetail.usageSourceNewApiUser",
        UsageProbeSource::SubApiV1Usage => "providerDetail.usageSourceSubApi",
        UsageProbeSource::Unknown => "providerDetail.usageSourceUnknown",
    }
}
fn apply_request(entry: &EntrySummary, result: &UsageProbeResult) -> AgentRequest {
    let clear = result.source == UsageProbeSource::SubApiV1Usage;
    let quota = result
        .quota
        .as_ref()
        .map(|q| QuotaInfo {
            label: q.label.clone(),
            unit: q.unit.clone(),
            limit: q.limit.clone(),
            used: q.used.clone(),
            remaining: q.remaining.clone(),
            reset_at: q.reset_at.clone(),
        })
        .or_else(|| {
            if clear {
                Some(QuotaInfo {
                    label: None,
                    unit: None,
                    limit: None,
                    used: None,
                    remaining: None,
                    reset_at: None,
                })
            } else {
                entry.quota.clone()
            }
        });
    let quota = quota.filter(|q| {
        clear
            || [&q.label, &q.limit, &q.used, &q.remaining, &q.reset_at]
                .iter()
                .any(|v| v.as_ref().is_some_and(|v| !v.is_empty()))
    });
    let gateway = GatewayMetadata {
        group: result
            .gateway
            .as_ref()
            .and_then(|g| g.group.clone())
            .or_else(|| {
                if clear {
                    None
                } else {
                    entry.gateway.as_ref().and_then(|g| g.group.clone())
                }
            }),
        rate: result
            .gateway
            .as_ref()
            .and_then(|g| g.rate.clone())
            .or_else(|| {
                if clear {
                    None
                } else {
                    entry.gateway.as_ref().and_then(|g| g.rate.clone())
                }
            }),
    };
    let gateway = (clear
        || [&gateway.group, &gateway.rate]
            .iter()
            .any(|v| v.as_ref().is_some_and(|v| !v.is_empty())))
    .then_some(gateway);
    AgentRequest::ProviderUsageApply {
        id: entry.id,
        quota,
        gateway,
        source: (result.source != UsageProbeSource::Unknown).then_some(result.source),
        subscription: result.subscription.clone(),
    }
}

#[cfg(test)]
mod tests;
