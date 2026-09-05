// Tungstenite's handshake callback requires Response<Option<String>> as Err.
#![allow(clippy::result_large_err)]

use super::*;
use tokio::net::TcpStream;
use tokio_tungstenite::{accept_hdr_async, client_async, tungstenite::client::IntoClientRequest};

const LOCAL_TOKEN: &str = "local-ws-test-token";
const UPSTREAM_KEY: &str = "upstream-ws-test-key";

fn test_route(base_url: String) -> ResolvedRoute {
    let target = ResolvedTarget {
        config: ProxyTargetConfig {
            id: Uuid::new_v4(),
            provider_entry_id: Uuid::new_v4(),
            secret_id: "primary".into(),
            label: "test".into(),
            base_url,
            auth_scheme: "bearer".into(),
            headers: vec![("chatgpt-account-id".into(), "test-account".into())],
            group: None,
            priority: 0,
            weight: 1,
            enabled: true,
            protocol: None,
        },
        api_key: UPSTREAM_KEY.into(),
    };
    ResolvedRoute {
        config: ProxyRouteConfig {
            id: Uuid::new_v4(),
            name: "ws".into(),
            token: LOCAL_TOKEN.into(),
            inbound_protocol: ProxyProtocol::OpenAiResponses,
            upstream_protocol: ProxyProtocol::OpenAiResponses,
            conversion_enabled: false,
            strategy: RouteStrategy::Fallback,
            targets: vec![target.config.clone()],
            retry: RetryPolicy {
                first_byte_timeout_ms: 500,
                stream_idle_timeout_ms: 1000,
                ..RetryPolicy::default()
            },
            enabled: true,
        },
        local_token: LOCAL_TOKEN.into(),
        targets: vec![target],
    }
}

fn start_proxy(
    routes: Vec<ResolvedRoute>,
    outbound: UpstreamProxyConfig,
) -> (ProxyHandle, Arc<UsageStore>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(dir.path().join("usage.sqlite")).unwrap());
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let mut config = RuntimeConfig::from_routes(addr.to_string(), routes);
    config.upstream_proxy = outbound;
    let handle = ProxyHandle::start(config, usage.clone()).unwrap();
    (handle, usage, dir)
}

fn direct() -> UpstreamProxyConfig {
    UpstreamProxyConfig {
        mode: UpstreamProxyMode::Direct,
        custom_url: None,
    }
}

async fn connect(
    handle: &ProxyHandle,
    path: &str,
    token: &str,
) -> Result<WebSocketStream<TcpStream>, tokio_tungstenite::tungstenite::Error> {
    let stream = TcpStream::connect(&handle.bind_addr).await.unwrap();
    let mut request = format!("ws://{}{path}", handle.bind_addr)
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
    );
    request.headers_mut().insert(
        "openai-beta",
        HeaderValue::from_static("responses_websockets=2026-02-06"),
    );
    request.headers_mut().insert(
        header::SEC_WEBSOCKET_EXTENSIONS,
        HeaderValue::from_static("permessage-deflate"),
    );
    request.headers_mut().insert(
        "x-api-key",
        HeaderValue::from_static("another-local-secret"),
    );
    let (socket, _) = client_async(request, stream).await?;
    Ok(socket)
}

async fn receive<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
    socket: &mut WebSocketStream<S>,
) -> Message {
    tokio::time::timeout(Duration::from_secs(3), socket.next())
        .await
        .expect("WS read timed out")
        .expect("WS closed")
        .expect("WS error")
}

fn event(value: serde_json::Value) -> Message {
    Message::text(value.to_string())
}

fn assert_rejection(
    result: Result<WebSocketStream<TcpStream>, tokio_tungstenite::tungstenite::Error>,
    status: StatusCode,
) {
    match result {
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
            assert_eq!(response.status(), status)
        }
        other => panic!("expected HTTP {status}, got {other:?}"),
    }
}

