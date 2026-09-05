use crate::logging::{write_component_log, RequestScope, AGENT_LOG};
use aipass_agent_protocol::{AgentRequest, AgentResponse};
use std::time::Instant;
use uuid::Uuid;

pub(crate) struct OperationLog {
    event: &'static str,
    resource_id: Option<Uuid>,
    poll: bool,
    started: Instant,
    finished: bool,
    _scope: RequestScope,
}

impl OperationLog {
    pub(crate) fn background(event: &'static str) -> Option<Self> {
        if crate::logging::current_request_id().is_some() {
            return None;
        }
        let log = Self {
            event,
            resource_id: None,
            poll: false,
            started: Instant::now(),
            finished: false,
            _scope: RequestScope::new(Uuid::new_v4()),
        };
        log.write("INFO", "started", "source=background");
        Some(log)
    }

    pub(crate) fn start(request: &AgentRequest) -> Self {
        let log = Self {
            event: request.event_name(),
            resource_id: resource_id(request),
            poll: request.is_background_poll(),
            started: Instant::now(),
            finished: false,
            _scope: RequestScope::new(
                crate::logging::current_request_id().unwrap_or_else(Uuid::new_v4),
            ),
        };
        if !log.poll {
            log.write("INFO", "started", "");
        }
        log
    }

    pub(crate) fn finish(mut self, response: &AgentResponse) {
        self.finished = true;
        if response.ok {
            // Only decode known UUID locations, never stringify response data.
            let created = match self.event {
                "provider.add" => response.data.as_str(),
                "browser.save_detected" => response.data["entryId"].as_str(),
                _ => None,
            };
            if let Some(id) = created.and_then(|id| Uuid::parse_str(id).ok()) {
                self.resource_id = Some(id);
            }
        }
        let semantic_failure = response.data.get("ok").and_then(|v| v.as_bool()) == Some(false)
            || matches!(
                response.data.get("status").and_then(|v| v.as_str()),
                Some("offline" | "auth_failed" | "server_error" | "expired" | "error")
            );
        let successful = response.ok && !semantic_failure;
        let mut detail = format!("code={:?}", response.code);
        // Status names and counters are allowlisted; no arbitrary response text.
        if let Some(status) = response
            .data
            .get("status")
            .and_then(|v| v.as_str())
            .filter(|value| {
                matches!(
                    *value,
                    "idle"
                        | "syncing"
                        | "conflict"
                        | "offline"
                        | "auth_failed"
                        | "server_error"
                        | "pending"
                        | "authorized"
                        | "expired"
                        | "error"
                )
            })
        {
            detail.push_str(&format!(" status={status}"));
        }
        for key in [
            "status",
            "uploaded",
            "downloaded",
            "conflicts",
            "quarantined",
        ] {
            if let Some(value) = response.data.get(key).and_then(|v| v.as_u64()) {
                detail.push_str(&format!(" {key}={value}"));
            }
        }
        if !self.poll || !successful {
            self.write(
                if successful { "INFO" } else { "WARN" },
                if successful { "completed" } else { "failed" },
                &detail,
            );
        }
    }

    fn write(&self, level: &str, outcome: &str, detail: &str) {
        write_component_log(
            AGENT_LOG,
            level,
            &format!(
                "event={} outcome={outcome} resource_id={} elapsed_ms={} {detail}",
                self.event,
                self.resource_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "none".into()),
                self.started.elapsed().as_millis(),
            ),
        );
    }
}

impl Drop for OperationLog {
    fn drop(&mut self) {
        if !self.finished {
            self.write("ERROR", "interrupted", "");
        }
    }
}

fn resource_id(request: &AgentRequest) -> Option<Uuid> {
    match request {
        AgentRequest::ProviderGet { id }
        | AgentRequest::ProviderUpdate { id, .. }
        | AgentRequest::ProviderArchive { id }
        | AgentRequest::ProviderRestore { id }
        | AgentRequest::ProviderTrash { id }
        | AgentRequest::ProviderFavorite { id, .. }
        | AgentRequest::ProviderDelete { id }
        | AgentRequest::SecretRevealField { id, .. }
        | AgentRequest::SecretRevealHeaders { id }
        | AgentRequest::SecretAdd { id, .. }
        | AgentRequest::SecretUpdate { id, .. }
        | AgentRequest::SecretMetadataSet { id, .. }
        | AgentRequest::SecretRemove { id, .. }
        | AgentRequest::DeviceRevoke { id }
        | AgentRequest::ProviderProbe { id, .. }
        | AgentRequest::ProviderUsageProbe { id, .. }
        | AgentRequest::ProviderUsageApply { id, .. }
        | AgentRequest::ServerPricingRemoteSync { id, .. } => Some(*id),
        AgentRequest::ServerRouteSelect { route_id }
        | AgentRequest::ServerRouteSetEnabled { route_id, .. }
        | AgentRequest::ServerTokenRotate { route_id } => Some(*route_id),
        AgentRequest::ToolConfigPreview { request } | AgentRequest::ToolConfigApply { request } => {
            Some(request.id)
        }
        AgentRequest::ToolConfigProxyPreview { request }
        | AgentRequest::ToolConfigProxyApply { request } => Some(request.route_id),
        AgentRequest::ToolConfigRollback { operation_id } => Some(*operation_id),
        AgentRequest::OAuthAccountsRemove { account_id, .. }
        | AgentRequest::OAuthAccountsSetDefault { account_id, .. } => Some(*account_id),
        AgentRequest::ServerPricingAssignmentSet { entry_id, .. } => Some(*entry_id),
        AgentRequest::ServerPricingGroupDelete { group_id }
        | AgentRequest::ServerPricingGroupVersionDelete { group_id, .. } => Some(*group_id),
        AgentRequest::ServerPricingGroupUpsert { group, .. } => Some(group.id),
        AgentRequest::BrowserSecretFill { entry_id, .. } => *entry_id,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_errors_and_unwinding_are_logged_without_payloads() {
        let temp = tempfile::tempdir().unwrap();
        crate::logging::with_test_log_dir(temp.path(), || {
            OperationLog::start(&AgentRequest::SyncConfigured).finish(&AgentResponse::success(
                serde_json::json!({"status":"auth_failed", "message":"private-sync-password", "uploaded":0}),
            ));
            OperationLog::start(&AgentRequest::ProviderProbe {
                id: Uuid::new_v4(),
                timeout_seconds: 1,
            })
            .finish(&AgentResponse::success(
                serde_json::json!({"ok":false, "status":401, "error":"private-api-key"}),
            ));
            let _ = std::panic::catch_unwind(|| {
                let _operation =
                    OperationLog::start(&AgentRequest::ProviderDelete { id: Uuid::new_v4() });
                panic!("test interruption");
            });
        });
        let text = std::fs::read_to_string(temp.path().join("agent.log")).unwrap();
        assert!(text.contains("event=sync.configured outcome=failed"));
        assert!(text.contains("status=auth_failed"));
        assert!(text.contains("event=provider.probe outcome=failed"));
        assert!(text.contains("status=401"));
        assert!(text.contains("event=provider.delete outcome=interrupted"));
        assert!(!text.contains("private-"));
    }
}
