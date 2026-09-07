use super::*;
use serde_json::{json, Value};

struct Mock {
    address: String,
    upgrades: Arc<AtomicU64>,
    status: Arc<AtomicU64>,
    handshake_script: Arc<Mutex<VecDeque<u16>>>,
    disconnect: Arc<AtomicU64>,
    pings: Arc<AtomicU64>,
    live: Arc<AtomicU64>,
    output: Arc<Mutex<Option<Vec<Value>>>>,
    events: Arc<Mutex<Option<Vec<Value>>>>,
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
    let handshake_script = Arc::new(Mutex::new(VecDeque::<u16>::new()));
    let script = handshake_script.clone();
    let disconnect = Arc::new(AtomicU64::new(0));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let pings = Arc::new(AtomicU64::new(0));
    let ping_count = pings.clone();
    let live = Arc::new(AtomicU64::new(0));
    let active = live.clone();
    let output = Arc::new(Mutex::new(None::<Vec<Value>>));
    let output_override = output.clone();
    let events = Arc::new(Mutex::new(None::<Vec<Value>>));
    let events_override = events.clone();
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
            let script = script.clone();
            let ping_count = ping_count.clone();
            let active = active.clone();
            let output_override = output_override.clone();
            let events_override = events_override.clone();
            tokio::spawn(async move {
                let service = service_fn(move |mut request: Request<Incoming>| {
                    let script = script.clone();
                    let ping_count = ping_count.clone();
                    let active = active.clone();
                    let output_override = output_override.clone();
                    let events_override = events_override.clone();
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
                            let status = script
                                .lock()
                                .unwrap()
                                .pop_front()
                                .map(u64::from)
                                .unwrap_or_else(|| reject.load(Ordering::SeqCst));
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
                                    if let Some(scripted) = events_override.lock().unwrap().clone()
                                    {
                                        events = scripted;
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
                        if request.uri().path().ends_with("/models") {
                            return Ok(Response::builder()
                                .header("content-type", "application/json")
                                .body(Full::new(Bytes::from_static(
                                    br#"{"data":[{"id":"test"}]}"#,
                                )))
                                .unwrap());
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
        handshake_script,
        disconnect,
        calls,
        pings,
        live,
        output,
        events,
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
    assert_eq!(upstream.upgrades.load(Ordering::SeqCst), 1);
    assert_eq!(upstream.calls.lock().unwrap().len(), 4);
    assert_eq!(store.count().unwrap(), 4);
}

#[tokio::test]
async fn adapted_ws_without_session_key_keeps_one_provider_across_round_robin_turns() {
    let first = mock(0).await;
    let second = mock(0).await;
    let mut route = test_route(first.address.clone());
    route
        .targets
        .push(test_route(second.address.clone()).targets.remove(0));
    route.config.strategy = RouteStrategy::RoundRobin;
    for target in &mut route.targets {
        target.supports_websockets = false;
    }
    let (handle, _, _dir) = start_proxy(vec![route], direct());
    let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    let mut previous = Value::Null;
    for _ in 0..6 {
        let events = turn(&mut ws, Some(&previous)).await;
        assert_eq!(events.last().unwrap()["type"], "response.completed");
        previous = events.last().unwrap()["response"]["id"].clone();
    }
    let counts = [
        first.calls.lock().unwrap().len(),
        second.calls.lock().unwrap().len(),
    ];
    assert!(
        counts == [6, 0] || counts == [0, 6],
        "session must stay on one provider: {counts:?}"
    );
    ws.close(None).await.unwrap();
}

#[tokio::test]
async fn native_ws_rejects_new_generations_while_target_is_blacklisted() {
    let upstream = mock(0).await;
    let mut route = test_route(upstream.address.clone());
    route.config.retry.failure_threshold = 1;
    let id = route.targets[0].config.id;
    let retry = route.config.retry.clone();
    let (handle, _, _dir) = start_proxy(vec![route], direct());
    let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    assert_eq!(
        turn(&mut ws, None).await.last().unwrap()["type"],
        "response.completed"
    );
    mark_failure(&handle.state, id, &retry);
    let events = turn(&mut ws, None).await;
    assert_eq!(
        events.last().unwrap()["error"]["code"],
        "provider_temporarily_unavailable"
    );
    assert_eq!(upstream.calls.lock().unwrap().len(), 1);
    ws.close(None).await.unwrap();
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

fn notified_responses() -> Vec<Value> {
    let mut events = responses();
    events.insert(
        0,
        json!({"type":"codex.rate_limits","rate_limits":{"remaining":42}}),
    );
    events.insert(
        2,
        json!({"type":"provider.notification","metadata":{"opaque":[1,"value"]}}),
    );
    events
}

#[tokio::test]
async fn http_ws_preserves_auxiliary_events_and_waits_for_response_completion() {
    let upstream = mock(0).await;
    let events = notified_responses();
    *upstream.events.lock().unwrap() = Some(events.clone());
    let (handle, store, _dir) = start_proxy(vec![test_route(upstream.address.clone())], direct());
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    for streaming in [true, false] {
        let response = client
            .post(format!("http://{}/v1/responses", handle.bind_addr))
            .bearer_auth(LOCAL_TOKEN)
            .json(&json!({"model":"test","input":"hello","stream":streaming}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        if streaming {
            let actual: Vec<Value> = response
                .text()
                .await
                .unwrap()
                .lines()
                .filter_map(|line| line.strip_prefix("data: "))
                .map(|data| serde_json::from_str(data).unwrap())
                .collect();
            assert_eq!(actual, events);
        } else {
            assert_eq!(
                response.json::<Value>().await.unwrap(),
                events.last().unwrap()["response"]
            );
        }
    }
    assert_eq!(upstream.calls.lock().unwrap().len(), 2);
    assert_eq!(store.summary(|_| 0).unwrap().successful_attempts, 2);
}

#[tokio::test]
async fn native_and_adapted_ws_preserve_auxiliary_notifications() {
    for adapted in [false, true] {
        let upstream = mock(0).await;
        let expected = notified_responses();
        *upstream.events.lock().unwrap() = Some(expected.clone());
        let (handle, store, _dir) =
            start_proxy(vec![test_route(upstream.address.clone())], direct());
        let mut ws = if adapted {
            fallback_session(&handle, &upstream).await
        } else {
            connect(&handle, "/v1/responses", LOCAL_TOKEN)
                .await
                .unwrap()
        };
        let events = turn(&mut ws, None).await;
        assert_eq!(events.len(), expected.len());
        assert_eq!(events[0], expected[0]);
        assert_eq!(events[2], expected[2]);
        assert_eq!(events.last().unwrap()["type"], "response.completed");
        assert_eq!(store.summary(|_| 0).unwrap().successful_attempts, 1);
        assert_eq!(upstream.calls.lock().unwrap().len(), 1);
        ws.close(None).await.unwrap();
    }
}

#[tokio::test]
async fn http_ws_auxiliary_or_malformed_events_cannot_finish_or_replay_a_request() {
    for streaming in [true, false] {
        for event in [
            json!({"type":"codex.rate_limits","rate_limits":{"remaining":42}}),
            json!({"type":"response.completed"}),
            json!({"type":""}),
            json!({"type":42}),
            json!([]),
        ] {
            let upstream = mock(0).await;
            *upstream.events.lock().unwrap() = Some(vec![event]);
            upstream.disconnect.store(3, Ordering::SeqCst);
            let mut route = test_route(upstream.address.clone());
            route.config.retry.silent_retry = true;
            let (handle, store, _dir) = start_proxy(vec![route], direct());
            let response = reqwest::Client::builder()
                .no_proxy()
                .build()
                .unwrap()
                .post(format!("http://{}/v1/responses", handle.bind_addr))
                .bearer_auth(LOCAL_TOKEN)
                .json(&json!({"model":"test","input":"hello","stream":streaming}))
                .send()
                .await
                .unwrap();
            if streaming {
                assert!(response.bytes().await.is_err());
            } else {
                assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
            }
            assert_eq!(upstream.calls.lock().unwrap().len(), 1);
            let summary = store.summary(|_| 0).unwrap();
            assert_eq!(summary.request_count, 1);
            assert_eq!(summary.successful_attempts, 0);
            assert_eq!(summary.average_first_token_ms, None);
            assert!(handle.websocket_capability_events().is_empty());
        }
    }
}

#[tokio::test]
async fn probe_skips_auxiliary_events_but_requires_a_valid_empty_warmup_completion() {
    for ending in [
        Some(json!({"type":"response.completed","response":{"id":"warmup","output":[]}})),
        Some(json!({"type":"response.completed"})),
        Some(json!({"type":"response.completed","response":{"id":"","output":[]}})),
        Some(
            json!({"type":"response.completed","response":{"id":"warmup","output":[{"type":"message"}]}}),
        ),
        Some(json!({"type":"error","error":{"code":"rate_limit_exceeded"}})),
        None,
    ] {
        let upstream = mock(0).await;
        let mut events = notified_responses();
        events.truncate(3); // Notification, response.created, notification.
        let expected = ending.as_ref().is_some_and(|event| {
            event.pointer("/response/output") == Some(&json!([]))
                && event["response"]["id"] == "warmup"
        });
        events.extend(ending);
        *upstream.events.lock().unwrap() = Some(events);
        upstream.disconnect.store(3, Ordering::SeqCst);
        let target = test_route(upstream.address.clone()).targets.remove(0);
        let result = tokio::task::spawn_blocking(move || {
            probe_websocket(target, &direct(), Duration::from_secs(2), Some("test"))
        })
        .await
        .unwrap();
        assert_eq!(result.supported, expected.then_some(true));
        assert_eq!(result.status, Some(101));
        let calls = upstream.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1["generate"], false);
        assert_eq!(calls[0].1["input"], json!([]));
    }
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
    assert_eq!(upstream.upgrades.load(Ordering::SeqCst), 3);
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
        assert_eq!(upstream.upgrades.load(Ordering::SeqCst), 1);
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
    assert_eq!(upstream.upgrades.load(Ordering::SeqCst), 2);
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
    assert_eq!(upstream.upgrades.load(Ordering::SeqCst), 2);
    assert_eq!(upstream.calls.lock().unwrap().len(), 4);
    assert!(upstream.calls.lock().unwrap().iter().all(|(ws, _)| !ws));
    assert_eq!(store.count().unwrap(), 4);
}

#[tokio::test]
async fn temporary_handshake_failures_stay_in_the_session_and_new_sessions_retry_ws() {
    for status in [426, 502, 503] {
        let upstream = mock(status).await;
        let route = test_route(upstream.address.clone());
        let (handle, _, _dir) = start_proxy(vec![route], direct());
        for round in 0..4 {
            let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
                .await
                .unwrap();
            upstream.status.store(0, Ordering::SeqCst);
            for _ in 0..2 {
                assert_eq!(
                    turn(&mut ws, None).await.last().unwrap()["type"],
                    "response.completed"
                );
                assert!(!upstream.calls.lock().unwrap().last().unwrap().0);
            }
            assert_eq!(upstream.upgrades.load(Ordering::SeqCst), (round + 1) * 2);
            assert!(handle.websocket_capability_events().is_empty());
            ws.close(None).await.unwrap();
            upstream.status.store(status, Ordering::SeqCst);
        }
        upstream.status.store(0, Ordering::SeqCst);
        let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
            .await
            .unwrap();
        assert_eq!(
            turn(&mut ws, None).await.last().unwrap()["type"],
            "response.completed"
        );
        assert!(upstream.calls.lock().unwrap().last().unwrap().0);
    }
}

#[tokio::test]
async fn repeated_submitted_ws_disconnects_never_disable_or_fall_back() {
    let upstream = mock(0).await;
    upstream.disconnect.store(1, Ordering::SeqCst);
    let route = test_route(upstream.address.clone());
    let (handle, _, _dir) = start_proxy(vec![route], direct());
    let mut ws = fallback_session(&handle, &upstream).await;
    for _ in 0..4 {
        assert_eq!(turn(&mut ws, None).await.last().unwrap()["type"], "error");
    }
    upstream.disconnect.store(0, Ordering::SeqCst);
    assert_eq!(
        turn(&mut ws, None).await.last().unwrap()["type"],
        "response.completed"
    );
    assert_eq!(upstream.upgrades.load(Ordering::SeqCst), 5);
    assert!(upstream.calls.lock().unwrap().iter().all(|(ws, _)| *ws));
    assert!(handle.websocket_capability_events().is_empty());
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
        (426, None),
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

// Exercise the bridge's upstream-WS path directly. A native handshake failure
// deliberately pins the real downstream session to HTTP, so it cannot be used
// as a fixture for socket reuse, heartbeats or submitted WS failures.
async fn fallback_session(handle: &ProxyHandle, _upstream: &Mock) -> WebSocketStream<TcpStream> {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let state = handle.state.clone();
    let route = state.config.read().unwrap().routes[0].clone();
    tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let service = service_fn(move |request: Request<Incoming>| {
            let state = state.clone();
            let route = route.clone();
            async move {
                let response = create_response_with_body(&request, empty_body).unwrap();
                let mut changed = ConfigWatch::subscribe(&state);
                changed.scope(route.config.id);
                Ok::<_, Infallible>(bridge::upgrade(
                    request,
                    state,
                    route,
                    vec![],
                    changed,
                    response,
                    Arc::new(pool::Pool::default()),
                ))
            }
        });
        let _ = hyper::server::conn::http1::Builder::new()
            .serve_connection(TokioIo::new(socket), service)
            .with_upgrades()
            .await;
    });
    let stream = TcpStream::connect(address).await.unwrap();
    client_async(format!("ws://{address}/v1/responses"), stream)
        .await
        .unwrap()
        .0
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
        assert!(handle.websocket_capability_events().is_empty());
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
    let first = mock(0).await;
    let second = mock(0).await;
    let mut route = test_route(first.address.clone());
    route.config.strategy = RouteStrategy::RoundRobin;
    route
        .targets
        .push(test_route(second.address.clone()).targets.remove(0));
    route.config.retry.failure_threshold = 1;
    let targets = route.targets.clone();
    let retry = route.config.retry.clone();
    let (handle, store, _dir) = start_proxy(vec![route], direct());
    let mut ws = fallback_session(&handle, &first).await;
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
        if index == 1 {
            // Provider changes require a real failure, not round-robin churn.
            let failed = if first.calls.lock().unwrap().len() == 2 {
                0
            } else {
                1
            };
            mark_failure(&handle.state, targets[failed].config.id, &retry);
        }
    }
    for upstream in [&first, &second] {
        assert_eq!(
            upstream.upgrades.load(Ordering::SeqCst),
            1,
            "one retained socket per provider"
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
    let first = mock(0).await;
    let second = mock(0).await;
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
    let first = mock(0).await;
    let second = mock(0).await;
    let mut route = test_route(first.address.clone());
    route.config.strategy = RouteStrategy::RoundRobin;
    route
        .targets
        .push(test_route(second.address.clone()).targets.remove(0));
    let (handle, _, _dir) = start_proxy(vec![route], direct());
    let mut ws = fallback_session(&handle, &first).await;
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

#[tokio::test]
async fn first_handshake_failure_reconnects_once_before_any_generation() {
    for bridge in [false, true] {
        let upstream = mock(0).await;
        upstream.handshake_script.lock().unwrap().extend([503, 0]);
        let (handle, _, _dir) = start_proxy(vec![test_route(upstream.address.clone())], direct());
        let mut ws = if bridge {
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
        assert_eq!(upstream.upgrades.load(Ordering::SeqCst), 2);
        assert_eq!(upstream.calls.lock().unwrap().len(), 1);
        assert!(upstream.calls.lock().unwrap()[0].0);
        assert!(handle.websocket_capability_events().is_empty());
    }
}

async fn http_turn(handle: &ProxyHandle, session: Option<&str>) -> (StatusCode, String) {
    let mut request = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://{}/v1/responses", handle.bind_addr))
        .bearer_auth(LOCAL_TOKEN)
        .json(&json!({"model":"test","input":"hello","stream":true}));
    if let Some(session) = session {
        request = request.header("x-aipass-session-id", session);
    }
    let response = request.send().await.unwrap();
    (response.status(), response.text().await.unwrap_or_default())
}

#[tokio::test]
async fn http_fallback_uses_session_identity_or_only_the_current_request() {
    let upstream = mock(503).await;
    let (handle, _, _dir) = start_proxy(vec![test_route(upstream.address.clone())], direct());
    for (session, attempts) in [
        (Some("one"), 2),
        (Some("one"), 2),
        (Some("two"), 4),
        (None, 6),
        (None, 8),
    ] {
        assert!(http_turn(&handle, session)
            .await
            .1
            .contains("response.completed"));
        assert_eq!(upstream.upgrades.load(Ordering::SeqCst), attempts);
        assert!(handle.websocket_capability_events().is_empty());
    }
    upstream.status.store(0, Ordering::SeqCst);
    http_turn(&handle, Some("three")).await;
    assert!(upstream.calls.lock().unwrap().last().unwrap().0);
}

#[tokio::test]
async fn only_repeated_protocol_rejection_with_responses_http_success_confirms_unsupported() {
    for status in [404, 405, 501] {
        let upstream = mock(status).await;
        upstream.disconnect.store(2, Ordering::SeqCst); // Incomplete HTTP is insufficient.
        let (handle, _, _dir) = start_proxy(vec![test_route(upstream.address.clone())], direct());
        http_turn(&handle, Some("one")).await;
        assert!(handle.websocket_capability_events().is_empty());
        let models = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://{}/v1/models", handle.bind_addr))
            .bearer_auth(LOCAL_TOKEN)
            .send()
            .await
            .unwrap();
        assert!(models.status().is_success());
        assert!(handle.websocket_capability_events().is_empty());
        upstream.disconnect.store(0, Ordering::SeqCst);
        http_turn(&handle, Some("one")).await;
        let events = handle.websocket_capability_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].status, status as u16);
        http_turn(&handle, Some("two")).await;
        assert_eq!(upstream.upgrades.load(Ordering::SeqCst), 2);
        assert_eq!(handle.websocket_capability_events(), events);
        handle.acknowledge_websocket_capability_event(&events[0]);
        assert!(handle.websocket_capability_events().is_empty());
    }
    let upstream = mock(503).await;
    upstream.handshake_script.lock().unwrap().extend([404, 503]);
    let (handle, _, _dir) = start_proxy(vec![test_route(upstream.address.clone())], direct());
    http_turn(&handle, None).await;
    assert!(handle.websocket_capability_events().is_empty());
}

#[tokio::test]
async fn auth_and_quota_handshake_errors_preserve_status_without_http_replay() {
    for status in [400, 401, 403, 429] {
        let upstream = mock(status).await;
        let (handle, _, _dir) = start_proxy(vec![test_route(upstream.address.clone())], direct());
        assert_eq!(http_turn(&handle, None).await.0.as_u16(), status as u16);
        assert_eq!(upstream.upgrades.load(Ordering::SeqCst), 1);
        assert!(upstream.calls.lock().unwrap().is_empty());
        assert!(handle.websocket_capability_events().is_empty());
    }
}

#[test]
fn capability_evidence_isolated_by_config_and_invalidated_by_success_or_reload() {
    let route = test_route("http://127.0.0.1:1/v1".into());
    let target = &route.targets[0];
    let (handle, _, _dir) = start_proxy(vec![route.clone()], direct());
    let key = capability::key(&handle.state, target);
    let observation = capability::observe(&handle.state, key);
    let started = Instant::now();
    let evidence = capability::candidate(
        &handle.state,
        target,
        observation,
        started,
        StatusCode::NOT_FOUND,
    )
    .unwrap();
    assert!(handle.websocket_capability_events().is_empty());
    capability::success(&handle.state, observation);
    capability::http_success(&handle.state, Some(&evidence));
    assert!(handle.websocket_capability_events().is_empty());
    assert!(capability::candidate(
        &handle.state,
        target,
        observation,
        started,
        StatusCode::NOT_FOUND
    )
    .is_none());
    let mut next = route.clone();
    next.targets[0].api_key = "rotated".into();
    let mut config = RuntimeConfig::from_routes(handle.bind_addr.clone(), vec![next]);
    config.upstream_proxy = direct();
    handle.update_config(config.clone()).unwrap();
    assert!(capability::candidate(
        &handle.state,
        target,
        observation,
        Instant::now(),
        StatusCode::NOT_FOUND
    )
    .is_none());
    config.routes = vec![route.clone()];
    handle.update_config(config).unwrap();
    // Even switching away and back cannot admit old operations.
    assert!(capability::candidate(
        &handle.state,
        target,
        observation,
        Instant::now(),
        StatusCode::NOT_FOUND
    )
    .is_none());
    let fresh = capability::observe(&handle.state, key);
    let evidence = capability::candidate(
        &handle.state,
        target,
        fresh,
        Instant::now(),
        StatusCode::NOT_FOUND,
    )
    .unwrap();
    capability::success(&handle.state, observation); // Stale success cannot erase new evidence.
    capability::http_success(&handle.state, Some(&evidence));
    assert_eq!(handle.websocket_capability_events().len(), 1);
    let event = handle.websocket_capability_events().remove(0);
    capability::success(&handle.state, fresh);
    assert!(handle
        .with_websocket_capability_event(&event, || panic!("stale event must not persist"))
        .is_none());
    assert!(capability::candidate(
        &handle.state,
        target,
        fresh,
        Instant::now() - SESSION_AFFINITY_TTL,
        StatusCode::NOT_FOUND
    )
    .is_none());
    let mut sibling = target.clone();
    sibling.config.provider_entry_id = Uuid::new_v4();
    assert_ne!(websocket_config_key(&sibling, &direct()), key);
}

#[tokio::test]
async fn preference_only_refresh_keeps_native_and_fallback_sessions_alive() {
    for status in [0, 404] {
        let upstream = mock(status).await;
        let mut route = test_route(upstream.address.clone());
        let (handle, _, _dir) = start_proxy(vec![route.clone()], direct());
        let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
            .await
            .unwrap();
        turn(&mut ws, None).await;
        route.targets[0].supports_websockets = false;
        let mut config = RuntimeConfig::from_routes(handle.bind_addr.clone(), vec![route]);
        config.upstream_proxy = direct();
        handle.update_config(config).unwrap();
        assert_eq!(
            turn(&mut ws, None).await.last().unwrap()["type"],
            "response.completed"
        );
    }
}

#[tokio::test]
async fn provider_concurrency_routes_each_ws_generation_and_idle_sockets_take_no_slots() {
    let primary = mock(0).await;
    let backup = mock(0).await;
    *primary.events.lock().unwrap() = Some(responses()[..2].to_vec());
    let mut route = test_route(primary.address.clone());
    route.targets[0].max_concurrent_requests = Some(1);
    route.config.retry.max_attempts = 1;
    route
        .targets
        .push(test_route(backup.address.clone()).targets.remove(0));
    let (handle, _, _dir) = start_proxy(vec![route], direct());
    let mut a = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    let mut b = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    assert!(handle.state.provider_activity.lock().unwrap().is_empty());
    a.send(event(
        json!({"type":"response.create","model":"test","input":"hello"}),
    ))
    .await
    .unwrap();
    for _ in 0..2 {
        receive(&mut a).await;
    }
    assert_eq!(
        handle
            .state
            .provider_activity
            .lock()
            .unwrap()
            .values()
            .sum::<u64>(),
        1
    );
    let result = turn(&mut b, None).await;
    assert_eq!(result.last().unwrap()["type"], "response.completed");
    assert_eq!(primary.calls.lock().unwrap().len(), 1);
    assert_eq!(backup.calls.lock().unwrap().len(), 1);
    a.close(None).await.unwrap();
    b.close(None).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while !handle.state.provider_activity.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    *primary.events.lock().unwrap() = None;
    let mut c = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    assert_eq!(
        turn(&mut c, None).await.last().unwrap()["type"],
        "response.completed"
    );
    assert_eq!(primary.calls.lock().unwrap().len(), 2);
    c.close(None).await.unwrap();
}

#[tokio::test]
async fn provider_concurrency_limits_apply_to_native_ws_sessions_opened_before_the_change() {
    let upstream = mock(0).await;
    *upstream.events.lock().unwrap() = Some(responses()[..2].to_vec());
    let route = test_route(upstream.address.clone());
    let (handle, _, _dir) = start_proxy(vec![route], direct());
    let mut a = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    let mut b = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    a.send(event(
        json!({"type":"response.create","model":"test","input":"hello"}),
    ))
    .await
    .unwrap();
    for _ in 0..2 {
        receive(&mut a).await;
    }
    let mut config = handle.state.config.read().unwrap().clone();
    config.routes[0].targets[0].max_concurrent_requests = Some(1);
    handle.update_config(config).unwrap();
    let rejected = turn(&mut b, None).await;
    assert_eq!(rejected.last().unwrap()["status"], 429);
    assert_eq!(
        rejected.last().unwrap()["error"]["code"],
        "provider_concurrency_limit"
    );
    assert_eq!(upstream.calls.lock().unwrap().len(), 1);
    a.close(None).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while !handle.state.provider_activity.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    *upstream.events.lock().unwrap() = None;
    assert_eq!(
        turn(&mut b, None).await.last().unwrap()["type"],
        "response.completed"
    );
    b.close(None).await.unwrap();
}

#[tokio::test]
async fn provider_concurrency_ws_distinguishes_local_capacity_from_upstream_rate_limits() {
    for local_capacity in [false, true] {
        let upstream = mock(if local_capacity { 0 } else { 429 }).await;
        let mut route = test_route(upstream.address.clone());
        route.targets[0].max_concurrent_requests = Some(1);
        let target = route.targets[0].clone();
        let (handle, _, _dir) = start_proxy(vec![route], direct());
        let _occupied =
            local_capacity.then(|| ProviderPermit::acquire(&handle.state, &target).unwrap());
        let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
            .await
            .unwrap();
        let events = turn(&mut ws, None).await;
        let error = events.last().unwrap();
        assert_eq!(error["status"], 429);
        assert_eq!(
            error["error"]["code"],
            if local_capacity {
                "provider_concurrency_limit"
            } else {
                "upstream_error"
            }
        );
        if local_capacity {
            assert_eq!(upstream.upgrades.load(Ordering::SeqCst), 0);
        }
        ws.close(None).await.unwrap();
    }
}
