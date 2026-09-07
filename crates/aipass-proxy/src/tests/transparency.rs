use super::*;

#[test]
fn upstream_version_follows_the_explicit_v1_path_segment() {
    for (base, prefix) in [
        ("", "/v1"),
        ("/gateway", "/gateway/v1"),
        ("/v1", "/v1"),
        ("/api/v1", "/api/v1"),
        ("/v1/gateway", "/v1/gateway"),
        ("/compatible-mode/v1", "/compatible-mode/v1"),
        ("/api/paas/v4", "/api/paas/v4/v1"),
        ("/api/v3", "/api/v3/v1"),
        ("/v3/openai", "/v3/openai/v1"),
        ("/v1beta/openai", "/v1beta/openai/v1"),
        ("/v10", "/v10/v1"),
        ("/openai", "/openai/v1"),
        ("/inference/v1", "/inference/v1"),
        ("/openai/v1", "/openai/v1"),
        ("/backend-api/codex", "/backend-api/codex"),
    ] {
        for resource in ["responses", "chat/completions", "models", "messages"] {
            assert_eq!(
                upstream_url_with_query(
                    &format!("https://v1.provider.test{base}/?api-version=v1"),
                    &format!("/v1/{resource}"),
                    Some("client=codex"),
                )
                .unwrap(),
                format!("https://v1.provider.test{prefix}/{resource}?api-version=v1&client=codex"),
            );
        }
    }
    for base in ["", "/gateway", "/anthropic", "/archive-v4"] {
        assert_eq!(
            upstream_url(&format!("https://provider.test{base}"), "/v1/messages").unwrap(),
            format!("https://provider.test{base}/v1/messages"),
        );
    }
}

fn client_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in [
        ("authorization", "Bearer local-token"),
        ("x-api-key", "local-token"),
        ("api-key", "local-token"),
        ("user-agent", "codex_cli_rs/1.0.0 (Mac OS; arm64)"),
        ("x-user-agent", "client-sdk/1.0"),
        ("originator", "codex_cli_rs"),
        ("session_id", "conversation"),
        ("x-codex-turn-metadata", "opaque-client-metadata"),
        ("x-aipass-session-id", "local-session"),
        ("x-aipass-trace-id", "local-trace"),
    ] {
        headers.insert(
            header::HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
    headers
}

fn configured_headers() -> Vec<(String, String)> {
    [
        ("User-Agent", "AIPass/1.0"),
        ("X-User-Agent", "another-sdk/2.0"),
        ("Originator", "aipass"),
        ("X-AIPass-Trace-Id", "configured-local-trace"),
        ("aipass-version", "1.0"),
        ("HTTP-Referer", "https://aipass.example.test"),
        ("X-Title", "AIPass"),
        ("chatgpt-account-id", "provider-account"),
    ]
    .into_iter()
    .map(|(name, value)| (name.into(), value.into()))
    .collect()
}

#[test]
fn client_identity_wins_without_leaking_proxy_identity_or_local_auth() {
    for (protocol, scheme, auth_name, auth_value) in [
        (
            ProxyProtocol::OpenAiResponses,
            "bearer",
            "authorization",
            "Bearer upstream-secret",
        ),
        (
            ProxyProtocol::OpenAiResponses,
            "custom_header",
            "authorization",
            "upstream-secret",
        ),
        (
            ProxyProtocol::OpenAiChatCompletions,
            "azure_api_key",
            "api-key",
            "upstream-secret",
        ),
        (
            ProxyProtocol::AnthropicMessages,
            "x_api_key",
            "x-api-key",
            "upstream-secret",
        ),
    ] {
        let mut target = test_target("https://provider.test/v1".into(), 0);
        target.config.auth_scheme = scheme.into();
        target.config.headers = configured_headers();
        let headers = build_upstream_headers(&client_headers(), &target, protocol).unwrap();
        for name in [
            "user-agent",
            "x-user-agent",
            "originator",
            "session_id",
            "x-codex-turn-metadata",
        ] {
            assert_eq!(headers[name], client_headers()[name]);
        }
        assert_eq!(headers[auth_name], auth_value);
        for name in ["authorization", "api-key", "x-api-key"] {
            assert_eq!(headers.contains_key(name), name == auth_name);
        }
        assert_eq!(headers["chatgpt-account-id"], "provider-account");
        assert!(!headers.keys().any(|name| name.as_str().contains("aipass")));
        assert!(!headers.values().any(|value| value
            .as_bytes()
            .windows(6)
            .any(|part| part.eq_ignore_ascii_case(b"aipass"))));
    }
    // No invented client identity when the caller supplies none. Reject stale
    // product identity in both imported configuration and incoming headers.
    let mut target = test_target("https://provider.test/v1".into(), 0);
    target.config.headers = configured_headers();
    let mut incoming = HeaderMap::new();
    incoming.insert(header::USER_AGENT, HeaderValue::from_static("AiPaSs/old"));
    let headers =
        build_upstream_headers(&incoming, &target, ProxyProtocol::OpenAiResponses).unwrap();
    assert!(!headers.contains_key(header::USER_AGENT));
    assert!(!headers.contains_key("originator"));
    assert_eq!(headers["x-user-agent"], "another-sdk/2.0");
}

#[test]
fn native_http_and_sse_preserve_codex_bytes_and_identity_on_the_wire() {
    for streaming in [false, true] {
        let response_body = if streaming {
            "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_test\",\"status\":\"completed\",\"output\":[]}}\n\n"
        } else {
            "{ \"id\":\"resp_test\", \"status\":\"completed\", \"output\":[], \"provider_extension\":true }"
        };
        let (address, upstream) = mock_upstream(
            "200 OK",
            if streaming {
                "text/event-stream"
            } else {
                "application/json"
            },
            response_body.into(),
        );
        let mut route = single_target_route(
            "local-token",
            format!("http://{address}/api/v3"),
            RetryPolicy::default(),
        );
        route.targets[0].config.headers = configured_headers();
        let bind_addr = available_addr();
        let _proxy = start_proxy(bind_addr, route);
        // Opaque fields, tool definitions and whitespace survive native relay.
        let body = format!(
            r#"{{ "model":"test", "stream":{streaming}, "input":[{{"type":"function_call_output","call_id":"call_1","output":"ok"}}], "tools":[{{"type":"custom","name":"terminal","format":{{"type":"text"}}}}], "provider_extension":{{"opaque":true}} }}"#
        );
        let response = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap()
            .post(format!("http://{bind_addr}/v1/responses?client=codex"))
            .headers(client_headers())
            .body(body.clone())
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.text().unwrap(), response_body);
        let (headers, received_body) = upstream.join().unwrap();
        assert!(headers.starts_with("POST /api/v3/v1/responses?client=codex HTTP/1.1\r\n"));
        assert_eq!(received_body, body.as_bytes());
        let headers = headers.to_ascii_lowercase();
        assert!(headers.contains("user-agent: codex_cli_rs/1.0.0 (mac os; arm64)\r\n"));
        assert!(headers.contains("originator: codex_cli_rs\r\n"));
        assert!(headers.contains("authorization: bearer upstream-secret\r\n"));
        assert!(!headers.contains("local-token"));
        assert!(!headers.contains("aipass"));
    }
}