#[tokio::test]
async fn responses_websocket_relays_multiplexed_turns_and_records_each_response() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_hdr_async(
            stream,
            |request: &Request<()>, mut response: Response<()>| {
                assert_eq!(
                    request.uri().path_and_query().unwrap().as_str(),
                    "/backend-api/codex/responses?configured=yes&client=yes"
                );
                assert_eq!(
                    request.headers()[header::AUTHORIZATION],
                    format!("Bearer {UPSTREAM_KEY}")
                );
                assert_eq!(request.headers()["chatgpt-account-id"], "test-account");
                assert_eq!(
                    request.headers()["openai-beta"],
                    "responses_websockets=2026-02-06"
                );
                assert!(!request.headers().contains_key("x-api-key"));
                assert!(!request
                    .headers()
                    .contains_key(header::SEC_WEBSOCKET_EXTENSIONS));
                assert!(!request
                    .headers()
                    .contains_key(header::SEC_WEBSOCKET_PROTOCOL));
                response
                    .headers_mut()
                    .insert("x-request-id", HeaderValue::from_static("test-request-id"));
                Ok(response)
            },
        )
        .await
        .unwrap();
        for lane in ["main", "research"] {
            let message = receive(&mut ws).await;
            let value: serde_json::Value =
                serde_json::from_str(message.to_text().unwrap()).unwrap();
            assert_eq!(value["type"], "response.create");
            assert_eq!(value["stream_id"], lane);
            ws.send(event(serde_json::json!({"type":"response.created","stream_id":lane,"response":{"id":lane,"model":"test-model"}}))).await.unwrap();
        }
        for lane in ["research", "main"] {
            ws.send(event(serde_json::json!({"type":"response.output_text.delta","stream_id":lane,"response_id":lane,"delta":"hello"}))).await.unwrap();
            ws.send(event(serde_json::json!({"type":"response.completed","stream_id":lane,"response":{"id":lane,"model":"test-model","usage":{"input_tokens":10,"output_tokens":4,"input_tokens_details":{"cached_tokens":2}}}}))).await.unwrap();
        }
        let continuation = receive(&mut ws).await;
        let value: serde_json::Value =
            serde_json::from_str(continuation.to_text().unwrap()).unwrap();
        assert_eq!(value["previous_response_id"], "main");
        assert_eq!(value["input"][0]["call_id"], "tool-call");
        ws.send(event(serde_json::json!({"type":"response.created","stream_id":"main","response":{"id":"next"}}))).await.unwrap();
        ws.send(event(serde_json::json!({"type":"response.completed","stream_id":"main","response":{"id":"next","usage":{"input_tokens":3,"output_tokens":1}}}))).await.unwrap();
        assert_eq!(
            receive(&mut ws).await,
            Message::Binary(Bytes::from_static(b"binary"))
        );
        ws.send(Message::Binary(Bytes::from_static(b"binary-reply")))
            .await
            .unwrap();
        ws.send(Message::Ping(Bytes::from_static(b"upstream-ping")))
            .await
            .unwrap();
        assert_eq!(
            receive(&mut ws).await,
            Message::Pong(Bytes::from_static(b"upstream-ping"))
        );
        ws.close(Some(CloseFrame {
            code: CloseCode::Normal,
            reason: "done".into(),
        }))
        .await
        .unwrap();
        assert!(receive(&mut ws).await.is_close());
    });
    let route = test_route(format!("http://{addr}/backend-api/codex?configured=yes"));
    let (handle, store, _dir) = start_proxy(vec![route], direct());
    let mut ws = connect(&handle, "/v1/responses?client=yes", LOCAL_TOKEN)
        .await
        .unwrap();
    ws.send(Message::Ping(Bytes::from_static(b"client-ping")))
        .await
        .unwrap();
    assert_eq!(
        receive(&mut ws).await,
        Message::Pong(Bytes::from_static(b"client-ping"))
    );
    for lane in ["main", "research"] {
        ws.send(event(serde_json::json!({"type":"response.create","stream_id":lane,"model":"test-model","input":"hello","store":false}))).await.unwrap();
    }
    let mut events = Vec::new();
    for _ in 0..6 {
        events.push(receive(&mut ws).await);
    }
    assert!(events.iter().all(Message::is_text));
    ws.send(event(serde_json::json!({"type":"response.create","stream_id":"main","model":"test-model","previous_response_id":"main","input":[{"type":"function_call_output","call_id":"tool-call","output":"ok"}]}))).await.unwrap();
    for _ in 0..2 {
        assert!(receive(&mut ws).await.is_text());
    }
    ws.send(Message::Binary(Bytes::from_static(b"binary")))
        .await
        .unwrap();
    assert_eq!(
        receive(&mut ws).await,
        Message::Binary(Bytes::from_static(b"binary-reply"))
    );
    assert_eq!(
        receive(&mut ws).await,
        Message::Close(Some(CloseFrame {
            code: CloseCode::Normal,
            reason: "done".into()
        }))
    );
    ws.flush().await.unwrap();
    server.await.unwrap();
    let summary = store.summary(|_| 0).unwrap();
    assert_eq!(summary.request_count, 3);
    assert_eq!(summary.input_tokens, 19);
    assert_eq!(summary.cache_read_tokens, 4);
    assert_eq!(summary.output_tokens, 9);
    assert_eq!(summary.successful_attempts, 3);
    assert_eq!(handle.status().requests, 3);
}

#[tokio::test]
async fn websocket_handshake_uses_custom_outbound_proxy() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_hdr_async(stream, |request: &Request<()>, response| {
            assert_eq!(
                request.uri().to_string(),
                "http://upstream.invalid/v1/responses?client=yes"
            );
            assert_eq!(
                request.headers()[header::AUTHORIZATION],
                format!("Bearer {UPSTREAM_KEY}")
            );
            Ok(response)
        })
        .await
        .unwrap();
        ws.send(Message::text("through-custom-proxy"))
            .await
            .unwrap();
        assert!(receive(&mut ws).await.is_close());
        ws.flush().await.unwrap();
    });
    let (handle, _, _dir) = start_proxy(
        vec![test_route("http://upstream.invalid/v1".into())],
        UpstreamProxyConfig {
            mode: UpstreamProxyMode::Custom,
            custom_url: Some(format!("http://{addr}")),
        },
    );
    let mut ws = connect(&handle, "/v1/responses?client=yes", LOCAL_TOKEN)
        .await
        .unwrap();
    assert_eq!(
        receive(&mut ws).await,
        Message::text("through-custom-proxy")
    );
    ws.close(None).await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn websocket_rejects_unauthorized_and_malformed_requests_before_upstream() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut route = test_route(format!("http://{}/v1", listener.local_addr().unwrap()));
    route.config.conversion_enabled = true;
    route.targets[0].config.protocol = Some(ProxyProtocol::AnthropicMessages);
    let (handle, _, _dir) = start_proxy(vec![route], direct());
    assert_rejection(
        connect(&handle, "/v1/responses", "wrong-token").await,
        StatusCode::UNAUTHORIZED,
    );
    assert_rejection(
        connect(&handle, "/v1/chat/completions", LOCAL_TOKEN).await,
        StatusCode::NOT_FOUND,
    );
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://{}/v1/responses", handle.bind_addr))
        .bearer_auth(LOCAL_TOKEN)
        .header(header::UPGRADE, "websocket")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn websocket_falls_back_on_bad_accept() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let bad = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let good = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bad_addr = bad.local_addr().unwrap();
    let good_addr = good.local_addr().unwrap();
    let bad_server = tokio::spawn(async move {
        let (mut socket, _) = bad.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buf = [0; 1024];
        while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let read = socket.read(&mut buf).await.unwrap();
            assert!(read > 0);
            request.extend_from_slice(&buf[..read]);
        }
        socket.write_all(b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Accept: incorrect\r\n\r\n").await.unwrap();
    });
    let good_server = tokio::spawn(async move {
        let (stream, _) = good.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        ws.send(Message::text("connected-to-fallback"))
            .await
            .unwrap();
        assert!(receive(&mut ws).await.is_close());
        ws.flush().await.unwrap();
    });
    let mut route = test_route(format!("http://{bad_addr}/v1"));
    route
        .targets
        .push(test_route(format!("http://{good_addr}/v1")).targets[0].clone());
    route.config.retry.max_attempts = 2;
    let (handle, store, _dir) = start_proxy(vec![route], direct());
    let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    assert_eq!(
        receive(&mut ws).await,
        Message::text("connected-to-fallback")
    );
    ws.close(None).await.unwrap();
    bad_server.await.unwrap();
    good_server.await.unwrap();
    assert_eq!(store.summary(|_| 0).unwrap().attempt_count, 1);
}

