//! Controlled storage failures for typed switch/status results.
use super::*;
fn storage(error: &ServiceError) -> bool {
    error.code != AgentErrorCode::Locked
        && [
            "credential store",
            "credential write",
            "credential file",
            "backup",
            "transaction",
            "directory",
            "symlink",
            "Codex encrypted secrets",
            "unsupported on this platform",
        ]
        .iter()
        .any(|word| error.message.contains(word))
}
pub(super) fn apply_error(
    request: &ToolConfigRequest,
    error: ServiceError,
) -> ServiceResult<ToolConfigApplyResponse> {
    if storage(&error) {
        Ok(outcome(request, ToolConfigOutcome::StorageUnavailable,
        Some("Credential storage or encrypted backup unavailable. Switch was not completed; resolve any pending recovery before retrying.".into())))
    } else {
        Err(error)
    }
}
pub(super) fn status_error(
    tool: ToolConfigTool,
    error: ServiceError,
) -> ServiceResult<ToolConfigStatus> {
    if !storage(&error) {
        return Err(error);
    }
    Ok(ToolConfigStatus {
        tool,
        state: "storage_unavailable".into(),
        entry_title: None,
        entry_id: None,
        secret_id: None,
        mode: None,
        account_identity: None,
        operation_id: None,
        message: Some("Credential storage unavailable; no credentials were changed".into()),
        overrides: Vec::new(),
    })
}
