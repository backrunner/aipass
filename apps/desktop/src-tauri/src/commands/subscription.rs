use super::*;
#[tauri::command]
pub(crate) async fn community_catalog(app: AppHandle) -> Result<serde_json::Value, String> {
    agent_request_async(app, AgentRequest::CommunityCatalog).await
}
#[tauri::command]
pub(crate) async fn community_login_start(
    app: AppHandle,
    input: aipass_agent_protocol::CommunityLoginInput,
) -> Result<aipass_agent_protocol::CommunityLoginStatus, String> {
    agent_request_async(app, AgentRequest::CommunityLoginStart { input }).await
}
#[tauri::command]
pub(crate) async fn community_login_poll(
    app: AppHandle,
    ticket: Uuid,
) -> Result<aipass_agent_protocol::CommunityLoginStatus, String> {
    agent_request_no_unlock_async(app, AgentRequest::CommunityLoginPoll { ticket }).await
}
#[tauri::command]
pub(crate) async fn community_login_code(
    app: AppHandle,
    ticket: Uuid,
    code: SensitiveString,
) -> Result<(), String> {
    agent_request_no_unlock_async(app, AgentRequest::CommunityLoginCode { ticket, code }).await
}
#[tauri::command]
pub(crate) async fn community_login_cancel(app: AppHandle, ticket: Uuid) -> Result<(), String> {
    agent_request_no_unlock_async(app, AgentRequest::CommunityLoginCancel { ticket }).await
}
#[tauri::command]
pub(crate) async fn community_refresh(
    app: AppHandle,
    id: Uuid,
) -> Result<SubscriptionSnapshot, String> {
    agent_request_async(app, AgentRequest::CommunityRefresh { entry_id: id }).await
}

#[tauri::command]
pub(crate) async fn subscription_plugin(
    app: AppHandle,
    operation: aipass_agent_protocol::SubscriptionPluginAction,
) -> Result<serde_json::Value, String> {
    agent_request_async(app, AgentRequest::SubscriptionPlugin { operation }).await
}
