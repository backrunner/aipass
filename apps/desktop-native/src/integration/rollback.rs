//! The Agent returns a native-account switch or a generic file restoration.
//! Keep both existing wire contracts without manufacturing an apply response.
use super::Outcome;
use aipass_agent_protocol::ToolConfigApplyResponse;
use aipass_config_writers::ApplyResult;

#[derive(serde::Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub(super) enum Response {
    Native(ToolConfigApplyResponse),
    Files {
        operation_id: uuid::Uuid,
        target_path: std::path::PathBuf,
        backup_path: std::path::PathBuf,
    },
}

impl From<Response> for Outcome {
    fn from(response: Response) -> Self {
        match response {
            Response::Native(result) => Self::Applied(result),
            Response::Files {
                operation_id,
                target_path,
                backup_path,
            } => Self::Restored(ApplyResult {
                operation_id,
                target_path,
                backup_path,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aipass_agent_protocol::{ToolConfigMode, ToolConfigOutcome, ToolConfigTool};

    #[test]
    fn rollback_accepts_file_restoration_and_preserves_native_attention_states() {
        let id = uuid::Uuid::new_v4();
        let files = ApplyResult {
            operation_id: id,
            target_path: "fixture/config.json".into(),
            backup_path: "fixture/backup".into(),
        };
        let response: Response =
            serde_json::from_value(serde_json::to_value(files).unwrap()).unwrap();
        assert!(
            matches!(Outcome::from(response), Outcome::Restored(result) if result.operation_id == id)
        );
        for outcome in [
            ToolConfigOutcome::Applied,
            ToolConfigOutcome::Conflict,
            ToolConfigOutcome::LoginRequired,
        ] {
            let native = ToolConfigApplyResponse {
                outcome: outcome.clone(),
                message: None,
                tool: ToolConfigTool::Codex,
                mode: ToolConfigMode::Official,
                entry_id: id,
                entry_title: "fixture".into(),
                operation_id: id,
                target_path: "fixture/config.json".into(),
                backup_path: "fixture/backup".into(),
                summary: "fixture".into(),
            };
            let response: Response =
                serde_json::from_value(serde_json::to_value(native).unwrap()).unwrap();
            assert!(
                matches!(Outcome::from(response), Outcome::Applied(result) if result.outcome == outcome)
            );
        }
        assert!(serde_json::from_str::<Response>(r#"{"ok":true}"#).is_err());
        assert!(serde_json::from_value::<Response>(serde_json::json!({
            "operation_id": id, "target_path": "fixture", "backup_path": "fixture",
            "outcome": "conflict"
        }))
        .is_err());
    }
}
