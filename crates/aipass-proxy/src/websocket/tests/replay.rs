use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// Desktop port discovery may open TCP sockets or issue unauthenticated root
// GET/HEAD probes. Those are not upstream handshakes or submitted generations.
async fn no_upstream_request(
    listener: &TcpListener,
    window: Duration,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let deadline = tokio::time::Instant::now() + window;
    loop {
        let accepted = tokio::time::timeout_at(deadline, listener.accept()).await;
        let Ok(accepted) = accepted else {
            return Ok(());
        };
        let (mut stream, _) = accepted?;
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 1024];
        loop {
            match tokio::time::timeout(Duration::from_millis(20), stream.read(&mut buffer)).await {
                Ok(Ok(0)) | Err(_) => break,
                Ok(Err(error))
                    if bytes.is_empty() && error.kind() == std::io::ErrorKind::ConnectionReset =>
                {
                    break
                }
                Ok(Err(error)) => return Err(error.into()),
                Ok(Ok(count)) => {
                    bytes.extend_from_slice(&buffer[..count]);
                    if bytes.len() > 8192 {
                        return Err("unexpected upstream request".into());
                    }
                    if bytes.windows(4).any(|v| v == b"\r\n\r\n") {
                        break;
                    }
                }
            }
        }
        if bytes.is_empty() {
            continue;
        }
        let text = std::str::from_utf8(&bytes)?;
        let mut lines = text.lines();
        let root = matches!(
            lines.next(),
            Some("GET / HTTP/1.1" | "HEAD / HTTP/1.1" | "GET / HTTP/1.0" | "HEAD / HTTP/1.0")
        );
        let authenticated_or_upgrade = lines.any(|line| {
            line.split_once(':').is_some_and(|(name, _)| {
                matches!(
                    name.to_ascii_lowercase().as_str(),
                    "authorization" | "proxy-authorization" | "upgrade" | "sec-websocket-key"
                )
            })
        });
        if !root || authenticated_or_upgrade {
            return Err("unexpected upstream request after disconnect".into());
        }
    }
}

#[tokio::test]
async fn discovery_probes_are_ignored_but_real_upstream_handshakes_are_rejected() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    drop(
        TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap(),
    );
    for method in ["GET", "HEAD"] {
        let mut stream = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        stream
            .write_all(format!("{method} / HTTP/1.1\r\nHost: fixture\r\n\r\n").as_bytes())
            .await
            .unwrap();
    }
    no_upstream_request(&listener, Duration::from_millis(100))
        .await
        .unwrap();
    for request in [
        "GET /v1/responses HTTP/1.1\r\nHost: fixture\r\n\r\n",
        "GET / HTTP/1.1\r\nAuthorization: Bearer fixture\r\n\r\n",
        "GET / HTTP/1.1\r\nUpgrade: websocket\r\n\r\n",
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut stream = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        assert!(no_upstream_request(&listener, Duration::from_millis(100))
            .await
            .is_err());
    }
}

#[tokio::test]
async fn websocket_does_not_replay_after_upstream_disconnect() {
    let primary = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fallback = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut route = test_route(format!("http://{}/v1", primary.local_addr().unwrap()));
    route.config.retry.silent_retry = true;
    route.config.retry.hold_on_failure = true;
    route.targets.push(
        test_route(format!("http://{}/v1", fallback.local_addr().unwrap())).targets[0].clone(),
    );
    let server = tokio::spawn(async move {
        let (stream, _) = primary.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        assert!(receive(&mut ws).await.is_text());
        ws.send(event(
            serde_json::json!({"type":"response.created","response":{"id":"partial"}}),
        ))
        .await
        .unwrap();
        ws.send(event(serde_json::json!({"type":"response.output_text.delta","item_id":"item","delta":"partial"}))).await.unwrap();
        // Drop without a Close or response.completed after delivering output.
    });
    let (handle, store, _dir) = start_proxy(vec![route], direct());
    let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    ws.send(event(
        serde_json::json!({"type":"response.create","model":"test","input":"hello"}),
    ))
    .await
    .unwrap();
    for _ in 0..2 {
        assert!(receive(&mut ws).await.is_text());
    }
    let end = tokio::time::timeout(Duration::from_secs(1), ws.next())
        .await
        .unwrap();
    assert!(!matches!(end, Some(Ok(Message::Text(_)))));
    server.await.unwrap();
    no_upstream_request(&fallback, Duration::from_millis(100))
        .await
        .unwrap();
    let summary = store.summary(|_| 0).unwrap();
    assert_eq!(summary.request_count, 1);
    assert_eq!(summary.completed_attempts, 1);
    assert_eq!(summary.successful_attempts, 0);
    assert!(summary.average_first_token_ms.is_some());
    assert_eq!(handle.status().failures, 1);
}