#[tokio::test]
async fn websocket_config_reload_closes_existing_session() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let route = test_route(format!("http://{}/v1", listener.local_addr().unwrap()));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        ws.send(Message::text("ready")).await.unwrap();
        assert_eq!(
            receive(&mut ws).await,
            Message::Close(Some(CloseFrame {
                code: CloseCode::Restart,
                reason: "proxy configuration changed; reconnect".into()
            }))
        );
    });
    let (handle, _, _dir) = start_proxy(vec![route.clone()], direct());
    let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    assert_eq!(receive(&mut ws).await, Message::text("ready"));
    let mut config = RuntimeConfig::from_routes(&handle.bind_addr, vec![route]);
    config.upstream_proxy = direct();
    handle.update_config(config).unwrap();
    assert_eq!(
        receive(&mut ws).await,
        Message::Close(Some(CloseFrame {
            code: CloseCode::Restart,
            reason: "proxy configuration changed; reconnect".into()
        }))
    );
    server.await.unwrap();
}

#[tokio::test]
async fn websocket_idle_budget_applies_to_pending_responses_only() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut route = test_route(format!("http://{}/v1", listener.local_addr().unwrap()));
    route.config.retry.stream_idle_timeout_ms = 50;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        // Tool execution can exceed the response idle budget between turns.
        tokio::time::sleep(Duration::from_millis(150)).await;
        ws.send(Message::text("still-connected")).await.unwrap();
        assert!(receive(&mut ws).await.is_text());
        assert_eq!(
            receive(&mut ws).await,
            Message::Close(Some(CloseFrame {
                code: CloseCode::Error,
                reason: "upstream response idle timeout".into(),
            }))
        );
    });
    let (handle, store, _dir) = start_proxy(vec![route], direct());
    let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    assert_eq!(receive(&mut ws).await, Message::text("still-connected"));
    ws.send(event(
        serde_json::json!({"type":"response.create","model":"test","input":"hello"}),
    ))
    .await
    .unwrap();
    assert!(receive(&mut ws).await.is_close());
    server.await.unwrap();
    // The close reaches the client just before final bookkeeping completes.
    tokio::time::timeout(Duration::from_secs(1), async {
        while store.count().unwrap() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(store.summary(|_| 0).unwrap().request_count, 1);
    assert_eq!(handle.status().failures, 1);
}

#[tokio::test]
async fn websocket_does_not_replay_after_upstream_disconnect() {
    let primary = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fallback = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut route = test_route(format!("http://{}/v1", primary.local_addr().unwrap()));
    route.config.retry.silent_retry = true;
    route.config.retry.hold_on_failure = true;
    route.targets.push(
        test_route(format!("http://{}/v1", fallback.local_addr().unwrap())).targets[0].clone(),
    );
    let server = tokio::spawn(async move {
        let (stream, _) = primary.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        assert!(receive(&mut ws).await.is_text());
        ws.send(event(
            serde_json::json!({"type":"response.created","response":{"id":"partial"}}),
        ))
        .await
        .unwrap();
        ws.send(event(serde_json::json!({"type":"response.output_text.delta","item_id":"item","delta":"partial"}))).await.unwrap();
        // Drop without a Close or response.completed after delivering output.
    });
    let (handle, store, _dir) = start_proxy(vec![route], direct());
    let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    ws.send(event(
        serde_json::json!({"type":"response.create","model":"test","input":"hello"}),
    ))
    .await
    .unwrap();
    for _ in 0..2 {
        assert!(receive(&mut ws).await.is_text());
    }
    let end = tokio::time::timeout(Duration::from_secs(1), ws.next())
        .await
        .unwrap();
    assert!(!matches!(end, Some(Ok(Message::Text(_)))));
    server.await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), fallback.accept())
            .await
            .is_err()
    );
    let summary = store.summary(|_| 0).unwrap();
    assert_eq!(summary.request_count, 1);
    assert_eq!(summary.completed_attempts, 1);
    assert_eq!(summary.successful_attempts, 0);
    assert!(summary.average_first_token_ms.is_some());
    assert_eq!(handle.status().failures, 1);
}

