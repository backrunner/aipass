use super::*;
use serde_json::{json, Value};

struct Mock {
    address: String,
    upgrades: Arc<AtomicU64>,
    status: Arc<AtomicU64>,
    disconnect: Arc<AtomicU64>,
    pings: Arc<AtomicU64>,
    live: Arc<AtomicU64>,
    output: Arc<Mutex<Option<Vec<Value>>>>,
    calls: Arc<Mutex<Vec<(bool, Value)>>>,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for Mock {
    fn drop(&mut self) {
        self.server.abort();
    }
}

fn responses() -> Vec<Value> {
    vec![
        json!({"type":"response.created","response":{"id":"resp_mock","status":"in_progress","output":[]}}),
        json!({"type":"response.output_text.delta","delta":"hello"}),
        json!({"type":"response.completed","response":{"id":"resp_mock","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"hello"}]}],"usage":{"input_tokens":1,"output_tokens":1}}}),
    ]
}

async fn mock(status_code: u64) -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let upgrades = Arc::new(AtomicU64::new(0));
    let status = Arc::new(AtomicU64::new(status_code));
    let disconnect = Arc::new(AtomicU64::new(0));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let pings = Arc::new(AtomicU64::new(0));
    let ping_count = pings.clone();
    let live = Arc::new(AtomicU64::new(0));
    let active = live.clone();
    let output = Arc::new(Mutex::new(None::<Vec<Value>>));
    let output_override = output.clone();
    let (count, reject, mode, history) = (
        upgrades.clone(),
        status.clone(),
        disconnect.clone(),
        calls.clone(),
    );
    let server = tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            let (count, reject, mode, history) =
                (count.clone(), reject.clone(), mode.clone(), history.clone());
            let ping_count = ping_count.clone();
            let active = active.clone();
            let output_override = output_override.clone();
            tokio::spawn(async move {
                let service = service_fn(move |mut request: Request<Incoming>| {
                    let ping_count = ping_count.clone();
                    let active = active.clone();
                    let output_override = output_override.clone();
                    let (count, reject, mode, history) =
                        (count.clone(), reject.clone(), mode.clone(), history.clone());
                    async move {
                        assert_eq!(
                            request.headers()["authorization"],
                            format!("Bearer {UPSTREAM_KEY}")
                        );
                        assert_eq!(request.headers()["chatgpt-account-id"], "test-account");
                        if is_upgrade_request(&request) {
                            count.fetch_add(1, Ordering::SeqCst);
                            let status = reject.load(Ordering::SeqCst);
                            if status != 0 {
                                return Ok::<_, Infallible>(
                                    Response::builder()
                                        .status(status as u16)
                                        .body(Full::new(Bytes::new()))
                                        .unwrap(),
                                );
                            }
                            let response =
                                create_response_with_body(&request, || Full::new(Bytes::new()))
                                    .unwrap();
                            let upgrade = hyper::upgrade::on(&mut request);
                            tokio::spawn(async move {
                                let Ok(stream) = upgrade.await else {
                                    return;
                                };
                                let _active = InFlightGuard::new(active);
                                let mut ws = WebSocketStream::from_raw_socket(
                                    TokioIo::new(stream),
                                    Role::Server,
                                    None,
                                )
                                .await;
                                while let Some(Ok(message)) = ws.next().await {
                                    let text = match message {
                                        Message::Text(text) => text,
                                        Message::Ping(_) => {
                                            ping_count.fetch_add(1, Ordering::SeqCst);
                                            if ws.flush().await.is_err() {
                                                return;
                                            }
                                            continue;
                                        }
                                        Message::Pong(_) => continue,
                                        Message::Close(_) => {
                                            let _ = ws.flush().await;
                                            break;
                                        }
                                        _ => break,
                                    };
                                    let body: Value = serde_json::from_str(&text).unwrap();
                                    history.lock().unwrap().push((true, body.clone()));
                                    let mode = mode.load(Ordering::SeqCst);
                                    if mode == 1 {
                                        break;
                                    }
                                    let mut events = if body["generate"] == false {
                                        vec![
                                            json!({"type":"response.completed","response":{"id":"warmup","status":"completed","output":[]}}),
                                        ]
                                    } else {
                                        responses()
                                    };
                                    if let Some(output) = output_override.lock().unwrap().clone() {
                                        events.last_mut().unwrap()["response"]["output"] =
                                            json!(output);
                                    }
                                    for event in events {
                                        if ws.send(super::event(event)).await.is_err() {
                                            return;
                                        }
                                        if mode == 2 {
                                            return;
                                        }
                                        if mode == 4 {
                                            break;
                                        }
                                    }
                                    if mode == 3 {
                                        return;
                                    }
                                }
                            });
                            return Ok(response);
                        }
                        let body: Value = serde_json::from_slice(
                            &request.into_body().collect().await.unwrap().to_bytes(),
                        )
                        .unwrap();
                        history.lock().unwrap().push((false, body));
                        let events = if mode.load(Ordering::SeqCst) > 0 {
                            vec![responses().remove(0)]
                        } else {
                            responses()
                        };
                        let payload: String = events
                            .iter()
                            .map(|event| format!("data: {event}\n\n"))
                            .collect();
                        Ok(Response::builder()
                            .header("content-type", "text/event-stream")
                            .body(Full::new(Bytes::from(payload)))
                            .unwrap())
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(socket), service)
                    .with_upgrades()
                    .await;
            });
        }
    });
    Mock {
        address,
        upgrades,
        status,
        disconnect,
        calls,
        pings,
        live,
        output,
        server,
    }
}

#[tokio::test]
async fn mixed_ws_turns_reuse_the_upstream_socket() {
    let upstream = mock(0).await;
    let mut route = test_route(upstream.address.clone());
    route.config.conversion_enabled = true;
    let (handle, store, _dir) = start_proxy(vec![route], direct());
    let mut ws = fallback_session(&handle, &upstream).await;
    let mut previous = turn(&mut ws, None).await;
    for _ in 0..3 {
        let events = turn(&mut ws, Some(&previous.last().unwrap()["response"]["id"])).await;
        assert_eq!(events.last().unwrap()["type"], "response.completed");
        previous = events;
    }
    assert_eq!(upstream.upgrades.load(Ordering::SeqCst), 2);
    assert_eq!(upstream.calls.lock().unwrap().len(), 4);
    assert_eq!(store.count().unwrap(), 4);
}

#[tokio::test]
async fn http_responses_prefer_ws_for_streaming_and_buffered_requests_without_retaining_sockets() {
    let upstream = mock(0).await;
    let route = test_route(upstream.address.clone());
    let (handle, store, _dir) = start_proxy(vec![route], direct());
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    for streaming in [true, false, true] {
        let response = client
            .post(format!("http://{}/v1/responses", handle.bind_addr))
            .bearer_auth(LOCAL_TOKEN)
            .json(&json!({"model":"test", "input":"hello", "stream":streaming}))
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        if streaming {
            assert_eq!(response.headers()["content-type"], "text/event-stream");
            assert!(response
                .text()
                .await
                .unwrap()
                .contains("response.completed"));
        } else {
            assert_eq!(response.headers()["content-type"], "application/json");
            assert_eq!(
                response.json::<Value>().await.unwrap()["status"],
                "completed"
            );
        }
    }
    assert_eq!(upstream.upgrades.load(Ordering::SeqCst), 3);
    assert!(upstream.calls.lock().unwrap().iter().all(|(ws, _)| *ws));
    assert_eq!(store.count().unwrap(), 3);
}

#[tokio::test]
async fn http_requests_never_share_upstream_ws_state() {
    let upstream = mock(0).await;
    let route = test_route(upstream.address.clone());
    let (handle, _store, _dir) = start_proxy(vec![route], direct());
    for _ in 0..2 {
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        for session in ["one", "two", "one"] {
            let response = client
                .post(format!("http://{}/v1/responses", handle.bind_addr))
                .bearer_auth(LOCAL_TOKEN)
                .header("x-aipass-session-id", session)
                .json(&json!({"model":"test","input":"hello","stream":true}))
                .send()
                .await
                .unwrap();
            assert!(response
                .text()
                .await
                .unwrap()
                .contains("response.completed"));
        }
    }
    assert_eq!(upstream.upgrades.load(Ordering::SeqCst), 6);
}

#[tokio::test]
async fn closed_idle_socket_reconnects_before_sending_the_next_generation() {
    let upstream = mock(0).await;
    upstream.disconnect.store(3, Ordering::SeqCst);
    let mut route = test_route(upstream.address.clone());
    route.config.conversion_enabled = true;
    let (handle, store, _dir) = start_proxy(vec![route], direct());
    let mut ws = fallback_session(&handle, &upstream).await;
    for _ in 0..3 {
        assert_eq!(
            turn(&mut ws, None).await.last().unwrap()["type"],
            "response.completed"
        );
    }
    assert_eq!(upstream.upgrades.load(Ordering::SeqCst), 4);
    assert_eq!(upstream.calls.lock().unwrap().len(), 3);
    assert_eq!(store.count().unwrap(), 3);
}

#[tokio::test]
async fn native_and_mixed_ws_connections_send_idle_heartbeats_and_remain_reusable() {
    async fn check(mixed: bool) {
        let upstream = mock(0).await;
        let mut route = test_route(upstream.address.clone());
        route.config.conversion_enabled = mixed;
        let (handle, _, _dir) = start_proxy(vec![route], direct());
        let mut ws = if mixed {
            fallback_session(&handle, &upstream).await
        } else {
            connect(&handle, "/v1/responses", LOCAL_TOKEN)
                .await
                .unwrap()
        };
        assert_eq!(
            turn(&mut ws, None).await.last().unwrap()["type"],
            "response.completed"
        );
        tokio::time::timeout(Duration::from_secs(25), async {
            while upstream.pings.load(Ordering::SeqCst) == 0 {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("proxy must proactively ping idle upstreams");
        assert_eq!(
            turn(&mut ws, None).await.last().unwrap()["type"],
            "response.completed"
        );
        assert_eq!(
            upstream.upgrades.load(Ordering::SeqCst),
            if mixed { 2 } else { 1 }
        );
    }
    tokio::join!(check(false), check(true));
}

#[tokio::test]
async fn http_ws_preference_respects_opt_out_and_does_not_replay_submitted_requests() {
    for enabled in [true, false] {
        let upstream = mock(0).await;
        let mut route = test_route(upstream.address.clone());
        route.targets[0].supports_websockets = enabled;
        if enabled {
            upstream.disconnect.store(1, Ordering::SeqCst);
        }
        route.config.retry.silent_retry = true;
        let (handle, _store, _dir) = start_proxy(vec![route], direct());
        let response = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .post(format!("http://{}/v1/responses", handle.bind_addr))
            .bearer_auth(LOCAL_TOKEN)
            .json(&json!({"model":"test","input":"hello"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().is_success(), !enabled);
        let _body = response.bytes().await.unwrap();
        assert_eq!(upstream.upgrades.load(Ordering::SeqCst), u64::from(enabled));
        let calls = upstream.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, enabled);
    }
}

#[tokio::test]
async fn configuration_refresh_discards_idle_upstream_connections() {
    let upstream = mock(0).await;
    let route = test_route(upstream.address.clone());
    let (handle, _store, _dir) = start_proxy(vec![route.clone()], direct());
    let mut ws = fallback_session(&handle, &upstream).await;
    assert_eq!(
        turn(&mut ws, None).await.last().unwrap()["type"],
        "response.completed"
    );
    assert_eq!(upstream.live.load(Ordering::SeqCst), 1);
    handle
        .update_config(RuntimeConfig::from_routes(
            handle.bind_addr.clone(),
            vec![route],
        ))
        .unwrap();
    assert!(matches!(receive(&mut ws).await, Message::Close(_)));
    wait_for_no_connections(&upstream).await;
    let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    assert_eq!(
        turn(&mut ws, None).await.last().unwrap()["type"],
        "response.completed"
    );
    assert_eq!(upstream.upgrades.load(Ordering::SeqCst), 3);
}

async fn turn(ws: &mut WebSocketStream<TcpStream>, previous: Option<&Value>) -> Vec<Value> {
    ws.send(event(json!({"type":"response.create","model":"test","input":"hello", "previous_response_id":previous}))).await.unwrap();
    receive_response(ws).await
}

#[tokio::test]
async fn mixed_route_keeps_native_ws_transparent_and_pinned() {
    let native = mock(0).await;
    let http = mock(0).await;
    let mut route = test_route(native.address.clone());
    route.config.strategy = RouteStrategy::RoundRobin;
    let mut target = test_route(http.address.clone()).targets.remove(0);
    target.supports_websockets = false;
    route.targets.push(target);
    let (handle, store, _dir) = start_proxy(vec![route], direct());
    let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    let first = turn(&mut ws, None).await;
    let second = turn(&mut ws, Some(&first.last().unwrap()["response"]["id"])).await;
    assert_eq!(second.last().unwrap()["type"], "response.completed");
    assert_eq!(native.upgrades.load(Ordering::SeqCst), 1);
    assert_eq!(http.upgrades.load(Ordering::SeqCst), 0);
    let calls = http.calls.lock().unwrap();
    assert!(calls.is_empty());
    let calls = native.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[1].1["previous_response_id"],
        first.last().unwrap()["response"]["id"]
    );
    assert_eq!(calls[1].1["input"], "hello");
    assert!(calls[0].0);
    assert_eq!(calls[0].1["type"], "response.create");
    assert!(calls[0].1.get("stream").is_none());
    assert_eq!(store.summary(|_| 0).unwrap().request_count, 2);
}

#[tokio::test]
async fn native_handshake_rejection_keeps_the_client_on_ws_and_falls_back_to_sse() {
    let upstream = mock(404).await;
    let route = test_route(upstream.address.clone());
    let (handle, store, _dir) = start_proxy(vec![route], direct());
    let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    for _ in 0..4 {
        assert_eq!(
            turn(&mut ws, None).await.last().unwrap()["type"],
            "response.completed"
        );
    }
    assert_eq!(
        upstream.upgrades.load(Ordering::SeqCst),
        u64::from(WS_FAILURE_THRESHOLD)
    );
    assert_eq!(upstream.calls.lock().unwrap().len(), 4);
    assert!(upstream.calls.lock().unwrap().iter().all(|(ws, _)| !ws));
    assert_eq!(store.count().unwrap(), 4);
}

#[tokio::test]
async fn ws_failures_cool_down_only_that_provider_and_recover_on_same_client_connection() {
    let upstream = mock(404).await;
    let mut route = test_route(upstream.address.clone());
    route.config.conversion_enabled = true; // Request-scoped transport selection.
    let provider_id = route.targets[0].config.provider_entry_id;
    let target_id = route.targets[0].config.id;
    let (handle, _store, _dir) = start_proxy(vec![route], direct());
    let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    for _ in 0..5 {
        assert_eq!(
            turn(&mut ws, None).await.last().unwrap()["type"],
            "response.completed"
        );
    }
    assert_eq!(
        upstream.upgrades.load(Ordering::SeqCst),
        u64::from(WS_FAILURE_THRESHOLD)
    );
    assert!(!circuit_open(&handle.state, target_id)); // HTTP success does not erase WS cooldown.
    assert!(acquire_ws_transport(&handle.state, Uuid::new_v4()).is_some());
    upstream.status.store(0, Ordering::SeqCst);
    handle
        .state
        .ws_health
        .lock()
        .unwrap()
        .get_mut(&provider_id)
        .unwrap()
        .disabled_until = Some(Instant::now() - Duration::from_secs(1));
    assert_eq!(
        turn(&mut ws, None).await.last().unwrap()["type"],
        "response.completed"
    );
    assert_eq!(upstream.upgrades.load(Ordering::SeqCst), 4);
    assert!(!handle
        .state
        .ws_health
        .lock()
        .unwrap()
        .contains_key(&provider_id));
    assert!(upstream.calls.lock().unwrap().last().unwrap().0);
}

#[tokio::test]
async fn repeated_ws_disconnects_leave_sse_available_after_cooldown() {
    let upstream = mock(0).await;
    upstream.disconnect.store(1, Ordering::SeqCst);
    let mut route = test_route(upstream.address.clone());
    route.config.conversion_enabled = true;
    let target_id = route.targets[0].config.id;
    let (handle, _store, _dir) = start_proxy(vec![route], direct());
    let mut ws = fallback_session(&handle, &upstream).await;
    for _ in 0..WS_FAILURE_THRESHOLD - 1 {
        assert_eq!(turn(&mut ws, None).await.last().unwrap()["type"], "error");
    }
    assert!(!circuit_open(&handle.state, target_id));
    upstream.disconnect.store(0, Ordering::SeqCst);
    assert_eq!(
        turn(&mut ws, None).await.last().unwrap()["type"],
        "response.completed"
    );
    assert_eq!(
        upstream.upgrades.load(Ordering::SeqCst),
        u64::from(WS_FAILURE_THRESHOLD)
    );
    assert!(!upstream.calls.lock().unwrap().last().unwrap().0);
}

#[tokio::test]
async fn submitted_ws_and_sse_requests_never_replay_even_without_output_or_with_silent_retry() {
    for (http, disconnect) in [(false, 1), (false, 2), (true, 2)] {
        let upstream = mock(0).await;
        upstream.disconnect.store(disconnect, Ordering::SeqCst);
        let backup = mock(0).await;
        let mut route = test_route(upstream.address.clone());
        route.config.conversion_enabled = true;
        route.targets[0].supports_websockets = !http;
        let mut backup_target = test_route(backup.address.clone()).targets.remove(0);
        backup_target.supports_websockets = false;
        route.targets.push(backup_target);
        route.config.retry.silent_retry = true;
        route.config.retry.hold_on_failure = true;
        let (handle, store, _dir) = start_proxy(vec![route], direct());
        let mut ws = if http {
            connect(&handle, "/v1/responses", LOCAL_TOKEN)
                .await
                .unwrap()
        } else {
            backup.status.store(404, Ordering::SeqCst);
            let ws = fallback_session(&handle, &upstream).await;
            backup.status.store(0, Ordering::SeqCst);
            ws
        };
        let backup_handshakes = backup.upgrades.load(Ordering::SeqCst);
        assert_eq!(turn(&mut ws, None).await.last().unwrap()["type"], "error");
        assert_eq!(upstream.calls.lock().unwrap().len(), 1);
        assert_eq!(backup.upgrades.load(Ordering::SeqCst), backup_handshakes);
        assert!(backup.calls.lock().unwrap().is_empty());
        assert_eq!(store.count().unwrap(), 1);
        // The same client WS accepts a new explicitly requested turn.
        upstream.disconnect.store(0, Ordering::SeqCst);
        assert_eq!(
            turn(&mut ws, None).await.last().unwrap()["type"],
            "response.completed"
        );
    }
}

#[tokio::test]
async fn probe_requires_a_responses_warmup_and_distinguishes_rejection_from_uncertainty() {
    let upstream = mock(0).await;
    for (status, expected) in [
        (0, Some(true)),
        (404, Some(false)),
        (401, None),
        (429, None),
    ] {
        upstream.status.store(status, Ordering::SeqCst);
        let target = test_route(upstream.address.clone()).targets.remove(0);
        let result = tokio::task::spawn_blocking(move || {
            probe_websocket(target, &direct(), Duration::from_secs(2), Some("test"))
        })
        .await
        .unwrap();
        assert_eq!(result.supported, expected);
    }
    let calls = upstream.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1["generate"], false);
    assert_eq!(calls[0].1["input"], json!([]));
}

#[tokio::test]
async fn probe_does_not_infer_responses_support_from_a_successful_upgrade_alone() {
    let upstream = mock(0).await;
    upstream.disconnect.store(1, Ordering::SeqCst);
    for model in [None, Some("test")] {
        let target = test_route(upstream.address.clone()).targets.remove(0);
        let result = tokio::task::spawn_blocking(move || {
            probe_websocket(target, &direct(), Duration::from_secs(1), model)
        })
        .await
        .unwrap();
        assert_eq!(result.supported, None);
        assert_eq!(result.status, Some(101));
    }
    assert_eq!(upstream.calls.lock().unwrap().len(), 1);
}

#[test]
fn ws_cooldown_shares_provider_identity_allows_one_recovery_and_resets_on_reload() {
    let route = test_route("http://127.0.0.1:1".into());
    let provider_id = route.targets[0].config.provider_entry_id;
    let (handle, _, _dir) = start_proxy(vec![route.clone()], direct());
    for _ in 0..WS_FAILURE_THRESHOLD {
        mark_ws_failure(&handle.state, provider_id);
    }
    assert!(acquire_ws_transport(&handle.state, provider_id).is_none());
    handle
        .state
        .ws_health
        .lock()
        .unwrap()
        .get_mut(&provider_id)
        .unwrap()
        .disabled_until = Some(Instant::now() - Duration::from_secs(1));
    let permit = acquire_ws_transport(&handle.state, provider_id).expect("recovery permitted");
    assert!(acquire_ws_transport(&handle.state, provider_id).is_none());
    drop(permit);
    assert!(acquire_ws_transport(&handle.state, provider_id).is_some());
    handle
        .update_config(RuntimeConfig::from_routes(
            handle.bind_addr.clone(),
            vec![route],
        ))
        .unwrap();
    assert!(acquire_ws_transport(&handle.state, provider_id).is_some());
}

async fn fallback_session(handle: &ProxyHandle, upstream: &Mock) -> WebSocketStream<TcpStream> {
    let original = upstream.status.swap(404, Ordering::SeqCst);
    let ws = connect(handle, "/v1/responses", LOCAL_TOKEN).await.unwrap();
    upstream.status.store(original, Ordering::SeqCst);
    ws
}

async fn wait_for_no_connections(upstream: &Mock) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while upstream.live.load(Ordering::SeqCst) != 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("closing the client must close its upstream sockets");
}

#[tokio::test]
async fn client_close_ends_native_and_fallback_connections_including_active_turns() {
    for (fallback, active) in [(false, false), (true, false), (false, true), (true, true)] {
        let upstream = mock(0).await;
        let route = test_route(upstream.address.clone());
        let provider_id = route.targets[0].config.provider_entry_id;
        let target_id = route.targets[0].config.id;
        let (handle, _, _dir) = start_proxy(vec![route], direct());
        let mut ws = if fallback {
            fallback_session(&handle, &upstream).await
        } else {
            connect(&handle, "/v1/responses", LOCAL_TOKEN)
                .await
                .unwrap()
        };
        if active {
            upstream.disconnect.store(4, Ordering::SeqCst);
            ws.send(event(
                json!({"type":"response.create","model":"test","input":"hello"}),
            ))
            .await
            .unwrap();
            assert!(matches!(receive(&mut ws).await, Message::Text(_)));
        } else {
            assert_eq!(
                turn(&mut ws, None).await.last().unwrap()["type"],
                "response.completed"
            );
        }
        ws.close(Some(CloseFrame {
            code: CloseCode::Normal,
            reason: "client done".into(),
        }))
        .await
        .unwrap();
        wait_for_no_connections(&upstream).await;
        assert!(!circuit_open(&handle.state, target_id));
        assert_eq!(
            handle
                .state
                .ws_health
                .lock()
                .unwrap()
                .get(&provider_id)
                .map_or(0, |health| health.consecutive_failures),
            if active && fallback { 1 } else { 0 }
        );
        upstream.disconnect.store(0, Ordering::SeqCst);
        let before = upstream.upgrades.load(Ordering::SeqCst);
        let mut next = connect(&handle, "/v1/responses", LOCAL_TOKEN)
            .await
            .unwrap();
        assert_eq!(
            turn(&mut next, None).await.last().unwrap()["type"],
            "response.completed"
        );
        assert_eq!(upstream.upgrades.load(Ordering::SeqCst), before + 1);
    }
}

#[tokio::test]
async fn fallback_across_providers_reconstructs_incremental_tools_and_closes_every_socket() {
    let first = mock(404).await;
    let second = mock(404).await;
    let mut route = test_route(first.address.clone());
    route.config.strategy = RouteStrategy::RoundRobin;
    route
        .targets
        .push(test_route(second.address.clone()).targets.remove(0));
    let (handle, store, _dir) = start_proxy(vec![route], direct());
    let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    first.status.store(0, Ordering::SeqCst);
    second.status.store(0, Ordering::SeqCst);
    let tools = json!([{"type":"function","name":"exec_command","parameters":{"type":"object"}}]);
    let mut previous = Value::Null;
    for index in 0..4 {
        let output = vec![
            json!({"type":"function_call","id":format!("fc_{index}"),"call_id":format!("call_{index}"),"name":"exec_command","arguments":"{}"}),
        ];
        *first.output.lock().unwrap() = Some(output.clone());
        *second.output.lock().unwrap() = Some(output);
        let input = if index == 0 {
            json!([{"role":"user","content":"run the checks"}])
        } else {
            json!([{"type":"function_call_output","call_id":format!("call_{}",index-1),"output":"passed"}])
        };
        ws.send(event(json!({"type":"response.create","model":"test","store":false,"tools":tools,"input":input,"previous_response_id":previous}))).await.unwrap();
        let events = receive_response(&mut ws).await;
        assert_eq!(events.last().unwrap()["type"], "response.completed");
        previous = events.last().unwrap()["response"]["id"].clone();
    }
    for upstream in [&first, &second] {
        assert_eq!(
            upstream.upgrades.load(Ordering::SeqCst),
            2,
            "one rejected handshake and one retained socket per provider"
        );
        let calls = upstream.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        for (native, request) in calls.iter() {
            assert!(*native);
            assert_eq!(request["tools"], tools);
            assert!(request.get("previous_response_id").is_none());
            let input = request["input"].as_array().unwrap();
            assert_eq!(input[0]["content"], "run the checks");
            for item in input
                .iter()
                .filter(|item| item["type"] == "function_call_output")
            {
                assert!(input
                    .iter()
                    .any(|call| call["type"] == "function_call"
                        && call["call_id"] == item["call_id"]));
            }
        }
    }
    assert_eq!(store.count().unwrap(), 4);
    ws.close(None).await.unwrap();
    wait_for_no_connections(&first).await;
    wait_for_no_connections(&second).await;
}

#[tokio::test]
async fn fallback_rejects_orphan_tool_results_before_generation() {
    let upstream = mock(0).await;
    let (handle, _, _dir) = start_proxy(vec![test_route(upstream.address.clone())], direct());
    let mut ws = fallback_session(&handle, &upstream).await;
    ws.send(event(json!({"type":"response.create","model":"test","input":[{"type":"function_call_output","call_id":"missing","output":"result"}]}))).await.unwrap();
    let events = receive_response(&mut ws).await;
    assert_eq!(
        events.last().unwrap()["error"]["code"],
        "previous_response_not_found"
    );
    assert!(upstream.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn fallback_keeps_provider_bound_reasoning_on_the_originating_target() {
    let first = mock(404).await;
    let second = mock(404).await;
    let mut route = test_route(first.address.clone());
    route.config.strategy = RouteStrategy::RoundRobin;
    route
        .targets
        .push(test_route(second.address.clone()).targets.remove(0));
    let (handle, _, _dir) = start_proxy(vec![route], direct());
    let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    first.status.store(0, Ordering::SeqCst);
    second.status.store(0, Ordering::SeqCst);
    let output = vec![
        json!({"type":"reasoning","encrypted_content":"provider-bound-test-state","summary":[]}),
    ];
    *first.output.lock().unwrap() = Some(output.clone());
    *second.output.lock().unwrap() = Some(output);
    let response = turn(&mut ws, None).await;
    assert_eq!(
        turn(&mut ws, Some(&response.last().unwrap()["response"]["id"]))
            .await
            .last()
            .unwrap()["type"],
        "response.completed"
    );
    // A tools/settings change makes Codex send full input without a parent ID.
    ws.send(event(
        json!({"type":"response.create","model":"test","tools":[],"input":[
            {"role":"user","content":"continue with changed tools"},
            {"type":"reasoning","encrypted_content":"provider-bound-test-state","summary":[]}
        ]}),
    ))
    .await
    .unwrap();
    assert_eq!(
        receive_response(&mut ws).await.last().unwrap()["type"],
        "response.completed"
    );
    let counts = [
        first.calls.lock().unwrap().len(),
        second.calls.lock().unwrap().len(),
    ];
    assert!(
        counts == [3, 0] || counts == [0, 3],
        "opaque state must never move to a sibling provider: {counts:?}"
    );
}

#[tokio::test]
async fn fallback_binds_client_supplied_state_to_the_parent_origin() {
    let first = mock(404).await;
    let second = mock(404).await;
    let mut route = test_route(first.address.clone());
    route.config.strategy = RouteStrategy::RoundRobin;
    route
        .targets
        .push(test_route(second.address.clone()).targets.remove(0));
    let (handle, _, _dir) = start_proxy(vec![route], direct());
    let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    first.status.store(0, Ordering::SeqCst);
    second.status.store(0, Ordering::SeqCst);
    ws.send(event(json!({"type":"response.create","model":"test","input":[{"type":"item_reference","id":"unknown"}]}))).await.unwrap();
    assert_eq!(
        receive_response(&mut ws).await.last().unwrap()["error"]["code"],
        "previous_response_not_found"
    );
    assert!(first.calls.lock().unwrap().is_empty());
    assert!(second.calls.lock().unwrap().is_empty());
    let response = turn(&mut ws, None).await;
    ws.send(event(json!({"type":"response.create","model":"test","previous_response_id":response.last().unwrap()["response"]["id"],"input":[{"type":"item_reference","id":"item-from-parent-provider"}]}))).await.unwrap();
    assert_eq!(
        receive_response(&mut ws).await.last().unwrap()["type"],
        "response.completed"
    );
    let counts = [
        first.calls.lock().unwrap().len(),
        second.calls.lock().unwrap().len(),
    ];
    assert!(
        counts == [2, 0] || counts == [0, 2],
        "client-supplied state must stay with its parent provider: {counts:?}"
    );
}
