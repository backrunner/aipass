use super::*;

fn route() -> ResolvedRoute {
    fallback_route(
        "stability",
        &[
            "127.0.0.1:1".parse().unwrap(),
            "127.0.0.1:2".parse().unwrap(),
        ],
        RetryPolicy::default(),
    )
}

fn expire_circuit(state: &RuntimeState, id: Uuid) {
    state
        .health
        .lock()
        .unwrap()
        .get_mut(&id)
        .unwrap()
        .open_until = Some(Instant::now() - Duration::from_secs(1));
}

#[test]
fn fallback_sessions_keep_stable_provider_after_primary_recovers_or_finishes_late() {
    let route = route();
    let proxy = start_proxy(available_addr(), route.clone());
    let primary = route.targets[0].config.id;
    let stable = route.targets[1].config.id;
    let old_request = Instant::now();
    remember_affinity_target(&proxy.state, route.config.id, Some("session"), primary);
    mark_failure(&proxy.state, primary, &route.config.retry);
    complete_target_success(
        &proxy.state,
        route.config.id,
        Some("session"),
        stable,
        Instant::now(),
        None,
    );
    complete_target_success(
        &proxy.state,
        route.config.id,
        Some("session"),
        primary,
        old_request,
        None,
    );
    assert_eq!(
        proxy.state.health.lock().unwrap()[&primary].consecutive_successes,
        0
    );
    mark_success(&proxy.state, primary);
    mark_success(&proxy.state, primary);
    // Even once the primary is fully stable and again wins new sessions,
    // existing sessions must not donate their prompt cache to recovery probes.
    proxy
        .state
        .health
        .lock()
        .unwrap()
        .get_mut(&primary)
        .unwrap()
        .last_failure_at = Some(Instant::now() - Duration::from_secs(601));
    for _ in 0..20 {
        complete_target_success(
            &proxy.state,
            route.config.id,
            Some("session"),
            primary,
            Instant::now(),
            None,
        );
        let selected = select_route_targets_with_affinity(&proxy.state, &route, Some("session"));
        assert_eq!(selected[0].config.id, stable);
        assert_eq!(
            select_route_targets(&proxy.state, &route)[0].config.id,
            primary
        );
    }
}

#[test]
fn degraded_targets_follow_stable_targets_and_recovery_is_exclusive() {
    let route = route();
    let proxy = start_proxy(available_addr(), route.clone());
    let primary = route.targets[0].config.id;
    let stable = route.targets[1].config.id;
    mark_failure(&proxy.state, primary, &route.config.retry);
    assert_eq!(
        select_route_targets(&proxy.state, &route)[0].config.id,
        stable
    );
    mark_failure(&proxy.state, primary, &route.config.retry);
    mark_failure(&proxy.state, primary, &route.config.retry);
    expire_circuit(&proxy.state, primary);
    let permit = RecoveryPermit::acquire(&proxy.state, primary).unwrap();
    assert!(RecoveryPermit::acquire(&proxy.state, primary).is_none());
    assert!(select_route_targets(&proxy.state, &route)
        .iter()
        .all(|t| t.config.id != primary));
    // Cancellation frees the slot, but is not evidence of recovery.
    drop(permit);
    assert!(RecoveryPermit::acquire(&proxy.state, primary).is_some());
    assert!(
        proxy
            .status()
            .channels
            .iter()
            .find(|c| c.target_id == primary)
            .unwrap()
            .degraded
    );
    mark_success(&proxy.state, primary);
    assert!(proxy.state.health.lock().unwrap()[&primary].degraded());
    mark_success(&proxy.state, primary);
    assert!(!proxy.state.health.lock().unwrap()[&primary].degraded());
    // Recovery success retains flapping history for new requests too.
    assert_eq!(
        select_route_targets(&proxy.state, &route)[0].config.id,
        stable
    );
}

