use super::*;
#[test]
fn native_catalog_runs_without_node_or_any_path_executable() {
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "community::tests::native_catalog_matches_rust_allowlist_and_has_no_credentials",
            "--nocapture",
        ])
        .env("PATH", "/aipass-no-executables")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("1 passed"));
}

#[test]
fn provider_token_lifetimes_keep_millisecond_and_second_contracts_and_bound_overflow() {
    let before = now();
    let job = expires_after(&json!(60000), 86400000, 1);
    let device = expires_after(&json!(60), 86400, 1000);
    let after = now();
    for expiry in [job, device] {
        assert!((before + 60000..=after + 60000).contains(&expiry));
    }
    assert!(
        (before + 86400000..=now() + 86400000).contains(&expires_after(&json!(0), 86400, 1000))
    );
    assert!(
        (before + 30 * 86400000..=now() + 30 * 86400000).contains(&expires_after(
            &json!(u64::MAX),
            3600,
            1000
        ))
    );
}
pub(super) fn context(client: Client) -> (Context, mpsc::Sender<Value>, mpsc::Receiver<Value>) {
    let (tx, input) = mpsc::channel(8);
    let (output, rx) = mpsc::channel(8);
    let (_, cancellation) = watch::channel(false);
    let (refreshing, _) = watch::channel(false);
    (
        Context {
            client,
            outbound: None,
            input,
            output,
            provider: "factory".into(),
            auth: json!({"type":"oauth","access":"test-access","accountId":"alice"}),
            models: json!({}),
            session: "test-session".into(),
            native_token: None,
            native_workspace: String::new(),
            sequence: 0,
            cancellation,
            refreshing,
        },
        tx,
        rx,
    )
}

#[tokio::test]
async fn native_workbuddy_rereads_the_explicit_source_and_preserves_it_through_ack() {
    let temp = tempfile::tempdir().unwrap();
    let write = |uid: &str, token: &str| {
        std::fs::write(
            temp.path().join("workbuddy-desktop.info"),
            json!({"auth":{"accessToken":token},"account":{"uid":uid}}).to_string(),
        )
        .unwrap()
    };
    write("alice", "first");
    let source = aipass_agent_protocol::SubscriptionImportSource {
        provider: "workbuddy".into(),
        root: temp.path().to_owned(),
        selector: String::new(),
    };
    let auth = native_import::read(&source, &Default::default())
        .await
        .unwrap();
    let (mut c, input, mut output) = context(Client::new());
    c.provider = "workbuddy".into();
    c.auth = serde_json::from_str(auth.expose()).unwrap();
    write("alice", "rotated");
    let peer = tokio::spawn(async move {
        let frame = output.recv().await.unwrap();
        assert_eq!(frame["value"]["access"], "rotated");
        assert_eq!(
            frame["value"]["nativeSource"]["root"],
            temp.path().to_str().unwrap()
        );
        assert_eq!(frame["value"]["nativeDevice"], "local-test-device");
        input
            .send(json!({"type":"ack","id":frame["id"],"ok":true}))
            .await
            .unwrap();
        temp
    });
    workbuddy::fresh(&mut c).await.unwrap();
    let temp = peer.await.unwrap();
    assert_eq!(c.auth["access"], "rotated");
    std::fs::write(
        temp.path().join("workbuddy-desktop.info"),
        json!({"auth":{"accessToken":"foreign"},"account":{"uid":"bob"}}).to_string(),
    )
    .unwrap();
    assert!(workbuddy::fresh(&mut c)
        .await
        .unwrap_err()
        .contains("ownership changed"));
    assert_eq!(c.auth["access"], "rotated");
}

