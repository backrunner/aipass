//! Token-free contracts for device-local subscription discovery.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscriptionImportInput {
    #[serde(default)]
    pub provider_ids: Vec<String>,
    #[serde(default)]
    pub sources: Vec<SubscriptionImportSource>,
    /// Retry only these sources from a completed task, without rescanning successes.
    #[serde(default)]
    pub retry: Option<SubscriptionImportRetry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscriptionImportSource {
    pub provider: String,
    /// Provider configuration directory (Gemini uses its home root).
    pub root: PathBuf,
    #[serde(default)]
    pub selector: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscriptionImportRetry {
    pub ticket: Uuid,
    pub source_ids: Vec<Uuid>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionImportStatus {
    Imported,
    Existing,
    Updated,
    NeedsLogin,
    NotFound,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionImportResult {
    pub source_id: Uuid,
    pub source: SubscriptionImportSource,
    pub account_identity: Option<String>,
    pub status: SubscriptionImportStatus,
    pub entry_id: Option<Uuid>,
    /// Stable sanitized code; no vendor response text or subprocess output.
    pub error_code: Option<String>,
    /// login, install_cli, choose_directory, retry, or none.
    pub action: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionImportTask {
    pub ticket: Uuid,
    /// discovering, importing, complete, or cancelled.
    pub phase: String,
    pub total: usize,
    pub completed: usize,
    pub results: Vec<SubscriptionImportResult>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AgentRequest;

    #[test]
    fn import_tags_and_legacy_json_remain_stable() {
        let ticket = Uuid::nil();
        for request in [
            AgentRequest::SubscriptionImportStart {
                input: Default::default(),
            },
            AgentRequest::SubscriptionImportPoll { ticket },
            AgentRequest::SubscriptionImportCancel { ticket },
        ] {
            let value = serde_json::to_value(&request).unwrap();
            assert_eq!(value["type"], request.event_name());
            assert_eq!(
                serde_json::from_value::<AgentRequest>(value)
                    .unwrap()
                    .event_name(),
                request.event_name()
            );
        }
        let old: AgentRequest =
            serde_json::from_str(r#"{"type":"official_accounts.refresh"}"#).unwrap();
        assert_eq!(old.event_name(), "official_accounts.refresh");
    }

    #[test]
    fn import_json_accepts_sources_and_retry_but_rejects_credential_payloads() {
        let request: AgentRequest = serde_json::from_value(serde_json::json!({
            "type":"subscription.import.start", "input":{
                "providerIds":["gemini-cli"],
                "sources":[{"provider":"gemini-cli","root":"/fixture/home"}],
                "retry":{"ticket":Uuid::nil(),"sourceIds":[Uuid::nil()]}
            }
        }))
        .unwrap();
        let AgentRequest::SubscriptionImportStart { input } = request else {
            panic!()
        };
        assert_eq!(input.sources[0].selector, "");
        assert_eq!(input.retry.unwrap().source_ids, vec![Uuid::nil()]);
        assert!(
            serde_json::from_value::<SubscriptionImportSource>(serde_json::json!({
                "provider":"cursor","root":"/fixture","accessToken":"fixture-private"
            }))
            .is_err()
        );
        let old = crate::OfficialAccountRefreshResult {
            provider_id: "codex".into(),
            account_identity: Some("alice:workspace".into()),
            credential_kind: aipass_provider_registry::CredentialKind::OAuth,
            snapshot: None,
            status: "imported".into(),
            error: None,
        };
        assert_eq!(
            serde_json::to_value(old).unwrap(),
            serde_json::json!({
                "providerId":"codex","accountIdentity":"alice:workspace","credentialKind":"oauth",
                "snapshot":null,"status":"imported","error":null
            })
        );
    }
}