#[test]
fn repeated_recovery_failures_back_off_and_late_completions_cannot_heal() {
    let mut health = TargetHealth::default();
    let retry = RetryPolicy::default();
    let mut now = Instant::now();
    for _ in 0..2 {
        health.fail(&retry, now);
        assert!(health.open_until.is_none());
    }
    for seconds in [30, 60, 120, 240, 480, 900, 900] {
        health.fail(&retry, now);
        let until = health.open_until.unwrap();
        assert_eq!(until.duration_since(now), Duration::from_secs(seconds));
        // A sibling failure already in flight does not multiply this ban.
        health.fail(&retry, now);
        assert_eq!(health.open_until, Some(until));
        now = until + Duration::from_millis(1);
        assert!(!health.circuit_open(now));
        assert!(health.recovering);
    }
    // Recovery needs new requests, not old results arriving after cooldown.
    assert!(!health.succeed(Instant::now(), now));
    assert!(health.succeed(now, now));
    assert!(health.degraded());
    assert!(health.succeed(now, now));
    assert!(!health.degraded());
    now += Duration::from_secs(601);
    for _ in 0..3 {
        health.fail(&retry, now);
    }
    assert_eq!(
        health.open_until.unwrap().duration_since(now),
        Duration::from_secs(30)
    );
}

#[test]
fn metadata_reload_preserves_affinity_and_blacklist_but_credentials_invalidate_them() {
    let mut route = route();
    let proxy = start_proxy(available_addr(), route.clone());
    let primary = route.targets[0].config.id;
    let stable = route.targets[1].config.id;
    for _ in 0..3 {
        mark_failure(&proxy.state, primary, &route.config.retry);
    }
    remember_affinity_target(&proxy.state, route.config.id, Some("session"), stable);
    route.targets.reverse();
    route.targets[0].config.label = "renamed".into();
    route.targets[0].config.priority = 7;
    let mut config = proxy.state.config.read().unwrap().clone();
    config.routes = vec![route.clone()];
    proxy.update_config(config.clone()).unwrap();
    assert!(circuit_open(&proxy.state, primary));
    assert_eq!(
        affinity_target(
            &proxy.state,
            route.config.id,
            Some("session"),
            &route.targets
        ),
        Some(stable)
    );
    for target in &mut config.routes[0].targets {
        target.api_key = "replacement".into();
    }
    proxy.update_config(config).unwrap();
    assert!(proxy.state.health.lock().unwrap().is_empty());
    assert!(proxy.state.session_affinity.lock().unwrap().is_empty());
}

async fn mock_provider(
    fail_generation: bool,
) -> (SocketAddr, Arc<AtomicU64>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let count = Arc::new(AtomicU64::new(0));
    let seen = count.clone();
    let server = tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            let seen = seen.clone();
            tokio::spawn(async move {
                let service = service_fn(move |request: Request<Incoming>| {
                    let seen = seen.clone();
                    async move {
                        let models = request.method() == http::Method::GET;
                        let bytes = request.into_body().collect().await.unwrap().to_bytes();
                        let streaming = serde_json::from_slice::<serde_json::Value>(&bytes)
                            .ok()
                            .is_some_and(|body| body["stream"] == true);
                        let number = if models {
                            0
                        } else {
                            seen.fetch_add(1, Ordering::Relaxed) + 1
                        };
                        let (status, mut payload) = if models {
                            (StatusCode::OK, r#"{"data":[{"id":"test"}]}"#.to_owned())
                        } else if fail_generation {
                            (
                                StatusCode::SERVICE_UNAVAILABLE,
                                r#"{"error":{"message":"unavailable"}}"#.to_owned(),
                            )
                        } else {
                            (StatusCode::OK, serde_json::json!({"id":format!("resp_{number}"),"status":"completed","output":[]}).to_string())
                        };
                        let sse = streaming && status.is_success();
                        if sse {
                            payload = format!("data: {{\"type\":\"response.completed\",\"response\":{payload}}}\n\n");
                        }
                        Ok::<_, Infallible>(
                            Response::builder()
                                .status(status)
                                .header(
                                    header::CONTENT_TYPE,
                                    if sse {
                                        "text/event-stream"
                                    } else {
                                        "application/json"
                                    },
                                )
                                .body(Full::new(Bytes::from(payload)))
                                .unwrap(),
                        )
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(socket), service)
                    .await;
            });
        }
    });
    (addr, count, server)
}