#[tokio::test]
async fn websocket_request_error_keeps_connection_available_for_next_turn() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let route = test_route(format!("http://{}/v1", listener.local_addr().unwrap()));
    let error = event(
        serde_json::json!({"type":"error","status":400,"stream_id":"main","error":{"code":"previous_response_not_found","message":"missing previous response"}}),
    );
    let expected_error = error.clone();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        assert!(receive(&mut ws).await.is_text());
        ws.send(error.clone()).await.unwrap();
        assert!(receive(&mut ws).await.is_text());
        ws.send(event(serde_json::json!({"type":"response.created","stream_id":"main","response":{"id":"failed-active"}}))).await.unwrap();
        ws.send(error).await.unwrap();
        assert!(receive(&mut ws).await.is_text());
        ws.send(event(serde_json::json!({"type":"response.in_progress","stream_id":"main","response":{"id":"recovered"}}))).await.unwrap();
        ws.send(event(serde_json::json!({"type":"response.completed","stream_id":"main","response":{"id":"recovered","usage":{"input_tokens":5,"output_tokens":2}}}))).await.unwrap();
        assert!(receive(&mut ws).await.is_close());
        ws.flush().await.unwrap();
    });
    let (handle, store, _dir) = start_proxy(vec![route], direct());
    let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    ws.send(event(serde_json::json!({"type":"response.create","stream_id":"main","model":"test","previous_response_id":"missing"}))).await.unwrap();
    assert_eq!(receive(&mut ws).await, expected_error);
    ws.send(event(serde_json::json!({"type":"response.create","stream_id":"main","model":"test","input":"fail after creation"}))).await.unwrap();
    assert!(receive(&mut ws).await.is_text());
    assert_eq!(receive(&mut ws).await, expected_error);
    ws.send(event(serde_json::json!({"type":"response.create","stream_id":"main","model":"test","previous_response_id":null,"input":"full context"}))).await.unwrap();
    for _ in 0..2 {
        assert!(receive(&mut ws).await.is_text());
    }
    ws.close(None).await.unwrap();
    server.await.unwrap();
    let summary = store.summary(|_| 0).unwrap();
    assert_eq!(summary.request_count, 3);
    assert_eq!(summary.attempt_count, 3);
    assert_eq!(summary.successful_attempts, 1);
    assert_eq!(summary.output_tokens, 2);
}

#[tokio::test]
async fn websocket_upstream_rejection_preserves_http_status_without_error_secrets() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let route = test_route(format!("http://{}/v1", listener.local_addr().unwrap()));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let result = accept_hdr_async(stream, |_: &Request<()>, _| {
            Err(Response::builder()
                .status(StatusCode::UPGRADE_REQUIRED)
                .body(Some(format!("rejected {UPSTREAM_KEY}")))
                .unwrap())
        })
        .await;
        assert!(result.is_err());
    });
    let (handle, _, _dir) = start_proxy(vec![route], direct());
    match connect(&handle, "/v1/responses", LOCAL_TOKEN).await {
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
            assert_eq!(response.status(), StatusCode::UPGRADE_REQUIRED);
            assert!(
                !String::from_utf8_lossy(response.body().as_ref().unwrap()).contains(UPSTREAM_KEY)
            );
        }
        other => panic!("expected rejection, got {other:?}"),
    }
    server.await.unwrap();
    assert!(!format!("{:?}", handle.logs().unwrap()).contains(UPSTREAM_KEY));
}

#[tokio::test]
async fn websocket_converts_responses_turns_to_anthropic_sse_and_replays_lane_context() {
    use serde_json::Value;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_addr = listener.local_addr().unwrap();
    let upstream = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut pending = Vec::new();
        for turn in 0..2 {
            let header_end = loop {
                if let Some(end) = pending.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                    break end + 4;
                }
                let mut chunk = [0; 4096];
                let read = socket.read(&mut chunk).await.unwrap();
                assert!(read > 0);
                pending.extend_from_slice(&chunk[..read]);
            };
            let headers = String::from_utf8_lossy(&pending[..header_end]).to_string();
            assert!(headers.starts_with("POST /v1/messages HTTP/1.1"));
            assert!(
                headers.contains(&format!("authorization: Bearer {UPSTREAM_KEY}"))
                    || headers.contains(&format!("Authorization: Bearer {UPSTREAM_KEY}"))
            );
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            while pending.len() < header_end + content_length {
                let mut chunk = [0; 4096];
                let read = socket.read(&mut chunk).await.unwrap();
                assert!(read > 0);
                pending.extend_from_slice(&chunk[..read]);
            }
            let body: Value =
                serde_json::from_slice(&pending[header_end..header_end + content_length]).unwrap();
            assert_eq!(body["stream"], true);
            assert_eq!(body["messages"][0]["role"], "user");
            if turn == 1 {
                assert_eq!(body["messages"][1]["role"], "assistant");
                assert_eq!(body["messages"][1]["content"][0]["text"], "hello");
            }
            pending.drain(..header_end + content_length);
            let response = format!("event: message_start\ndata: {{\"type\":\"message_start\",\"message\":{{\"id\":\"msg_{}\",\"model\":\"claude\",\"usage\":{{\"input_tokens\":5}}}}}}\n\n", turn + 1)
                + "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n"
                + "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n"
                + "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n"
                + "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}\n\n"
                + "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{}", response.len(), response);
            socket.write_all(response.as_bytes()).await.unwrap();
        }
    });
    let mut route = test_route(format!("http://{upstream_addr}"));
    route.config.conversion_enabled = true;
    route.config.upstream_protocol = ProxyProtocol::AnthropicMessages;
    route.targets[0].config.protocol = Some(ProxyProtocol::AnthropicMessages);
    let (handle, store, _dir) = start_proxy(vec![route], direct());
    let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    ws.send(event(serde_json::json!({"type":"response.create","stream_id":"main","model":"gpt-5","input":"hello"}))).await.unwrap();
    let first = receive_response(&mut ws).await;
    assert!(first.iter().all(|value| value["stream_id"] == "main"));
    assert!(first
        .iter()
        .any(|value| value["type"] == "response.output_text.delta" && value["delta"] == "hello"));
    let response_id = first
        .iter()
        .find(|value| value["type"] == "response.completed")
        .unwrap()["response"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    ws.send(event(serde_json::json!({"type":"response.create","stream_id":"main","previous_response_id":response_id,"model":"gpt-5","input":"again"}))).await.unwrap();
    let second = receive_response(&mut ws).await;
    assert!(second.iter().all(|value| value["stream_id"] == "main"));
    ws.close(None).await.unwrap();
    upstream.await.unwrap();
    let summary = store.summary(|_| 0).unwrap();
    assert_eq!(summary.request_count, 2);
    assert_eq!(summary.successful_attempts, 2);
}

