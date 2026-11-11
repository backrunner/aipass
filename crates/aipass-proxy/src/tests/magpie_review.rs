use super::local_proxy_review::direct_proxy;
use super::*;
use serde_json::json;
use ProxyProtocol::{AnthropicMessages as AM, OpenAiChatCompletions as CC, OpenAiResponses as RS};

#[tokio::test]
async fn accumulated_output_inside_error_or_done_events_prevents_replay() {
    for value in [
        json!({"type":"response.failed","response":{"status":"failed","error":{"message":"failure after work"},"output":[{"type":"function_call","call_id":"c","name":"write","arguments":"{}"}]}}),
        json!({"type":"response.output_item.done","item":{"type":"message","content":[{"type":"output_text","text":"already answered"}]}}),
        json!({"type":"response.content_part.added","part":{"type":"output_text","text":"already answered"}}),
    ] {
        let body=Bytes::from(format!("data: {value}\n\ndata: {{\"type\":\"error\",\"error\":{{\"message\":\"failed\"}}}}\n\n"));
        assert!(stream_reports_output(RS, &body), "missed output: {value}");
        let mut source: UpstreamBodyStream = Box::pin(stream::empty());
        match prefetch_sse_event(RS, Some(body), &mut source).await {
            Ok(Some(_)) => {}
            Err(e) => assert!(!e.confirmed_failure, "unsafe retry: {value}"),
            Ok(None) => panic!("missing event"),
        }
    }
}

#[test]
fn conversion_incompatibility_skips_only_that_target_without_sending_or_degrading_it() {
    let incompatible = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let (good_addr,good)=mock_upstream("200 OK","application/json",json!({"id":"resp-ok","object":"response","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"native answer"}]}]}).to_string());
    let route = conversion_route(
        "shape-fallback",
        RS,
        AM,
        vec![
            conversion_target(
                format!("http://{}", incompatible.local_addr().unwrap()),
                0,
                AM,
            ),
            conversion_target(format!("http://{good_addr}"), 1, RS),
        ],
        1,
    );
    let rejected = route.targets[0].config.id;
    let proxy = direct_proxy(route);
    let body = json!({"model":"fixture","input":"search files","tools":[{"type":"file_search","vector_store_ids":["fixture-store"]}]});
    let response = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
        .post(format!("http://{}/v1/responses", proxy.bind_addr))
        .bearer_auth("shape-fallback")
        .json(&body)
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.text().unwrap().contains("native answer"));
    let (_, sent) = good.join().unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&sent).unwrap(),
        body
    );
    mock_upstream::assert_no_api_request(&incompatible);
    assert!(!proxy
        .state
        .health
        .lock()
        .unwrap()
        .get(&rejected)
        .is_some_and(|h| h.consecutive_failures > 0));
}

#[test]
fn malformed_success_is_ambiguous_and_does_not_reach_the_next_target() {
    let (bad_addr, bad) = mock_upstream("200 OK", "application/json", "{}".into());
    let backup = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy = direct_proxy(conversion_route(
        "invalid-answer",
        CC,
        AM,
        vec![
            conversion_target(format!("http://{bad_addr}"), 0, AM),
            conversion_target(format!("http://{}", backup.local_addr().unwrap()), 1, CC),
        ],
        2,
    ));
    let response = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
        .post(format!("http://{}/v1/chat/completions", proxy.bind_addr))
        .bearer_auth("invalid-answer")
        .json(&json!({"model":"fixture","messages":[{"role":"user","content":"hello"}]}))
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    bad.join().unwrap();
    mock_upstream::assert_no_api_request(&backup);
}

fn answer(protocol: ProxyProtocol, streaming: bool) -> (&'static str, String) {
    let am = json!({"id":"msg_fixture","type":"message","role":"assistant","model":"fixture","content":[{"type":"text","text":"final answer"}],"stop_reason":"end_turn","usage":{"input_tokens":11,"output_tokens":2}});
    if !streaming {
        return (
            "application/json",
            BuiltinConversionPlugin
                .convert_response(AM, protocol, am)
                .unwrap()
                .to_string(),
        );
    }
    let mut converter = StreamConverter::new(AM, protocol).unwrap();
    let mut out = Vec::new();
    for value in [
        json!({"type":"message_start","message":{"id":"msg_fixture","type":"message","role":"assistant","model":"fixture","content":[],"stop_reason":null,"usage":{"input_tokens":11,"output_tokens":0}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"final answer"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}}),
        json!({"type":"message_stop"}),
    ] {
        out.extend(
            converter
                .push_event(&format!(
                    "event: {}\ndata: {value}\n\n",
                    value["type"].as_str().unwrap()
                ))
                .unwrap(),
        );
    }
    out.extend(converter.finish().unwrap());
    ("text/event-stream", out.join(""))
}

