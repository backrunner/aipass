use super::*;
use std::io::{Read, Write};

mod concurrency;
mod image_api;
mod runtime_status;
mod stability;
mod transparency;

#[test]
fn route_protocol_scope_keeps_codex_tokens_on_responses_only() {
    let route = ProxyRouteConfig {
        id: Uuid::new_v4(),
        name: "Codex".into(),
        token: "route-token".into(),
        inbound_protocol: ProxyProtocol::OpenAiResponses,
        upstream_protocol: ProxyProtocol::OpenAiResponses,
        conversion_enabled: false,
        strategy: RouteStrategy::Fallback,
        targets: Vec::new(),
        retry: RetryPolicy::default(),
        enabled: true,
    };

    assert!(route_accepts_inbound_protocol(
        &route,
        ProxyProtocol::OpenAiResponses
    ));
    assert!(!route_accepts_inbound_protocol(
        &route,
        ProxyProtocol::OpenAiChatCompletions
    ));
    assert!(!route_accepts_inbound_protocol(
        &route,
        ProxyProtocol::AnthropicMessages
    ));
}

#[test]
fn route_selection_rejects_codex_token_on_other_http_protocols() {
    let token = "codex-responses-token";
    let route = single_target_route(
        token,
        "http://127.0.0.1:1/v1".into(),
        RetryPolicy::default(),
    );
    let proxy = start_proxy(available_addr(), route);
    assert!(select_route(
        &proxy.state,
        Some(token),
        None,
        Some(ProxyProtocol::OpenAiResponses)
    )
    .is_some());
    assert!(select_route(
        &proxy.state,
        Some(token),
        None,
        Some(ProxyProtocol::OpenAiChatCompletions)
    )
    .is_none());
    assert!(select_route(
        &proxy.state,
        Some(token),
        None,
        Some(ProxyProtocol::AnthropicMessages)
    )
    .is_none());
}

#[test]
fn in_flight_guard_tracks_nested_request_lifetimes() {
    let counter = Arc::new(AtomicU64::new(0));
    {
        let _first = InFlightGuard::new(counter.clone());
        assert_eq!(counter.load(Ordering::Relaxed), 1);
        {
            let _second = InFlightGuard::new(counter.clone());
            assert_eq!(counter.load(Ordering::Relaxed), 2);
        }
        assert_eq!(counter.load(Ordering::Relaxed), 1);
    }
    assert_eq!(counter.load(Ordering::Relaxed), 0);
}

#[test]
fn proxy_config_defaults_upstream_proxy_to_system_for_legacy_json() {
    let config: ProxyConfig = serde_json::from_str(
        r#"{"enabled":true,"bindAddr":"127.0.0.1:8787","routes":[],"pricing":[]}"#,
    )
    .expect("legacy config without upstreamProxy still deserializes");
    assert_eq!(config.upstream_proxy.mode, UpstreamProxyMode::System);
    assert_eq!(config.upstream_proxy.custom_url, None);
}

#[test]
fn upstream_proxy_config_serde_roundtrip() {
    let config = UpstreamProxyConfig {
        mode: UpstreamProxyMode::Custom,
        custom_url: Some("http://user:pass@127.0.0.1:7890".into()),
    };
    let json = serde_json::to_string(&config).unwrap();
    assert_eq!(
        json,
        r#"{"mode":"custom","customUrl":"http://user:pass@127.0.0.1:7890"}"#
    );
    let parsed: UpstreamProxyConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, config);
}

#[test]
fn apply_upstream_proxy_rejects_custom_mode_without_url() {
    let builder = reqwest::Client::builder();
    let result = apply_upstream_proxy(
        builder,
        &UpstreamProxyConfig {
            mode: UpstreamProxyMode::Custom,
            custom_url: None,
        },
    );
    assert!(result.is_err());
}

#[test]
fn apply_upstream_proxy_rejects_invalid_custom_url() {
    let builder = reqwest::Client::builder();
    let result = apply_upstream_proxy(
        builder,
        &UpstreamProxyConfig {
            mode: UpstreamProxyMode::Custom,
            custom_url: Some("not a url".into()),
        },
    );
    assert!(result.is_err());
}

#[test]
fn apply_upstream_proxy_accepts_valid_modes() {
    for config in [
        UpstreamProxyConfig::default(),
        UpstreamProxyConfig {
            mode: UpstreamProxyMode::Direct,
            custom_url: None,
        },
        UpstreamProxyConfig {
            mode: UpstreamProxyMode::Environment,
            custom_url: None,
        },
        UpstreamProxyConfig {
            mode: UpstreamProxyMode::Custom,
            custom_url: Some("socks5://127.0.0.1:1080".into()),
        },
    ] {
        let builder = reqwest::Client::builder();
        let builder = apply_upstream_proxy(builder, &config).expect("valid proxy config");
        builder.build().expect("client builds");
    }
}

#[tokio::test]
async fn custom_upstream_proxy_routes_http_traffic_through_proxy() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = listener.local_addr().unwrap();
    let capture = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut chunk = [0_u8; 1024];
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            let read = socket.read(&mut chunk).await.unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&chunk[..read]);
        }
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .await
            .unwrap();
        String::from_utf8_lossy(&request).to_string()
    });

    let builder = apply_upstream_proxy(
        reqwest::Client::builder(),
        &UpstreamProxyConfig {
            mode: UpstreamProxyMode::Custom,
            custom_url: Some(format!("http://{proxy_addr}")),
        },
    )
    .expect("custom proxy config");
    let body = builder
        .build()
        .unwrap()
        .get("http://example.com/upstream")
        .send()
        .await
        .expect("request through proxy")
        .text()
        .await
        .unwrap();
    assert_eq!(body, "ok");
    let request = capture.await.unwrap();
    assert!(
        request.starts_with("GET http://example.com/upstream"),
        "proxy received an absolute-URI request, got: {request:?}"
    );
}

fn available_addr() -> SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    addr
}

fn single_target_route(token: &str, base_url: String, retry: RetryPolicy) -> ResolvedRoute {
    ResolvedRoute {
        config: ProxyRouteConfig {
            id: Uuid::new_v4(),
            name: "test".into(),
            token: String::new(),
            inbound_protocol: ProxyProtocol::OpenAiResponses,
            upstream_protocol: ProxyProtocol::OpenAiResponses,
            conversion_enabled: false,
            strategy: RouteStrategy::Fallback,
            targets: Vec::new(),
            retry,
            enabled: true,
        },
        local_token: token.into(),
        targets: vec![test_target(base_url, 0)],
    }
}

pub(crate) fn test_target(base_url: String, priority: u16) -> ResolvedTarget {
    ResolvedTarget {
        // These fixtures exercise the HTTP retry/streaming pipeline.
        // WS preference and fallback have dedicated adaptive WS tests.
        max_concurrent_requests: None,
        supports_websockets: false,
        config: ProxyTargetConfig {
            id: Uuid::new_v4(),
            provider_entry_id: Uuid::new_v4(),
            secret_id: "primary".into(),
            label: "primary".into(),
            base_url,
            auth_scheme: "bearer".into(),
            headers: Vec::new(),
            group: None,
            priority,
            weight: 1,
            enabled: true,
            protocol: None,
            prefer_ws: false,
        },
        api_key: "upstream-secret".into(),
    }
}

fn fallback_route(token: &str, upstreams: &[SocketAddr], retry: RetryPolicy) -> ResolvedRoute {
    let mut route = single_target_route(token, String::new(), retry);
    route.targets = upstreams
        .iter()
        .enumerate()
        .map(|(index, addr)| test_target(format!("http://{addr}/v1"), index as u16))
        .collect();
    route
}

fn read_http_request(stream: &mut std::net::TcpStream) -> (String, Vec<u8>) {
    let mut received = Vec::new();
    let mut buffer = [0_u8; 8192];
    let header_end = loop {
        let read = stream.read(&mut buffer).unwrap();
        assert!(
            read > 0,
            "connection closed before request headers completed"
        );
        received.extend_from_slice(&buffer[..read]);
        if let Some(index) = received.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = String::from_utf8(received[..header_end].to_vec()).unwrap();
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap_or_default();
    while received.len() - header_end < content_length {
        let read = stream.read(&mut buffer).unwrap();
        assert!(read > 0, "connection closed before request body completed");
        received.extend_from_slice(&buffer[..read]);
    }
    (
        headers,
        received[header_end..header_end + content_length].to_vec(),
    )
}

#[test]
fn plaintext_tokens_use_constant_time_comparison() {
    assert!(tokens_match("local-test-token", "local-test-token"));
    assert!(!tokens_match("local-test-token", "other"));
    assert!(!tokens_match("local-test-token", "local-test-token-longer"));

    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer wrong-token"),
    );
    headers.insert("x-api-key", HeaderValue::from_static("local-test-token"));
    let (bearer, api_key) = local_proxy_tokens(&headers);
    assert_eq!(bearer, Some("wrong-token"));
    assert_eq!(api_key, Some("local-test-token"));
    assert!(
        bearer.is_some_and(|token| tokens_match("local-test-token", token))
            || api_key.is_some_and(|token| tokens_match("local-test-token", token))
    );

    headers.insert(header::AUTHORIZATION, HeaderValue::from_static("Bearer "));
    headers.insert("x-api-key", HeaderValue::from_static(""));
    assert_eq!(local_proxy_tokens(&headers), (None, None));
}

#[test]
fn every_upstream_error_status_is_eligible_for_failover() {
    assert!(is_retryable_status(StatusCode::BAD_REQUEST));
    assert!(is_retryable_status(StatusCode::NOT_FOUND));
    assert!(is_retryable_status(StatusCode::UNPROCESSABLE_ENTITY));
    assert!(is_retryable_status(StatusCode::FORBIDDEN));
    assert!(is_retryable_status(StatusCode::INTERNAL_SERVER_ERROR));
    assert!(is_retryable_status(StatusCode::MOVED_PERMANENTLY));
    assert!(!is_retryable_status(StatusCode::OK));
    assert!(!status_affects_circuit(StatusCode::BAD_REQUEST));
    assert!(!status_affects_circuit(StatusCode::NOT_FOUND));
    assert!(status_affects_circuit(StatusCode::UNAUTHORIZED));
    assert!(status_affects_circuit(StatusCode::TOO_MANY_REQUESTS));
    assert!(status_affects_circuit(StatusCode::BAD_GATEWAY));
}

#[test]
fn connection_declared_headers_are_treated_as_hop_by_hop() {
    let mut headers = http::HeaderMap::new();
    headers.append(
        header::CONNECTION,
        http::HeaderValue::from_static("keep-alive, x-internal-hop"),
    );
    headers.append(
        header::CONNECTION,
        http::HeaderValue::from_static("x-second-hop"),
    );

    let declared = connection_header_names(&headers);

    assert!(declared.contains(&header::HeaderName::from_static("x-internal-hop")));
    assert!(declared.contains(&header::HeaderName::from_static("x-second-hop")));
    assert!(!declared.contains(&header::CONTENT_TYPE));

    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer local-token"),
    );
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"));
    headers.insert(header::ACCEPT_ENCODING, HeaderValue::from_static("gzip"));
    headers.insert(
        header::HeaderName::from_static("x-internal-hop"),
        HeaderValue::from_static("must-not-forward"),
    );
    headers.insert(
        header::HeaderName::from_static("x-end-to-end"),
        HeaderValue::from_static("keep"),
    );
    let mut target = test_target("https://api.example.test/v1".into(), 0);
    target.config.headers = vec![
        ("connection".into(), "x-target-hop".into()),
        ("x-target-hop".into(), "must-not-forward".into()),
        ("content-type".into(), "application/octet-stream".into()),
    ];

    let forwarded =
        build_upstream_headers(&headers, &target, ProxyProtocol::OpenAiResponses).unwrap();

    assert!(!forwarded.contains_key("x-internal-hop"));
    assert!(!forwarded.contains_key("x-target-hop"));
    assert!(!forwarded.contains_key(header::ACCEPT_ENCODING));
    assert_eq!(forwarded["x-end-to-end"], "keep");
    assert_eq!(forwarded[header::CONTENT_TYPE], "text/plain");
    assert_eq!(forwarded[header::AUTHORIZATION], "Bearer upstream-secret");

    headers.remove(header::CONTENT_TYPE);
    let forwarded =
        build_upstream_headers(&headers, &target, ProxyProtocol::OpenAiResponses).unwrap();
    assert_eq!(forwarded[header::CONTENT_TYPE], "application/json");
}

#[test]
fn sse_prefetch_classifies_heartbeats_errors_and_completion_markers() {
    assert!(sse_event_is_heartbeat(
        b"event: ping\ndata: {\"type\":\"ping\"}\n\n"
    ));
    assert!(sse_event_reports_error(
        b"event: response.failed\ndata: {\"type\":\"response.failed\"}\n\n"
    ));
    assert!(stream_reports_completion(
        ProxyProtocol::OpenAiResponses,
        b"event: response.completed\ndata: {\"type\":\"response.completed\"}\n\n"
    ));
    assert!(stream_reports_completion(
        ProxyProtocol::OpenAiChatCompletions,
        b"data: [DONE]\n\n"
    ));
    assert!(sse_event_reports_completion(
        ProxyProtocol::OpenAiChatCompletions,
        b"data: {\"choices\":[{\"finish_reason\":\"stop\"}]}\n\n"
    ));
    assert!(!sse_event_is_terminal(
        ProxyProtocol::OpenAiChatCompletions,
        b"data: {\"choices\":[{\"finish_reason\":\"stop\"}]}\n\n"
    ));
    assert!(sse_event_is_terminal(
        ProxyProtocol::OpenAiChatCompletions,
        b"data: [DONE]\n\n"
    ));
    assert!(stream_reports_completion(
        ProxyProtocol::AnthropicMessages,
        b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
    ));
    assert!(!stream_reports_completion(
        ProxyProtocol::OpenAiResponses,
        b"data: {\"type\":\"response.output_text.delta\"}\n\n"
    ));
    assert!(!sse_event_reports_output(
        ProxyProtocol::OpenAiResponses,
        b"data: {\"type\":\"response.created\"}\n\n"
    ));
    assert!(sse_event_reports_output(
        ProxyProtocol::OpenAiResponses,
        b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n"
    ));
    assert!(!sse_event_reports_output(
        ProxyProtocol::OpenAiChatCompletions,
        b"data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n"
    ));
    assert!(sse_event_reports_output(
        ProxyProtocol::OpenAiChatCompletions,
        b"data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n"
    ));
    assert!(!sse_event_reports_output(
        ProxyProtocol::AnthropicMessages,
        b"event: message_start\ndata: {\"type\":\"message_start\"}\n\n"
    ));
    assert!(sse_event_reports_output(
            ProxyProtocol::AnthropicMessages,
            b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n"
        ));
}

