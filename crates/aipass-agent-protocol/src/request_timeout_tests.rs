use super::*;

/// An ordinary request must not be able to hang a caller indefinitely.
#[test]
fn ordinary_requests_use_the_default_response_timeout() {
    assert_eq!(
        AgentRequest::SessionStatus.response_timeout(),
        DEFAULT_RESPONSE_TIMEOUT
    );
    assert_eq!(
        AgentRequest::EntriesList { archived: false }.response_timeout(),
        DEFAULT_RESPONSE_TIMEOUT
    );
}

#[test]
fn tool_config_requests_use_the_long_response_timeout() {
    let request = AgentRequest::ToolConfigApply {
        request: ToolConfigRequest {
            tool: ToolConfigTool::Codex,
            id: uuid::Uuid::nil(),
            secret_id: None,
            mode: ToolConfigMode::Plaintext,
            codex_api_key_mode: None,
            preview_id: None,
        },
    };
    assert_eq!(request.response_timeout(), LONG_RESPONSE_TIMEOUT);
}