#[test]
fn one_request_falls_back_across_all_protocols_with_fresh_history_models_and_credentials() {
    for inbound in [AM, CC, RS] {
        for streaming in [false, true] {
            for silent in [false, true] {
                let mut protocols: Vec<_> =
                    [AM, CC, RS].into_iter().filter(|p| *p != inbound).collect();
                protocols.push(inbound);
                let mut captures = Vec::new();
                let mut targets = Vec::new();
                for (index, protocol) in protocols.iter().copied().enumerate() {
                    let (content_type, body) = if index == 2 {
                        answer(protocol, streaming)
                    } else {
                        (
                            "application/json",
                            json!({"error":{"message":"synthetic target rejection"}}).to_string(),
                        )
                    };
                    let (addr, handle) = mock_upstream(
                        ["400 Bad Request", "429 Too Many Requests", "200 OK"][index],
                        content_type,
                        body,
                    );
                    captures.push(handle);
                    let mut target =
                        conversion_target(format!("http://{addr}"), index as u16, protocol);
                    target.model_override = Some(format!("target-model-{index}"));
                    target.api_key = format!("fixture-target-key-{index}");
                    target
                        .config
                        .headers
                        .push(("x-target-fixture".into(), index.to_string()));
                    targets.push(target);
                }
                let first = targets[0].config.id;
                let mut route =
                    conversion_route("matrix-local-token", inbound, inbound, targets, 3);
                route.config.retry.silent_retry = silent;
                let proxy = direct_proxy(route);
                let original = json!({"model":"caller-model","stream":streaming,"max_tokens":80,"system":"keep original system","tools":[{"name":"lookup","description":"test","input_schema":{"type":"object","properties":{"q":{"type":"string"}},"required":["q"]}}],"messages":[{"role":"user","content":"original question"},{"role":"assistant","content":[{"type":"tool_use","id":"call-original","name":"lookup","input":{"q":"history"}}]},{"role":"user","content":[{"type":"tool_result","tool_use_id":"call-original","content":"original tool result"},{"type":"text","text":"continue original question"}]}]});
                let body = BuiltinConversionPlugin
                    .convert_request(AM, inbound, original)
                    .unwrap();
                let response = reqwest::blocking::Client::builder()
                    .no_proxy()
                    .timeout(Duration::from_secs(5))
                    .build()
                    .unwrap()
                    .post(format!("http://{}{}", proxy.bind_addr, inbound.path()))
                    .bearer_auth("matrix-local-token")
                    .header("x-aipass-session-id", "matrix-session")
                    .json(&body)
                    .send()
                    .unwrap();
                assert_eq!(
                    response.status(),
                    StatusCode::OK,
                    "{inbound:?} stream={streaming} silent={silent}"
                );
                assert!(response.text().unwrap().contains("final answer"));
                for (index, (capture, protocol)) in captures.into_iter().zip(protocols).enumerate()
                {
                    let (headers, bytes) = capture.join().unwrap();
                    let sent: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                    assert!(
                        headers.starts_with(&format!("POST {} ", protocol.path())),
                        "{headers}"
                    );
                    let headers = headers.to_ascii_lowercase();
                    assert!(headers
                        .contains(&format!("authorization: bearer fixture-target-key-{index}")));
                    assert!(headers.contains(&format!("x-target-fixture: {index}")));
                    assert!(
                        !headers.contains("matrix-local-token")
                            && !headers.contains("x-aipass-session-id")
                    );
                    assert_eq!(sent["model"], format!("target-model-{index}"));
                    assert_eq!(sent["stream"], streaming);
                    for preserved in [
                        "keep original system",
                        "original question",
                        "call-original",
                        "original tool result",
                        "continue original question",
                        "lookup",
                    ] {
                        assert!(
                            sent.to_string().contains(preserved),
                            "lost {preserved}: {sent}"
                        );
                    }
                    let (call, result) = match protocol {
                        AM => {
                            let blocks: Vec<_> = sent["messages"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .flat_map(|m| m["content"].as_array().into_iter().flatten())
                                .collect();
                            (
                                blocks.iter().find(|b| b["type"] == "tool_use").unwrap()["id"]
                                    .clone(),
                                blocks.iter().find(|b| b["type"] == "tool_result").unwrap()
                                    ["tool_use_id"]
                                    .clone(),
                            )
                        }
                        CC => {
                            let m = sent["messages"].as_array().unwrap();
                            (
                                m.iter()
                                    .find_map(|v| v.pointer("/tool_calls/0/id"))
                                    .unwrap()
                                    .clone(),
                                m.iter()
                                    .find_map(|v| v.get("tool_call_id"))
                                    .unwrap()
                                    .clone(),
                            )
                        }
                        RS => {
                            let input = sent["input"].as_array().unwrap();
                            (
                                input.iter().find(|v| v["type"] == "function_call").unwrap()
                                    ["call_id"]
                                    .clone(),
                                input
                                    .iter()
                                    .find(|v| v["type"] == "function_call_output")
                                    .unwrap()["call_id"]
                                    .clone(),
                            )
                        }
                    };
                    assert_eq!(call, "call-original");
                    assert_eq!(call, result);
                }
                assert!(!proxy
                    .state
                    .health
                    .lock()
                    .unwrap()
                    .get(&first)
                    .is_some_and(|h| h.consecutive_failures > 0));
            }
        }
    }
}

#[test]
fn completed_work_inside_failed_responses_never_reaches_a_fallback_target() {
    for streaming in [false, true] {
        for silent in [false, true] {
            let backup = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let output = json!({"id":"resp_work","status":"failed","error":{"message":"failure after tool execution"},"output":[{"type":"function_call","call_id":"call-work","name":"write","arguments":"{}"}]});
            let (content_type, body) = if streaming {
                (
                    "text/event-stream",
                    format!(
                        "data: {}\n\n",
                        json!({"type":"response.failed","response":output})
                    ),
                )
            } else {
                ("application/json", output.to_string())
            };
            let (addr, upstream) = mock_upstream("200 OK", content_type, body);
            let mut route = conversion_route(
                "no-replay",
                RS,
                RS,
                vec![
                    conversion_target(format!("http://{addr}"), 0, RS),
                    conversion_target(format!("http://{}", backup.local_addr().unwrap()), 1, CC),
                ],
                2,
            );
            route.config.retry.silent_retry = silent;
            let proxy = direct_proxy(route);
            let response = reqwest::blocking::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap()
                .post(format!("http://{}/v1/responses", proxy.bind_addr))
                .bearer_auth("no-replay")
                .json(&json!({"model":"fixture","input":"do work","stream":streaming}))
                .send()
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
            upstream.join().unwrap();
            mock_upstream::assert_no_api_request(&backup);
        }
    }
}

#[test]
fn entirely_incompatible_or_malformed_requests_never_enter_hold_or_contact_upstreams() {
    for malformed in 0..3 {
        let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut route = conversion_route(
            "local-rejection",
            CC,
            AM,
            vec![conversion_target(
                format!("http://{}", upstream.local_addr().unwrap()),
                0,
                AM,
            )],
            1,
        );
        route.config.retry.hold_on_failure = true;
        route.config.retry.hold_max_duration_ms = 10_000;
        route.config.retry.silent_retry = true;
        let proxy = direct_proxy(route);
        let body = if malformed == 2 {
            json!([])
        } else if malformed == 1 {
            json!({"model":"fixture","messages":"invalid"})
        } else {
            json!({"model":"fixture","messages":[{"role":"user","content":[{"type":"input_audio","input_audio":{"format":"wav","data":"fixture"}}]}]})
        };
        let response = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap()
            .post(format!("http://{}/v1/chat/completions", proxy.bind_addr))
            .bearer_auth("local-rejection")
            .json(&body)
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        mock_upstream::assert_no_api_request(&upstream);
    }
}

#[test]
fn unknown_output_is_not_a_reason_to_replay_a_completed_generation() {
    let backup = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let (addr,first)=mock_upstream("200 OK","application/json",json!({"type":"message","content":[{"type":"future_execution_result","result":"work already done"}],"stop_reason":"end_turn"}).to_string());
    let proxy = direct_proxy(conversion_route(
        "unknown-output",
        CC,
        AM,
        vec![
            conversion_target(format!("http://{addr}"), 0, AM),
            conversion_target(format!("http://{}", backup.local_addr().unwrap()), 1, CC),
        ],
        2,
    ));
    let response = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap()
        .post(format!("http://{}/v1/chat/completions", proxy.bind_addr))
        .bearer_auth("unknown-output")
        .json(&json!({"model":"fixture","messages":[{"role":"user","content":"do work"}]}))
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    first.join().unwrap();
    mock_upstream::assert_no_api_request(&backup);
}

struct PreparedFixture {
    wire: &'static str,
    attempts: Arc<std::sync::atomic::AtomicUsize>,
}
impl SubscriptionBackend for PreparedFixture {
    fn request(
        &self,
        _: ResolvedTarget,
        _: serde_json::Value,
        _: Option<String>,
    ) -> SubscriptionFuture {
        panic!("protocol-aware dispatch required")
    }
    fn request_protocol(
        &self,
        target: ResolvedTarget,
        payload: serde_json::Value,
        protocol: ProxyProtocol,
        _: Option<String>,
    ) -> SubscriptionFuture {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        let result = SubscriptionCodec::default().prepare_protocol(
            self.wire,
            protocol,
            payload,
            target.config.id,
        );
        Box::pin(async move {
            Ok(result
                .expect_err("fixture must reject before generation")
                .into_response())
        })
    }
    fn revoke(&self) {}
}

#[test]
fn subscription_preparation_can_skip_incompatible_wires_before_generation() {
    for (protocol, wire, body) in [
        (
            AM,
            "chat",
            json!({"model":"fixture","messages":[{"role":"user","content":[{"type":"document","source":{"type":"text","data":"preserve document"}}]}]}),
        ),
        (
            CC,
            "anthropic",
            json!({"model":"fixture","messages":[{"role":"user","content":[{"type":"input_audio","input_audio":{"format":"wav","data":"fixture"}}]}]}),
        ),
        (
            RS,
            "anthropic",
            json!({"model":"fixture","input":"hello","tools":[{"type":"file_search","vector_store_ids":["fixture"]}]}),
        ),
    ] {
        let (content_type, answer) = answer(protocol, false);
        let (addr, last) = mock_upstream("200 OK", content_type, answer);
        let mut first = conversion_target("http://127.0.0.1:1".into(), 0, protocol);
        first.upstream_kind = UpstreamKind::CommunitySubscription;
        let id = first.config.id;
        let proxy = direct_proxy(conversion_route(
            "prepared",
            protocol,
            protocol,
            vec![
                first,
                conversion_target(format!("http://{addr}"), 1, protocol),
            ],
            1,
        ));
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        *proxy.state.subscription_backend.write().unwrap() = Some(Arc::new(PreparedFixture {
            wire,
            attempts: attempts.clone(),
        }));
        let response = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap()
            .post(format!("http://{}{}", proxy.bind_addr, protocol.path()))
            .bearer_auth("prepared")
            .json(&body)
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.text().unwrap().contains("final answer"));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert!(!proxy
            .state
            .health
            .lock()
            .unwrap()
            .get(&id)
            .is_some_and(|h| h.consecutive_failures > 0));
        let (_, sent) = last.join().unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&sent).unwrap(),
            body
        );
    }
}