#[test]
fn streaming_usage_is_extracted_incrementally_from_nested_events() {
    let anthropic = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":12,\"cache_read_input_tokens\":4}}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":7}}\n\n"
        )
        .as_bytes();
    let mut buffer = Vec::new();
    let mut usage = TokenUsage::default();
    for chunk in anthropic.chunks(23) {
        observe_sse_usage(
            ProxyProtocol::AnthropicMessages,
            &mut buffer,
            chunk,
            &mut usage,
        );
    }
    assert_eq!(usage.input_tokens, 12);
    assert_eq!(usage.output_tokens, 7);
    assert_eq!(usage.cache_read_tokens, 4);

    let chat = concat!(
        "data: {\"choices\":[{\"finish_reason\":\"stop\"}]}\n\n",
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":4}}\n\n",
        "data: [DONE]\n\n"
    )
    .as_bytes();
    let mut buffer = Vec::new();
    let mut usage = TokenUsage::default();
    let mut completed = false;
    let mut terminal = false;
    for chunk in chat.chunks(19) {
        let signals = observe_sse_usage(
            ProxyProtocol::OpenAiChatCompletions,
            &mut buffer,
            chunk,
            &mut usage,
        );
        completed |= signals.completed;
        terminal |= signals.terminal;
    }
    assert!(completed);
    assert!(terminal);
    assert_eq!(usage.input_tokens, 9);
    assert_eq!(usage.output_tokens, 4);

    let responses = usage_from_wire_bytes(
            ProxyProtocol::OpenAiResponses,
            b"data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":20,\"output_tokens\":5,\"input_tokens_details\":{\"cached_tokens\":3}}}}\n\n",
        );
    assert_eq!(responses.input_tokens, 17);
    assert_eq!(responses.output_tokens, 5);
    assert_eq!(responses.cache_read_tokens, 3);
}

#[test]
fn chat_stream_requests_enable_usage_reporting() {
    let payload = Bytes::from_static(
        br#"{"model":"gpt-test","stream":true,"stream_options":{"custom":true}}"#,
    );
    let updated = request_stream_usage(ProxyProtocol::OpenAiChatCompletions, true, payload.clone());
    let value: serde_json::Value = serde_json::from_slice(&updated).unwrap();
    assert_eq!(value["stream_options"]["include_usage"], true);
    assert_eq!(value["stream_options"]["custom"], true);
    assert_eq!(
        request_stream_usage(ProxyProtocol::OpenAiResponses, true, payload.clone()),
        payload
    );
    assert_eq!(
        request_stream_usage(
            ProxyProtocol::OpenAiChatCompletions,
            false,
            Bytes::from_static(br#"{"stream":false}"#),
        ),
        Bytes::from_static(br#"{"stream":false}"#)
    );
}

#[test]
fn degraded_targets_follow_recent_failures_circuits_and_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let retry = RetryPolicy::default();
    let mut route = single_target_route(
        "aipass_health_status_test",
        "http://127.0.0.1:1/v1".into(),
        retry.clone(),
    );
    let target_id = route.targets[0].config.id;
    let mut disabled_target = test_target("http://127.0.0.1:2/v1".into(), 1);
    disabled_target.config.enabled = false;
    let disabled_target_id = disabled_target.config.id;
    route.targets.push(disabled_target);
    let mut disabled_route = single_target_route(
        "aipass_disabled_health_status_test",
        "http://127.0.0.1:3/v1".into(),
        retry.clone(),
    );
    disabled_route.config.enabled = false;
    let disabled_route_target_id = disabled_route.targets[0].config.id;
    let proxy = ProxyHandle::start(
        RuntimeConfig::from_routes("127.0.0.1:0", vec![route, disabled_route]),
        usage,
    )
    .unwrap();

    assert!(!proxy.status().degraded);
    for id in [
        target_id,
        disabled_target_id,
        disabled_route_target_id,
        Uuid::new_v4(),
    ] {
        mark_failure(&proxy.state, id, &retry);
    }
    assert!(proxy.status().degraded);
    assert_eq!(proxy.status().degraded_target_ids, vec![target_id]);

    // Recovery belongs to the affected target; a healthy fallback cannot clear it.
    mark_success(&proxy.state, disabled_target_id);
    assert_eq!(proxy.status().degraded_target_ids, vec![target_id]);
    mark_success(&proxy.state, target_id);
    mark_success(&proxy.state, target_id);
    assert!(!proxy.status().degraded);

    mark_failure(&proxy.state, target_id, &retry);
    {
        let mut health = proxy.state.health.lock().unwrap();
        let target = health.get_mut(&target_id).unwrap();
        target.last_failure_at = Some(Instant::now() - Duration::from_secs(61));
    }
    assert_eq!(proxy.status().degraded_target_ids, vec![target_id]);
    assert!(proxy.status().degraded);
    {
        let mut health = proxy.state.health.lock().unwrap();
        health.get_mut(&target_id).unwrap().open_until =
            Some(Instant::now() + Duration::from_secs(120));
    }
    assert_eq!(proxy.status().degraded_target_ids, vec![target_id]);
    {
        let mut health = proxy.state.health.lock().unwrap();
        health.get_mut(&target_id).unwrap().open_until =
            Some(Instant::now() - Duration::from_secs(1));
    }
    assert!(proxy.status().degraded);

    // The additive status field remains compatible with older serialized statuses.
    let mut legacy = serde_json::to_value(proxy.status()).unwrap();
    legacy.as_object_mut().unwrap().remove("degradedTargetIds");
    assert!(serde_json::from_value::<ProxyStatus>(legacy)
        .unwrap()
        .degraded_target_ids
        .is_empty());
}

#[test]
fn circuit_recovery_requires_consecutive_successes() {
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let retry = RetryPolicy {
        failure_threshold: 1,
        circuit_open_seconds: 60,
        ..RetryPolicy::default()
    };
    let route = single_target_route(
        "aipass_recovery_threshold_test",
        "http://127.0.0.1:1/v1".into(),
        retry.clone(),
    );
    let target_id = route.targets[0].config.id;
    let proxy = ProxyHandle::start(
        RuntimeConfig::from_routes("127.0.0.1:0", vec![route]),
        usage,
    )
    .unwrap();

    mark_failure(&proxy.state, target_id, &retry);
    {
        let mut health = proxy.state.health.lock().unwrap();
        health.get_mut(&target_id).unwrap().open_until =
            Some(Instant::now() - Duration::from_secs(1));
    }
    assert!(!circuit_open(&proxy.state, target_id));
    mark_success(&proxy.state, target_id);
    assert!(proxy.state.health.lock().unwrap().contains_key(&target_id));
    mark_success(&proxy.state, target_id);
    assert!(!proxy.state.health.lock().unwrap()[&target_id].degraded());
}

#[test]
fn runtime_config_update_resets_circuit_and_round_robin_state() {
    let bind_addr = available_addr();
    let dead_addr = available_addr();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let retry = RetryPolicy {
        failure_threshold: 1,
        circuit_open_seconds: 60,
        ..RetryPolicy::default()
    };
    let route = single_target_route(
        "aipass_runtime_reset_test",
        format!("http://{dead_addr}/v1"),
        retry.clone(),
    );
    let route_id = route.config.id;
    let target_id = route.targets[0].config.id;
    let proxy = ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage,
    )
    .unwrap();

    mark_failure(&proxy.state, target_id, &retry);
    let _ = round_robin_start(&proxy.state, route_id, &[1]);
    remember_affinity_target(&proxy.state, route_id, Some("session"), target_id);
    assert!(circuit_open(&proxy.state, target_id));
    assert!(!proxy.state.rr_counters.lock().unwrap().is_empty());
    assert!(!proxy.state.session_affinity.lock().unwrap().is_empty());
    assert_eq!(proxy.status().degraded_target_ids, vec![target_id]);

    let mut replacement = single_target_route(
        "aipass_runtime_reset_test",
        format!("http://{dead_addr}/v1"),
        retry,
    );
    replacement.config.id = route_id;
    replacement.targets[0].config.id = target_id;
    proxy
        .update_config(RuntimeConfig::from_routes(
            bind_addr.to_string(),
            vec![replacement],
        ))
        .unwrap();

    assert!(!circuit_open(&proxy.state, target_id));
    assert!(proxy.state.rr_counters.lock().unwrap().is_empty());
    assert!(proxy.state.session_affinity.lock().unwrap().is_empty());
    assert!(proxy.status().degraded_target_ids.is_empty());
    assert!(!proxy.status().degraded);
}

#[test]
fn upstream_url_does_not_duplicate_v1() {
    assert_eq!(
        upstream_url("https://api.example.test/v1", "/v1/messages").unwrap(),
        "https://api.example.test/v1/messages"
    );
}

#[test]
fn upstream_url_adds_v1_for_root_endpoint() {
    assert_eq!(
        upstream_url("https://api.example.test", "/v1/messages").unwrap(),
        "https://api.example.test/v1/messages"
    );
}

#[test]
fn upstream_url_strips_v1_for_chatgpt_codex_backend() {
    assert_eq!(
        upstream_url("https://chatgpt.com/backend-api/codex", "/v1/responses").unwrap(),
        "https://chatgpt.com/backend-api/codex/responses"
    );
}

#[test]
fn upstream_url_preserves_azure_query_without_v1_path() {
    assert_eq!(
            upstream_url_with_query(
                "https://example.openai.azure.com/openai/deployments/gpt?api-version=2024-10-21",
                "/chat/completions",
                Some("trace=enabled"),
            )
            .unwrap(),
            "https://example.openai.azure.com/openai/deployments/gpt/chat/completions?api-version=2024-10-21&trace=enabled"
        );
}

fn beta_target(configured_beta: Option<&str>) -> ResolvedTarget {
    let mut target = test_target("https://api.anthropic.com".into(), 0);
    if let Some(value) = configured_beta {
        target
            .config
            .headers
            .push(("anthropic-beta".into(), value.into()));
    }
    target
}

fn beta_header(headers: &HeaderMap) -> String {
    headers
        .get("anthropic-beta")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

#[test]
fn anthropic_beta_incoming_only_is_preserved() {
    let mut incoming = HeaderMap::new();
    incoming.insert(
        "anthropic-beta",
        HeaderValue::from_static("fine-grained-tool-results-2025-05-14"),
    );
    let target = beta_target(None);

    let headers =
        build_upstream_headers(&incoming, &target, ProxyProtocol::AnthropicMessages).unwrap();

    assert_eq!(
        beta_header(&headers),
        "fine-grained-tool-results-2025-05-14"
    );
}

#[test]
fn anthropic_beta_configured_only_is_applied() {
    let incoming = HeaderMap::new();
    let target = beta_target(Some("oauth-2025-04-20"));

    let headers =
        build_upstream_headers(&incoming, &target, ProxyProtocol::AnthropicMessages).unwrap();

    assert_eq!(beta_header(&headers), "oauth-2025-04-20");
}

#[test]
fn anthropic_beta_incoming_and_configured_are_merged_and_deduped() {
    let mut incoming = HeaderMap::new();
    incoming.insert(
        "anthropic-beta",
        HeaderValue::from_static("claude-code-20250219, oauth-2025-04-20"),
    );
    let target = beta_target(Some("oauth-2025-04-20, interleaved-thinking-2025-05-14"));

    let headers =
        build_upstream_headers(&incoming, &target, ProxyProtocol::AnthropicMessages).unwrap();

    assert_eq!(
        beta_header(&headers),
        "claude-code-20250219, oauth-2025-04-20, interleaved-thinking-2025-05-14"
    );
}

#[test]
fn unrelated_configured_headers_still_replace_incoming() {
    let mut incoming = HeaderMap::new();
    incoming.insert("x-custom-flag", HeaderValue::from_static("incoming"));
    let mut target = beta_target(Some("oauth-2025-04-20"));
    target
        .config
        .headers
        .push(("x-custom-flag".into(), "configured".into()));

    let headers =
        build_upstream_headers(&incoming, &target, ProxyProtocol::AnthropicMessages).unwrap();

    assert_eq!(
        headers
            .get("x-custom-flag")
            .and_then(|value| value.to_str().ok()),
        Some("configured")
    );
}

#[test]
fn model_discovery_uses_route_token_and_adds_cursor_protocol_metadata() {
    let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let upstream_addr = upstream.local_addr().unwrap();
    let upstream_thread = std::thread::spawn(move || {
        let (mut stream, _) = upstream.accept().unwrap();
        let (headers, body) = read_http_request(&mut stream);
        assert!(headers.starts_with("GET /v1/models HTTP/1.1\r\n"));
        assert!(headers
            .lines()
            .any(|line| line.eq_ignore_ascii_case("authorization: Bearer upstream-secret")));
        assert!(body.is_empty());
        let response = r#"{"object":"list","data":[{"id":"local-model","object":"model"}]}"#;
        write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response.len(),
                response
            )
            .unwrap();
    });

    let bind_addr = available_addr();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let token = "aipass_cursor_models_test";
    let mut route = single_target_route(
        token,
        format!("http://{upstream_addr}/v1"),
        RetryPolicy::default(),
    );
    route.config.inbound_protocol = ProxyProtocol::OpenAiChatCompletions;
    route.config.upstream_protocol = ProxyProtocol::OpenAiChatCompletions;
    let _proxy = ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage,
    )
    .unwrap();

    let response = reqwest::blocking::Client::new()
        .get(format!("http://{bind_addr}/v1/models"))
        .bearer_auth(token)
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value = response.json().unwrap();
    assert_eq!(payload["data"][0]["id"], "local-model");
    assert_eq!(payload["data"][0]["api_types"][0], "chat_completions");
    assert_eq!(
        payload["data"][0]["capabilities"]["supports_tool_use"],
        true
    );
    upstream_thread.join().unwrap();
}

