use super::*;

#[test]
fn grok_api_restores_namespace_and_integral_arguments_in_json_and_sse() {
    for streaming in [false, true] {
        let body = serde_json::json!({"model":"grok-test","stream":streaming,"input":"hello","tools":[{"type":"namespace","name":"team","tools":[{"type":"function","name":"run","parameters":{"type":"object"}}]}]});
        let mut native = body.clone();
        aipass_proxy_conversion::providers::grok::Adapter::prepare(&mut native).unwrap();
        let response = serde_json::json!({"id":"resp_grok","object":"response","status":"completed","output":[{"type":"function_call","id":"item_1","call_id":"call_1","name":native["tools"][0]["name"],"arguments":"{\"n\":2500.0}"}],"usage":{"input_tokens":3,"output_tokens":4}});
        let (content_type, wire) = if streaming {
            (
                "text/event-stream",
                format!(
                    "data: {}\n\n",
                    serde_json::json!({"type":"response.completed","response":response})
                ),
            )
        } else {
            ("application/json", response.to_string())
        };
        let (address, upstream) = mock_upstream("200 OK", content_type, wire);
        let mut target = conversion_target(
            format!("http://{address}"),
            0,
            ProxyProtocol::OpenAiResponses,
        );
        target.profile = ProviderProfile::Grok;
        let proxy = direct_proxy(conversion_route(
            "grok-review",
            ProxyProtocol::OpenAiResponses,
            ProxyProtocol::OpenAiResponses,
            vec![target],
            1,
        ));
        let response = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap()
            .post(format!("http://{}/v1/responses", proxy.bind_addr))
            .bearer_auth("grok-review")
            .json(&body)
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let text = response.text().unwrap();
        let value: serde_json::Value = serde_json::from_str(if streaming {
            text.lines().find_map(|l| l.strip_prefix("data: ")).unwrap()
        } else {
            &text
        })
        .unwrap();
        let item = if streaming {
            &value["response"]["output"][0]
        } else {
            &value["output"][0]
        };
        assert_eq!(item["namespace"], "team");
        assert_eq!(item["name"], "run");
        assert_eq!(item["arguments"], "{\"n\":2500}");
        let (_, sent) = upstream.join().unwrap();
        let sent: serde_json::Value = serde_json::from_slice(&sent).unwrap();
        assert_eq!(sent["tools"][0]["type"], "function");
        assert_eq!(sent["tools"][0]["name"], native["tools"][0]["name"]);
    }
}

struct CacheCapture(Arc<Mutex<Vec<serde_json::Value>>>);
impl SubscriptionBackend for CacheCapture {
    fn request(
        &self,
        _: ResolvedTarget,
        payload: serde_json::Value,
        _: Option<String>,
    ) -> SubscriptionFuture {
        if payload["localError"] == true {
            self.0.lock().unwrap().push(payload);
            return Box::pin(async {
                Ok(SubscriptionResponse::rejected_request(
                    StatusCode::BAD_REQUEST,
                    "invalid cache mark",
                ))
            });
        }
        self.0.lock().unwrap().push(payload);
        Box::pin(async {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            );
            headers.insert(
                "x-aipass-cache-marks-dropped",
                HeaderValue::from_static("tools=1"),
            );
            Ok(SubscriptionResponse {
                status: StatusCode::OK,
                headers,
                body: Box::pin(stream::once(async {
                    Ok(Bytes::from_static(b"{\"id\":\"msg_cache\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"ok\"}],\"stop_reason\":\"end_turn\",\"usage\":{\"output_tokens\":1}}"))
                })),
            })
        })
    }
    fn revoke(&self) {}
}

