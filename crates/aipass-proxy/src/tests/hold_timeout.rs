use super::*;

#[test]
fn hold_on_failure_returns_502_after_max_duration() {
    let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let upstream_addr = upstream.local_addr().unwrap();
    let request_count = Arc::new(AtomicU64::new(0));
    let counter = request_count.clone();
    std::thread::spawn(move || {
        // Keep accepted sockets open until the process ends so an abrupt
        // close cannot race the proxy's read into a transport error.
        let mut streams = Vec::new();
        for _ in 0..20 {
            let Ok((mut stream, _)) = upstream.accept() else {
                return;
            };
            let _ = read_http_request(&mut stream);
            counter.fetch_add(1, Ordering::SeqCst);
            let body = r#"{"error":{"message":"upstream down"}}"#;
            if write!(
                stream,
                "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .is_err()
            {
                continue;
            }
            streams.push(stream);
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

    let started = Instant::now();
    let response = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
        .post(format!("http://{bind_addr}/v1/responses"))
        .bearer_auth(token)
        .json(&serde_json::json!({"model":"hold-test","input":[]}))
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert!(started.elapsed() >= Duration::from_millis(300));
    // The hold budget is a deadline, not a minimum attempt count. A slow
    // runner may exhaust it after one confirmed failure. Recovery/retry
    // behavior is covered by hold_on_failure_retries_with_backoff_until_upstream_recovers.
    assert!(request_count.load(Ordering::SeqCst) >= 1);
}
