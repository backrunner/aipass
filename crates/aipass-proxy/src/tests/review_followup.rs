use super::local_proxy_review::direct_proxy;
use super::*;
use serde_json::json;
use ProxyProtocol::{AnthropicMessages as AM, OpenAiChatCompletions as CC, OpenAiResponses as RS};

#[test]
fn codex_collection_never_labels_post_output_errors_as_safe_to_retry() {
    for before in [
        json!({"type":"response.output_text.delta","delta":"already generated"}),
        json!({"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","call_id":"write","name":"write","arguments":"{}"}}),
        json!({"type":"response.failed","response":{"error":{"message":"failed"},"output":[{"type":"function_call","call_id":"write","name":"write","arguments":"{}"}]}}),
        json!({"type":"response.completed","response":{"id":"done","output":[]}}),
    ] {
        let wire=format!("data: {before}\n\ndata: {{\"type\":\"response.failed\",\"response\":{{\"error\":{{\"message\":\"failed\"}}}}}}\n\n");
        assert!(
            !codex::collect_response(wire.as_bytes()).unwrap_err().0,
            "unsafe retry: {before}"
        );
    }
    assert!(
        codex::collect_response(b"data: {\"type\":\"response.failed\"}\n\n")
            .unwrap_err()
            .0
    );
}

#[test]
fn failed_stream_conversion_is_not_accounted_as_a_success_or_replayed() {
    for silent in [false, true] {
        let body=[
            json!({"choices":[{"index":0,"delta":{"content":"started work"}}]}),
            json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"write","type":"function","function":{"name":"write","arguments":"{"}}]}}]}),
            json!({"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":4,"completion_tokens":2}}),
        ].into_iter().map(|v|format!("data: {v}\n\n")).collect::<String>()+"data: [DONE]\n\n";
        let (addr, upstream) = mock_upstream("200 OK", "text/event-stream", body);
        let backup = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let first = conversion_target(format!("http://{addr}"), 0, CC);
        let id = first.config.id;
        let mut route = conversion_route(
            "conversion-failure",
            AM,
            CC,
            vec![
                first,
                conversion_target(format!("http://{}", backup.local_addr().unwrap()), 1, RS),
            ],
            2,
        );
        route.config.retry.silent_retry = silent;
        let proxy = direct_proxy(route);
        let response=reqwest::blocking::Client::builder().no_proxy().timeout(Duration::from_secs(3)).build().unwrap()
            .post(format!("http://{}/v1/messages",proxy.bind_addr)).bearer_auth("conversion-failure")
            .json(&json!({"model":"fixture","stream":true,"messages":[{"role":"user","content":"write"}]})).send();
        if let Ok(response) = response {
            assert!(response.bytes().is_err());
        }
        upstream.join().unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while proxy.state.usage.count().unwrap() == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let rows = proxy.state.usage.iter_rows().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].status, 502,
            "conversion failure recorded as success"
        );
        assert_eq!(rows[0].input_tokens, 4);
        assert_eq!(rows[0].output_tokens, 2);
        assert_eq!(
            proxy
                .state
                .usage
                .summary(|_| 0)
                .unwrap()
                .successful_attempts,
            0
        );
        assert!(proxy
            .state
            .health
            .lock()
            .unwrap()
            .get(&id)
            .is_some_and(|h| h.consecutive_failures > 0));
        mock_upstream::assert_no_api_request(&backup);
    }
}

fn tracking(proxy: &ProxyHandle, route: &ResolvedRoute) -> UsageTrackingContext {
    let id = Uuid::new_v4();
    let target = &route.targets[0];
    let started = Instant::now();
    let mut changed = ConfigWatch::subscribe(&proxy.state);
    changed.scope(route.config.id);
    UsageTrackingContext {
        ws_evidence: None,
        protocol: CC,
        store: proxy.state.usage.clone(),
        pricing: None,
        streaming: true,
        attempt_started: started,
        started,
        failure_state: proxy.state.clone(),
        config_changed: changed,
        route_id: route.config.id,
        target_id: target.config.id,
        session_key: Some("delivery-session".into()),
        retry_policy: route.config.retry.clone(),
        record: UsageRecord {
            id,
            started_at: now_unix(),
            duration_ms: 0,
            first_token_ms: None,
            route_id: route.config.id,
            provider_entry_id: target.config.provider_entry_id,
            secret_id: target.config.secret_id.clone(),
            model: None,
            inbound_protocol: RS,
            upstream_protocol: CC,
            status: 200,
            attempts: 1,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            cache_creation_1h_tokens: 0,
            estimated_cost_micros: 0,
        },
        attempt: Some((
            AttemptRecord {
                id: Uuid::new_v4(),
                request_id: Some(id),
                started_at: now_unix(),
                duration_ms: 0,
                first_token_ms: None,
                route_id: route.config.id,
                target_id: target.config.id,
                provider_entry_id: target.config.provider_entry_id,
                secret_id: target.config.secret_id.clone(),
                model: None,
                status: Some(200),
                success: None,
            },
            started,
        )),
    }
}

