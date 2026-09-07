use super::*;
use serde_json::{json, Value};

fn response_events() -> Vec<Value> {
    vec![
        json!({"type":"response.created","sequence_number":0,"response":{"id":"PRIVATE_RESPONSE_ID"}}),
        // Deliberately reproduce the event ordering seen in the Codex logs.
        json!({"type":"response.output_text.delta","sequence_number":1,"output_index":0,"delta":"PRIVATE_OUTPUT"}),
        json!({"type":"response.completed","sequence_number":2,"response":{"id":"PRIVATE_RESPONSE_ID","status":"completed","output":[]}}),
    ]
}

fn tool_request() -> Value {
    json!({"model":"PRIVATE_MODEL","input":"PRIVATE_PROMPT","stream":true,
        "tools":[{"type":"custom","name":"exec","description":"PRIVATE_TOOL_DESCRIPTION","format":{"type":"text"}}]})
}

async fn logs_with(store: &UsageStore, marker: &str) -> Vec<ProxyLogEntry> {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let logs = store.logs().unwrap();
            if logs.iter().any(|entry| entry.message.contains(marker)) {
                return logs;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap()
}

fn assert_summaries(logs: &[ProxyLogEntry], transport: &str) {
    let summary = logs
        .iter()
        .find(|entry| entry.message.contains("event=proxy.responses.summary"))
        .unwrap();
    assert_eq!(summary.level, "warn");
    assert!(summary.message.contains(transport));
    assert!(summary.message.contains("delta_without_item=1"));
    assert!(summary.message.contains("terminal=1"));
    let request_id = summary
        .message
        .split_whitespace()
        .find(|field| field.starts_with("request_id="))
        .unwrap();
    assert!(logs
        .iter()
        .any(|entry| entry.message.contains("event=proxy.tools.summary")
            && entry.message.contains(request_id)
            && entry.message.contains("custom=1")
            && entry.message.contains("exec=1")));
    assert_eq!(
        logs.iter()
            .filter(|entry| entry.message.contains("event=proxy.responses.summary"))
            .count(),
        1
    );
    assert!(!logs.iter().any(|entry| entry.message.contains("PRIVATE")
        || entry.message.contains(UPSTREAM_KEY)
        || entry.message.contains(LOCAL_TOKEN)));
}

#[tokio::test]
async fn http_ws_adaptation_matches_native_request_headers_and_payload() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let upstream_captured = captured.clone();
    let server = tokio::spawn(async move {
        for _ in 0..2 {
            let (stream, _) = listener.accept().await.unwrap();
            let mut headers = HeaderMap::new();
            let mut socket = accept_hdr_async(stream, |request: &Request<()>, response| {
                headers = request.headers().clone();
                headers.remove(header::SEC_WEBSOCKET_KEY);
                Ok(response)
            })
            .await
            .unwrap();
            let request = receive(&mut socket).await;
            let payload: Value = serde_json::from_str(request.to_text().unwrap()).unwrap();
            upstream_captured.lock().unwrap().push((headers, payload));
            socket.send(event(json!({"type":"response.completed","response":{"id":"resp_test","status":"completed","output":[]}}))).await.unwrap();
        }
    });
    let (handle, _, _dir) = start_proxy(vec![test_route(format!("http://{addr}"))], direct());
    let mut body = json!({"model":"test","instructions":"Reply with OK only.","input":[{"role":"user","content":[{"type":"input_text","text":"Connection check."}]}],"store":false,"stream":true});
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://{}/v1/responses", handle.bind_addr))
        .bearer_auth(LOCAL_TOKEN)
        .header("user-agent", "codex_cli_rs/1.0.0")
        .header("originator", "codex_cli_rs")
        .json(&body)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(response.contains("response.completed"));
    body.as_object_mut().unwrap().remove("stream");
    body["type"] = json!("response.create");
    let mut socket = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    socket.send(event(body.clone())).await.unwrap();
    assert!(receive(&mut socket)
        .await
        .to_text()
        .unwrap()
        .contains("response.completed"));
    server.await.unwrap();
    let requests = captured.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].1, body);
    assert_eq!(requests[0], requests[1]);
}

