use super::*;

#[tokio::test]
async fn slow_http_generations_wait_without_failover_and_report_channel_activity() {
    for mode in [
        "headers",
        "body",
        "prefetch",
        "stream",
        "silent_stream",
        "cancel",
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let fallback = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (received, arrival) = oneshot::channel();
        let (release, wait) = oneshot::channel();
        let received = Arc::new(Mutex::new(Some(received)));
        let wait = Arc::new(Mutex::new(Some(wait)));
        let streaming = matches!(mode, "prefetch" | "stream" | "silent_stream");
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let service = service_fn(move |request: Request<Incoming>| {
                let received = received.lock().unwrap().take().unwrap();
                let wait = wait.lock().unwrap().take().unwrap();
                async move {
                    request.into_body().collect().await.unwrap();
                    received.send(()).unwrap();
                    let mut wait = Some(wait);
                    if matches!(mode, "headers" | "cancel") {
                        let _ = wait.take().unwrap().await;
                    }
                    let first = if mode == "prefetch" {
                        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"slow\"}}\n\n"
                    } else if streaming {
                        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n"
                    } else {
                        "{\"output\":"
                    };
                    let last = if streaming {
                        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"slow\",\"status\":\"completed\",\"output\":[]}}\n\n"
                    } else {
                        "[]}"
                    };
                    let body = stream::once(async move {
                        Ok::<_, BoxError>(Frame::data(Bytes::from_static(first.as_bytes())))
                    })
                    .chain(stream::once(async move {
                        if let Some(wait) = wait {
                            let _ = wait.await;
                        }
                        Ok::<_, BoxError>(Frame::data(Bytes::from_static(last.as_bytes())))
                    }));
                    Ok::<_, Infallible>(
                        Response::builder()
                            .header(
                                header::CONTENT_TYPE,
                                if streaming {
                                    "text/event-stream"
                                } else {
                                    "application/json"
                                },
                            )
                            .body(BodyExt::boxed_unsync(StreamBody::new(body)))
                            .unwrap(),
                    )
                }
            });
            let _ = hyper::server::conn::http1::Builder::new()
                .serve_connection(TokioIo::new(socket), service)
                .await;
        });
        let temp = tempfile::tempdir().unwrap();
        let store = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let route = fallback_route(
            "slow-test",
            &[addr, fallback.local_addr().unwrap()],
            RetryPolicy {
                max_attempts: 2,
                first_byte_timeout_ms: 20,
                stream_idle_timeout_ms: 20,
                hold_on_failure: true,
                hold_max_duration_ms: 30,
                silent_retry: mode == "silent_stream",
                ..RetryPolicy::default()
            },
        );
        let mut config = RuntimeConfig::from_routes(available_addr().to_string(), vec![route]);
        config.upstream_proxy.mode = UpstreamProxyMode::Direct;
        let proxy = ProxyHandle::start(config.clone(), store).unwrap();
        assert_eq!(
            (
                proxy.status().available_channels,
                proxy.status().total_channels
            ),
            (2, 2)
        );
        let url = format!("http://{}/v1/responses", proxy.bind_addr);
        let request = tokio::spawn(async move {
            let response = reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(3))
                .build()
                .unwrap()
                .post(url)
                .bearer_auth("slow-test")
                .json(&serde_json::json!({"model":"test","stream":streaming}))
                .send()
                .await
                .unwrap();
            (response.status(), response.text().await.unwrap())
        });
        tokio::time::timeout(Duration::from_secs(1), arrival)
            .await
            .unwrap()
            .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            !request.is_finished(),
            "{mode}: a submitted request must keep waiting"
        );
        let status = proxy.status();
        assert_eq!(status.in_flight_requests, 1, "{mode}");
        assert_eq!(status.channels[0].in_flight_requests, 1, "{mode}");
        assert_eq!(status.channels[1].in_flight_requests, 0, "{mode}");
        assert_eq!(status.available_channels, 2);
        assert!(!status.degraded);
        if mode == "cancel" {
            config.routes[0].local_token = "revoked-local-token".into();
            proxy.update_config(config).unwrap();
        } else {
            release.send(()).unwrap();
        }
        let (code, body) = request.await.unwrap();
        assert_eq!(
            code,
            if mode == "cancel" {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::OK
            },
            "{mode}: {body}"
        );
        tokio::time::timeout(Duration::from_secs(1), async {
            while proxy.status().in_flight_requests > 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(proxy.status().channels[0].in_flight_requests, 0);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), fallback.accept())
                .await
                .is_err(),
            "{mode}: backup must remain untouched"
        );
        server.abort();
    }
}

#[test]
fn available_channels_exclude_open_circuits_but_include_recovering_targets() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let retry = RetryPolicy {
        failure_threshold: 2,
        ..RetryPolicy::default()
    };
    let route = single_target_route("health", "http://127.0.0.1:1/v1".into(), retry.clone());
    let target_id = route.targets[0].config.id;
    let proxy = ProxyHandle::start(
        RuntimeConfig::from_routes("127.0.0.1:0", vec![route]),
        store,
    )
    .unwrap();
    mark_failure(&proxy.state, target_id, &retry);
    assert!(proxy.status().channels[0].degraded);
    assert_eq!(proxy.status().available_channels, 1);
    mark_failure(&proxy.state, target_id, &retry);
    assert_eq!(proxy.status().available_channels, 0);
    assert!(proxy.status().channels[0].cooldown_remaining_ms > 0);
    proxy
        .state
        .health
        .lock()
        .unwrap()
        .get_mut(&target_id)
        .unwrap()
        .open_until = Some(Instant::now() - Duration::from_secs(1));
    assert_eq!(proxy.status().available_channels, 1);
    assert!(proxy.status().channels[0].degraded);
    mark_success(&proxy.state, target_id);
    assert!(proxy.status().channels[0].degraded);
    mark_success(&proxy.state, target_id);
    assert!(!proxy.status().channels[0].degraded);
}