#[tokio::test]
async fn conversion_delivery_finishes_without_deadlock_and_distinguishes_cancellation() {
    for outcome in [
        "complete",
        "cancel",
        "reconfigure",
        "transport_error",
        "conversion_error",
    ] {
        let route = conversion_route(
            "delivery",
            RS,
            CC,
            vec![conversion_target("http://127.0.0.1:1".into(), 0, CC)],
            1,
        );
        let target = route.targets[0].config.id;
        let proxy = direct_proxy(route.clone());
        let first = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"}}]}\n\n";
        let source: UpstreamBodyStream = match outcome {
            "cancel" | "reconfigure" => Box::pin(
                stream::once(async move { Ok(Bytes::from_static(first.as_bytes())) })
                    .chain(stream::pending()),
            ),
            "transport_error" => Box::pin(stream::once(async {
                Err(std::io::Error::other("synthetic upstream disconnect").into())
            })),
            "conversion_error" => Box::pin(
                stream::once(async {
                    Ok(Bytes::from_static(
                        b"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":42}}]}\n\n",
                    ))
                })
                .chain(stream::pending()),
            ),
            _ => Box::pin(stream::once(async move {
                Ok(Bytes::from(format!("{first}data: {{\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\ndata: [DONE]\n\n")))
            })),
        };
        let mut output = track_converted_usage_stream(source, tracking(&proxy, &route), RS);
        loop {
            let next = tokio::time::timeout(Duration::from_secs(2), output.next())
                .await
                .expect("conversion deadlocked");
            match next {
                Some(Ok(bytes)) if outcome == "cancel" || stream_reports_completion(RS, &bytes) => {
                    break
                }
                Some(Ok(_)) if outcome == "reconfigure" => {
                    let mut config = proxy.state.config.read().unwrap().clone();
                    config.routes[0].targets[0].api_key = "rotated-fixture".into();
                    proxy.update_config(config).unwrap();
                    while let Some(item) =
                        tokio::time::timeout(Duration::from_secs(2), output.next())
                            .await
                            .expect("reconfiguration deadlocked")
                    {
                        if item.is_err() {
                            break;
                        }
                    }
                    break;
                }
                Some(Err(_)) => {
                    assert!(matches!(outcome, "transport_error" | "conversion_error"));
                    break;
                }
                Some(Ok(_)) => {}
                None => panic!("missing expected delivery outcome"),
            }
        }
        // A WS consumer stops at response.completed without polling EOF.
        drop(output);
        if outcome != "cancel" {
            assert_eq!(proxy.state.usage.count().unwrap(), 1, "{outcome}");
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while proxy.state.usage.count().unwrap() == 0 && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(
            proxy.state.usage.count().unwrap(),
            1,
            "unfinished accounting: {outcome}"
        );
        let summary = proxy.state.usage.summary(|_| 0).unwrap();
        assert_eq!(
            summary.successful_attempts,
            u64::from(outcome == "complete")
        );
        assert_eq!(
            summary.completed_attempts,
            u64::from(matches!(
                outcome,
                "complete" | "transport_error" | "conversion_error"
            ))
        );
        let failures = proxy
            .state
            .health
            .lock()
            .unwrap()
            .get(&target)
            .map_or(0, |h| h.consecutive_failures);
        assert_eq!(
            failures,
            u8::from(matches!(outcome, "transport_error" | "conversion_error"))
        );
    }
}

