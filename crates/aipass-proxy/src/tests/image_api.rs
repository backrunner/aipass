use super::*;

const IMAGE: &str =
    r#"{"created":1,"data":[{"b64_json":"aW1hZ2U="}],"vendor_extra":{"preserve":true}}"#;
const UNSUPPORTED: &str =
    r#"{"error":{"code":"unsupported_endpoint","message":"image generation is not supported"}}"#;

#[derive(Clone)]
struct Reply {
    status: StatusCode,
    content_type: &'static str,
    chunks: Vec<Bytes>,
    delay: Duration,
}

impl Reply {
    fn json(status: StatusCode, body: &str) -> Self {
        Self {
            status,
            content_type: "application/json",
            chunks: vec![Bytes::copy_from_slice(body.as_bytes())],
            delay: Duration::ZERO,
        }
    }
}

struct Captured {
    uri: String,
    headers: HeaderMap,
    body: Bytes,
}

struct Provider {
    addr: SocketAddr,
    requests: Arc<Mutex<Vec<Captured>>>,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for Provider {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl Provider {
    async fn new(reply: Reply) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let server = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    connection = listener.accept() => {
                        let (socket, _) = connection.unwrap();
                        let captured = captured.clone();
                        let reply = reply.clone();
                        connections.spawn(async move {
                            let service = service_fn(move |request: Request<Incoming>| {
                                let captured = captured.clone();
                                let reply = reply.clone();
                                async move {
                                    let (parts, body) = request.into_parts();
                                    let body = body.collect().await.unwrap().to_bytes();
                                    captured.lock().unwrap().push(Captured { uri: parts.uri.to_string(), headers: parts.headers, body });
                                    let chunks = stream::unfold((reply.chunks.into_iter(), false), move |(mut chunks, sent)| async move {
                                        let chunk = chunks.next()?;
                                        if sent { tokio::time::sleep(reply.delay).await; }
                                        Some((Ok::<_, Infallible>(Frame::data(chunk)), (chunks, true)))
                                    });
                                    Ok::<_, Infallible>(Response::builder().status(reply.status)
                                        .header(header::CONTENT_TYPE, reply.content_type)
                                        .header("x-provider-image", "preserved")
                                        .body(StreamBody::new(chunks)).unwrap())
                                }
                            });
                            let _ = hyper::server::conn::http1::Builder::new().serve_connection(TokioIo::new(socket), service).await;
                        });
                    }
                    _ = connections.join_next(), if !connections.is_empty() => {}
                }
            }
        });
        Self {
            addr,
            requests,
            server,
        }
    }

    fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

fn proxy(route: ResolvedRoute) -> (tempfile::TempDir, ProxyHandle) {
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let mut config = RuntimeConfig::from_routes(available_addr().to_string(), vec![route]);
    config.upstream_proxy.mode = UpstreamProxyMode::Direct;
    (temp, ProxyHandle::start(config, usage).unwrap())
}

fn image_request(proxy: &ProxyHandle, operation: &str, model: &str) -> reqwest::RequestBuilder {
    reqwest::Client::new()
        .post(format!("http://{}/v1/images/{operation}", proxy.bind_addr))
        .bearer_auth("image-local-token")
        .json(&serde_json::json!({"model": model, "prompt": "private image prompt"}))
}

