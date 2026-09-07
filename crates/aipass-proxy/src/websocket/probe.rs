use super::*;
use serde_json::json;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebsocketProbeResult {
    /// None means inconclusive (e.g. auth, quota, timeout, or no model).
    pub supported: Option<bool>,
    pub status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Probe with no user input and generate=false. Share routing, proxy settings,
/// credentials, URL construction and handshake validation with live traffic.
pub fn probe_websocket(
    target: ResolvedTarget,
    outbound: &UpstreamProxyConfig,
    timeout: Duration,
    model: Option<&str>,
) -> WebsocketProbeResult {
    let unknown = WebsocketProbeResult {
        supported: None,
        status: None,
        error: Some("WS probe timed out or transport could not be established".into()),
    };
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return unknown;
    };
    runtime.block_on(async {
        tokio::time::timeout(timeout, probe(&target, outbound, timeout, model))
            .await
            .unwrap_or(unknown)
    })
}

async fn probe(
    target: &ResolvedTarget,
    outbound: &UpstreamProxyConfig,
    timeout: Duration,
    model: Option<&str>,
) -> WebsocketProbeResult {
    let unknown = WebsocketProbeResult {
        supported: None,
        status: None,
        error: Some("WS probe timed out or transport could not be established".into()),
    };
    let builder = reqwest::Client::builder()
        .http1_only()
        .connect_timeout(timeout)
        .redirect(reqwest::redirect::Policy::none());
    let Ok(client) = apply_upstream_proxy(builder, outbound)
        .and_then(|builder| builder.build().map_err(|e| e.to_string()))
    else {
        return unknown;
    };
    let (upgraded, _) = match connect_upstream(&client, &HeaderMap::new(), None, target, None).await
    {
        Ok(connected) => connected,
        Err(error) => {
            let status = error.status();
            return WebsocketProbeResult {
                // HTTP auth/quota and transient server failures do not prove absence.
                supported: matches!(
                    status,
                    StatusCode::NOT_FOUND
                        | StatusCode::METHOD_NOT_ALLOWED
                        | StatusCode::NOT_IMPLEMENTED
                )
                .then_some(false),
                status: Some(status.as_u16()),
                error: Some(format!(
                    "WS handshake rejected with HTTP {}",
                    status.as_u16()
                )),
            };
        }
    };
    let unknown = WebsocketProbeResult {
        supported: None,
        status: Some(101),
        error: Some("WS connected but no valid empty Responses completion was received".into()),
    };
    let Some(model) = model.filter(|model| !model.trim().is_empty()) else {
        return WebsocketProbeResult {
            error: Some(
                "a default model or a model from discovery is required for WS validation".into(),
            ),
            ..unknown
        };
    };
    let config = WebSocketConfig::default()
        .max_message_size(Some(1024 * 1024))
        .max_frame_size(Some(1024 * 1024));
    let mut socket = WebSocketStream::from_raw_socket(upgraded, Role::Client, Some(config)).await;
    if socket
        .send(Message::text(
            json!({
                "type":"response.create", "model":model, "generate":false,
                "input":[], "store":false,
            })
            .to_string(),
        ))
        .await
        .is_err()
    {
        return unknown;
    }
    loop {
        match socket.next().await {
            Some(Ok(Message::Text(text))) => {
                let Ok(event) = serde_json::from_str::<serde_json::Value>(&text) else {
                    return unknown;
                };
                match event["type"].as_str() {
                    Some("response.completed")
                        if capability::valid_completion(&event)
                            && event.pointer("/response/output").is_some_and(|output| {
                                output.as_array().is_some_and(Vec::is_empty)
                            }) =>
                    {
                        return WebsocketProbeResult {
                            supported: Some(true),
                            status: Some(101),
                            error: None,
                        };
                    }
                    Some("error" | "response.failed" | "response.incomplete") => return unknown,
                    Some("response.created" | "response.in_progress") => {}
                    // Auxiliary notifications (e.g. codex.rate_limits) are not
                    // generation output or evidence of Responses support.
                    Some(kind) if !kind.trim().is_empty() && !kind.starts_with("response.") => {}
                    _ => return unknown,
                }
            }
            Some(Ok(Message::Ping(_))) => {
                if socket.flush().await.is_err() {
                    return unknown;
                }
            }
            Some(Ok(Message::Pong(_))) => {}
            _ => return unknown,
        }
    }
}