#[test]
fn model_discovery_empty_and_error_success_bodies_fail_over() {
    for body in ["", r#"{"error":{"message":"quota exhausted"}}"#] {
        let (bad_addr, bad) = mock_upstream("200 OK", "application/json", body.into());
        let (good_addr, good) = mock_upstream(
            "200 OK",
            "application/json",
            r#"{"object":"list","data":[{"id":"fallback","object":"model"}]}"#.into(),
        );
        let token = "models_payload_fallback";
        let route = fallback_route(token, &[bad_addr, good_addr], RetryPolicy::default());
        let failed_id = route.targets[0].config.id;
        let bind_addr = available_addr();
        let proxy = start_proxy(bind_addr, route);
        let response = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap()
            .get(format!("http://{bind_addr}/v1/models"))
            .bearer_auth(token)
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let payload: serde_json::Value = response.json().unwrap();
        assert_eq!(payload["data"][0]["id"], "fallback");
        assert!(proxy.status().degraded_target_ids.contains(&failed_id));
        bad.join().unwrap();
        good.join().unwrap();
    }
}

#[test]
fn model_discovery_not_found_does_not_pollute_proxy_health() {
    let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let upstream_addr = upstream.local_addr().unwrap();
    let upstream_thread = std::thread::spawn(move || {
        let (mut stream, _) = upstream.accept().unwrap();
        let (headers, body) = read_http_request(&mut stream);
        assert!(headers.starts_with("GET /v1/models HTTP/1.1\r\n"));
        assert!(body.is_empty());
        write!(
            stream,
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
    });

    let bind_addr = available_addr();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let token = "aipass_models_not_found_health_test";
    let route = single_target_route(
        token,
        format!("http://{upstream_addr}/v1"),
        RetryPolicy::default(),
    );
    let proxy = ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage,
    )
    .unwrap();

    let response = reqwest::blocking::Client::new()
        .get(format!("http://{bind_addr}/v1/models"))
        .bearer_auth(token)
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(proxy.status().failures, 0);
    assert_eq!(proxy.status().last_error, None);
    upstream_thread.join().unwrap();
}

#[test]
fn local_health_endpoint_does_not_forward_to_upstream() {
    let bind_addr = available_addr();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let route = single_target_route(
        "aipass_health_endpoint_test",
        "http://127.0.0.1:1/v1".into(),
        RetryPolicy::default(),
    );
    let proxy = ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage,
    )
    .unwrap();

    let response = reqwest::blocking::Client::new()
        .get(format!("http://{bind_addr}/health"))
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.json::<serde_json::Value>().unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["service"], "aipass-proxy");
    assert_eq!(body["activeRoutes"], 1);
    assert_eq!(proxy.status().requests, 0);
    assert_eq!(proxy.status().failures, 0);
    assert_eq!(proxy.status().last_error, None);
}

#[test]
fn usage_store_persists_records() {
    let temp = tempfile::tempdir().unwrap();
    let store = UsageStore::open(temp.path().join("usage.sqlite")).unwrap();
    store
        .record(&UsageRecord {
            id: Uuid::new_v4(),
            started_at: 1,
            duration_ms: 2,
            first_token_ms: None,
            route_id: Uuid::new_v4(),
            provider_entry_id: Uuid::new_v4(),
            secret_id: "key".into(),
            model: Some("gpt".into()),
            inbound_protocol: ProxyProtocol::OpenAiResponses,
            upstream_protocol: ProxyProtocol::OpenAiResponses,
            status: 200,
            attempts: 1,
            input_tokens: 1,
            output_tokens: 2,
            cache_read_tokens: 3,
            cache_creation_tokens: 4,
            estimated_cost_micros: 5,
        })
        .unwrap();
    assert_eq!(store.count().unwrap(), 1);
}

#[test]
fn usage_store_can_clear_records_without_reopening_database() {
    let temp = tempfile::tempdir().unwrap();
    let store = UsageStore::open(temp.path().join("usage.sqlite")).unwrap();
    store
        .record(&UsageRecord {
            id: Uuid::new_v4(),
            started_at: 1,
            duration_ms: 2,
            first_token_ms: None,
            route_id: Uuid::new_v4(),
            provider_entry_id: Uuid::new_v4(),
            secret_id: "key".into(),
            model: None,
            inbound_protocol: ProxyProtocol::OpenAiResponses,
            upstream_protocol: ProxyProtocol::OpenAiResponses,
            status: 200,
            attempts: 1,
            input_tokens: 1,
            output_tokens: 1,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            estimated_cost_micros: 0,
        })
        .unwrap();
    store.clear().unwrap();
    assert_eq!(store.count().unwrap(), 0);
    store.clear().unwrap();
}

#[test]
fn start_reports_bind_conflicts() {
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let result = ProxyHandle::start(
        RuntimeConfig::from_routes(occupied.local_addr().unwrap().to_string(), Vec::new()),
        usage,
    );
    assert!(matches!(result, Err(ProxyError::InvalidConfig(_))));
}

#[tokio::test]
async fn request_body_spills_to_disk_and_remains_replayable() {
    let chunks = stream::iter(vec![
        Ok::<Bytes, std::io::Error>(Bytes::from_static(b"1234")),
        Ok(Bytes::from_static(b"56789")),
    ]);
    let body = read_replayable_request_chunks(chunks, 16, 4).await.unwrap();
    assert!(matches!(body, ReplayableRequestBody::File { .. }));
    assert_eq!(body.len(), 9);

    let first = body
        .request_body()
        .await
        .unwrap()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let second = body
        .request_body()
        .await
        .unwrap()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    assert_eq!(first, Bytes::from_static(b"123456789"));
    assert_eq!(second, first);
}

#[tokio::test]
async fn request_body_limit_is_enforced_while_reading() {
    let chunks = stream::iter(vec![
        Ok::<Bytes, std::io::Error>(Bytes::from_static(b"1234")),
        Ok(Bytes::from_static(b"56789")),
    ]);
    let result = read_replayable_request_chunks(chunks, 8, 4).await;
    assert!(matches!(result, Err(RequestBodyReadError::TooLarge)));
}

#[test]
fn responses_image_inputs_are_forwarded_without_rewriting() {
    for (case, image) in [
        (
            "url",
            serde_json::json!({
                "type": "input_image",
                "image_url": "https://example.test/image.png",
                "detail": "high"
            }),
        ),
        (
            "base64",
            serde_json::json!({
                "type": "input_image",
                "image_url": "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAAB",
                "detail": "auto"
            }),
        ),
    ] {
        let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let upstream_addr = upstream.local_addr().unwrap();
        let expected = serde_json::json!({
            "model": "gpt-vision-test",
            "input": [{
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "describe this image"},
                    image
                ]
            }]
        });
        let expected_upstream = expected.clone();
        let upstream_thread = std::thread::spawn(move || {
            let (mut stream, _) = upstream.accept().unwrap();
            let (headers, body) = read_http_request(&mut stream);
            assert!(headers
                .lines()
                .any(|line| line.eq_ignore_ascii_case("content-type: application/json")));
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
                expected_upstream
            );
            let response = r#"{"id":"resp_test","status":"completed","output":[]}"#;
            write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response.len(),
                    response
                )
                .unwrap();
        });

        let bind_addr = available_addr();
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let token = format!("aipass_image_passthrough_{case}");
        let route = single_target_route(
            &token,
            format!("http://{upstream_addr}/v1"),
            RetryPolicy::default(),
        );
        let _proxy = ProxyHandle::start(
            RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
            usage,
        )
        .unwrap();

        let response = reqwest::blocking::Client::new()
            .post(format!("http://{bind_addr}/v1/responses"))
            .bearer_auth(&token)
            .json(&expected)
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        upstream_thread.join().unwrap();
    }
}

#[test]
fn unsupported_multimodal_primary_fails_over_with_the_same_request() {
    let base64_image = "A".repeat(REQUEST_BODY_MEMORY_THRESHOLD + 1024);
    let request = serde_json::json!({
        "model": "gpt-vision-test",
        "input": [{
            "role": "user",
            "content": [
                {"type": "input_text", "text": "describe this image"},
                {
                    "type": "input_image",
                    "image_url": format!("data:image/png;base64,{base64_image}")
                }
            ]
        }]
    });

    let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let primary_addr = primary.local_addr().unwrap();
    let primary_request = request.clone();
    let primary_thread = std::thread::spawn(move || {
        let (mut stream, body) = loop {
            let (mut stream, _) = primary.accept().unwrap();
            let (headers, body) = read_http_request(&mut stream);
            let headers = headers.to_ascii_lowercase();
            if headers.starts_with("get / http/1.1\r\n") && !headers.contains("\r\nauthorization:")
            {
                write!(
                    stream,
                    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
                continue;
            }
            assert!(headers.starts_with("post /v1/responses "));
            break (stream, body);
        };
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
            primary_request
        );
        let response =
            r#"{"error":{"message":"Unsupported content type","type":"invalid_request_error"}}"#;
        write!(
                stream,
                "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response.len(),
                response
            )
            .unwrap();
    });

    let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let fallback_addr = fallback.local_addr().unwrap();
    let fallback_request = request.clone();
    let fallback_thread = std::thread::spawn(move || {
        let (mut stream, body) = loop {
            let (mut stream, _) = fallback.accept().unwrap();
            let (headers, body) = read_http_request(&mut stream);
            let headers = headers.to_ascii_lowercase();
            if headers.starts_with("get / http/1.1\r\n") && !headers.contains("\r\nauthorization:")
            {
                write!(
                    stream,
                    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
                continue;
            }
            assert!(headers.starts_with("post /v1/responses "));
            break (stream, body);
        };
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
            fallback_request
        );
        let response = r#"{"source":"fallback","status":"completed"}"#;
        write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response.len(),
                response
            )
            .unwrap();
    });

    let bind_addr = available_addr();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let token = "aipass_multimodal_fallback_test";
    let route = fallback_route(
        token,
        &[primary_addr, fallback_addr],
        RetryPolicy {
            max_attempts: 2,
            ..RetryPolicy::default()
        },
    );
    let _proxy = ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage,
    )
    .unwrap();

    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/responses"))
        .bearer_auth(token)
        .json(&request)
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value = response.json::<serde_json::Value>().unwrap();
    assert_eq!(value["source"], "fallback");
    assert!(!value.to_string().contains("Unsupported content type"));
    primary_thread.join().unwrap();
    fallback_thread.join().unwrap();
}

#[test]
fn forbidden_insufficient_balance_fails_over_before_returning_an_empty_response() {
    let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let primary_addr = primary.local_addr().unwrap();
    let primary_thread = std::thread::spawn(move || {
        let (mut stream, _) = primary.accept().unwrap();
        let _ = read_http_request(&mut stream);
        let body = r#"{"error":{"message":"insufficient balance: upstream-secret", "code":"balance_exhausted"},"input":"private prompt"}"#;
        write!(
                stream,
                "HTTP/1.1 403 Forbidden\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
    });

    let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let fallback_addr = fallback.local_addr().unwrap();
    let fallback_thread = std::thread::spawn(move || {
        let (mut stream, _) = fallback.accept().unwrap();
        let _ = read_http_request(&mut stream);
        let body = r#"{"status":"completed","source":"fallback"}"#;
        write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
    });

    let bind_addr = available_addr();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let token = "aipass_forbidden_balance_fallback_test";
    let route = fallback_route(
        token,
        &[primary_addr, fallback_addr],
        RetryPolicy {
            max_attempts: 2,
            ..RetryPolicy::default()
        },
    );
    let provider_id = route.targets[0].config.provider_entry_id;
    let _proxy = ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage.clone(),
    )
    .unwrap();

    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/responses"))
        .bearer_auth(token)
        .json(&serde_json::json!({"model":"balance-test","input":[]}))
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().unwrap();
    assert!(body.contains("fallback"));
    assert!(!body.contains("insufficient balance"));
    let logs = usage.logs().unwrap();
    let error = logs
        .iter()
        .find(|entry| entry.message.contains("event=proxy.upstream.rejected"))
        .unwrap();
    assert!(error
        .message
        .contains(&format!("provider_id={provider_id}")));
    assert!(error.message.contains("status=403"));
    assert!(error.message.contains("insufficient balance"));
    assert!(error.message.contains("balance_exhausted"));
    assert!(!error.message.contains("upstream-secret"));
    assert!(!error.message.contains("private prompt"));
    primary_thread.join().unwrap();
    fallback_thread.join().unwrap();
}

#[test]
fn successful_empty_upstream_body_fails_over_before_returning_to_client() {
    let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let primary_addr = primary.local_addr().unwrap();
    let primary_thread = std::thread::spawn(move || {
        let (mut stream, _) = primary.accept().unwrap();
        let _ = read_http_request(&mut stream);
        write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
    });

    let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let fallback_addr = fallback.local_addr().unwrap();
    let fallback_thread = std::thread::spawn(move || {
        let (mut stream, _) = fallback.accept().unwrap();
        let _ = read_http_request(&mut stream);
        let body = r#"{"status":"completed","source":"fallback"}"#;
        write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
    });

    let bind_addr = available_addr();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let token = "aipass_empty_body_fallback_test";
    let route = fallback_route(
        token,
        &[primary_addr, fallback_addr],
        RetryPolicy {
            max_attempts: 2,
            ..RetryPolicy::default()
        },
    );
    let _proxy = ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage,
    )
    .unwrap();

    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/responses"))
        .bearer_auth(token)
        .json(&serde_json::json!({"model":"empty-body-test","input":[]}))
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.text().unwrap().contains("fallback"));
    primary_thread.join().unwrap();
    fallback_thread.join().unwrap();
}

#[test]
fn silent_retry_replays_a_failed_upstream_round_before_returning_error() {
    let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let upstream_addr = upstream.local_addr().unwrap();
    let upstream_thread = std::thread::spawn(move || {
        for (index, status) in ["HTTP/1.1 503 Service Unavailable", "HTTP/1.1 200 OK"]
            .into_iter()
            .enumerate()
        {
            let (mut stream, _) = upstream.accept().unwrap();
            let (_, body) = read_http_request(&mut stream);
            assert!(!body.is_empty());
            let response = if index == 0 {
                r#"{"error":{"message":"temporary upstream failure"}}"#
            } else {
                r#"{"status":"completed"}"#
            };
            write!(
                    stream,
                    "{status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response.len(),
                    response
                )
                .unwrap();
        }
    });

    let bind_addr = available_addr();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let token = "aipass_silent_retry_test";
    let route = single_target_route(
        token,
        format!("http://{upstream_addr}/v1"),
        RetryPolicy {
            silent_retry: true,
            max_silent_retries: 1,
            ..RetryPolicy::default()
        },
    );
    let _proxy = ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage,
    )
    .unwrap();

    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/responses"))
        .bearer_auth(token)
        .json(&serde_json::json!({"model":"retry-test","input":[]}))
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.json::<serde_json::Value>().unwrap()["status"],
        "completed"
    );
    upstream_thread.join().unwrap();
}