async fn settled(proxy: &ProxyHandle, expected: u64) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while proxy.status().requests < expected {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn images_forward_json_headers_and_provider_base_without_websocket_or_conversion() {
    let provider = Provider::new(Reply::json(StatusCode::OK, IMAGE)).await;
    let mut route = fallback_route(
        "image-local-token",
        &[provider.addr],
        RetryPolicy::default(),
    );
    route.targets[0].config.base_url = format!("http://{}/gateway/v1?upstream=1", provider.addr);
    route.targets[0].supports_websockets = true;
    route.targets[0].config.headers = vec![("content-type".into(), "should-not-replace".into())];
    let (_temp, proxy) = proxy(route);
    let body =
        "{ \"model\":\"image-model\", \"prompt\":\"private image prompt\", \"vendor_extra\":42 }";
    let response = reqwest::Client::new()
        .post(format!(
            "http://{}/v1/images/generations?client=1",
            proxy.bind_addr
        ))
        .bearer_auth("image-local-token")
        .header("x-aipass-session-id", "local-only")
        .header("user-agent", "codex-test")
        .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-provider-image"], "preserved");
    assert_eq!(response.text().await.unwrap(), IMAGE);
    settled(&proxy, 1).await;
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].uri,
        "/gateway/v1/images/generations?upstream=1&client=1"
    );
    assert_eq!(requests[0].body.as_ref(), body.as_bytes());
    assert_eq!(
        requests[0].headers[header::AUTHORIZATION],
        "Bearer upstream-secret"
    );
    assert_eq!(
        requests[0].headers[header::CONTENT_TYPE],
        "application/json; charset=utf-8"
    );
    assert_eq!(requests[0].headers[header::USER_AGENT], "codex-test");
    assert!(!requests[0].headers.contains_key("x-aipass-session-id"));
    assert!(!requests[0].headers.contains_key(header::UPGRADE));
    let logs = proxy
        .logs()
        .unwrap()
        .iter()
        .map(|l| l.message.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(logs.contains("support=Supported"));
    assert!(!logs.contains("private image prompt"));
    assert!(!logs.contains("upstream-secret"));
    assert!(!logs.contains("aW1hZ2U="));
}