#[test]
fn claude_acp_options_cross_protocol_conversion_only_from_explicit_headers() {
    for protocol in [
        ProxyProtocol::AnthropicMessages,
        ProxyProtocol::OpenAiChatCompletions,
        ProxyProtocol::OpenAiResponses,
    ] {
        let mut target = conversion_target(
            "https://unused.invalid".into(),
            0,
            ProxyProtocol::AnthropicMessages,
        );
        target.upstream_kind = UpstreamKind::ClaudeSubscription;
        let proxy = direct_proxy(conversion_route(
            "acp-options",
            protocol,
            ProxyProtocol::AnthropicMessages,
            vec![target],
            1,
        ));
        let captured = Arc::new(Mutex::new(Vec::new()));
        *proxy.state.subscription_backend.write().unwrap() =
            Some(Arc::new(CacheCapture(captured.clone())));
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let mut body = if protocol == ProxyProtocol::OpenAiResponses {
            serde_json::json!({"model":"claude","input":"hello"})
        } else {
            serde_json::json!({"model":"claude","messages":[{"role":"user","content":"hello"}]})
        };
        body["_aipass_claude_session"] =
            serde_json::json!({"cwd":"/spoofed","permissionMode":"bypassPermissions"});
        for header in [
            None,
            Some(r#"{"cwd":"/fixture","permissionMode":"plan"}"#),
            Some("invalid"),
        ] {
            let mut request = client
                .post(format!("http://{}{}", proxy.bind_addr, protocol.path()))
                .bearer_auth("acp-options")
                .json(&body);
            if let Some(header) = header {
                request = request
                    .header("x-aipass-claude-session", header)
                    .header("x-aipass-session-id", "fixture");
            }
            let response = request.send().unwrap();
            if header == Some("invalid") {
                assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            } else {
                assert_eq!(response.status(), StatusCode::OK);
                response.bytes().unwrap();
                let messages = captured.lock().unwrap();
                let payload = messages.last().unwrap();
                if header.is_some() {
                    assert_eq!(payload["_aipass_claude_session"]["cwd"], "/fixture");
                    assert_eq!(payload["_aipass_claude_session"]["permissionMode"], "plan");
                } else {
                    assert!(payload.get("_aipass_claude_session").is_none());
                }
            }
        }
        assert_eq!(captured.lock().unwrap().len(), 2);
    }
}
#[test]
fn claude_client_cache_requires_a_messages_header_and_cannot_be_enabled_by_body_spoofing() {
    for (protocol, opt_in) in [
        (ProxyProtocol::AnthropicMessages, false),
        (ProxyProtocol::AnthropicMessages, true),
        (ProxyProtocol::OpenAiChatCompletions, true),
    ] {
        let mut target = conversion_target(
            "https://unused.invalid".into(),
            0,
            ProxyProtocol::AnthropicMessages,
        );
        target.upstream_kind = UpstreamKind::ClaudeSubscription;
        let proxy = direct_proxy(conversion_route(
            "cache-review",
            protocol,
            ProxyProtocol::AnthropicMessages,
            vec![target],
            1,
        ));
        let captured = Arc::new(Mutex::new(Vec::new()));
        *proxy.state.subscription_backend.write().unwrap() =
            Some(Arc::new(CacheCapture(captured.clone())));
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let mut request=client.post(format!("http://{}{}",proxy.bind_addr,protocol.path())).bearer_auth("cache-review").json(&serde_json::json!({"model":"claude","messages":[{"role":"user","content":"hello"}],"_aipass_client_cache":true}));
        if opt_in {
            request = request.header("x-aipass-cache-marks", "client");
        }
        let response = request.send().unwrap();
        if protocol == ProxyProtocol::AnthropicMessages {
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.headers()["x-aipass-cache-marks-dropped"],
                "tools=1"
            );
            response.bytes().unwrap();
            assert_eq!(
                captured.lock().unwrap()[0]["_aipass_client_cache"] == true,
                opt_in
            );
        } else {
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert!(captured.lock().unwrap().is_empty());
        }
    }
}