#[tokio::test]
async fn http_fallback_stays_stable_across_turns_and_model_discovery_cannot_heal() {
    let (primary, primary_calls, primary_server) = mock_provider(true).await;
    let (stable, stable_calls, stable_server) = mock_provider(false).await;
    let route = fallback_route("stability", &[primary, stable], RetryPolicy::default());
    let id = route.targets[0].config.id;
    let temp = tempfile::tempdir().unwrap();
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let mut config = RuntimeConfig::from_routes(available_addr().to_string(), vec![route.clone()]);
    config.upstream_proxy.mode = UpstreamProxyMode::Direct;
    let proxy = ProxyHandle::start(config, usage).unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    for _ in 0..8 {
        let response = client
            .post(format!("http://{}/v1/responses", proxy.bind_addr))
            .bearer_auth("stability")
            .json(&serde_json::json!({"prompt_cache_key":"conversation", "input":[]}))
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        response.bytes().await.unwrap();
    }
    assert_eq!(primary_calls.load(Ordering::Relaxed), 1);
    assert_eq!(stable_calls.load(Ordering::Relaxed), 8);
    // Force model discovery to the degraded primary, without resetting health.
    let mut config = proxy.state.config.read().unwrap().clone();
    config.routes[0].targets[1].config.enabled = false;
    proxy.update_config(config).unwrap();
    for _ in 0..3 {
        let response = client
            .get(format!("http://{}/v1/models", proxy.bind_addr))
            .bearer_auth("stability")
            .header("x-aipass-session-id", "models")
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        response.bytes().await.unwrap();
    }
    assert!(proxy.state.health.lock().unwrap()[&id].degraded());
    assert_eq!(
        affinity_target(
            &proxy.state,
            route.config.id,
            Some("models"),
            &route.targets
        ),
        None
    );
    primary_server.abort();
    stable_server.abort();
}

#[tokio::test]
async fn held_request_does_not_bypass_a_blacklist_before_its_budget_expires() {
    let (upstream, calls, server) = mock_provider(true).await;
    let route = fallback_route(
        "stability",
        &[upstream],
        RetryPolicy {
            failure_threshold: 1,
            hold_on_failure: true,
            hold_initial_delay_ms: 10,
            hold_max_delay_ms: 20,
            hold_max_duration_ms: 120,
            silent_retry: true,
            ..RetryPolicy::default()
        },
    );
    let id = route.targets[0].config.id;
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let mut config = RuntimeConfig::from_routes(available_addr().to_string(), vec![route]);
    config.upstream_proxy.mode = UpstreamProxyMode::Direct;
    let proxy = ProxyHandle::start(config, store).unwrap();
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://{}/v1/responses", proxy.bind_addr))
        .bearer_auth("stability")
        .json(&serde_json::json!({"input":[]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(circuit_open(&proxy.state, id));
    expire_circuit(&proxy.state, id);
    let permit = RecoveryPermit::acquire(&proxy.state, id).unwrap();
    assert!(RecoveryPermit::acquire(&proxy.state, id).is_none());
    drop(permit);
    server.abort();
}

#[tokio::test]
async fn previous_response_ids_keep_http_and_sse_conversations_on_their_origin() {
    for streaming in [false, true] {
        let (primary, primary_calls, primary_server) = mock_provider(true).await;
        let (stable, stable_calls, stable_server) = mock_provider(false).await;
        let route = fallback_route("stability", &[primary, stable], RetryPolicy::default());
        let id = route.targets[0].config.id;
        let temp = tempfile::tempdir().unwrap();
        let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
        let mut config = RuntimeConfig::from_routes(available_addr().to_string(), vec![route]);
        config.upstream_proxy.mode = UpstreamProxyMode::Direct;
        let proxy = ProxyHandle::start(config, usage).unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let mut previous = serde_json::Value::Null;
        for _ in 0..5 {
            let response = client.post(format!("http://{}/v1/responses", proxy.bind_addr)).bearer_auth("stability")
                .json(&serde_json::json!({"input":[],"stream":streaming,"previous_response_id":previous})).send().await.unwrap();
            assert!(response.status().is_success());
            let text = response.text().await.unwrap();
            let payload: serde_json::Value =
                serde_json::from_str(text.trim().trim_start_matches("data: ")).unwrap();
            previous = if streaming {
                payload["response"]["id"].clone()
            } else {
                payload["id"].clone()
            };
            assert!(previous.is_string());
            // Simulate a fully healthy, higher-priority provider between turns.
            proxy.state.health.lock().unwrap().remove(&id);
        }
        assert_eq!(primary_calls.load(Ordering::Relaxed), 1);
        assert_eq!(stable_calls.load(Ordering::Relaxed), 5);
        primary_server.abort();
        stable_server.abort();
    }
}