#[test]
fn codex_non_streaming_error_after_work_never_reaches_another_account() {
    for inbound in [AM, CC, RS] {
        let wire = format!(
            "data: {}\n\ndata: {}\n\n",
            json!({"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","call_id":"performed","name":"write","arguments":"{}"}}),
            json!({"type":"response.failed","response":{"error":{"message":"failed after work"}}})
        );
        let (addr, upstream) = mock_upstream("200 OK", "text/event-stream", wire);
        let backup = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut first = conversion_target(format!("http://{addr}"), 0, RS);
        first.upstream_kind = UpstreamKind::CodexSubscription;
        let proxy = direct_proxy(conversion_route(
            "codex-no-replay",
            inbound,
            RS,
            vec![
                first,
                conversion_target(
                    format!("http://{}", backup.local_addr().unwrap()),
                    1,
                    inbound,
                ),
            ],
            2,
        ));
        let payload=BuiltinConversionPlugin.convert_request(AM,inbound,json!({"model":"fixture","stream":false,"messages":[{"role":"user","content":"write fixture"}]})).unwrap();
        let response = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap()
            .post(format!("http://{}{}", proxy.bind_addr, inbound.path()))
            .bearer_auth("codex-no-replay")
            .json(&payload)
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let (_, sent) = upstream.join().unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&sent).unwrap()["stream"],
            true
        );
        mock_upstream::assert_no_api_request(&backup);
    }
}

#[tokio::test]
async fn converted_sse_is_independent_of_network_byte_boundaries() {
    fn without_clock(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                map.remove("created");
                map.remove("created_at");
                for value in map.values_mut() {
                    without_clock(value);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    without_clock(item);
                }
            }
            _ => {}
        }
    }
    async fn converted(
        wire: &[u8],
        cut: usize,
        from: ProxyProtocol,
        to: ProxyProtocol,
    ) -> Vec<serde_json::Value> {
        let chunks = vec![
            Ok::<_, BoxError>(Bytes::copy_from_slice(&wire[..cut])),
            Ok(Bytes::copy_from_slice(&wire[cut..])),
        ];
        let mut output = convert_sse_stream(stream::iter(chunks), from, to);
        let mut events = Vec::new();
        while let Some(chunk) = output.next().await {
            let chunk = chunk.unwrap();
            if let Some(data) = sse_event_data(&chunk) {
                if data == b"[DONE]" {
                    continue;
                }
                let mut value: serde_json::Value = serde_json::from_slice(&data).unwrap();
                without_clock(&mut value);
                events.push(value);
            }
        }
        events
    }
    let frames = [
        json!({"id":"chatcmpl_fixture","created":1,"model":"fixture","choices":[{"index":0,"delta":{"role":"assistant","content":"中文🙂"}}]}),
        json!({"id":"chatcmpl_fixture","created":1,"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call-fixture","type":"function","function":{"name":"read","arguments":"{\"path\":\"中.txt\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":4,"completion_tokens":2}}),
    ];
    for from in [AM, CC, RS] {
        let mut native = StreamConverter::new(CC, from).unwrap();
        let mut wire = Vec::new();
        for frame in &frames {
            wire.extend(native.push_event(&format!("data: {frame}\n\n")).unwrap());
        }
        wire.extend(native.push_event("data: [DONE]\n\n").unwrap());
        wire.extend(native.finish().unwrap());
        let wire = wire.join("").replace('\n', "\r\n");
        for to in [AM, CC, RS].into_iter().filter(|p| *p != from) {
            let expected = converted(wire.as_bytes(), 0, from, to).await;
            for cut in 1..wire.len() {
                assert_eq!(
                    converted(wire.as_bytes(), cut, from, to).await,
                    expected,
                    "{from:?}->{to:?} split at byte {cut}"
                );
            }
        }
    }
}