#[test]
fn retry_policy_defaults_hold_fields_for_legacy_json() {
    let retry: RetryPolicy = serde_json::from_value(serde_json::json!({
        "maxAttempts": 3,
        "failureThreshold": 3,
        "circuitOpenSeconds": 30,
        "connectTimeoutMs": 10000,
        "firstByteTimeoutMs": 30000,
        "streamIdleTimeoutMs": 120000
    }))
    .unwrap();
    assert!(!retry.hold_on_failure);
    assert_eq!(retry.hold_initial_delay_ms, 500);
    assert_eq!(retry.hold_max_delay_ms, 10_000);
    assert_eq!(retry.hold_max_duration_ms, 300_000);
}

#[test]
fn hold_on_failure_retries_with_backoff_until_upstream_recovers() {
    let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let upstream_addr = upstream.local_addr().unwrap();
    let upstream_thread = std::thread::spawn(move || {
        for index in 0..3 {
            let (mut stream, _) = upstream.accept().unwrap();
            let _ = read_http_request(&mut stream);
            let (status, body) = if index < 2 {
                (
                    "HTTP/1.1 500 Internal Server Error",
                    r#"{"error":{"message":"upstream down"}}"#,
                )
            } else {
                ("HTTP/1.1 200 OK", r#"{"status":"completed"}"#)
            };
            write!(
                    stream,
                    "{status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
        }
    });

    let bind_addr = available_addr();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let token = "aipass_hold_backoff_test";
    let route = single_target_route(
        token,
        format!("http://{upstream_addr}/v1"),
        RetryPolicy {
            max_attempts: 1,
            hold_on_failure: true,
            hold_initial_delay_ms: 50,
            hold_max_delay_ms: 200,
            hold_max_duration_ms: 10_000,
            ..RetryPolicy::default()
        },
    );
    let _proxy = ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage,
    )
    .unwrap();

    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/responses"))
        .bearer_auth(token)
        .json(&serde_json::json!({"model":"hold-test","input":[]}))
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.json::<serde_json::Value>().unwrap()["status"],
        "completed"
    );
    upstream_thread.join().unwrap();
}

#[test]
fn hold_on_failure_returns_502_after_max_duration() {
    let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let upstream_addr = upstream.local_addr().unwrap();
    let request_count = Arc::new(AtomicU64::new(0));
    let counter = request_count.clone();
    std::thread::spawn(move || {
        for _ in 0..20 {
            let Ok((mut stream, _)) = upstream.accept() else {
                return;
            };
            let _ = read_http_request(&mut stream);
            counter.fetch_add(1, Ordering::SeqCst);
            let body = r#"{"error":{"message":"upstream down"}}"#;
            write!(
                    stream,
                    "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
        }
    });

    let bind_addr = available_addr();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let token = "aipass_hold_timeout_test";
    let route = single_target_route(
        token,
        format!("http://{upstream_addr}/v1"),
        RetryPolicy {
            max_attempts: 1,
            hold_on_failure: true,
            hold_initial_delay_ms: 50,
            hold_max_delay_ms: 200,
            hold_max_duration_ms: 300,
            ..RetryPolicy::default()
        },
    );
    let _proxy = ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage,
    )
    .unwrap();

    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/responses"))
        .bearer_auth(token)
        .json(&serde_json::json!({"model":"hold-test","input":[]}))
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert!(request_count.load(Ordering::SeqCst) >= 2);
}

#[tokio::test]
async fn hold_deadline_bounds_backoff_after_confirmed_failure() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let count = Arc::new(AtomicU64::new(0));
    let counter = count.clone();
    let server = tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            let counter = counter.clone();
            tokio::spawn(async move {
                let service = service_fn(move |request: Request<Incoming>| {
                    let counter = counter.clone();
                    async move {
                        if request.method() == http::Method::POST {
                            request.into_body().collect().await.unwrap();
                            counter.fetch_add(1, Ordering::SeqCst);
                        }
                        Ok::<_, Infallible>(error_response(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "unavailable",
                        ))
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(socket), service)
                    .await;
            });
        }
    });
    let route = single_target_route(
        "hold",
        format!("http://{addr}/v1"),
        RetryPolicy {
            hold_on_failure: true,
            hold_initial_delay_ms: 5_000,
            hold_max_delay_ms: 5_000,
            hold_max_duration_ms: 300,
            ..RetryPolicy::default()
        },
    );
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let mut config = RuntimeConfig::from_routes(available_addr().to_string(), vec![route]);
    config.upstream_proxy.mode = UpstreamProxyMode::Direct;
    let proxy = ProxyHandle::start(config, usage).unwrap();
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap()
        .post(format!("http://{}/v1/responses", proxy.bind_addr))
        .bearer_auth("hold")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "hold expiry must prevent the next submission"
    );
    server.abort();
}

#[tokio::test]
async fn hold_deadline_does_not_cut_off_a_committed_stream() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let service = service_fn(|_: Request<Incoming>| async {
            let source = stream::iter([0, 1]).then(|index| async move {
                    let data = if index == 0 {
                        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n"
                    } else {
                        tokio::time::sleep(Duration::from_millis(300)).await;
                        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n"
                    };
                    Ok::<_, BoxError>(Frame::data(Bytes::from_static(data.as_bytes())))
                });
            Ok::<_, Infallible>(
                Response::builder()
                    .header(header::CONTENT_TYPE, "text/event-stream")
                    .body(BodyExt::boxed_unsync(StreamBody::new(source)))
                    .unwrap(),
            )
        });
        let _ = hyper::server::conn::http1::Builder::new()
            .serve_connection(TokioIo::new(socket), service)
            .await;
    });
    let token = "hold_live_stream_test";
    let route = single_target_route(
        token,
        format!("http://{addr}/v1"),
        RetryPolicy {
            hold_on_failure: true,
            hold_max_duration_ms: 150,
            stream_idle_timeout_ms: 2_000,
            ..RetryPolicy::default()
        },
    );
    let bind_addr = available_addr();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let mut config = RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]);
    config.upstream_proxy.mode = UpstreamProxyMode::Direct;
    let proxy = ProxyHandle::start(config, usage.clone()).unwrap();
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap()
        .post(format!("http://{bind_addr}/v1/responses"))
        .bearer_auth(token)
        .json(&serde_json::json!({"model":"test","input":[],"stream":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response
        .text()
        .await
        .unwrap()
        .contains("response.completed"));
    for _ in 0..100 {
        if usage.count().unwrap() == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(proxy.status().requests, 1);
    assert_eq!(proxy.status().failures, 0);
    server.abort();
}

#[test]
fn hold_on_failure_waits_for_circuit_cooldown() {
    let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let upstream_addr = upstream.local_addr().unwrap();
    let upstream_thread = std::thread::spawn(move || {
        for index in 0..2 {
            let (mut stream, _) = upstream.accept().unwrap();
            let _ = read_http_request(&mut stream);
            let (status, body) = if index == 0 {
                (
                    "HTTP/1.1 500 Internal Server Error",
                    r#"{"error":{"message":"upstream down"}}"#,
                )
            } else {
                ("HTTP/1.1 200 OK", r#"{"status":"completed"}"#)
            };
            write!(
                    stream,
                    "{status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
        }
    });

    let bind_addr = available_addr();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let token = "aipass_hold_circuit_test";
    let route = single_target_route(
        token,
        format!("http://{upstream_addr}/v1"),
        RetryPolicy {
            max_attempts: 1,
            failure_threshold: 1,
            circuit_open_seconds: 1,
            hold_on_failure: true,
            hold_initial_delay_ms: 50,
            hold_max_delay_ms: 100,
            hold_max_duration_ms: 5_000,
            ..RetryPolicy::default()
        },
    );
    let _proxy = ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage,
    )
    .unwrap();

    let started = Instant::now();
    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/responses"))
        .bearer_auth(token)
        .json(&serde_json::json!({"model":"hold-test","input":[]}))
        .send()
        .unwrap();
    assert!(started.elapsed() >= Duration::from_secs(1));
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.json::<serde_json::Value>().unwrap()["status"],
        "completed"
    );
    upstream_thread.join().unwrap();
}

#[test]
fn silent_retry_does_not_replay_an_incomplete_stream() {
    let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let primary_addr = primary.local_addr().unwrap();
    let primary_thread = std::thread::spawn(move || {
        let (mut stream, _) = primary.accept().unwrap();
        let _ = read_http_request(&mut stream);
        let partial = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n";
        write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                partial.len(),
                partial
            )
            .unwrap();
    });

    let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let fallback_addr = fallback.local_addr().unwrap();

    let bind_addr = available_addr();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let token = "aipass_silent_stream_retry_test";
    let mut route = fallback_route(
        token,
        &[primary_addr, fallback_addr],
        RetryPolicy {
            max_attempts: 1,
            silent_retry: true,
            max_silent_retries: 1,
            ..RetryPolicy::default()
        },
    );
    route.config.strategy = RouteStrategy::RoundRobin;
    let _proxy = ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage,
    )
    .unwrap();

    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/responses"))
        .bearer_auth(token)
        .json(&serde_json::json!({"model":"stream-retry-test","stream":true}))
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let _ = response.text().unwrap();
    primary_thread.join().unwrap();
    fallback.set_nonblocking(true).unwrap();
    assert_eq!(
        fallback.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn truncated_non_stream_response_is_rejected_after_disconnect() {
    let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let upstream_addr = upstream.local_addr().unwrap();
    let upstream_thread = std::thread::spawn(move || {
        let (mut stream, _) = upstream.accept().unwrap();
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).unwrap();
        write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n"
            )
            .unwrap();
        stream.flush().unwrap();
        std::thread::sleep(Duration::from_millis(250));
    });

    let bind_addr = available_addr();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let token = "aipass_stream_timeout_test";
    let route = single_target_route(
        token,
        format!("http://{upstream_addr}/v1"),
        RetryPolicy {
            max_attempts: 1,
            first_byte_timeout_ms: 500,
            stream_idle_timeout_ms: 50,
            ..RetryPolicy::default()
        },
    );
    let _proxy = ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage,
    )
    .unwrap();

    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/responses"))
        .bearer_auth(token)
        .body("{}")
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(
        response.json::<serde_json::Value>().unwrap()["error"]["message"],
        "all upstream targets failed"
    );
    upstream_thread.join().unwrap();
}

#[test]
fn upstream_error_status_fails_over_without_reaching_the_client() {
    let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let primary_addr = primary.local_addr().unwrap();
    let primary_thread = std::thread::spawn(move || {
        let (mut stream, _) = primary.accept().unwrap();
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).unwrap();
        write!(
                stream,
                "HTTP/1.1 400 Bad Request\r\nContent-Length: 15\r\nConnection: close\r\n\r\nprimary failed"
            )
            .unwrap();
    });
    let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let fallback_addr = fallback.local_addr().unwrap();
    let fallback_thread = std::thread::spawn(move || {
        let (mut stream, _) = fallback.accept().unwrap();
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).unwrap();
        let body = r#"{"source":"fallback"}"#;
        write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
    });

    let bind_addr = available_addr();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let token = "aipass_status_failover_test";
    let route = fallback_route(
        token,
        &[primary_addr, fallback_addr],
        RetryPolicy {
            max_attempts: 2,
            ..RetryPolicy::default()
        },
    );
    let _proxy = ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage,
    )
    .unwrap();

    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/responses"))
        .bearer_auth(token)
        .body("{}")
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.json::<serde_json::Value>().unwrap()["source"],
        "fallback"
    );
    primary_thread.join().unwrap();
    fallback_thread.join().unwrap();
}

#[test]
fn upstream_redirect_is_not_followed_and_fails_over_internally() {
    let redirect_target = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    redirect_target.set_nonblocking(true).unwrap();
    let redirect_target_addr = redirect_target.local_addr().unwrap();
    let (redirect_hit_tx, redirect_hit_rx) = std::sync::mpsc::channel();
    let (redirect_stop_tx, redirect_stop_rx) = std::sync::mpsc::channel();
    let redirect_target_thread = std::thread::spawn(move || loop {
        if redirect_stop_rx.try_recv().is_ok() {
            break;
        }
        match redirect_target.accept() {
            Ok((mut stream, _)) => {
                let _ = redirect_hit_tx.send(());
                let mut request = [0_u8; 4096];
                let _ = stream.read(&mut request);
                let _ = write!(
                        stream,
                        "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    );
                break;
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(_) => break,
        }
    });

    let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let primary_addr = primary.local_addr().unwrap();
    let primary_thread = std::thread::spawn(move || {
        let (mut stream, _) = primary.accept().unwrap();
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).unwrap();
        write!(
                stream,
                "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://{redirect_target_addr}/v1/responses\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
    });
    let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let fallback_addr = fallback.local_addr().unwrap();
    let fallback_thread = std::thread::spawn(move || {
        let (mut stream, _) = fallback.accept().unwrap();
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).unwrap();
        let body = r#"{"source":"fallback"}"#;
        write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
    });

    let bind_addr = available_addr();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let token = "aipass_redirect_failover_test";
    let route = fallback_route(
        token,
        &[primary_addr, fallback_addr],
        RetryPolicy {
            max_attempts: 2,
            ..RetryPolicy::default()
        },
    );
    let _proxy = ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage,
    )
    .unwrap();

    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/responses"))
        .bearer_auth(token)
        .body("{}")
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.json::<serde_json::Value>().unwrap()["source"],
        "fallback"
    );
    assert!(redirect_hit_rx.try_recv().is_err());

    let _ = redirect_stop_tx.send(());
    redirect_target_thread.join().unwrap();
    primary_thread.join().unwrap();
    fallback_thread.join().unwrap();
}

#[test]
fn successful_json_error_payload_fails_over_internally() {
    let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let primary_addr = primary.local_addr().unwrap();
    let primary_thread = std::thread::spawn(move || {
        let (mut stream, _) = primary.accept().unwrap();
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).unwrap();
        let body = r#"{"error":{"message":"primary failed"}}"#;
        write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
    });
    let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let fallback_addr = fallback.local_addr().unwrap();
    let fallback_thread = std::thread::spawn(move || {
        let (mut stream, _) = fallback.accept().unwrap();
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).unwrap();
        let body = r#"{"source":"fallback"}"#;
        write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
    });

    let bind_addr = available_addr();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let token = "aipass_payload_failover_test";
    let route = fallback_route(
        token,
        &[primary_addr, fallback_addr],
        RetryPolicy {
            max_attempts: 2,
            ..RetryPolicy::default()
        },
    );
    let _proxy = ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage,
    )
    .unwrap();

    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/responses"))
        .bearer_auth(token)
        .body("{}")
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.json::<serde_json::Value>().unwrap()["source"],
        "fallback"
    );
    primary_thread.join().unwrap();
    fallback_thread.join().unwrap();
}