async fn receive_response(socket: &mut WebSocketStream<TcpStream>) -> Vec<serde_json::Value> {
    let mut events = Vec::new();
    loop {
        let message = receive(socket).await;
        let value: serde_json::Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
        let terminal = matches!(
            value["type"].as_str(),
            Some("response.completed" | "response.incomplete" | "response.failed" | "error")
        );
        events.push(value);
        if terminal {
            return events;
        }
    }
}

struct HttpCall {
    path: String,
    headers: HeaderMap,
    body: serde_json::Value,
    reply: oneshot::Sender<Response<Full<Bytes>>>,
}

async fn mock_http_upstream() -> (
    String,
    tokio::sync::mpsc::Receiver<HttpCall>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = tokio::sync::mpsc::channel(8);
    let server = tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            let tx = tx.clone();
            tokio::spawn(async move {
                let service = service_fn(move |request: Request<Incoming>| {
                    let tx = tx.clone();
                    async move {
                        let (parts, body) = request.into_parts();
                        let body = body.collect().await.unwrap().to_bytes();
                        let (reply, response) = oneshot::channel();
                        tx.send(HttpCall {
                            path: parts.uri.path().to_owned(),
                            headers: parts.headers,
                            body: serde_json::from_slice(&body).unwrap(),
                            reply,
                        })
                        .await
                        .unwrap();
                        Ok::<_, Infallible>(response.await.unwrap())
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(socket), service)
                    .await;
            });
        }
    });
    (address, rx, server)
}

fn reply_sse(call: HttpCall, events: Vec<serde_json::Value>) {
    let body: String = events
        .into_iter()
        .map(|value| {
            format!(
                "event: {}\ndata: {value}\n\n",
                value["type"].as_str().unwrap()
            )
        })
        .collect();
    call.reply
        .send(
            Response::builder()
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(Full::new(Bytes::from(body)))
                .unwrap(),
        )
        .unwrap();
}

