use crate::commands::agent_request_async;
use crate::models::{
    from_agent_tool_config_apply, from_agent_tool_config_preview, into_agent_tool_config_request,
    into_tool_id, ToolConfigApplyResponse, ToolConfigPreviewResponse, ToolConfigRequest,
    ToolConfigTool,
};
use aipass_agent_protocol::{
    AgentRequest, ToolConfigApplyResponse as AgentToolConfigApplyResponse,
    ToolConfigPreviewResponse as AgentToolConfigPreviewResponse,
    ToolConfigProxyRequest as AgentToolConfigProxyRequest,
};
use tauri::AppHandle;
use uuid::Uuid;
#[tauri::command]
pub(crate) async fn tool_config_preview(
    app: AppHandle,
    request: ToolConfigRequest,
) -> Result<ToolConfigPreviewResponse, String> {
    let response: AgentToolConfigPreviewResponse = agent_request_async(
        app,
        AgentRequest::ToolConfigPreview {
            request: into_agent_tool_config_request(request),
        },
    )
    .await?;
    Ok(from_agent_tool_config_preview(response))
}

#[tauri::command]
pub(crate) async fn tool_config_apply(
    app: AppHandle,
    request: ToolConfigRequest,
) -> Result<ToolConfigApplyResponse, String> {
    let response: AgentToolConfigApplyResponse = agent_request_async(
        app,
        AgentRequest::ToolConfigApply {
            request: into_agent_tool_config_request(request),
        },
    )
    .await?;
    Ok(from_agent_tool_config_apply(response))
}

#[tauri::command]
pub(crate) async fn tool_config_proxy_preview(
    app: AppHandle,
    tool: ToolConfigTool,
    route_id: Uuid,
) -> Result<ToolConfigPreviewResponse, String> {
    let response: AgentToolConfigPreviewResponse = agent_request_async(
        app,
        AgentRequest::ToolConfigProxyPreview {
            request: AgentToolConfigProxyRequest {
                tool: into_tool_id(tool),
                route_id,
            },
        },
    )
    .await?;
    Ok(from_agent_tool_config_preview(response))
}

#[tauri::command]
pub(crate) async fn tool_config_proxy_apply(
    app: AppHandle,
    tool: ToolConfigTool,
    route_id: Uuid,
) -> Result<ToolConfigApplyResponse, String> {
    let response: AgentToolConfigApplyResponse = agent_request_async(
        app,
        AgentRequest::ToolConfigProxyApply {
            request: AgentToolConfigProxyRequest {
                tool: into_tool_id(tool),
                route_id,
            },
        },
    )
    .await?;
    Ok(from_agent_tool_config_apply(response))
}

#[tauri::command]
pub(crate) async fn tool_config_status(
    app: AppHandle,
    tool: aipass_agent_protocol::ToolConfigTool,
) -> Result<aipass_agent_protocol::ToolConfigStatus, String> {
    agent_request_async(app, AgentRequest::ToolConfigStatus { tool }).await
}
#[tauri::command]
pub(crate) async fn tool_config_rollback(
    app: AppHandle,
    operation_id: uuid::Uuid,
) -> Result<aipass_agent_protocol::ToolConfigApplyResponse, String> {
    agent_request_async(app, AgentRequest::ToolConfigRollback { operation_id }).await
}
#[tauri::command]
pub(crate) async fn tool_config_login_start(
    app: AppHandle,
    request: ToolConfigRequest,
) -> Result<aipass_agent_protocol::ToolConfigLoginStatus, String> {
    agent_request_async(
        app,
        AgentRequest::ToolConfigLoginStart {
            request: into_agent_tool_config_request(request),
        },
    )
    .await
}
#[tauri::command]
pub(crate) async fn tool_config_login_poll(
    app: AppHandle,
    ticket: uuid::Uuid,
) -> Result<aipass_agent_protocol::ToolConfigLoginStatus, String> {
    agent_request_async(app, AgentRequest::ToolConfigLoginPoll { ticket }).await
}
#[tauri::command]
pub(crate) async fn tool_config_login_code(
    app: AppHandle,
    ticket: uuid::Uuid,
    code: String,
) -> Result<(), String> {
    agent_request_async(
        app,
        AgentRequest::ToolConfigLoginCode {
            ticket,
            code: aipass_agent_protocol::SensitiveString::new(code),
        },
    )
    .await
}
#[tauri::command]
pub(crate) async fn tool_config_login_cancel(
    app: AppHandle,
    ticket: uuid::Uuid,
) -> Result<(), String> {
    crate::commands::agent_request_no_unlock_async(
        app,
        AgentRequest::ToolConfigLoginCancel { ticket },
    )
    .await
}
