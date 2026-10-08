//! Thin authenticated Agent adapters for local account imports.
use crate::commands::agent_request_no_unlock_async;
use aipass_agent_protocol::*;
use tauri::AppHandle;
use uuid::Uuid;

#[tauri::command]
pub(crate) async fn subscription_import_start(
    app: AppHandle,
    input: SubscriptionImportInput,
) -> Result<SubscriptionImportTask, String> {
    agent_request_no_unlock_async(app, AgentRequest::SubscriptionImportStart { input }).await
}
#[tauri::command]
pub(crate) async fn subscription_import_poll(
    app: AppHandle,
    ticket: Uuid,
) -> Result<SubscriptionImportTask, String> {
    agent_request_no_unlock_async(app, AgentRequest::SubscriptionImportPoll { ticket }).await
}
#[tauri::command]
pub(crate) async fn subscription_import_cancel(
    app: AppHandle,
    ticket: Uuid,
) -> Result<SubscriptionImportTask, String> {
    agent_request_no_unlock_async(app, AgentRequest::SubscriptionImportCancel { ticket }).await
}

#[tauri::command]
pub(crate) async fn official_accounts_refresh(
    app: AppHandle,
    provider_ids: Option<Vec<String>>,
) -> Result<Vec<OfficialAccountRefreshResult>, String> {
    agent_request_no_unlock_async(
        app,
        AgentRequest::OfficialAccountsRefresh {
            provider_ids: provider_ids.unwrap_or_default(),
        },
    )
    .await
}