fn anthropic_text_events() -> Vec<serde_json::Value> {
    use serde_json::json;
    vec![
        json!({"type":"message_start","message":{"id":"msg_test","model":"claude","usage":{"input_tokens":5}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hello"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}}),
        json!({"type":"message_stop"}),
    ]
}

fn converted_route(address: String) -> ResolvedRoute {
    let mut route = test_route(address);
    route.config.conversion_enabled = true;
    route.config.upstream_protocol = ProxyProtocol::AnthropicMessages;
    route.targets[0].config.protocol = Some(ProxyProtocol::AnthropicMessages);
    route
}

async fn next_http_call(rx: &mut tokio::sync::mpsc::Receiver<HttpCall>) -> HttpCall {
    tokio::time::timeout(Duration::from_secs(3), rx.recv())
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn websocket_conversion_preserves_tool_calls_results_and_forked_context() {
    use serde_json::json;
    let (address, mut calls, server) = mock_http_upstream().await;
    let (handle, store, _dir) = start_proxy(vec![converted_route(address)], direct());
    let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    let tools = json!([
        {"type":"function","name":"lookup","parameters":{"type":"object"}},
        {"type":"function","name":"refresh","parameters":{"type":"object"}}
    ]);
    ws.send(event(json!({"type":"response.create","stream_id":"main","model":"claude","input":[{"role":"user","content":"check"}],"tools":tools}))).await.unwrap();
    let first_call = next_http_call(&mut calls).await;
    assert_eq!(first_call.path, "/v1/messages");
    assert_eq!(
        first_call.headers[header::AUTHORIZATION],
        format!("Bearer {UPSTREAM_KEY}")
    );
    assert!(!first_call.headers.contains_key("sec-websocket-key"));
    assert!(!first_call.headers.contains_key("x-api-key"));
    assert_eq!(first_call.body["tools"][0]["name"], "lookup");
    let mut events = anthropic_text_events();
    events.truncate(4);
    events.extend([
        json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"call_1","name":"lookup","input":{}}}),
        json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"city\":"}}),
        json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"Paris\"}"}}),
        json!({"type":"content_block_stop","index":1}),
        json!({"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"call_2","name":"refresh","input":{}}}),
        json!({"type":"content_block_stop","index":2}),
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":8}}),
        json!({"type":"message_stop"}),
    ]);
    reply_sse(first_call, events);
    let first = receive_response(&mut ws).await;
    for (index, value) in first.iter().enumerate() {
        assert_eq!(value["sequence_number"], index);
        assert_eq!(value["stream_id"], "main");
    }
    let completed = &first.last().unwrap()["response"];
    assert_eq!(completed["status"], "completed");
    assert_eq!(completed["output"][0]["content"][0]["text"], "hello");
    assert_eq!(completed["output"][1]["arguments"], "{\"city\":\"Paris\"}");
    assert_eq!(completed["output"][2]["arguments"], "{}");
    assert_eq!(
        first
            .iter()
            .filter(|value| value["type"] == "response.function_call_arguments.done")
            .count(),
        2
    );
    ws.send(event(json!({"type":"response.create","stream_id":"fork","previous_response_id":completed["id"],"model":"claude","tools":tools,"input":[
        {"type":"function_call_output","call_id":"call_1","output":"sunny"},
        {"type":"function_call_output","call_id":"call_2","output":"ok"},
        {"role":"user","content":"summarize"}
    ]}))).await.unwrap();
    let second_call = next_http_call(&mut calls).await;
    assert_eq!(
        second_call.body["messages"],
        json!([
            {"role":"user","content":[{"type":"text","text":"check"}]},
            {"role":"assistant","content":[
                {"type":"text","text":"hello"},
                {"type":"tool_use","id":"call_1","name":"lookup","input":{"city":"Paris"}},
                {"type":"tool_use","id":"call_2","name":"refresh","input":{}}
            ]},
            {"role":"user","content":[
                {"type":"tool_result","tool_use_id":"call_1","content":"sunny"},
                {"type":"tool_result","tool_use_id":"call_2","content":"ok"},
                {"type":"text","text":"summarize"}
            ]}
        ])
    );
    assert!(second_call.body.get("previous_response_id").is_none());
    assert!(second_call.body.get("stream_id").is_none());
    reply_sse(second_call, anthropic_text_events());
    let second = receive_response(&mut ws).await;
    assert!(second.iter().all(|value| value["stream_id"] == "fork"));
    assert_ne!(second.last().unwrap()["response"]["id"], completed["id"]);
    ws.close(None).await.unwrap();
    assert_eq!(store.summary(|_| 0).unwrap().successful_attempts, 2);
    server.abort();
}