#[test]
fn ambiguous_success_bodies_never_replay_on_another_target() {
    for inbound in [AM, CC, RS] {
        let upstream = if inbound == AM { CC } else { AM };
        for silent in [false, true] {
            for bad in ["", "{}", "not-json", "null"] {
                let (addr, first) = mock_upstream("200 OK", "application/json", bad.into());
                let backup = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
                let mut route = conversion_route(
                    "ambiguous",
                    inbound,
                    upstream,
                    vec![
                        conversion_target(format!("http://{addr}"), 0, upstream),
                        conversion_target(
                            format!("http://{}", backup.local_addr().unwrap()),
                            1,
                            inbound,
                        ),
                    ],
                    2,
                );
                route.config.retry.silent_retry = silent;
                let proxy = direct_proxy(route);
                let body = BuiltinConversionPlugin.convert_request(AM, inbound, json!({"model":"fixture", "messages":[{"role":"user","content":"perform action"}]})).unwrap();
                let response = reqwest::blocking::Client::builder()
                    .no_proxy()
                    .timeout(Duration::from_secs(3))
                    .build()
                    .unwrap()
                    .post(format!("http://{}{}", proxy.bind_addr, inbound.path()))
                    .bearer_auth("ambiguous")
                    .json(&body)
                    .send()
                    .unwrap();
                assert_eq!(
                    response.status(),
                    StatusCode::BAD_GATEWAY,
                    "{inbound:?} silent={silent}: {bad}"
                );
                first.join().unwrap();
                mock_upstream::assert_no_api_request(&backup);
                let summary = proxy.state.usage.summary(|_| 0).unwrap();
                assert_eq!(summary.attempt_count, 1);
                assert_eq!(summary.successful_attempts, 0);
            }
        }
    }
}

#[tokio::test]
async fn sse_line_endings_bom_and_heartbeats_preserve_fragmented_output() {
    for from in [AM, CC, RS] {
        let frames = match from {
            AM => vec![
                json!({"type":"message_start","message":{"id":"msg_fixture","model":"fixture"}}),
                json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":"首🙂"}}),
                json!({"type":"content_block_stop","index":0}),
                json!({"type":"message_delta","delta":{"stop_reason":"end_turn"}}),
                json!({"type":"message_stop"}),
            ],
            CC => vec![
                json!({"id":"chatcmpl_fixture","choices":[{"index":0,"delta":{"content":"首🙂"},"finish_reason":"stop"}]}),
            ],
            RS => vec![
                json!({"type":"response.completed","response":{"id":"resp_fixture","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"首🙂"}]}]}}),
            ],
        };
        for newline in ["\n", "\r\n", "\r"] {
            for bom in ["", "\u{feff}"] {
                let mut wire = bom.to_string();
                for frame in &frames {
                    wire +=
                        &format!("data: {frame}{newline}{newline}: keep-alive{newline}{newline}");
                }
                if from == CC {
                    wire += &format!("data: [DONE]{newline}{newline}");
                }
                let first =
                    sse_event_boundary_end(wire.as_bytes()).expect("missing event boundary");
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(
                        &sse_event_data(&wire.as_bytes()[..first]).unwrap()
                    )
                    .unwrap(),
                    frames[0]
                );
                assert!(stream_reports_output(from, wire.as_bytes()));
                assert!(stream_reports_completion(from, wire.as_bytes()));
                let control = format!("event: error{newline}{newline}event: response.failed{newline}data:{newline}{newline}");
                assert!(!stream_reports_error(control.as_bytes()));
                for reset in ["event:", "event", "event: message"] {
                    let reset = format!(
                        "event: error{newline}{reset}{newline}data: {}{newline}{newline}",
                        frames[0]
                    );
                    assert!(!stream_reports_error(reset.as_bytes()));
                }
                let mut source: UpstreamBodyStream = Box::pin(stream::once({
                    let wire = wire.clone();
                    async move { Ok(Bytes::from(wire)) }
                }));
                assert!(
                    prefetch_sse_event(from, Some(Bytes::from(control)), &mut source)
                        .await
                        .is_ok()
                );
                for to in [AM, CC, RS].into_iter().filter(|to| *to != from) {
                    for size in [1, 2, 13, wire.len()] {
                        let chunks = wire
                            .as_bytes()
                            .chunks(size)
                            .map(|v| Ok::<_, BoxError>(Bytes::copy_from_slice(v)))
                            .collect::<Vec<_>>();
                        let mut output = convert_sse_stream(stream::iter(chunks), from, to);
                        let mut body = Vec::new();
                        while let Some(chunk) = output.next().await {
                            body.extend_from_slice(&chunk.unwrap());
                        }
                        assert!(
                            String::from_utf8_lossy(&body).contains("首🙂"),
                            "{from:?}->{to:?}: {newline:?} {size}"
                        );
                        assert!(stream_reports_completion(to, &body));
                        assert!(!stream_reports_error(&body));
                    }
                }
                let failed = format!(
                    "{bom}event: error{newline}data: {{\"message\":\"failed\"}}{newline}{newline}"
                );
                assert!(stream_reports_error(failed.as_bytes()));
            }
        }
    }
}