#[tokio::test]
async fn canceled_request_keeps_an_inflight_rotation_until_its_durable_ack() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (arrived, received) = tokio::sync::oneshot::channel();
    let (reply, response_ready) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        assert!(socket.read(&mut request).await.unwrap() > 0);
        arrived.send(()).unwrap();
        response_ready.await.unwrap();
        let body = br#"{"access":"rotated-access","refresh":"rotated-refresh","accountId":"alice","type":"oauth"}"#;
        socket
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        socket.write_all(body).await.unwrap();
    });
    let (mut c, input, mut output) = context(Client::builder().no_proxy().build().unwrap());
    let (cancel, canceled) = watch::channel(false);
    let (refreshing, phase) = watch::channel(false);
    c.cancellation = canceled.clone();
    c.refreshing = refreshing;
    let (saved, applied) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(run_cancellable(
        async move {
            let _guard = c.refresh_guard()?;
            let next = json_request(c.client.get(format!("http://{address}/refresh"))).await?;
            let result = c.save(next).await;
            saved.send(c.auth.clone()).unwrap();
            result
        },
        canceled,
        phase,
    ));
    received.await.unwrap();
    cancel.send_replace(true);
    tokio::task::yield_now().await;
    assert!(!task.is_finished());
    reply.send(()).unwrap();
    let frame = tokio::time::timeout(Duration::from_secs(2), output.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(frame["type"], "auth");
    assert_eq!(frame["value"]["refresh"], "rotated-refresh");
    assert!(!task.is_finished());
    input
        .send(json!({"type":"ack","id":frame["id"],"ok":true}))
        .await
        .unwrap();
    assert_eq!(applied.await.unwrap()["refresh"], "rotated-refresh");
    assert!(task.await.unwrap().is_err()); // generation stays canceled
    server.await.unwrap();
}

#[tokio::test]
async fn cancellation_stops_work_outside_rotation_but_allows_the_credential_ack() {
    let (cancel, cancellation) = watch::channel(false);
    let (refreshing, phase) = watch::channel(false);
    let (input, mut receiver) = mpsc::channel(8);
    let operation = Operation {
        input,
        cancel,
        canceled: AtomicBool::new(false),
    };
    operation.kill();
    assert!(operation.send(&json!({"type":"request"})).is_err());
    operation
        .send(&json!({"type":"ack","id":1,"ok":true}))
        .unwrap();
    assert_eq!(receiver.recv().await.unwrap()["type"], "ack");
    let outcome = tokio::time::timeout(
        Duration::from_secs(1),
        run_cancellable(std::future::pending(), cancellation, phase),
    )
    .await
    .unwrap();
    assert!(outcome.is_err());
    drop(refreshing);
}
#[tokio::test]
async fn rejected_credential_ack_never_activates_rotated_tokens() {
    let (mut c, input, mut output) = context(Client::new());
    let peer = tokio::spawn(async move {
        let frame = output.recv().await.unwrap();
        assert_eq!(frame["type"], "auth");
        input
            .send(json!({"type":"ack","id":frame["id"],"ok":false}))
            .await
            .unwrap();
    });
    assert!(c.save(json!({"type":"oauth","accountId":"alice","access":"new-access","refresh":"new-refresh"})).await.is_err());
    assert_eq!(c.auth["access"], "test-access");
    peer.await.unwrap();
}
#[tokio::test]
async fn data_requires_matching_ack_before_the_next_chunk() {
    let (mut c, input, mut output) = context(Client::new());
    let task = tokio::spawn(async move { c.data(&vec![1; 96 * 1024]).await });
    let first = output.recv().await.unwrap();
    assert_eq!(
        STANDARD.decode(s(&first, "value")).unwrap().len(),
        48 * 1024
    );
    tokio::task::yield_now().await;
    assert!(output.try_recv().is_err());
    input
        .send(json!({"type":"ack","id":first["id"],"ok":true}))
        .await
        .unwrap();
    let second = output.recv().await.unwrap();
    assert_ne!(first["id"], second["id"]);
    input
        .send(json!({"type":"ack","id":first["id"],"ok":true}))
        .await
        .unwrap();
    assert!(task.await.unwrap().is_err());
}
#[test]
fn owner_checks_include_non_primary_and_numeric_profile_fields() {
    let owner =
        json!({"accountId":"alice","uid":7,"profileArn":"arn:first","metadata":{"userId":"id"}});
    let mut next = owner.clone();
    next["access"] = json!("new");
    super::super::community::ensure_owner(&owner, &next).unwrap();
    for path in ["/uid", "/profileArn", "/metadata/userId"] {
        let mut next = owner.clone();
        *next.pointer_mut(path).unwrap() = Value::Null;
        assert!(super::super::community::ensure_owner(&owner, &next).is_err());
    }
}

