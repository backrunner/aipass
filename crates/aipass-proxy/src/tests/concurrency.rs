use super::*;

// Keep an upstream body open until explicitly released, so admission is tested
// against real streaming requests rather than a preloaded counter.
async fn streaming_upstream() -> (
    SocketAddr,
    tokio::sync::mpsc::UnboundedReceiver<oneshot::Sender<()>>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (arrivals, received) = tokio::sync::mpsc::unbounded_channel();
    let server = tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            let arrivals = arrivals.clone();
            tokio::spawn(async move {
                let service = service_fn(move |request: Request<Incoming>| {
                    let arrivals = arrivals.clone();
                    async move {
                        request.into_body().collect().await.unwrap();
                        let (release, wait) = oneshot::channel();
                        let _ = arrivals.send(release);
                        let body = stream::once(async {
                            Ok::<_, BoxError>(Frame::data(Bytes::from_static(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n")))
                        }).chain(stream::once(async move {
                            let _ = wait.await;
                            Ok::<_, BoxError>(Frame::data(Bytes::from_static(b"data: {\"type\":\"response.completed\",\"response\":{\"id\":\"done\",\"status\":\"completed\",\"output\":[]}}\n\n")))
                        }));
                        Ok::<_, Infallible>(
                            Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(BodyExt::boxed_unsync(StreamBody::new(body)))
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
    (address, received, server)
}

async fn no_occupancy(proxy: &ProxyHandle) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !proxy.state.provider_activity.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn provider_concurrency_is_shared_across_routes_and_releases_on_stream_end_or_cancel() {
    let (primary, mut primary_calls, primary_server) = streaming_upstream().await;
    let (backup, mut backup_calls, backup_server) = streaming_upstream().await;
    let mut first = fallback_route(
        "first",
        &[primary, backup],
        RetryPolicy {
            max_attempts: 1,
            ..RetryPolicy::default()
        },
    );
    for target in &mut first.targets {
        target.max_concurrent_requests = Some(1);
    }
    let mut second = first.clone();
    second.config.id = Uuid::new_v4();
    second.local_token = "second".into();
    for target in &mut second.targets {
        target.config.id = Uuid::new_v4();
    }
    // A second key/channel for the same provider cannot bypass its limit, and
    // two skipped channels must not exhaust a max_attempts=1 fallback budget.
    let mut duplicate = second.targets[0].clone();
    duplicate.config.id = Uuid::new_v4();
    duplicate.config.secret_id = "second-key".into();
    second.targets.insert(1, duplicate);
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let mut config = RuntimeConfig::from_routes(available_addr().to_string(), vec![first, second]);
    config.upstream_proxy.mode = UpstreamProxyMode::Direct;
    let proxy = ProxyHandle::start(config, store).unwrap();
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let send = |token: &'static str| {
        client
            .post(format!("http://{}/v1/responses", proxy.bind_addr))
            .bearer_auth(token)
            .json(&serde_json::json!({"stream":true}))
            .send()
    };
    let a = send("first").await.unwrap();
    assert_eq!(a.status(), StatusCode::OK);
    let release_a = primary_calls.recv().await.unwrap();
    let b = send("second").await.unwrap();
    assert_eq!(b.status(), StatusCode::OK);
    let release_b = backup_calls.recv().await.unwrap();
    assert!(primary_calls.try_recv().is_err());
    let full = send("second").await.unwrap();
    assert_eq!(full.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(full.text().await.unwrap().contains("concurrency limit"));
    let models = client
        .get(format!("http://{}/v1/models", proxy.bind_addr))
        .bearer_auth("second")
        .send()
        .await
        .unwrap();
    assert_eq!(models.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(primary_calls.try_recv().is_err());
    assert!(backup_calls.try_recv().is_err());
    assert!(proxy.status().degraded_target_ids.is_empty());
    assert!(proxy
        .state
        .health
        .lock()
        .unwrap()
        .values()
        .all(|h| h.consecutive_failures == 0));
    release_a.send(()).unwrap();
    a.text().await.unwrap();
    drop(b); // Downstream cancellation must release the backup slot too.
    drop(release_b);
    no_occupancy(&proxy).await;
    let again = send("first").await.unwrap();
    assert_eq!(again.status(), StatusCode::OK);
    primary_calls.recv().await.unwrap().send(()).unwrap();
    again.text().await.unwrap();
    no_occupancy(&proxy).await;
    primary_server.abort();
    backup_server.abort();
}

#[test]
fn provider_concurrency_admission_is_atomic_and_live_limits_preserve_occupancy() {
    let route = single_target_route(
        "limits",
        "http://127.0.0.1:1".into(),
        RetryPolicy::default(),
    );
    let target = route.targets[0].clone();
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    let mut config = RuntimeConfig::from_routes(available_addr().to_string(), vec![route]);
    let proxy = ProxyHandle::start(config.clone(), store).unwrap();
    let one = ProviderPermit::acquire(&proxy.state, &target).unwrap();
    let two = ProviderPermit::acquire(&proxy.state, &target).unwrap();
    config.routes[0].targets[0].max_concurrent_requests = Some(1);
    proxy.update_config(config.clone()).unwrap();
    assert!(ProviderPermit::acquire(&proxy.state, &target).is_none());
    drop(one);
    assert!(ProviderPermit::acquire(&proxy.state, &target).is_none());
    drop(two);
    let barrier = Arc::new(std::sync::Barrier::new(16));
    let wins = Arc::new(AtomicU64::new(0));
    std::thread::scope(|scope| {
        for _ in 0..16 {
            scope.spawn(|| {
                barrier.wait();
                let permit = ProviderPermit::acquire(&proxy.state, &target);
                if permit.is_some() {
                    wins.fetch_add(1, Ordering::SeqCst);
                }
                barrier.wait(); // All contenders must try before the winner releases.
                drop(permit);
            });
        }
    });
    assert_eq!(wins.load(Ordering::SeqCst), 1);
    config.routes[0].targets[0].max_concurrent_requests = Some(0);
    proxy.update_config(config).unwrap();
    let permits: Vec<_> = (0..20)
        .map(|_| ProviderPermit::acquire(&proxy.state, &target).unwrap())
        .collect();
    drop(permits);
    assert!(proxy.state.provider_activity.lock().unwrap().is_empty());
}