#[tokio::test]
async fn image_rejection_is_learned_per_model_and_operation_and_config() {
    let bad = Provider::new(Reply::json(StatusCode::NOT_IMPLEMENTED, UNSUPPORTED)).await;
    let good = Provider::new(Reply::json(StatusCode::OK, IMAGE)).await;
    let route = fallback_route(
        "image-local-token",
        &[bad.addr, good.addr],
        RetryPolicy::default(),
    );
    let good_id = route.targets[1].config.id;
    let (_temp, proxy) = proxy(route);
    let response = image_request(&proxy, "generations", "model-a")
        .send()
        .await
        .unwrap();
    assert_eq!(response.text().await.unwrap(), IMAGE);
    settled(&proxy, 1).await;
    assert_eq!(bad.count(), 1);
    assert_eq!(good.count(), 1);
    assert!(!proxy.status().degraded);
    // Exclusion is tested without a successful peer masking it by ranking.
    let mut config = proxy.state.config.read().unwrap().clone();
    config.routes[0].targets.retain(|t| t.config.id != good_id);
    proxy.update_config(config.clone()).unwrap();
    assert_eq!(
        image_request(&proxy, "generations", "model-a")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(bad.count(), 1);
    assert_eq!(
        image_request(&proxy, "edits", "model-a")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_GATEWAY
    );
    assert_eq!(bad.count(), 2);
    assert_eq!(
        image_request(&proxy, "generations", "model-b")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_GATEWAY
    );
    assert_eq!(bad.count(), 3);
    config.routes[0].targets[0].api_key = "new-upstream-key".into();
    proxy.update_config(config).unwrap();
    assert_eq!(
        image_request(&proxy, "generations", "model-a")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_GATEWAY
    );
    assert_eq!(bad.count(), 4);
}

#[tokio::test]
async fn image_filters_run_before_attempt_limit_and_do_not_change_chat_affinity() {
    let unsupported = Provider::new(Reply::json(StatusCode::BAD_REQUEST, UNSUPPORTED)).await;
    let good = Provider::new(Reply::json(StatusCode::OK, IMAGE)).await;
    let mut route = fallback_route(
        "image-local-token",
        &[unsupported.addr, good.addr],
        RetryPolicy {
            max_attempts: 1,
            ..RetryPolicy::default()
        },
    );
    route.config.conversion_enabled = true;
    let mut anthropic = test_target("http://127.0.0.1:1/v1".into(), 0);
    anthropic.config.protocol = Some(ProxyProtocol::AnthropicMessages);
    route.targets.insert(0, anthropic);
    let (_temp, proxy) = proxy(route.clone());
    remember_affinity_target(
        &proxy.state,
        route.config.id,
        Some("chat-session"),
        route.targets[1].config.id,
    );
    assert_eq!(
        image_request(&proxy, "generations", "model-a")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_GATEWAY
    );
    let response = image_request(&proxy, "generations", "model-a")
        .header("session_id", "chat-session")
        .send()
        .await
        .unwrap();
    assert_eq!(response.text().await.unwrap(), IMAGE);
    settled(&proxy, 2).await;
    assert_eq!(unsupported.count(), 1);
    assert_eq!(good.count(), 1);
    assert_eq!(
        affinity_target(
            &proxy.state,
            route.config.id,
            Some("chat-session"),
            &route.targets
        ),
        Some(route.targets[1].config.id)
    );
}

#[tokio::test]
async fn images_permissions_and_auth_failures_do_not_become_capability_bans() {
    let provider = Provider::new(Reply::json(
        StatusCode::FORBIDDEN,
        r#"{"error":{"message":"image generation is not supported by this group permission"}}"#,
    ))
    .await;
    let route = fallback_route(
        "image-local-token",
        &[provider.addr],
        RetryPolicy {
            failure_threshold: 10,
            ..RetryPolicy::default()
        },
    );
    let (_temp, proxy) = proxy(route);
    for _ in 0..2 {
        assert_eq!(
            image_request(&proxy, "generations", "model-a")
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_GATEWAY
        );
    }
    assert_eq!(provider.count(), 2);
    assert!(!proxy
        .logs()
        .unwrap()
        .iter()
        .any(|e| e.message.contains("support=Unsupported")));
    assert_eq!(
        reqwest::Client::new()
            .post(format!("http://{}/v1/images/generations", proxy.bind_addr))
            .bearer_auth("wrong")
            .json(&serde_json::json!({"model":"model-a", "prompt":"private"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        reqwest::Client::new()
            .post(format!("http://{}/v1/chat/completions", proxy.bind_addr))
            .bearer_auth("image-local-token")
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(provider.count(), 2);
}

#[tokio::test]
async fn images_multipart_edit_spools_large_binary_upload_and_preserves_boundary() {
    let provider = Provider::new(Reply::json(StatusCode::OK, IMAGE)).await;
    let route = fallback_route(
        "image-local-token",
        &[provider.addr],
        RetryPolicy::default(),
    );
    let (_temp, proxy) = proxy(route);
    let mut body = b"--image-boundary\r\nContent-Disposition: form-data; name=\"image[]\"; filename=\"input.png\"\r\nContent-Type: image/png\r\n\r\n".to_vec();
    body.extend(std::iter::repeat_n(
        0xff,
        REQUEST_BODY_MEMORY_THRESHOLD + 123,
    ));
    body.extend_from_slice(b"\r\n--image-boundary\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nimage-model\r\n--image-boundary\r\nContent-Disposition: form-data; name=\"prompt\"\r\n\r\nprivate edit prompt\r\n--image-boundary--\r\n");
    let response = reqwest::Client::new()
        .post(format!("http://{}/v1/images/edits", proxy.bind_addr))
        .bearer_auth("image-local-token")
        .header(
            header::CONTENT_TYPE,
            "multipart/form-data; boundary=\"image-boundary\"",
        )
        .body(body.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text().await.unwrap(), IMAGE);
    settled(&proxy, 1).await;
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests[0].uri, "/v1/images/edits");
    assert_eq!(requests[0].body.as_ref(), body);
    assert_eq!(
        requests[0].headers[header::CONTENT_TYPE],
        "multipart/form-data; boundary=\"image-boundary\""
    );
    assert_eq!(
        proxy.state.usage.rows_since(None).unwrap()[0]
            .1
            .model
            .as_deref(),
        Some("image-model")
    );
}

#[tokio::test]
async fn image_sse_preview_is_immediate_and_large_completion_is_observed() {
    let preview = Bytes::from_static(b"data: {\"type\":\"image_generation.partial_image\",\"b64_json\":\"cHJldmlldw==\",\"partial_image_index\":0}\n\n");
    let completed = format!(
        "data: {{\"type\":\"image_generation.completed\",\"b64_json\":\"{}\"}}\n\n",
        "A".repeat(2 * 1024 * 1024)
    );
    let provider = Provider::new(Reply {
        status: StatusCode::OK,
        content_type: "text/event-stream",
        chunks: vec![preview.clone(), Bytes::from(completed.clone())],
        delay: Duration::from_millis(600),
    })
    .await;
    let route = fallback_route(
        "image-local-token",
        &[provider.addr],
        RetryPolicy {
            first_byte_timeout_ms: 1,
            stream_idle_timeout_ms: 1,
            hold_max_duration_ms: 1,
            ..RetryPolicy::default()
        },
    );
    let (_temp, proxy) = proxy(route);
    let response = image_request(&proxy, "generations", "model-a")
        .json(&serde_json::json!({"model":"model-a", "prompt":"private", "stream":true}))
        .send()
        .await
        .unwrap();
    let mut chunks = response.bytes_stream();
    let first = tokio::time::timeout(Duration::from_millis(300), chunks.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(first, preview);
    let mut rest = Vec::new();
    while let Some(bytes) = chunks.next().await {
        rest.extend_from_slice(&bytes.unwrap());
    }
    assert_eq!(rest, completed.as_bytes());
    settled(&proxy, 1).await;
    assert_eq!(proxy.status().failures, 0);
    assert!(proxy
        .logs()
        .unwrap()
        .iter()
        .any(|e| e.message.contains("support=Supported")));
}

#[tokio::test]
async fn ambiguous_image_failure_and_partial_stream_never_replay() {
    for reply in [
        Reply::json(StatusCode::INTERNAL_SERVER_ERROR, r#"{"error":{"message":"internal failure after submission"}}"#),
        Reply { status: StatusCode::OK, content_type: "text/event-stream", chunks: vec![Bytes::from_static(b"data: {\"type\":\"image_generation.partial_image\",\"b64_json\":\"aW1hZ2U=\"}\n\n")], delay: Duration::ZERO },
    ] {
        let bad = Provider::new(reply).await;
        let good = Provider::new(Reply::json(StatusCode::OK, IMAGE)).await;
        let route = fallback_route("image-local-token", &[bad.addr, good.addr], RetryPolicy { silent_retry: true, hold_on_failure: true, ..RetryPolicy::default() });
        let (_temp, proxy) = proxy(route);
        let response = image_request(&proxy, "generations", "model-a").send().await.unwrap();
        let _ = response.bytes().await;
        settled(&proxy, 1).await;
        assert_eq!(bad.count(), 1);
        assert_eq!(good.count(), 0);
        assert!(!proxy.logs().unwrap().iter().any(|e| e.message.contains("support=")));
    }
}

#[tokio::test]
async fn images_reject_malformed_input_and_anthropic_tokens_before_forwarding() {
    let provider = Provider::new(Reply::json(StatusCode::OK, IMAGE)).await;
    let route = fallback_route(
        "image-local-token",
        &[provider.addr],
        RetryPolicy::default(),
    );
    let (_temp, proxy) = proxy(route);
    for (content_type, body) in [
        ("application/json", "{broken"),
        ("multipart/form-data", "bad boundary"),
        ("text/plain", "prompt"),
    ] {
        let response = reqwest::Client::new()
            .post(format!("http://{}/v1/images/edits", proxy.bind_addr))
            .bearer_auth("image-local-token")
            .header(header::CONTENT_TYPE, content_type)
            .body(body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    let mut config = proxy.state.config.read().unwrap().clone();
    config.routes[0].config.inbound_protocol = ProxyProtocol::AnthropicMessages;
    config.routes[0].config.upstream_protocol = ProxyProtocol::AnthropicMessages;
    proxy.update_config(config).unwrap();
    assert_eq!(
        image_request(&proxy, "generations", "model-a")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(provider.count(), 0);
}

#[tokio::test]
async fn image_json_error_with_http_success_fails_over_before_commit() {
    let bad = Provider::new(Reply::json(StatusCode::OK, UNSUPPORTED)).await;
    let good = Provider::new(Reply::json(StatusCode::OK, IMAGE)).await;
    let route = fallback_route(
        "image-local-token",
        &[bad.addr, good.addr],
        RetryPolicy::default(),
    );
    let (_temp, proxy) = proxy(route);
    assert_eq!(
        image_request(&proxy, "generations", "model-a")
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        IMAGE
    );
    settled(&proxy, 1).await;
    assert_eq!(bad.count(), 1);
    assert_eq!(good.count(), 1);
    assert!(proxy
        .logs()
        .unwrap()
        .iter()
        .any(|e| e.message.contains("support=Unsupported")));
}

#[tokio::test]
async fn image_stream_rejection_is_learned_without_replaying_committed_events() {
    let bad = Provider::new(Reply {
        status: StatusCode::OK,
        content_type: "text/event-stream",
        chunks: vec![Bytes::from_static(
            b"data: {\"type\":\"error\",\"error\":{\"code\":\"unsupported_endpoint\"}}\n\n",
        )],
        delay: Duration::ZERO,
    })
    .await;
    let route = fallback_route("image-local-token", &[bad.addr], RetryPolicy::default());
    let (_temp, proxy) = proxy(route);
    let response = image_request(&proxy, "generations", "model-a")
        .send()
        .await
        .unwrap();
    assert!(response
        .text()
        .await
        .unwrap()
        .contains("unsupported_endpoint"));
    settled(&proxy, 1).await;
    assert_eq!(
        image_request(&proxy, "generations", "model-a")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(bad.count(), 1);
}

#[tokio::test]
async fn image_stream_is_cancelled_on_credential_revocation_without_learning_failure() {
    let provider = Provider::new(Reply {
        status: StatusCode::OK,
        content_type: "text/event-stream",
        chunks: vec![
            Bytes::from_static(
                b"data: {\"type\":\"image_edit.partial_image\",\"b64_json\":\"preview\"}\n\n",
            ),
            Bytes::from_static(
                b"data: {\"type\":\"image_edit.completed\",\"b64_json\":\"result\"}\n\n",
            ),
        ],
        delay: Duration::from_secs(10),
    })
    .await;
    let route = fallback_route(
        "image-local-token",
        &[provider.addr],
        RetryPolicy::default(),
    );
    let (_temp, proxy) = proxy(route);
    let response = image_request(&proxy, "edits", "model-a")
        .send()
        .await
        .unwrap();
    let mut chunks = response.bytes_stream();
    assert!(chunks.next().await.unwrap().is_ok());
    let mut config = proxy.state.config.read().unwrap().clone();
    config.routes[0].targets[0].api_key = "rotated".into();
    proxy.update_config(config).unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        while chunks.next().await.is_some() {}
    })
    .await
    .unwrap();
    assert!(!proxy
        .logs()
        .unwrap()
        .iter()
        .any(|e| e.message.contains("support=")));
    assert_eq!(proxy.status().failures, 0);
}

#[tokio::test]
async fn provider_concurrency_images_skip_busy_targets_and_wait_only_before_submission() {
    for hold in [false, true] {
        let primary = Provider::new(Reply::json(StatusCode::OK, IMAGE)).await;
        let backup = Provider::new(Reply::json(StatusCode::OK, IMAGE)).await;
        let mut route = fallback_route(
            "image-local-token",
            &[primary.addr, backup.addr],
            RetryPolicy {
                max_attempts: 1,
                hold_on_failure: hold,
                hold_initial_delay_ms: 10,
                hold_max_delay_ms: 10,
                hold_max_duration_ms: 1000,
                ..RetryPolicy::default()
            },
        );
        for target in &mut route.targets {
            target.max_concurrent_requests = Some(1);
        }
        let targets = route.targets.clone();
        let (_temp, proxy) = proxy(route);
        let a = ProviderPermit::acquire(&proxy.state, &targets[0]).unwrap();
        let response = image_request(&proxy, "generations", "image-model")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        response.text().await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            while proxy
                .state
                .provider_activity
                .lock()
                .unwrap()
                .contains_key(&targets[1].config.provider_entry_id)
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(primary.count(), 0);
        assert_eq!(backup.count(), 1);
        let b = ProviderPermit::acquire(&proxy.state, &targets[1]).unwrap();
        let request = image_request(&proxy, "edits", "image-model");
        let pending = tokio::spawn(async move { request.send().await.unwrap() });
        if hold {
            tokio::time::sleep(Duration::from_millis(50)).await;
            assert!(!pending.is_finished());
            drop(a);
            let response = pending.await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            response.text().await.unwrap();
            assert_eq!(primary.count(), 1);
        } else {
            assert_eq!(
                pending.await.unwrap().status(),
                StatusCode::TOO_MANY_REQUESTS
            );
        }
        drop(b);
        assert!(proxy.status().degraded_target_ids.is_empty());
    }
}