#[test]
fn compressed_json_error_payload_fails_over_internally() {
    let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let primary_addr = primary.local_addr().unwrap();
    let primary_thread = std::thread::spawn(move || {
        let (mut stream, _) = primary.accept().unwrap();
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).unwrap();
        let compressed = [
            31, 139, 8, 0, 0, 0, 0, 0, 0, 19, 171, 86, 74, 45, 42, 202, 47, 82, 178, 170, 86, 202,
            77, 45, 46, 78, 76, 79, 85, 178, 82, 42, 40, 202, 204, 77, 44, 170, 84, 72, 75, 204,
            204, 73, 77, 81, 170, 173, 5, 0, 53, 129, 192, 235, 38, 0, 0, 0,
        ];
        write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                compressed.len()
            )
            .unwrap();
        stream.write_all(&compressed).unwrap();
    });
    let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let fallback_addr = fallback.local_addr().unwrap();
    let fallback_thread = std::thread::spawn(move || {
        let (mut stream, _) = fallback.accept().unwrap();
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).unwrap();
        let body = r#"{"source":"fallback"}"#;
        write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
    });

    let bind_addr = available_addr();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let token = "aipass_compressed_failover_test";
    let route = fallback_route(
        token,
        &[primary_addr, fallback_addr],
        RetryPolicy {
            max_attempts: 2,
            ..RetryPolicy::default()
        },
    );
    let _proxy = ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage,
    )
    .unwrap();

    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/responses"))
        .bearer_auth(token)
        .body("{}")
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.json::<serde_json::Value>().unwrap()["source"],
        "fallback"
    );
    primary_thread.join().unwrap();
    fallback_thread.join().unwrap();
}

#[test]
fn lost_response_headers_do_not_replay_a_submitted_request() {
    let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let primary_addr = primary.local_addr().unwrap();
    let primary_thread = std::thread::spawn(move || {
        let (mut stream, _) = primary.accept().unwrap();
        let _ = read_http_request(&mut stream);
        std::thread::sleep(Duration::from_millis(200));
    });
    let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let fallback_addr = fallback.local_addr().unwrap();

    let bind_addr = available_addr();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let token = "aipass_header_timeout_test";
    let route = fallback_route(
        token,
        &[primary_addr, fallback_addr],
        RetryPolicy {
            max_attempts: 2,
            first_byte_timeout_ms: 50,
            ..RetryPolicy::default()
        },
    );
    let _proxy = ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage,
    )
    .unwrap();

    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/responses"))
        .bearer_auth(token)
        .body("{}")
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let _ = response.text().unwrap();
    primary_thread.join().unwrap();
    fallback.set_nonblocking(true).unwrap();
    assert_eq!(
        fallback.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn truncated_non_stream_body_does_not_replay_a_submitted_request() {
    let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let primary_addr = primary.local_addr().unwrap();
    let primary_thread = std::thread::spawn(move || {
        let (mut stream, _) = primary.accept().unwrap();
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).unwrap();
        write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 64\r\nConnection: close\r\n\r\npartial"
            )
            .unwrap();
    });
    let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let fallback_addr = fallback.local_addr().unwrap();

    let bind_addr = available_addr();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let token = "aipass_body_failover_test";
    let route = fallback_route(
        token,
        &[primary_addr, fallback_addr],
        RetryPolicy {
            max_attempts: 2,
            ..RetryPolicy::default()
        },
    );
    let _proxy = ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage,
    )
    .unwrap();

    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/responses"))
        .bearer_auth(token)
        .body("{}")
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let _ = response.text().unwrap();
    primary_thread.join().unwrap();
    fallback.set_nonblocking(true).unwrap();
    assert_eq!(
        fallback.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn stream_request_with_truncated_json_does_not_replay() {
    let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let primary_addr = primary.local_addr().unwrap();
    let primary_thread = std::thread::spawn(move || {
        let (mut stream, _) = primary.accept().unwrap();
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).unwrap();
        write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 64\r\nConnection: close\r\n\r\npartial"
            )
            .unwrap();
    });
    let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let fallback_addr = fallback.local_addr().unwrap();

    let bind_addr = available_addr();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let token = "aipass_stream_json_failover_test";
    let route = fallback_route(
        token,
        &[primary_addr, fallback_addr],
        RetryPolicy {
            max_attempts: 2,
            ..RetryPolicy::default()
        },
    );
    let _proxy = ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage,
    )
    .unwrap();

    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/responses"))
        .bearer_auth(token)
        .json(&serde_json::json!({"stream": true}))
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let _ = response.text().unwrap();
    primary_thread.join().unwrap();
    fallback.set_nonblocking(true).unwrap();
    assert_eq!(
        fallback.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn incomplete_first_sse_event_does_not_replay_a_submitted_request() {
    let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let primary_addr = primary.local_addr().unwrap();
    let primary_thread = std::thread::spawn(move || {
        let (mut stream, _) = primary.accept().unwrap();
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).unwrap();
        let partial = r#"data: {"partial":true}"#;
        write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n{:X}\r\n{}\r\n",
                partial.len(),
                partial
            )
            .unwrap();
        stream.flush().unwrap();
        std::thread::sleep(Duration::from_millis(200));
    });
    let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let fallback_addr = fallback.local_addr().unwrap();

    let bind_addr = available_addr();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let token = "aipass_sse_failover_test";
    let route = fallback_route(
        token,
        &[primary_addr, fallback_addr],
        RetryPolicy {
            max_attempts: 2,
            first_byte_timeout_ms: 50,
            ..RetryPolicy::default()
        },
    );
    let _proxy = ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage,
    )
    .unwrap();

    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/responses"))
        .bearer_auth(token)
        .json(&serde_json::json!({"stream": true}))
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let _ = response.text().unwrap();
    primary_thread.join().unwrap();
    fallback.set_nonblocking(true).unwrap();
    assert_eq!(
        fallback.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn first_sse_error_event_fails_over_before_stream_commit() {
    let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let primary_addr = primary.local_addr().unwrap();
    let primary_thread = std::thread::spawn(move || {
        let (mut stream, _) = primary.accept().unwrap();
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).unwrap();
        let body = "event: error\ndata: {\"error\":{\"message\":\"primary failed\"}}\n\n";
        write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
    });
    let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let fallback_addr = fallback.local_addr().unwrap();
    let fallback_thread = std::thread::spawn(move || {
        let (mut stream, _) = fallback.accept().unwrap();
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).unwrap();
        let body = "data: {\"source\":\"fallback\"}\n\n";
        write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
    });

    let bind_addr = available_addr();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let token = "aipass_sse_error_failover_test";
    let route = fallback_route(
        token,
        &[primary_addr, fallback_addr],
        RetryPolicy {
            max_attempts: 2,
            ..RetryPolicy::default()
        },
    );
    let _proxy = ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage.clone(),
    )
    .unwrap();

    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/responses"))
        .bearer_auth(token)
        .json(&serde_json::json!({"stream": true}))
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().unwrap();
    assert!(body.contains("fallback"));
    assert!(!body.contains("primary failed"));
    let logs = usage.logs().unwrap();
    let error = logs
        .iter()
        .find(|entry| entry.message.contains("event=proxy.upstream.rejected"))
        .unwrap();
    assert!(error.message.contains("transport=sse"));
    assert!(error.message.contains("primary failed"));
    primary_thread.join().unwrap();
    fallback_thread.join().unwrap();
}

#[test]
fn metadata_then_error_fails_over_before_stream_commit() {
    let primary = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let primary_addr = primary.local_addr().unwrap();
    let primary_thread = std::thread::spawn(move || {
        let (mut stream, _) = primary.accept().unwrap();
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).unwrap();
        let body = concat!(
                "event: response.created\n",
                "data: {\"type\":\"response.created\"}\n\n",
                "event: response.failed\n",
                "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"message\":\"primary failed\"}}}\n\n"
            );
        write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
    });
    let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let fallback_addr = fallback.local_addr().unwrap();
    let fallback_thread = std::thread::spawn(move || {
        let (mut stream, _) = fallback.accept().unwrap();
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).unwrap();
        let body = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"fallback\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n"
        );
        write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
    });

    let bind_addr = available_addr();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let token = "aipass_sse_metadata_failover_test";
    let route = fallback_route(
        token,
        &[primary_addr, fallback_addr],
        RetryPolicy {
            max_attempts: 2,
            ..RetryPolicy::default()
        },
    );
    let _proxy = ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage,
    )
    .unwrap();

    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/responses"))
        .bearer_auth(token)
        .json(&serde_json::json!({"stream": true}))
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().unwrap();
    assert!(body.contains("fallback"));
    assert!(!body.contains("primary failed"));
    assert!(!body.contains("response.created"));
    primary_thread.join().unwrap();
    fallback_thread.join().unwrap();
}

#[test]
fn stream_failure_after_commit_opens_the_target_circuit() {
    let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let upstream_addr = upstream.local_addr().unwrap();
    let upstream_thread = std::thread::spawn(move || {
        let (mut stream, _) = upstream.accept().unwrap();
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).unwrap();
        let body = "data: {\"delta\":\"started\"}\n\n";
        write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n{:X}\r\n{}\r\n",
                body.len(),
                body
            )
            .unwrap();
        stream.flush().unwrap();
    });

    let bind_addr = available_addr();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let token = "aipass_stream_circuit_test";
    let route = single_target_route(
        token,
        format!("http://{upstream_addr}/v1"),
        RetryPolicy {
            max_attempts: 1,
            failure_threshold: 1,
            ..RetryPolicy::default()
        },
    );
    let target_id = route.targets[0].config.id;
    let proxy = ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage,
    )
    .unwrap();

    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/responses"))
        .bearer_auth(token)
        .json(&serde_json::json!({"stream": true}))
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.text().is_err());
    assert!(circuit_open(&proxy.state, target_id));
    upstream_thread.join().unwrap();
}

#[test]
fn stream_completion_controls_circuit_health_and_usage_duration() {
    let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let upstream_addr = upstream.local_addr().unwrap();
    let upstream_thread = std::thread::spawn(move || {
        let (mut stream, _) = upstream.accept().unwrap();
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).unwrap();
        write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {{\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}}\n\n"
            )
            .unwrap();
        stream.flush().unwrap();
        std::thread::sleep(Duration::from_millis(80));
        write!(
                stream,
                "event: response.completed\ndata: {{\"type\":\"response.completed\",\"response\":{{\"status\":\"completed\"}}}}\n\n"
            )
            .unwrap();
        stream.flush().unwrap();
        std::thread::sleep(Duration::from_millis(350));
    });

    let bind_addr = available_addr();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let token = "aipass_stream_completion_test";
    let route = single_target_route(
        token,
        format!("http://{upstream_addr}/v1"),
        RetryPolicy {
            failure_threshold: 1,
            stream_idle_timeout_ms: 150,
            ..RetryPolicy::default()
        },
    );
    let target_id = route.targets[0].config.id;
    let proxy = ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage.clone(),
    )
    .unwrap();

    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/responses"))
        .bearer_auth(token)
        .json(&serde_json::json!({"stream": true}))
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.text().unwrap().contains("response.completed"));
    upstream_thread.join().unwrap();
    for _ in 0..30 {
        if usage.count().unwrap() == 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let duration_ms = usage
        .connection
        .lock()
        .unwrap()
        .query_row("SELECT duration_ms FROM proxy_usage", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap();
    assert!(duration_ms >= 60, "recorded duration was {duration_ms}ms");
    assert!(!circuit_open(&proxy.state, target_id));
}

#[test]
fn natural_stream_eof_without_terminal_event_opens_the_circuit() {
    let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let upstream_addr = upstream.local_addr().unwrap();
    let upstream_thread = std::thread::spawn(move || {
        let (mut stream, _) = upstream.accept().unwrap();
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).unwrap();
        let body = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n";
        write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
    });

    let bind_addr = available_addr();
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let token = "aipass_stream_incomplete_test";
    let route = single_target_route(
        token,
        format!("http://{upstream_addr}/v1"),
        RetryPolicy {
            failure_threshold: 1,
            ..RetryPolicy::default()
        },
    );
    let target_id = route.targets[0].config.id;
    let proxy = ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage,
    )
    .unwrap();

    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/responses"))
        .bearer_auth(token)
        .json(&serde_json::json!({"stream": true}))
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let _ = response.text().unwrap();
    for _ in 0..30 {
        if circuit_open(&proxy.state, target_id) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(circuit_open(&proxy.state, target_id));
    upstream_thread.join().unwrap();
}