#[tokio::test]
async fn split_cache_usage_reaches_the_client_and_ledger_without_double_counting() {
    let chat = concat!(
        "data: {\"id\":\"chatcmpl-usage\",\"model\":\"m\",\"choices\":[{\"delta\":{\"content\":\"answer\"}}],\"usage\":{\"prompt_tokens\":100}}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens_details\":{\"cached_tokens\":80,\"cache_creation_tokens\":15,\"cache_creation_1h_tokens\":10}}}\n\n",
        "data: {\"choices\":[],\"usage\":{\"completion_tokens\":11}}\n\ndata: [DONE]\n\n"
    );
    let anthropic = concat!(
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_usage\",\"model\":\"m\",\"usage\":{\"input_tokens\":5,\"cache_read_input_tokens\":80,\"cache_creation_input_tokens\":15,\"cache_creation\":{\"ephemeral_1h_input_tokens\":10,\"ephemeral_5m_input_tokens\":5}}}}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"answer\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":11}}\n\n",
        "data: {\"type\":\"message_stop\"}\n\n"
    );
    for (upstream_protocol, inbound_protocol, wire) in [
        (
            ProxyProtocol::OpenAiChatCompletions,
            ProxyProtocol::OpenAiChatCompletions,
            chat,
        ),
        (
            ProxyProtocol::AnthropicMessages,
            ProxyProtocol::OpenAiChatCompletions,
            anthropic,
        ),
        (
            ProxyProtocol::AnthropicMessages,
            ProxyProtocol::OpenAiResponses,
            anthropic,
        ),
    ] {
        let (address, server) = mock_upstream("200 OK", "text/event-stream", wire.into());
        let target = conversion_target(format!("http://{address}"), 0, upstream_protocol);
        let proxy = direct_proxy(conversion_route(
            "usage-review",
            inbound_protocol,
            upstream_protocol,
            vec![target],
            1,
        ));
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let body = if inbound_protocol == ProxyProtocol::OpenAiResponses {
            serde_json::json!({"model":"m","stream":true,"input":"hello"})
        } else {
            serde_json::json!({"model":"m","stream":true,"messages":[{"role":"user","content":"hello"}]})
        };
        let response = client
            .post(format!(
                "http://{}{}",
                proxy.bind_addr,
                inbound_protocol.path()
            ))
            .bearer_auth("usage-review")
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response
            .bytes()
            .await
            .expect("a completed converted stream must end cleanly");
        let expected = TokenUsage {
            input_tokens: 5,
            output_tokens: 11,
            cache_read_tokens: 80,
            cache_creation_tokens: 15,
            cache_creation_1h_tokens: 10,
        };
        assert_eq!(usage_from_wire_bytes(inbound_protocol, &bytes), expected);
        tokio::time::timeout(Duration::from_secs(2), async {
            while proxy.state.usage.count().unwrap() == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let rows = proxy.state.usage.iter_rows().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, 200);
        assert_eq!(rows[0].input_tokens, 5);
        assert_eq!(rows[0].output_tokens, 11);
        assert_eq!(rows[0].cache_read_tokens, 80);
        assert_eq!(rows[0].cache_creation_tokens, 15);
        assert_eq!(rows[0].cache_creation_1h_tokens, 10);
        server.join().unwrap();
    }
}

#[tokio::test]
async fn mixed_line_endings_keep_split_sse_errors_explicit_before_commit() {
    for ending in ["\n\n", "\r\n\r\n", "\n\r\n", "\r\n\n"] {
        let bytes =
            format!("event: error\ndata: {{\"error\":{{\"message\":\"limited\"}}}}{ending}");
        let split = bytes.len() - 1;
        let mut source: UpstreamBodyStream = Box::pin(stream::iter(vec![Ok(
            Bytes::copy_from_slice(&bytes.as_bytes()[split..]),
        )]));
        let result = prefetch_sse_event(
            ProxyProtocol::OpenAiResponses,
            Some(Bytes::copy_from_slice(&bytes.as_bytes()[..split])),
            &mut source,
        )
        .await;
        match result {
            Err(error) => assert!(error.confirmed_failure),
            Ok(_) => panic!("error event must reject before stream commit"),
        }
    }
}

pub(super) fn direct_proxy(route: ResolvedRoute) -> ProxyHandle {
    let proxy = start_proxy(available_addr(), route);
    let mut config = proxy.state.config.read().unwrap().clone();
    config.upstream_proxy.mode = UpstreamProxyMode::Direct;
    proxy.update_config(config).unwrap();
    proxy
}

#[test]
fn held_request_observes_quota_recovery_without_reauthentication() {
    let (address, upstream) = mock_upstream(
        "200 OK",
        "application/json",
        r#"{"status":"completed","output":[]}"#.into(),
    );
    let mut route = single_target_route(
        "quota-recovery",
        format!("http://{address}/v1"),
        RetryPolicy {
            hold_on_failure: true,
            hold_initial_delay_ms: 10,
            hold_max_delay_ms: 30,
            hold_max_duration_ms: 3000,
            ..RetryPolicy::default()
        },
    );
    route.targets[0].quota = vec![QuotaWindow {
        span_seconds: None,
        model_family: None,
        models: None,
        not_models: vec![],
        used_basis_points: 10_000,
        observed_at: now_unix() as u64,
        resets_at: None,
    }];
    let proxy = direct_proxy(route.clone());
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let bind = proxy.bind_addr.clone();
    let request = std::thread::spawn(move || {
        client
            .post(format!("http://{bind}/v1/responses"))
            .bearer_auth("quota-recovery")
            .json(&serde_json::json!({"model":"m","input":[]}))
            .send()
            .unwrap()
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    while proxy.state.in_flight_requests.load(Ordering::Relaxed) == 0 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    // Allow a selection/hold cycle, then publish only passive quota feedback.
    std::thread::sleep(Duration::from_millis(60));
    assert!(!request.is_finished());
    route.targets[0].quota[0].used_basis_points = 0;
    let mut config = RuntimeConfig::from_routes(proxy.bind_addr.clone(), vec![route]);
    config.upstream_proxy.mode = UpstreamProxyMode::Direct;
    proxy.update_config(config).unwrap();
    assert_eq!(request.join().unwrap().status(), StatusCode::OK);
    upstream.join().unwrap();
}

#[test]
fn live_selection_refreshes_exhaustion_and_limits_but_keeps_credential_snapshot() {
    let route = single_target_route(
        "quota-current",
        "http://127.0.0.1:1".into(),
        RetryPolicy::default(),
    );
    let proxy = direct_proxy(route.clone());
    let mut config = proxy.state.config.read().unwrap().clone();
    config.routes[0].targets[0].quota = vec![QuotaWindow {
        span_seconds: None,
        model_family: None,
        models: None,
        not_models: vec![],
        used_basis_points: 10_000,
        observed_at: now_unix() as u64,
        resets_at: None,
    }];
    proxy.update_config(config.clone()).unwrap();
    assert!(live_route_targets_for_model(&proxy.state, &route, None, None).is_empty());
    config.routes[0].targets[0].quota[0].used_basis_points = 0;
    config.routes[0].targets[0].max_concurrent_requests = Some(2);
    proxy.update_config(config.clone()).unwrap();
    let targets = live_route_targets_for_model(&proxy.state, &route, None, None);
    assert_eq!(targets[0].max_concurrent_requests, Some(2));
    // Never transplant another account's quota into an old credential snapshot.
    config.routes[0].targets[0].api_key = "replacement-account".into();
    config.routes[0].targets[0].max_concurrent_requests = Some(7);
    proxy.update_config(config).unwrap();
    let targets = live_route_targets_for_model(&proxy.state, &route, None, None);
    assert_eq!(targets[0].api_key, route.targets[0].api_key);
    assert_eq!(
        targets[0].max_concurrent_requests,
        route.targets[0].max_concurrent_requests
    );
}

#[test]
fn local_preflight_rejections_never_replay_or_open_the_vendor_circuit() {
    let mut target = conversion_target(
        "https://unused.invalid".into(),
        0,
        ProxyProtocol::AnthropicMessages,
    );
    target.upstream_kind = UpstreamKind::ClaudeSubscription;
    let id = target.config.id;
    let mut fallback = target.clone();
    fallback.config.id = Uuid::new_v4();
    let proxy = direct_proxy(conversion_route(
        "local-review",
        ProxyProtocol::AnthropicMessages,
        ProxyProtocol::AnthropicMessages,
        vec![target, fallback],
        1,
    ));
    let captured = Arc::new(Mutex::new(Vec::new()));
    *proxy.state.subscription_backend.write().unwrap() =
        Some(Arc::new(CacheCapture(captured.clone())));
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    for _ in 0..5 {
        let response = client
            .post(format!("http://{}/v1/messages", proxy.bind_addr))
            .bearer_auth("local-review")
            .json(&serde_json::json!({"model":"m","messages":[],"localError":true}))
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(!response.headers().contains_key("x-aipass-local-rejection"));
        response.bytes().unwrap();
    }
    assert_eq!(captured.lock().unwrap().len(), 5);
    assert!(proxy
        .state
        .health
        .lock()
        .unwrap()
        .get(&id)
        .is_none_or(|health| health.consecutive_failures == 0 && health.open_until.is_none()));
}