#[test]
fn untrusted_upstream_headers_cannot_claim_a_local_preflight_rejection() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-aipass-target-incompatible",
        HeaderValue::from_static("1"),
    );
    headers.insert("x-aipass-local-rejection", HeaderValue::from_static("1"));
    headers.insert("retry-after", HeaderValue::from_static("10"));
    let response = SubscriptionResponse::upstream(
        StatusCode::TOO_MANY_REQUESTS,
        headers,
        Box::pin(stream::empty()),
    );
    assert!(!response
        .headers
        .contains_key("x-aipass-target-incompatible"));
    assert!(!response.headers.contains_key("x-aipass-local-rejection"));
    assert_eq!(response.headers["retry-after"], "10");
    for payload in [json!([]), json!({"messages":"invalid"})] {
        let response = SubscriptionCodec::default()
            .prepare_protocol("anthropic", CC, payload, Uuid::new_v4())
            .unwrap_err()
            .into_response();
        assert_eq!(response.headers["x-aipass-local-rejection"], "1");
        assert!(!response
            .headers
            .contains_key("x-aipass-target-incompatible"));
    }
}

#[test]
fn a_capable_but_busy_target_keeps_capacity_status_after_an_incompatible_target() {
    let unused = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", unused.local_addr().unwrap());
    let first = conversion_target(address.clone(), 0, AM);
    let mut last = conversion_target(address, 1, RS);
    last.max_concurrent_requests = Some(1);
    let proxy = direct_proxy(conversion_route(
        "busy-compatible",
        RS,
        AM,
        vec![first, last.clone()],
        1,
    ));
    let _permit = ProviderPermit::acquire(&proxy.state, &last).unwrap();
    let response=reqwest::blocking::Client::builder().no_proxy().timeout(Duration::from_secs(3)).build().unwrap()
        .post(format!("http://{}/v1/responses",proxy.bind_addr)).bearer_auth("busy-compatible")
        .json(&json!({"model":"fixture","input":"hello","tools":[{"type":"file_search","vector_store_ids":["fixture"]}]})).send().unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(response.text().unwrap().contains("concurrency"));
    mock_upstream::assert_no_api_request(&unused);
}