#[tokio::test]
async fn websocket_conversion_orders_lanes_and_recovers_after_upstream_failure() {
    use serde_json::json;
    let (address, mut calls, server) = mock_http_upstream().await;
    let route = converted_route(address);
    let target_id = route.targets[0].config.id;
    let (handle, store, _dir) = start_proxy(vec![route], direct());
    let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    for (lane, input) in [
        ("main", "first"),
        ("main", "second"),
        ("parallel", "independent"),
    ] {
        ws.send(event(
            json!({"type":"response.create","stream_id":lane,"model":"claude","input":input}),
        ))
        .await
        .unwrap();
    }
    let a = next_http_call(&mut calls).await;
    let b = next_http_call(&mut calls).await;
    let (first, independent) = if a.body["messages"][0]["content"][0]["text"] == "first" {
        (a, b)
    } else {
        (b, a)
    };
    assert_eq!(
        independent.body["messages"][0]["content"][0]["text"],
        "independent"
    );
    assert!(
        calls.try_recv().is_err(),
        "same-lane requests must not overlap"
    );
    // An upstream error before output must release the lane and count once.
    first
        .reply
        .send(
            Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .unwrap();
    let error = receive_response(&mut ws).await;
    assert_eq!(error.last().unwrap()["type"], "error");
    assert_eq!(error.last().unwrap()["stream_id"], "main");
    assert!(handle.state.health.lock().unwrap().contains_key(&target_id));
    let second = next_http_call(&mut calls).await;
    assert_eq!(second.body["messages"][0]["content"][0]["text"], "second");
    reply_sse(second, anthropic_text_events());
    let recovered = receive_response(&mut ws).await;
    assert_eq!(recovered.last().unwrap()["type"], "response.completed");
    reply_sse(independent, anthropic_text_events());
    let parallel = receive_response(&mut ws).await;
    assert_eq!(parallel.last().unwrap()["stream_id"], "parallel");
    assert_eq!(parallel.last().unwrap()["type"], "response.completed");
    ws.close(None).await.unwrap();
    let summary = store.summary(|_| 0).unwrap();
    assert_eq!(summary.request_count, 3);
    assert_eq!(summary.attempt_count, 3);
    assert_eq!(summary.successful_attempts, 2);
    assert!(handle.state.health.lock().unwrap().is_empty());
    server.abort();
}

#[tokio::test]
async fn websocket_conversion_warmup_and_error_cache_eviction_are_connection_local() {
    use serde_json::json;
    let (address, mut calls, server) = mock_http_upstream().await;
    let (handle, store, _dir) = start_proxy(vec![converted_route(address)], direct());
    let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    ws.send(event(json!({"type":"response.create","stream_id":"main","model":"claude","generate":false,"input":"context"}))).await.unwrap();
    let warmup = receive_response(&mut ws).await;
    assert_eq!(warmup[1]["type"], "response.in_progress");
    let parent = warmup.last().unwrap()["response"]["id"].clone();
    assert_eq!(warmup.last().unwrap()["response"]["output"], json!([]));
    ws.send(event(json!({"type":"response.create","stream_id":"fork","model":"claude","previous_response_id":parent,"background":true}))).await.unwrap();
    assert_eq!(
        receive_response(&mut ws).await.last().unwrap()["error"]["code"],
        "unsupported_parameter"
    );
    ws.send(event(json!({"type":"response.create","stream_id":"main","model":"claude","previous_response_id":parent,"generate":false}))).await.unwrap();
    let continued = receive_response(&mut ws).await;
    assert_eq!(continued.last().unwrap()["type"], "response.completed");
    let parent = continued.last().unwrap()["response"]["id"].clone();
    // A failed same-lane continuation evicts its referenced parent.
    ws.send(event(json!({"type":"response.create","stream_id":"main","model":"claude","previous_response_id":parent,"input":42}))).await.unwrap();
    assert_eq!(
        receive_response(&mut ws).await.last().unwrap()["error"]["code"],
        "invalid_request"
    );
    ws.send(event(json!({"type":"response.create","stream_id":"main","model":"claude","previous_response_id":parent,"generate":false}))).await.unwrap();
    assert_eq!(
        receive_response(&mut ws).await.last().unwrap()["error"]["code"],
        "previous_response_not_found"
    );
    ws.send(event(
        json!({"type":"response.create","model":"claude","generate":false}),
    ))
    .await
    .unwrap();
    assert_eq!(
        receive_response(&mut ws).await.last().unwrap()["type"],
        "response.completed"
    );
    ws.close(None).await.unwrap();
    assert!(calls.try_recv().is_err());
    assert_eq!(store.count().unwrap(), 0);
    server.abort();
}

#[tokio::test]
async fn websocket_conversion_mixed_route_falls_back_from_responses_http_to_anthropic() {
    use serde_json::json;
    let (native_address, mut native_calls, native_server) = mock_http_upstream().await;
    let (anthropic_address, mut anthropic_calls, anthropic_server) = mock_http_upstream().await;
    let mut route = test_route(native_address);
    route.config.conversion_enabled = true;
    let mut fallback = converted_route(anthropic_address).targets[0].clone();
    fallback.config.priority = 1;
    route.targets.push(fallback);
    let (handle, store, _dir) = start_proxy(vec![route], direct());
    let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    ws.send(event(
        json!({"type":"response.create","stream_id":"main","model":"test","input":"first"}),
    ))
    .await
    .unwrap();
    let native = next_http_call(&mut native_calls).await;
    assert_eq!(native.path, "/v1/responses");
    assert_eq!(native.body["stream"], true);
    assert!(native.body.get("stream_id").is_none());
    reply_sse(
        native,
        vec![
            json!({"type":"response.created","response":{"id":"resp_native","status":"in_progress","output":[]}}),
            json!({"type":"response.output_text.delta","delta":"native output"}),
            json!({"type":"response.completed","response":{"id":"resp_native","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"native output"}]}],"usage":{"input_tokens":3,"output_tokens":2}}}),
        ],
    );
    let first = receive_response(&mut ws).await;
    ws.send(event(json!({"type":"response.create","stream_id":"main","model":"test","previous_response_id":first.last().unwrap()["response"]["id"],"input":"again"}))).await.unwrap();
    let native = next_http_call(&mut native_calls).await;
    assert!(native.body.get("previous_response_id").is_none());
    assert_eq!(
        native.body["input"][1]["content"][0]["text"],
        "native output"
    );
    native
        .reply
        .send(
            Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .unwrap();
    let fallback = next_http_call(&mut anthropic_calls).await;
    assert_eq!(fallback.path, "/v1/messages");
    assert_eq!(
        fallback.body["messages"][1]["content"][0]["text"],
        "native output"
    );
    reply_sse(fallback, anthropic_text_events());
    assert_eq!(
        receive_response(&mut ws).await.last().unwrap()["type"],
        "response.completed"
    );
    ws.close(None).await.unwrap();
    let summary = store.summary(|_| 0).unwrap();
    assert_eq!(summary.request_count, 2);
    assert_eq!(summary.attempt_count, 3);
    assert_eq!(summary.successful_attempts, 2);
    native_server.abort();
    anthropic_server.abort();
}

#[tokio::test]
async fn websocket_conversion_does_not_replay_after_committed_stream_failure() {
    use serde_json::json;
    let (address, mut calls, server) = mock_http_upstream().await;
    let (backup_address, mut backup_calls, backup_server) = mock_http_upstream().await;
    let mut route = converted_route(address);
    route
        .targets
        .push(converted_route(backup_address).targets[0].clone());
    let (handle, store, _dir) = start_proxy(vec![route], direct());
    let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    ws.send(event(
        json!({"type":"response.create","model":"claude","input":"first"}),
    ))
    .await
    .unwrap();
    let call = next_http_call(&mut calls).await;
    let mut events = anthropic_text_events();
    events.truncate(3); // Text was generated but the stream never completed.
    reply_sse(call, events);
    let response = receive_response(&mut ws).await;
    assert!(response
        .iter()
        .any(|value| value["type"] == "response.output_text.delta"));
    assert_eq!(response.last().unwrap()["type"], "error");
    assert!(calls.try_recv().is_err());
    assert!(backup_calls.try_recv().is_err());
    ws.close(None).await.unwrap();
    let summary = store.summary(|_| 0).unwrap();
    assert_eq!(summary.request_count, 1);
    assert_eq!(summary.attempt_count, 1);
    assert_eq!(summary.successful_attempts, 0);
    assert_eq!(handle.status().failures, 1);
    server.abort();
    backup_server.abort();
}

#[tokio::test]
async fn websocket_incomplete_and_duplicate_terminal_events_do_not_degrade_upstream() {
    use serde_json::json;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let route = test_route(format!("http://{}", listener.local_addr().unwrap()));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        receive(&mut socket).await;
        socket
            .send(event(
                json!({"type":"response.created","response":{"id":"limited"}}),
            ))
            .await
            .unwrap();
        let terminal = event(
            json!({"type":"response.incomplete","response":{"id":"limited","status":"incomplete","output":[],"incomplete_details":{"reason":"max_output_tokens"},"usage":{"input_tokens":5,"output_tokens":8}}}),
        );
        socket.send(terminal.clone()).await.unwrap();
        socket.send(terminal).await.unwrap();
        receive(&mut socket).await;
    });
    let (handle, store, _dir) = start_proxy(vec![route], direct());
    let mut socket = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    socket
        .send(event(
            json!({"type":"response.create","model":"test","input":"hello","max_output_tokens":8}),
        ))
        .await
        .unwrap();
    let events = receive_response(&mut socket).await;
    assert_eq!(events.last().unwrap()["type"], "response.incomplete");
    receive(&mut socket).await;
    socket.close(None).await.unwrap();
    server.await.unwrap();
    let summary = store.summary(|_| 0).unwrap();
    assert_eq!(summary.request_count, 1);
    assert_eq!(summary.successful_attempts, 1);
    assert_eq!(summary.output_tokens, 8);
    assert_eq!(handle.status().failures, 0);
}

#[tokio::test]
async fn websocket_http_bridge_accepts_incomplete_as_a_terminal_response() {
    use serde_json::json;
    let (address, mut calls, server) = mock_http_upstream().await;
    let mut route = test_route(address);
    route.config.conversion_enabled = true;
    let (handle, store, _dir) = start_proxy(vec![route], direct());
    let mut socket = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    socket
        .send(event(
            json!({"type":"response.create","model":"test","input":"hello"}),
        ))
        .await
        .unwrap();
    reply_sse(
        next_http_call(&mut calls).await,
        vec![
            json!({"type":"response.created","response":{"id":"limited","status":"in_progress"}}),
            json!({"type":"response.output_text.delta","delta":"partial"}),
            json!({"type":"response.incomplete","response":{"id":"limited","status":"incomplete","output":[],"incomplete_details":{"reason":"max_output_tokens"},"usage":{"input_tokens":5,"output_tokens":8}}}),
        ],
    );
    assert_eq!(
        receive_response(&mut socket).await.last().unwrap()["type"],
        "response.incomplete"
    );
    socket.close(None).await.unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        while store.summary(|_| 0).unwrap().completed_attempts == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(store.summary(|_| 0).unwrap().successful_attempts, 1);
    assert_eq!(handle.status().failures, 0);
    server.abort();
}

#[tokio::test]
async fn websocket_conversion_config_reload_cancels_pending_http_and_queued_turns() {
    use serde_json::json;
    let (address, mut calls, server) = mock_http_upstream().await;
    let mut route = converted_route(address);
    route.config.retry.first_byte_timeout_ms = 30_000;
    let (handle, _, _dir) = start_proxy(vec![route.clone()], direct());
    let mut socket = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    for input in ["in-flight", "queued"] {
        socket
            .send(event(
                json!({"type":"response.create","model":"test","input":input}),
            ))
            .await
            .unwrap();
    }
    let call = next_http_call(&mut calls).await;
    let mut config = RuntimeConfig::from_routes(&handle.bind_addr, vec![route]);
    config.upstream_proxy = direct();
    handle.update_config(config).unwrap();
    assert_eq!(
        receive(&mut socket).await,
        Message::Close(Some(CloseFrame {
            code: CloseCode::Restart,
            reason: "proxy configuration changed; reconnect".into(),
        }))
    );
    // Let the fake upstream handler finish without an intentional panic.
    let _ = call.reply.send(Response::new(Full::new(Bytes::new())));
    assert!(calls.try_recv().is_err());
    server.abort();
}

#[tokio::test]
async fn websocket_busy_lane_and_pings_do_not_mask_another_lanes_timeout() {
    use serde_json::json;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut route = test_route(format!("http://{}", listener.local_addr().unwrap()));
    route.config.retry.stream_idle_timeout_ms = 80;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        for lane in ["stalled", "busy"] {
            receive(&mut socket).await;
            socket
                .send(event(
                    json!({"type":"response.created","stream_id":lane,"response":{"id":lane}}),
                ))
                .await
                .unwrap();
        }
        let mut tick = tokio::time::interval(Duration::from_millis(10));
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    if socket.send(event(json!({"type":"response.output_text.delta","stream_id":"busy","delta":"working"}))).await.is_err() { break; }
                    if socket.send(Message::Ping(Bytes::new())).await.is_err() { break; }
                }
                incoming = socket.next() => {
                    if matches!(incoming, None | Some(Err(_)) | Some(Ok(Message::Close(_)))) { break; }
                }
            }
        }
    });
    let (handle, _, _dir) = start_proxy(vec![route], direct());
    let mut socket = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    for lane in ["stalled", "busy"] {
        socket
            .send(event(
                json!({"type":"response.create","stream_id":lane,"model":"test","input":"hello"}),
            ))
            .await
            .unwrap();
    }
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if receive(&mut socket).await.is_close() {
                break;
            }
        }
    })
    .await
    .expect("a busy lane must not keep a stalled response alive");
    server.await.unwrap();
}