#[test]
fn token_owner_is_checked_independently_of_cached_metadata() {
    let token = |sub: &str| {
        format!(
            "header.{}.signature",
            URL_SAFE_NO_PAD.encode(json!({"sub":sub}).to_string())
        )
    };
    let before = json!({"access":token("alice")});
    let mut enriched = before.clone();
    enriched["accountId"] = json!("alice@example.test");
    enriched["profileArn"] = json!("arn:alice");
    super::super::community::ensure_owner(&before, &enriched).unwrap();
    let mut wrong = enriched.clone();
    wrong["access"] = json!(token("bob"));
    assert!(super::super::community::ensure_owner(&enriched, &wrong).is_err());
    assert!(super::super::community::ensure_owner(&before, &json!({"access":"unbound"})).is_err());
}
#[tokio::test]
async fn early_connect_quota_error_is_http_429_without_success_headers() {
    use aipass_proxy_conversion::providers::{connect::frame, devin::DevinStream};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        let n = socket.read(&mut request).await.unwrap();
        assert!(n > 0);
        let data = frame(
            2,
            br#"{"error":{"code":"resource_exhausted","message":"quota reached"}}"#,
        );
        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/connect+proto\r\nConnection: close\r\n\r\n",data.len()).as_bytes()).await.unwrap();
        socket.write_all(&data).await.unwrap();
    });
    let (mut c, input, mut output) = context(Client::builder().no_proxy().build().unwrap());
    let response = c
        .client
        .get(format!("http://{address}"))
        .send()
        .await
        .unwrap();
    let operation = tokio::spawn(async move {
        c.converted(response, &mut DevinStream::new("m", "id"))
            .await
    });
    let h = output.recv().await.unwrap();
    assert_eq!(h["type"], "headers");
    assert_eq!(h["value"]["status"], 429);
    let data = output.recv().await.unwrap();
    assert_eq!(data["type"], "data");
    input
        .send(json!({"type":"ack","id":data["id"],"ok":true}))
        .await
        .unwrap();
    assert_eq!(output.recv().await.unwrap()["type"], "end");
    operation.await.unwrap().unwrap();
    server.await.unwrap();
}
#[tokio::test]
async fn explicit_outbound_http_proxy_is_used_for_native_requests() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut b = vec![0; 4096];
        let n = socket.read(&mut b).await.unwrap();
        let request = String::from_utf8_lossy(&b[..n]);
        assert!(request.starts_with("GET http://provider.invalid/models HTTP/1.1"));
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
            .await
            .unwrap();
    });
    let proxy: UpstreamProxyConfig =
        serde_json::from_value(json!({"mode":"custom","customUrl":format!("http://{address}")}))
            .unwrap();
    let client = aipass_proxy::apply_upstream_proxy(Client::builder(), &proxy)
        .unwrap()
        .build()
        .unwrap();
    json_request(client.get("http://provider.invalid/models"))
        .await
        .unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn submitted_generation_failures_do_not_become_retryable_http_rejections() {
    use aipass_proxy_conversion::providers::{
        connect::{frame, string},
        devin::DevinStream,
    };
    let bodies = [
        [
            frame(0, &string(3, "already generated")),
            frame(2, br#"{"error":{"code":"resource_exhausted"}}"#),
        ]
        .concat(),
        frame(0, &[0]), // malformed protobuf after HTTP 200
        vec![0, 0],     // truncated Connect prefix
        vec![],         // successful HTTP headers with no protocol completion
    ];
    for body in bodies {
        // A single body chunk makes this independent of socket packetization.
        let response = Response::from(hyper::Response::builder().status(200).body(body).unwrap());
        let (mut c, input, mut output) = context(Client::new());
        let operation = tokio::spawn(async move {
            c.converted(response, &mut DevinStream::new("m", "id"))
                .await
        });
        let mut headers = vec![];
        while let Some(event) = output.recv().await {
            if event["type"] == "headers" {
                headers.push(event["value"]["status"].clone());
            }
            if event.get("id").is_some() {
                input
                    .send(json!({"type":"ack","id":event["id"],"ok":true}))
                    .await
                    .unwrap();
            }
        }
        assert!(operation.await.unwrap().is_err());
        assert!(
            headers.is_empty(),
            "must not return a retryable HTTP status: {headers:?}"
        );
    }
}