#[test]
fn proxy_authenticates_fails_over_and_records_usage() {
    let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let upstream_addr = upstream.local_addr().unwrap();
    let (request_tx, request_rx) = std::sync::mpsc::channel();
    let upstream_thread = std::thread::spawn(move || {
        let (mut stream, _) = upstream.accept().unwrap();
        let mut request = vec![0_u8; 8192];
        let count = stream.read(&mut request).unwrap();
        request.truncate(count);
        request_tx
            .send(String::from_utf8_lossy(&request).to_string())
            .unwrap();
        let body = serde_json::json!({
            "id": "response-test",
            "status": "completed",
            "output": [],
            "usage": {
                "input_tokens": 12,
                "output_tokens": 4,
                "input_tokens_details": {"cached_tokens": 7, "cache_creation_tokens": 2}
            }
        })
        .to_string();
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
    });

    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let bind_addr = probe.local_addr().unwrap();
    drop(probe);
    let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let dead_addr = dead.local_addr().unwrap();
    drop(dead);
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let route_id = Uuid::new_v4();
    let provider_id = Uuid::new_v4();
    let target = |id, base_url, priority| ResolvedTarget {
        max_concurrent_requests: None,
        supports_websockets: false,
        config: ProxyTargetConfig {
            id,
            provider_entry_id: provider_id,
            secret_id: "primary".into(),
            label: "primary".into(),
            base_url,
            auth_scheme: "bearer".into(),
            headers: Vec::new(),
            group: Some("default".into()),
            priority,
            weight: 1,
            enabled: true,
            protocol: None,
            prefer_ws: false,
        },
        api_key: "upstream-secret".into(),
    };
    let token = "aipass_local_test";
    let route = ResolvedRoute {
        config: ProxyRouteConfig {
            id: route_id,
            name: "test".into(),
            token: String::new(),
            inbound_protocol: ProxyProtocol::OpenAiResponses,
            upstream_protocol: ProxyProtocol::OpenAiResponses,
            conversion_enabled: false,
            strategy: RouteStrategy::Fallback,
            targets: Vec::new(),
            retry: RetryPolicy {
                max_attempts: 2,
                connect_timeout_ms: 100,
                ..RetryPolicy::default()
            },
            enabled: true,
        },
        local_token: token.to_string(),
        targets: vec![
            target(Uuid::new_v4(), format!("http://{dead_addr}/v1"), 0),
            target(Uuid::new_v4(), format!("http://{upstream_addr}/v1"), 1),
        ],
    };
    let failed_target_id = route.targets[0].config.id;
    let proxy = ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage.clone(),
    )
    .unwrap();
    let client = reqwest::blocking::Client::new();
    let url = format!("http://{bind_addr}/v1/responses");
    let mut response = None;
    for _ in 0..30 {
        match client
            .post(&url)
            .bearer_auth(token)
            .header("api-key", "local-credential-must-not-forward")
            .json(&serde_json::json!({"model":"gpt-test","input":"hello"}))
            .send()
        {
            Ok(value) => {
                response = Some(value);
                break;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    let response = response.expect("proxy response");
    assert_eq!(response.status(), StatusCode::OK);
    let _ = response.text().unwrap();
    let upstream_request = request_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(upstream_request
        .to_ascii_lowercase()
        .contains("authorization: bearer upstream-secret"));
    assert!(!upstream_request.contains("local-credential-must-not-forward"));
    upstream_thread.join().unwrap();
    for _ in 0..30 {
        if usage.count().unwrap() == 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(usage.count().unwrap(), 1);
    let summary = usage.summary(|_| 0).unwrap();
    assert_eq!(summary.attempt_count, 2);
    assert_eq!(summary.successful_attempts, 1);
    assert_eq!(summary.success_rate_bps, 10_000);
    assert!(proxy.status().degraded);
    assert_eq!(proxy.status().degraded_target_ids, vec![failed_target_id]);
    assert_eq!(proxy.status().failures, 0);
    let request_id: String = usage
        .connection
        .lock()
        .unwrap()
        .query_row("SELECT id FROM proxy_usage", [], |row| row.get(0))
        .unwrap();
    let linked_attempts: i64 = usage
        .connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM proxy_attempts WHERE request_id = ?1",
            [&request_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(linked_attempts, 2);
    drop(proxy);
    let reopened = UsageStore::open(usage.path()).unwrap();
    let logs = reopened.logs().unwrap();
    let text = logs
        .iter()
        .map(|entry| entry.message.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("event=proxy.stopped"));
    assert!(text.contains("outcome=failed"));
    assert!(text.contains("outcome=success"));
    assert!(text.contains(&format!(
        "event=proxy.request.completed request_id={request_id}"
    )));
    for forbidden in [
        token,
        "upstream-secret",
        "local-credential-must-not-forward",
        "gpt-test",
        "hello",
        "http://",
    ] {
        assert!(!text.contains(forbidden));
    }
}

#[test]
fn route_config_defaults_keep_fallback_strategy() {
    let route: ProxyRouteConfig = serde_json::from_value(serde_json::json!({
        "id": Uuid::new_v4(),
        "name": "legacy",
        "token": "legacy-plaintext-token",
        "tokenFingerprint": "abc",
        "inboundProtocol": "open_ai_responses",
        "upstreamProtocol": "open_ai_responses",
        "conversionEnabled": false,
        "targets": [{
            "id": Uuid::new_v4(),
            "providerEntryId": Uuid::new_v4(),
            "secretId": "key",
            "label": "primary",
            "baseUrl": "https://api.example.test",
            "authScheme": "bearer",
            "group": null,
            "priority": 0,
            "enabled": true
        }],
        "retry": {
            "maxAttempts": 3,
            "failureThreshold": 3,
            "circuitOpenSeconds": 30,
            "connectTimeoutMs": 10000,
            "firstByteTimeoutMs": 30000,
            "streamIdleTimeoutMs": 120000
        },
        "enabled": true
    }))
    .unwrap();
    assert_eq!(route.strategy, RouteStrategy::Fallback);
    assert_eq!(route.token, "legacy-plaintext-token");
    assert_eq!(route.targets[0].weight, 1);
}

#[test]
fn round_robin_redistributes_weight_among_available_targets() {
    let mut route = fallback_route(
        "healthy_weight_test",
        &[
            "127.0.0.1:1".parse().unwrap(),
            "127.0.0.1:2".parse().unwrap(),
            "127.0.0.1:3".parse().unwrap(),
        ],
        RetryPolicy {
            failure_threshold: 1,
            max_attempts: 1,
            ..RetryPolicy::default()
        },
    );
    route.config.strategy = RouteStrategy::RoundRobin;
    route.targets[0].config.weight = 100;
    let proxy = start_proxy(available_addr(), route.clone());
    mark_failure(
        &proxy.state,
        route.targets[0].config.id,
        &route.config.retry,
    );
    let mut counts = [0; 2];
    for _ in 0..20 {
        let selected = select_route_targets(&proxy.state, &route);
        assert_eq!(selected.len(), 1);
        let index = route.targets[1..]
            .iter()
            .position(|target| target.config.id == selected[0].config.id)
            .unwrap();
        counts[index] += 1;
    }
    assert_eq!(counts, [10, 10]);
}

#[test]
fn session_affinity_prefers_last_successful_target_across_round_robin() {
    let mut route = fallback_route(
        "session_affinity_test",
        &[
            "127.0.0.1:1".parse().unwrap(),
            "127.0.0.1:2".parse().unwrap(),
        ],
        RetryPolicy {
            max_attempts: 1,
            ..RetryPolicy::default()
        },
    );
    route.config.strategy = RouteStrategy::RoundRobin;
    let proxy = start_proxy(available_addr(), route.clone());
    let session = "conversation-1";
    let first = select_route_targets_with_affinity(&proxy.state, &route, Some(session));
    assert_eq!(first.len(), 1);
    let target = first[0].config.id;
    remember_affinity_target(&proxy.state, route.config.id, Some(session), target);

    // Round-robin advances on every selection, but the remembered healthy
    // target remains first for this session.
    for _ in 0..4 {
        let selected = select_route_targets_with_affinity(&proxy.state, &route, Some(session));
        assert_eq!(selected[0].config.id, target);
    }
}

#[test]
fn session_affinity_is_cleared_when_a_target_fails() {
    let route = fallback_route(
        "session_affinity_failure_test",
        &["127.0.0.1:1".parse().unwrap()],
        RetryPolicy::default(),
    );
    let proxy = start_proxy(available_addr(), route.clone());
    let session = "conversation-1";
    let target = route.targets[0].config.id;
    remember_affinity_target(&proxy.state, route.config.id, Some(session), target);
    assert_eq!(
        affinity_target(&proxy.state, route.config.id, Some(session), &route.targets),
        Some(target)
    );
    mark_failure(&proxy.state, target, &route.config.retry);
    assert_eq!(
        affinity_target(&proxy.state, route.config.id, Some(session), &route.targets),
        None
    );
}

#[test]
fn session_affinity_key_accepts_headers_and_prompt_cache_fields() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-aipass-session-id",
        HeaderValue::from_static(" header-session "),
    );
    assert_eq!(
        session_affinity_key(&headers, None).as_deref(),
        Some("header-session")
    );
    let metadata: RequestMetadata =
        serde_json::from_str(r#"{"prompt_cache_key":"prompt-session"}"#).unwrap();
    assert_eq!(
        session_affinity_key(&HeaderMap::new(), Some(&metadata)).as_deref(),
        Some("prompt-session")
    );
    let metadata: RequestMetadata =
        serde_json::from_str(r#"{"conversation":{"id":"conversation-session"}}"#).unwrap();
    assert_eq!(
        session_affinity_key(&HeaderMap::new(), Some(&metadata)).as_deref(),
        Some("conversation-session")
    );
}

#[test]
fn weighted_start_index_follows_weight_distribution() {
    let weights = [1_u32, 3];
    let mut counts = [0_usize; 2];
    for counter in 0..8_u64 {
        counts[weighted_start_index(counter, &weights)] += 1;
    }
    assert_eq!(counts, [2, 6]);
    assert_eq!(weighted_start_index(0, &[]), 0);
    assert_eq!(weighted_start_index(5, &[0, 0]), 1);
}

#[test]
fn usage_timeseries_groups_records_by_day() {
    let temp = tempfile::tempdir().unwrap();
    let store = UsageStore::open(temp.path().join("usage.sqlite")).unwrap();
    let record = |started_at, input_tokens, model: Option<&str>| UsageRecord {
        id: Uuid::new_v4(),
        started_at,
        duration_ms: 1,
        first_token_ms: None,
        route_id: Uuid::new_v4(),
        provider_entry_id: Uuid::new_v4(),
        secret_id: "key".into(),
        model: model.map(str::to_string),
        inbound_protocol: ProxyProtocol::OpenAiResponses,
        upstream_protocol: ProxyProtocol::OpenAiResponses,
        status: 200,
        attempts: 1,
        input_tokens,
        output_tokens: 2,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        estimated_cost_micros: 3,
    };
    let today_start = now_unix() / 86_400 * 86_400;
    store
        .record(&record(today_start + 60, 10, Some("gpt-4o")))
        .unwrap();
    store
        .record(&record(today_start + 120, 5, Some("claude-3-7-sonnet")))
        .unwrap();
    store
        .record(&record(today_start - 86_400, 7, None))
        .unwrap();
    store
        .record(&record(today_start - 10 * 86_400, 99, None))
        .unwrap();

    let points = store
        .timeseries(7, 0, UsageGranularity::Day, |_| 3)
        .unwrap();
    assert_eq!(points.len(), 2);
    assert_eq!(points[0].request_count, 1);
    assert_eq!(points[0].input_tokens, 7);
    assert_eq!(points[1].request_count, 2);
    assert_eq!(points[1].input_tokens, 15);
    assert_eq!(points[1].output_tokens, 4);
    assert_eq!(points[1].estimated_cost_micros, 6);
    assert_eq!(points[0].models.len(), 1);
    assert_eq!(points[0].models[0].model, None);
    assert_eq!(points[0].models[0].request_count, 1);
    assert_eq!(points[0].models[0].input_tokens, 7);
    assert_eq!(points[1].models.len(), 2);
    assert_eq!(points[1].models[0].model.as_deref(), Some("gpt-4o"));
    assert_eq!(points[1].models[0].input_tokens, 10);
    assert_eq!(
        points[1].models[1].model.as_deref(),
        Some("claude-3-7-sonnet")
    );
    assert_eq!(points[1].models[1].input_tokens, 5);
    assert!(points.iter().all(|point| point.input_tokens != 99));
}

#[test]
fn usage_timeseries_uses_the_requested_local_timezone() {
    let temp = tempfile::tempdir().unwrap();
    let store = UsageStore::open(temp.path().join("usage.sqlite")).unwrap();
    let record = |started_at| UsageRecord {
        id: Uuid::new_v4(),
        started_at,
        duration_ms: 1,
        first_token_ms: None,
        route_id: Uuid::new_v4(),
        provider_entry_id: Uuid::new_v4(),
        secret_id: "key".into(),
        model: None,
        inbound_protocol: ProxyProtocol::OpenAiResponses,
        upstream_protocol: ProxyProtocol::OpenAiResponses,
        status: 200,
        attempts: 1,
        input_tokens: 1,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        estimated_cost_micros: 3,
    };
    let offset_seconds = 8 * 60 * 60;
    let local_today_start = local_day_start(now_unix(), offset_seconds);
    store.record(&record(local_today_start - 60)).unwrap();
    store.record(&record(local_today_start + 60)).unwrap();

    let points = store
        .timeseries(2, 8 * 60, UsageGranularity::Day, |_| 3)
        .unwrap();
    assert_eq!(points.len(), 2);
    assert_eq!(points[0].input_tokens, 1);
    assert_eq!(points[1].input_tokens, 1);
}

#[test]
fn usage_timeseries_groups_the_last_24_hours_in_local_hour_buckets() {
    // Include both whole-hour and fractional timezone offsets.
    for offset_minutes in [0, 480, 330, -210, 345] {
        let temp = tempfile::tempdir().unwrap();
        let store = UsageStore::open(temp.path().join("usage.sqlite")).unwrap();
        let offset_seconds = i64::from(offset_minutes) * 60;
        let current_hour = (now_unix() + offset_seconds).div_euclid(3_600) * 3_600 - offset_seconds;
        let cutoff = current_hour - 23 * 3_600;
        let record = |started_at, model: Option<&str>| UsageRecord {
            id: Uuid::new_v4(),
            started_at,
            duration_ms: 1,
            first_token_ms: None,
            route_id: Uuid::new_v4(),
            provider_entry_id: Uuid::new_v4(),
            secret_id: "key".into(),
            model: model.map(str::to_string),
            inbound_protocol: ProxyProtocol::OpenAiResponses,
            upstream_protocol: ProxyProtocol::OpenAiResponses,
            status: 200,
            attempts: 1,
            input_tokens: 10,
            output_tokens: 2,
            cache_read_tokens: 3,
            cache_creation_tokens: 4,
            estimated_cost_micros: 99,
        };
        for (timestamp, model) in [
            (cutoff - 1, None),
            (cutoff, None),
            (current_hour - 3_600, Some("model-a")),
            (current_hour - 1, Some("model-a")),
            (current_hour - 2, Some("model-b")),
            (current_hour, Some("model-b")),
        ] {
            store.record(&record(timestamp, model)).unwrap();
        }
        let points = store
            .timeseries(1, offset_minutes, UsageGranularity::Hour, |_| 7)
            .unwrap();
        assert_eq!(points.len(), 3);
        assert_eq!(points[0].request_count, 1);
        assert_eq!(points[0].models[0].model, None);
        assert_eq!(points[1].request_count, 3);
        assert_eq!(points[1].input_tokens, 30);
        assert_eq!(points[1].output_tokens, 6);
        assert_eq!(points[1].cache_read_tokens, 9);
        assert_eq!(points[1].cache_creation_tokens, 12);
        assert_eq!(points[1].estimated_cost_micros, 21);
        assert_eq!(points[1].models.len(), 2);
        assert_eq!(points[1].models[0].model.as_deref(), Some("model-a"));
        assert_eq!(points[1].models[0].request_count, 2);
        assert_eq!(points[1].models[0].estimated_cost_micros, 14);
        assert_eq!(points[2].request_count, 1);
        let conn = store.connection.lock().unwrap();
        for (point, timestamp) in points
            .iter()
            .zip([cutoff, current_hour - 3_600, current_hour])
        {
            let bucket_start: i64 = conn
                .query_row("SELECT unixepoch(?1)", params![point.date], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(bucket_start, timestamp);
        }
    }
}

#[test]
fn usage_summary_matches_chart_periods_and_filters_attempts() {
    for offset in [0, 480, 330, -210, 345] {
        for (days, granularity) in [
            (1, UsageGranularity::Hour),
            (7, UsageGranularity::Day),
            (30, UsageGranularity::Day),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let store = UsageStore::open(temp.path().join("usage.sqlite")).unwrap();
            let cutoff = usage_window_start(days, offset, granularity);
            let provider = Uuid::new_v4();
            let old_provider = Uuid::new_v4();
            for (started_at, provider_entry_id, status, latency) in [
                (cutoff - 1, old_provider, 502, 900),
                (cutoff - 1, provider, 502, 900),
                (cutoff, provider, 200, 120),
                (now_unix(), provider, 200, 240),
            ] {
                let record = UsageRecord {
                    id: Uuid::new_v4(),
                    started_at,
                    duration_ms: 1000,
                    first_token_ms: Some(latency),
                    route_id: Uuid::new_v4(),
                    provider_entry_id,
                    secret_id: "key".into(),
                    model: Some("model".into()),
                    inbound_protocol: ProxyProtocol::OpenAiResponses,
                    upstream_protocol: ProxyProtocol::OpenAiResponses,
                    status,
                    attempts: 1,
                    input_tokens: 10,
                    output_tokens: 2,
                    cache_read_tokens: 3,
                    cache_creation_tokens: 4,
                    estimated_cost_micros: 99,
                };
                store.record(&record).unwrap();
                store
                    .record_attempt(&AttemptRecord {
                        id: Uuid::new_v4(),
                        request_id: Some(record.id),
                        started_at,
                        duration_ms: 1000,
                        first_token_ms: Some(latency),
                        route_id: record.route_id,
                        target_id: Uuid::new_v4(),
                        provider_entry_id,
                        secret_id: "key".into(),
                        model: Some("model".into()),
                        status: Some(status),
                        success: Some(status == 200),
                    })
                    .unwrap();
            }
            let summary = store.summary_since(Some(cutoff), |_| 7).unwrap();
            let points = store.timeseries(days, offset, granularity, |_| 7).unwrap();
            assert_eq!(summary.request_count, 2);
            assert_eq!(
                summary.request_count,
                points.iter().map(|p| p.request_count).sum::<u64>()
            );
            assert_eq!(
                summary.input_tokens,
                points.iter().map(|p| p.input_tokens).sum::<u64>()
            );
            assert_eq!(
                summary.output_tokens,
                points.iter().map(|p| p.output_tokens).sum::<u64>()
            );
            assert_eq!(
                summary.cache_read_tokens,
                points.iter().map(|p| p.cache_read_tokens).sum::<u64>()
            );
            assert_eq!(
                summary.cache_creation_tokens,
                points.iter().map(|p| p.cache_creation_tokens).sum::<u64>()
            );
            assert_eq!(
                summary.estimated_cost_micros,
                points.iter().map(|p| p.estimated_cost_micros).sum::<u64>()
            );
            assert_eq!(summary.attempt_count, 2);
            assert_eq!(summary.completed_attempts, 2);
            assert_eq!(summary.successful_attempts, 2);
            assert_eq!(summary.success_rate_bps, 10_000);
            assert_eq!(summary.average_first_token_ms, Some(180));
            assert_eq!(summary.providers.len(), 1);
            assert_eq!(summary.providers[0].provider_entry_id, provider);
            assert_eq!(summary.providers[0].request_count, 2);
            assert_eq!(summary.providers[0].attempt_count, 2);
            assert_eq!(summary.providers[0].success_rate_bps, 10_000);
            assert_eq!(summary.providers[0].average_first_token_ms, Some(180));
            assert_eq!(summary.models.len(), 1);
            assert_eq!(summary.models[0].attempt_count, 2);
            assert_eq!(store.summary(|_| 7).unwrap().request_count, 4);
        }
    }
}

#[test]
fn usage_summary_recomputes_cost_with_injected_resolver() {
    let temp = tempfile::tempdir().unwrap();
    let store = UsageStore::open(temp.path().join("usage.sqlite")).unwrap();
    let provider_a = Uuid::new_v4();
    let provider_b = Uuid::new_v4();
    let record =
        |provider_entry_id, secret_id: &str, model: Option<&str>, started_at, input_tokens| {
            UsageRecord {
                id: Uuid::new_v4(),
                started_at,
                duration_ms: 1,
                first_token_ms: None,
                route_id: Uuid::new_v4(),
                provider_entry_id,
                secret_id: secret_id.into(),
                model: model.map(str::to_string),
                inbound_protocol: ProxyProtocol::OpenAiResponses,
                upstream_protocol: ProxyProtocol::OpenAiResponses,
                status: 200,
                attempts: 1,
                input_tokens,
                output_tokens: 2,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                estimated_cost_micros: 0,
            }
        };
    store
        .record(&record(provider_a, "key", Some("gpt-test"), 10, 100))
        .unwrap();
    store
        .record(&record(provider_a, "key", Some("claude-test"), 20, 50))
        .unwrap();
    store
        .record(&record(provider_b, "key", Some("gpt-test"), 30, 10))
        .unwrap();
    store
        .record(&record(provider_b, "key", None, 40, 5))
        .unwrap();

    // Stored estimated_cost_micros is 0; the injected resolver recomputes
    // cost per row at query time.
    let summary = store.summary(|row| row.input_tokens * 2).unwrap();
    assert_eq!(summary.request_count, 4);
    assert_eq!(summary.input_tokens, 165);
    assert_eq!(summary.output_tokens, 8);
    assert_eq!(summary.estimated_cost_micros, 330);
    assert_eq!(summary.providers.len(), 2);
    // Providers are ordered by most recent usage first.
    assert_eq!(summary.providers[0].provider_entry_id, provider_b);
    assert_eq!(summary.providers[0].estimated_cost_micros, 30);
    assert_eq!(summary.providers[0].request_count, 2);
    assert_eq!(summary.providers[1].provider_entry_id, provider_a);
    assert_eq!(summary.providers[1].request_count, 2);
    assert_eq!(summary.providers[1].estimated_cost_micros, 300);
    assert_eq!(summary.models.len(), 4);
    // Models are aggregated per (provider, model) and ordered by most
    // recent usage first, including records without a detected model.
    assert_eq!(summary.models[0].model, None);
    assert_eq!(summary.models[0].provider_entry_id, provider_b);
    assert_eq!(summary.models[0].request_count, 1);
    assert_eq!(summary.models[0].estimated_cost_micros, 10);
    assert_eq!(summary.models[1].model.as_deref(), Some("gpt-test"));
    assert_eq!(summary.models[1].provider_entry_id, provider_b);
    assert_eq!(summary.models[1].request_count, 1);
    assert_eq!(summary.models[1].input_tokens, 10);
    assert_eq!(summary.models[1].estimated_cost_micros, 20);
    assert_eq!(summary.models[2].model.as_deref(), Some("claude-test"));
    assert_eq!(summary.models[2].provider_entry_id, provider_a);
    assert_eq!(summary.models[2].estimated_cost_micros, 100);
    // The same model on a different provider stays a separate row.
    assert_eq!(summary.models[3].model.as_deref(), Some("gpt-test"));
    assert_eq!(summary.models[3].provider_entry_id, provider_a);
    assert_eq!(summary.models[3].request_count, 1);
    assert_eq!(summary.models[3].input_tokens, 100);
    assert_eq!(summary.models[3].estimated_cost_micros, 200);

    let rows = store.iter_rows().unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].started_at, 10);
    assert_eq!(rows[0].model.as_deref(), Some("gpt-test"));
}

#[test]
fn usage_summary_aggregates_attempt_health_and_first_token_latency() {
    let temp = tempfile::tempdir().unwrap();
    let store = UsageStore::open(temp.path().join("usage.sqlite")).unwrap();
    let provider_a = Uuid::new_v4();
    let provider_b = Uuid::new_v4();
    let route_id = Uuid::new_v4();
    let request =
        |provider_entry_id, secret_id: &str, started_at, status, first_token_ms| UsageRecord {
            id: Uuid::new_v4(),
            started_at,
            duration_ms: 10,
            first_token_ms,
            route_id,
            provider_entry_id,
            secret_id: secret_id.into(),
            model: Some("gpt-test".into()),
            inbound_protocol: ProxyProtocol::OpenAiResponses,
            upstream_protocol: ProxyProtocol::OpenAiResponses,
            status,
            attempts: 1,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            estimated_cost_micros: 0,
        };
    store
        .record(&request(provider_a, "primary", 10, 502, None))
        .unwrap();
    store
        .record(&request(provider_a, "primary", 20, 200, Some(120)))
        .unwrap();
    store
        .record(&request(provider_b, "backup", 40, 502, None))
        .unwrap();
    let attempt =
        |provider_entry_id, secret_id: &str, started_at, success, first_token_ms| AttemptRecord {
            id: Uuid::new_v4(),
            request_id: None,
            started_at,
            duration_ms: 10,
            first_token_ms,
            route_id,
            target_id: Uuid::new_v4(),
            provider_entry_id,
            secret_id: secret_id.into(),
            model: Some("gpt-test".into()),
            status: Some(if success == Some(true) { 200 } else { 502 }),
            success,
        };
    store
        .record_attempt(&attempt(provider_a, "primary", 10, Some(false), None))
        .unwrap();
    store
        .record_attempt(&attempt(provider_a, "primary", 20, Some(true), Some(120)))
        .unwrap();
    store
        .record_attempt(&attempt(provider_a, "primary", 30, None, Some(90)))
        .unwrap();
    store
        .record_attempt(&attempt(provider_b, "backup", 40, Some(false), None))
        .unwrap();

    let summary = store.summary(|_| 0).unwrap();
    assert_eq!(summary.request_count, 3);
    assert_eq!(summary.attempt_count, 4);
    assert_eq!(summary.completed_attempts, 3);
    assert_eq!(summary.successful_attempts, 1);
    assert_eq!(summary.success_rate_bps, 3_333);
    assert_eq!(summary.average_first_token_ms, Some(120));
    let provider = summary
        .providers
        .iter()
        .find(|row| row.provider_entry_id == provider_a)
        .unwrap();
    assert_eq!(provider.attempt_count, 3);
    assert_eq!(provider.completed_attempts, 2);
    assert_eq!(provider.successful_attempts, 1);
    assert_eq!(provider.success_rate_bps, 5_000);
    assert_eq!(provider.average_first_token_ms, Some(120));
    let model = summary
        .models
        .iter()
        .find(|row| row.provider_entry_id == provider_a)
        .unwrap();
    assert_eq!(model.attempt_count, 3);
    assert_eq!(model.success_rate_bps, 5_000);
    assert_eq!(model.average_first_token_ms, Some(120));
}

#[test]
fn request_stats_use_request_denominator_and_last_100_first_tokens() {
    let provider = Uuid::new_v4();
    let mut stats = RequestStats::default();
    for index in 0..101 {
        stats.observe(&UsageRow {
            started_at: index,
            provider_entry_id: provider,
            secret_id: "key".into(),
            model: None,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            status: if index == 0 { 502 } else { 200 },
            first_token_ms: Some(index as u64),
        });
    }
    assert_eq!(stats.request_count, 101);
    assert_eq!(stats.success_rate_bps(), 9_901);
    assert_eq!(stats.average_first_token_ms(), Some(50));
}

// --- cross-protocol conversion end-to-end ------------------------------

fn conversion_target(base_url: String, priority: u16, protocol: ProxyProtocol) -> ResolvedTarget {
    let mut target = test_target(base_url, priority);
    target.config.protocol = Some(protocol);
    target
}

fn conversion_route(
    token: &str,
    inbound: ProxyProtocol,
    upstream_fallback: ProxyProtocol,
    targets: Vec<ResolvedTarget>,
    max_attempts: u8,
) -> ResolvedRoute {
    ResolvedRoute {
        config: ProxyRouteConfig {
            id: Uuid::new_v4(),
            name: "conversion".into(),
            token: String::new(),
            inbound_protocol: inbound,
            upstream_protocol: upstream_fallback,
            conversion_enabled: true,
            strategy: RouteStrategy::Fallback,
            targets: Vec::new(),
            retry: RetryPolicy {
                max_attempts,
                ..RetryPolicy::default()
            },
            enabled: true,
        },
        local_token: token.into(),
        targets,
    }
}

/// Runs a mock upstream that captures one request and replies with the
/// given status, content type, and body.
fn mock_upstream(
    status: &str,
    content_type: &str,
    body: String,
) -> (SocketAddr, std::thread::JoinHandle<(String, Vec<u8>)>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let status = status.to_string();
    let content_type = content_type.to_string();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let captured = read_http_request(&mut stream);
        write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        captured
    });
    (addr, handle)
}

fn start_proxy(bind_addr: SocketAddr, route: ResolvedRoute) -> ProxyHandle {
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    ProxyHandle::start(
        RuntimeConfig::from_routes(bind_addr.to_string(), vec![route]),
        usage,
    )
    .unwrap()
}

#[test]
fn anthropic_client_to_chat_completions_upstream_non_streaming() {
    let (upstream_addr, upstream) = mock_upstream(
            "200 OK",
            "application/json",
            serde_json::json!({
                "id": "chatcmpl_1", "object": "chat.completion", "model": "gpt-test",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "Hi there"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 12, "completion_tokens": 3}
            })
            .to_string(),
        );
    let bind_addr = available_addr();
    let token = "aipass_conv_am_cc_test";
    let route = conversion_route(
        token,
        ProxyProtocol::AnthropicMessages,
        ProxyProtocol::AnthropicMessages,
        vec![conversion_target(
            format!("http://{upstream_addr}/v1"),
            0,
            ProxyProtocol::OpenAiChatCompletions,
        )],
        1,
    );
    let _proxy = start_proxy(bind_addr, route);

    let request = serde_json::json!({
        "model": "claude-test",
        "system": "Be terse.",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "Hello"}]}],
        "max_tokens": 64
    });
    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/messages"))
        .bearer_auth(token)
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-beta", "fine-grained-tool-results-2025-05-14")
        .json(&request)
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value = response.json().unwrap();
    assert_eq!(payload["type"], "message");
    assert_eq!(payload["role"], "assistant");
    assert_eq!(
        payload["content"][0],
        serde_json::json!({"type": "text", "text": "Hi there"})
    );
    assert_eq!(payload["stop_reason"], "end_turn");
    assert_eq!(payload["usage"]["input_tokens"], 12);
    assert_eq!(payload["usage"]["output_tokens"], 3);

    let (headers, body) = upstream.join().unwrap();
    // The upstream saw a converted Chat Completions request at the CC path.
    let sent: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        sent["messages"][0],
        serde_json::json!({"role": "system", "content": "Be terse."})
    );
    assert_eq!(
        sent["messages"][1],
        serde_json::json!({"role": "user", "content": "Hello"})
    );
    assert_eq!(sent["max_tokens"], 64);
    assert!(sent.get("system").is_none());
    assert!(sent.get("thinking").is_none());
    assert!(headers.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"));
    // Anthropic-only headers must not leak to the OpenAI-wire upstream.
    assert!(!headers.lines().any(|line| line
        .to_ascii_lowercase()
        .starts_with("anthropic-version:")
        || line.to_ascii_lowercase().starts_with("anthropic-beta:")));
}