#[tokio::test]
async fn websocket_diagnostics_preserve_frames_and_correlate_tools_events_and_close() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut request = tool_request();
    request["type"] = json!("response.create");
    let expected = request.clone();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        let incoming = receive(&mut ws).await;
        assert_eq!(
            serde_json::from_str::<Value>(incoming.to_text().unwrap()).unwrap(),
            expected
        );
        for value in response_events() {
            ws.send(event(value)).await.unwrap();
        }
        ws.close(Some(CloseFrame {
            code: CloseCode::Normal,
            reason: "PRIVATE_CLOSE_REASON".into(),
        }))
        .await
        .unwrap();
    });
    let (handle, store, _dir) = start_proxy(vec![test_route(format!("http://{addr}"))], direct());
    let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    ws.send(event(request)).await.unwrap();
    for expected in response_events() {
        assert_eq!(
            serde_json::from_str::<Value>(receive(&mut ws).await.to_text().unwrap()).unwrap(),
            expected
        );
    }
    assert!(receive(&mut ws).await.is_close());
    let logs = logs_with(&store, "event=proxy.websocket.closed").await;
    assert_summaries(&logs, "transport=websocket_upstream");
    assert!(logs.iter().any(|entry| entry
        .message
        .contains("reason=upstream_close close_code=Some(1000)")));
    server.await.unwrap();
}

#[tokio::test]
async fn http_diagnostics_preserve_tools_and_observe_native_responses_sse() {
    let (address, mut calls, server) = mock_http_upstream().await;
    let (handle, store, _dir) = start_proxy(vec![test_route(address)], direct());
    let url = format!("http://{}/v1/responses", handle.bind_addr);
    let client = tokio::spawn(async move {
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .post(url)
            .bearer_auth(LOCAL_TOKEN)
            .json(&tool_request())
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap()
    });
    let call = next_http_call(&mut calls).await;
    assert_eq!(call.body, tool_request());
    reply_sse(call, response_events());
    let response = client.await.unwrap();
    assert!(response.contains("PRIVATE_OUTPUT"));
    let logs = logs_with(&store, "event=proxy.responses.summary").await;
    assert_summaries(&logs, "transport=http_sse_upstream");
    server.abort();
}

#[tokio::test]
async fn conversion_diagnostics_describe_the_actual_upstream_tools() {
    let (address, mut calls, server) = mock_http_upstream().await;
    let (handle, store, _dir) = start_proxy(vec![converted_route(address)], direct());
    let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    let mut request = tool_request();
    request["tools"] = json!([{"type":"function","name":"exec","parameters":{"type":"object"}}]);
    request["type"] = json!("response.create");
    ws.send(event(request)).await.unwrap();
    let call = next_http_call(&mut calls).await;
    let forwarded_count = call
        .body
        .get("tools")
        .and_then(Value::as_array)
        .map(Vec::len);
    reply_sse(call, anthropic_text_events());
    receive_response(&mut ws).await;
    let logs = logs_with(&store, "stage=converted_upstream").await;
    let before = logs
        .iter()
        .find(|entry| entry.message.contains("stage=ws_bridge_prepared"))
        .unwrap();
    let after = logs
        .iter()
        .find(|entry| entry.message.contains("stage=converted_upstream"))
        .unwrap();
    assert!(before.message.contains("function=1"));
    assert!(after.message.contains(&format!(
        "tools_present={} tools={}",
        forwarded_count.is_some(),
        forwarded_count.unwrap_or_default()
    )));
    let id = before
        .message
        .split_whitespace()
        .find(|field| field.starts_with("request_id="))
        .unwrap();
    assert!(after.message.contains(id));
    assert!(!logs.iter().any(|entry| entry.message.contains("PRIVATE")));
    server.abort();
}