#[test]
fn anthropic_client_to_chat_completions_upstream_streaming_tool_call() {
    let sse = concat!(
            "data: {\"id\":\"chatcmpl_1\",\"model\":\"gpt-test\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Let me check.\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"lookup\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"city\\\"\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\":\\\"Paris\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":20,\"completion_tokens\":9}}\n\n",
            "data: [DONE]\n\n",
        );
    let (upstream_addr, upstream) = mock_upstream("200 OK", "text/event-stream", sse.to_string());
    let bind_addr = available_addr();
    let token = "aipass_conv_am_cc_stream_test";
    let route = conversion_route(
        token,
        ProxyProtocol::AnthropicMessages,
        ProxyProtocol::AnthropicMessages,
        vec![conversion_target(
            format!("http://{upstream_addr}/v1"),
            0,
            ProxyProtocol::OpenAiChatCompletions,
        )],
        1,
    );
    let _proxy = start_proxy(bind_addr, route);

    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/messages"))
        .bearer_auth(token)
        .json(&serde_json::json!({
            "model": "claude-test",
            "messages": [{"role": "user", "content": "weather?"}],
            "max_tokens": 64,
            "stream": true
        }))
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().unwrap();

    for expected in [
        "event: message_start",
        "event: content_block_start",
        "event: content_block_delta",
        "event: content_block_stop",
        "event: message_delta",
        "event: message_stop",
    ] {
        assert!(body.contains(expected), "missing {expected} in:\n{body}");
    }
    assert!(!body.contains("[DONE]"));
    // serde_json emits object keys in sorted order.
    assert!(
        body.contains("\"text\":\"Let me check.\",\"type\":\"text_delta\""),
        "{body}"
    );
    assert!(
        body.contains("\"id\":\"call_1\",\"name\":\"lookup\",\"type\":\"tool_use\""),
        "{body}"
    );
    assert!(
        body.contains("\"partial_json\":\"{\\\"city\\\"\""),
        "{body}"
    );
    assert!(
        body.contains("\"partial_json\":\":\\\"Paris\\\"}\""),
        "{body}"
    );
    assert!(body.contains("\"stop_reason\":\"tool_use\""), "{body}");
    assert!(body.contains("\"output_tokens\":9"), "{body}");

    let (headers, body) = upstream.join().unwrap();
    let sent: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(sent["stream"], true);
    // Usage reporting is requested on the converted CC stream.
    assert_eq!(sent["stream_options"]["include_usage"], true);
    assert!(headers.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"));
}

#[test]
fn chat_completions_client_to_anthropic_upstream_non_streaming() {
    let (upstream_addr, upstream) = mock_upstream(
        "200 OK",
        "application/json",
        serde_json::json!({
            "id": "msg_1", "type": "message", "role": "assistant", "model": "claude-test",
            "content": [
                {"type": "text", "text": "Checking."},
                {"type": "tool_use", "id": "toolu_1", "name": "lookup", "input": {"city": "Paris"}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 10, "output_tokens": 8}
        })
        .to_string(),
    );
    let bind_addr = available_addr();
    let token = "aipass_conv_cc_am_test";
    let route = conversion_route(
        token,
        ProxyProtocol::OpenAiChatCompletions,
        ProxyProtocol::OpenAiChatCompletions,
        vec![conversion_target(
            format!("http://{upstream_addr}/v1"),
            0,
            ProxyProtocol::AnthropicMessages,
        )],
        1,
    );
    let _proxy = start_proxy(bind_addr, route);

    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/chat/completions"))
        .bearer_auth(token)
        .json(&serde_json::json!({
            "model": "gpt-test",
            "messages": [
                {"role": "system", "content": "Be terse."},
                {"role": "user", "content": "weather?"}
            ]
        }))
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value = response.json().unwrap();
    assert_eq!(payload["object"], "chat.completion");
    assert_eq!(payload["choices"][0]["message"]["content"], "Checking.");
    assert_eq!(
        payload["choices"][0]["message"]["tool_calls"][0],
        serde_json::json!({"id": "toolu_1", "type": "function", "function": {"name": "lookup", "arguments": "{\"city\":\"Paris\"}"}})
    );
    assert_eq!(payload["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(payload["usage"]["prompt_tokens"], 10);
    assert_eq!(payload["usage"]["completion_tokens"], 8);

    let (headers, body) = upstream.join().unwrap();
    let sent: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(sent["system"], "Be terse.");
    // max_tokens is required by the Anthropic API and defaults when absent.
    assert_eq!(sent["max_tokens"], 4096);
    assert_eq!(
        sent["messages"][0],
        serde_json::json!({"role": "user", "content": [{"type": "text", "text": "weather?"}]})
    );
    assert!(headers.starts_with("POST /v1/messages HTTP/1.1\r\n"));
    // The Anthropic upstream gets the required version header.
    assert!(headers
        .lines()
        .any(|line| line.eq_ignore_ascii_case("anthropic-version: 2023-06-01")));
}

#[test]
fn anthropic_client_to_responses_upstream_non_streaming() {
    let (upstream_addr, upstream) = mock_upstream(
            "200 OK",
            "application/json",
            serde_json::json!({
                "id": "resp_1", "status": "completed", "model": "gpt-test",
                "output": [
                    {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Hi there"}]}
                ],
                "usage": {"input_tokens": 12, "output_tokens": 3}
            })
            .to_string(),
        );
    let bind_addr = available_addr();
    let token = "aipass_conv_am_rs_test";
    let route = conversion_route(
        token,
        ProxyProtocol::AnthropicMessages,
        ProxyProtocol::AnthropicMessages,
        vec![conversion_target(
            format!("http://{upstream_addr}/v1"),
            0,
            ProxyProtocol::OpenAiResponses,
        )],
        1,
    );
    let _proxy = start_proxy(bind_addr, route);

    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/messages"))
        .bearer_auth(token)
        .header("anthropic-version", "2023-06-01")
        .json(&serde_json::json!({
            "model": "claude-test",
            "system": "Be terse.",
            "messages": [{"role": "user", "content": "Hello"}],
            "max_tokens": 64
        }))
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value = response.json().unwrap();
    assert_eq!(payload["type"], "message");
    assert_eq!(
        payload["content"][0],
        serde_json::json!({"type": "text", "text": "Hi there"})
    );
    assert_eq!(payload["stop_reason"], "end_turn");

    let (headers, body) = upstream.join().unwrap();
    let sent: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(sent["instructions"], "Be terse.");
    assert_eq!(sent["max_output_tokens"], 64);
    assert_eq!(
        sent["input"][0],
        serde_json::json!({"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Hello"}]})
    );
    assert!(headers.starts_with("POST /v1/responses HTTP/1.1\r\n"));
    assert!(!headers
        .lines()
        .any(|line| line.to_ascii_lowercase().starts_with("anthropic-version:")));
}

#[test]
fn invalid_converted_response_fails_over_without_recording_success() {
    let (bad_addr, bad) = mock_upstream("200 OK", "application/json", "not json".into());
    let (good_addr, good) = mock_upstream(
        "200 OK",
        "application/json",
        serde_json::json!({
            "id":"msg_ok", "type":"message", "role":"assistant", "model":"test",
            "content":[{"type":"text","text":"fallback"}],
            "stop_reason":"end_turn", "usage":{"input_tokens":5,"output_tokens":2}
        })
        .to_string(),
    );
    let token = "conversion_failure_test";
    let route = conversion_route(
        token,
        ProxyProtocol::AnthropicMessages,
        ProxyProtocol::AnthropicMessages,
        vec![
            conversion_target(
                format!("http://{bad_addr}/v1"),
                0,
                ProxyProtocol::OpenAiChatCompletions,
            ),
            conversion_target(
                format!("http://{good_addr}/v1"),
                1,
                ProxyProtocol::AnthropicMessages,
            ),
        ],
        2,
    );
    let failed_id = route.targets[0].config.id;
    let bind_addr = available_addr();
    let proxy = start_proxy(bind_addr, route);
    let response = reqwest::blocking::Client::builder().timeout(Duration::from_secs(5)).build().unwrap()
            .post(format!("http://{bind_addr}/v1/messages"))
            .bearer_auth(token)
            .json(&serde_json::json!({"model":"test","messages":[{"role":"user","content":"hello"}],"max_tokens":32}))
            .send().unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.text().unwrap().contains("fallback"));
    bad.join().unwrap();
    good.join().unwrap();
    for _ in 0..100 {
        if proxy.status().requests == 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(proxy.status().requests, 1);
    assert_eq!(proxy.status().failures, 0);
    assert!(proxy.status().degraded_target_ids.contains(&failed_id));
    let conn = proxy.state.usage.connection.lock().unwrap();
    let (failed, succeeded): (i64, i64) = conn
        .query_row(
            "SELECT SUM(success = 0), SUM(success = 1) FROM proxy_attempts",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((failed, succeeded), (1, 1));
}

#[test]
fn mixed_protocol_route_passes_native_through_and_converts_on_failover() {
    // Target 0 is Anthropic-native and fails; target 1 is Chat
    // Completions-native and must receive a converted request.
    let (native_addr, native) = mock_upstream(
        "503 Service Unavailable",
        "application/json",
        r#"{"error":{"message":"down"}}"#.to_string(),
    );
    let (foreign_addr, foreign) = mock_upstream(
            "200 OK",
            "application/json",
            serde_json::json!({
                "id": "chatcmpl_2", "object": "chat.completion", "model": "gpt-test",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "from fallback"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 5, "completion_tokens": 2}
            })
            .to_string(),
        );
    let bind_addr = available_addr();
    let token = "aipass_conv_mixed_test";
    let route = conversion_route(
        token,
        ProxyProtocol::AnthropicMessages,
        ProxyProtocol::AnthropicMessages,
        vec![
            conversion_target(
                format!("http://{native_addr}/v1"),
                0,
                ProxyProtocol::AnthropicMessages,
            ),
            conversion_target(
                format!("http://{foreign_addr}/v1"),
                1,
                ProxyProtocol::OpenAiChatCompletions,
            ),
        ],
        2,
    );
    let _proxy = start_proxy(bind_addr, route);

    let request = serde_json::json!({
        "model": "claude-test",
        "messages": [{"role": "user", "content": "Hello"}],
        "max_tokens": 32
    });
    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/messages"))
        .bearer_auth(token)
        .json(&request)
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value = response.json().unwrap();
    assert_eq!(
        payload["content"][0],
        serde_json::json!({"type": "text", "text": "from fallback"})
    );

    // The native target received the request bytes losslessly.
    let (_, native_body) = native.join().unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&native_body).unwrap(),
        request
    );
    // The foreign target received a converted Chat Completions request.
    let (_, foreign_body) = foreign.join().unwrap();
    let converted: serde_json::Value = serde_json::from_slice(&foreign_body).unwrap();
    assert_eq!(
        converted["messages"][0],
        serde_json::json!({"role": "user", "content": "Hello"})
    );
    assert!(converted.get("max_tokens").is_some());
}

#[test]
fn start_gate_rejects_unsupported_pairs_and_accepts_supported_conversion() {
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());

    // Chat Completions <-> Responses conversion is not implemented.
    let mut unsupported = conversion_route(
        "aipass_gate_unsupported",
        ProxyProtocol::OpenAiChatCompletions,
        ProxyProtocol::OpenAiChatCompletions,
        vec![conversion_target(
            "http://127.0.0.1:1/v1".into(),
            0,
            ProxyProtocol::OpenAiResponses,
        )],
        1,
    );
    assert!(matches!(
        ProxyHandle::start(
            RuntimeConfig::from_routes(available_addr().to_string(), vec![unsupported.clone()]),
            usage.clone(),
        ),
        Err(ProxyError::InvalidConfig(_))
    ));

    // A protocol mismatch without conversion enabled is a misconfiguration.
    unsupported.config.conversion_enabled = false;
    unsupported.targets[0].config.protocol = Some(ProxyProtocol::AnthropicMessages);
    assert!(matches!(
        ProxyHandle::start(
            RuntimeConfig::from_routes(available_addr().to_string(), vec![unsupported]),
            usage.clone(),
        ),
        Err(ProxyError::InvalidConfig(_))
    ));

    // Anthropic <-> Chat Completions conversion starts fine.
    let supported = conversion_route(
        "aipass_gate_supported",
        ProxyProtocol::AnthropicMessages,
        ProxyProtocol::AnthropicMessages,
        vec![conversion_target(
            "http://127.0.0.1:1/v1".into(),
            0,
            ProxyProtocol::OpenAiChatCompletions,
        )],
        1,
    );
    assert!(ProxyHandle::start(
        RuntimeConfig::from_routes(available_addr().to_string(), vec![supported]),
        usage,
    )
    .is_ok());
}

#[test]
fn start_gate_ignores_disabled_targets_when_validating_protocols() {
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let mut route = single_target_route(
        "aipass_disabled_target_protocol_test",
        "http://127.0.0.1:9/v1".into(),
        RetryPolicy::default(),
    );
    let mut disabled = route.targets[0].clone();
    disabled.config.id = Uuid::new_v4();
    disabled.config.enabled = false;
    disabled.config.protocol = Some(ProxyProtocol::AnthropicMessages);
    route.targets.push(disabled);
    assert!(ProxyHandle::start(
        RuntimeConfig::from_routes(available_addr().to_string(), vec![route]),
        usage,
    )
    .is_ok());
}
