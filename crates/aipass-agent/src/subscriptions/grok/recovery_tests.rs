use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// Local service discovery is unrelated to the retry/replay assertion. Only
// empty connections and unauthenticated GET / probes are filtered; authenticated
// root requests and every API-path request remain observable.
async fn receive(listener: &tokio::net::TcpListener) -> (tokio::net::TcpStream, String) {
    loop {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        let header_end = loop {
            let mut chunk = [0; 4096];
            let n = socket.read(&mut chunk).await.unwrap();
            if n == 0 {
                break None;
            }
            bytes.extend_from_slice(&chunk[..n]);
            assert!(bytes.len() <= 16384, "oversized fixture request");
            if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                break Some(end + 4);
            }
        };
        let Some(header_end) = header_end else {
            continue;
        };
        let headers = String::from_utf8_lossy(&bytes[..header_end]).to_string();
        let length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap_or(0);
        assert!(length <= 4096);
        while bytes.len() < header_end + length {
            let mut chunk = [0; 4096];
            let n = socket.read(&mut chunk).await.unwrap();
            assert_ne!(n, 0, "incomplete fixture body");
            bytes.extend_from_slice(&chunk[..n]);
        }
        let authenticated = headers.lines().any(|line| {
            line.split_once(':')
                .is_some_and(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        });
        if headers.starts_with("GET / HTTP/") && !authenticated {
            let _ = socket
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await;
            continue;
        }
        return (socket, String::from_utf8(bytes).unwrap());
    }
}

fn account(root: &std::path::Path) -> (Context, mpsc::Sender<Value>, mpsc::Receiver<Value>) {
    std::fs::write(root.join("auth.json"),json!({"https://auth.x.ai":{"key":"new","email":"alice","expires_at":"2040-01-01T00:00:00Z"}}).to_string()).unwrap();
    let (mut c, tx, rx) =
        super::super::tests::context(Client::builder().no_proxy().build().unwrap());
    c.provider = "grok".into();
    c.auth = cli_accounts::reference("grok", root).unwrap();
    c.native_token = Some(aipass_agent_protocol::SensitiveString::new("old"));
    (c, tx, rx)
}

async fn server(statuses: Vec<u16>) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/fixture", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        let mut requests = vec![];
        for status in statuses {
            let (mut socket, request) =
                tokio::time::timeout(Duration::from_secs(3), receive(&listener))
                    .await
                    .unwrap();
            requests.push(request);
            let body = if status == 200 {
                r#"{"ok":true}"#
            } else {
                r#"{"error":"unauthorized"}"#
            };
            socket.write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        }
        if let Ok((_, request)) =
            tokio::time::timeout(Duration::from_millis(100), receive(&listener)).await
        {
            panic!("unexpected extra request: {:?}", request.lines().next());
        }
        requests
    });
    // Deterministically reproduce discovery before the intended request, so it
    // cannot consume a response even when no external scanner is running.
    let discovery = Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(url.strip_suffix("fixture").unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(discovery.status(), 404);
    (url, handle)
}

#[tokio::test]
async fn discovery_filter_retains_authenticated_root_and_generation_traffic() {
    for path in ["/", "/fixture"] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}{path}", listener.local_addr().unwrap());
        let handle = tokio::spawn(async move {
            let (mut socket, request) =
                tokio::time::timeout(Duration::from_secs(3), receive(&listener))
                    .await
                    .unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            request
        });
        let client = Client::builder().no_proxy().build().unwrap();
        let response = if path == "/" {
            client.get(url).bearer_auth("fixture").send().await.unwrap()
        } else {
            client.post(url).body("fixture").send().await.unwrap()
        };
        assert_eq!(response.status(), 200);
        let request = handle.await.unwrap();
        assert!(request.starts_with(if path == "/" {
            "GET / HTTP/"
        } else {
            "POST /fixture HTTP/"
        }));
    }
}

#[tokio::test]
async fn read_only_401_retries_once_and_other_failures_do_not_refresh() {
    for statuses in [
        vec![401, 200],
        vec![401, 401],
        vec![403],
        vec![429],
        vec![500],
    ] {
        let root = tempfile::tempdir().unwrap();
        let (mut c, _tx, _rx) = account(root.path());
        let (url, handle) = server(statuses.clone()).await;
        let result = get(&mut c, &url).await;
        assert_eq!(result.is_ok(), statuses.last() == Some(&200));
        let requests = handle.await.unwrap();
        assert!(requests[0].contains("Bearer old"));
        if requests.len() == 2 {
            assert!(requests[1].contains("Bearer new"));
        } else {
            assert_eq!(c.token().unwrap(), "old");
        }
    }
}

#[tokio::test]
async fn generation_401_recovers_future_access_without_replaying() {
    let root = tempfile::tempdir().unwrap();
    let (mut c, tx, mut rx) = account(root.path());
    let (url, handle) = server(vec![401]).await;
    let response = send(sign(&c, c.client.post(url)).unwrap().body("fixture"))
        .await
        .unwrap();
    let peer = tokio::spawn(async move {
        let head = rx.recv().await.unwrap();
        assert_eq!(head["value"]["status"], 401);
        let data = rx.recv().await.unwrap();
        assert_eq!(data["type"], "data");
        tx.send(json!({"type":"ack","id":data["id"],"ok":true}))
            .await
            .unwrap();
        assert_eq!(rx.recv().await.unwrap()["type"], "end");
    });
    c.pipe(response).await.unwrap();
    peer.await.unwrap();
    assert_eq!(handle.await.unwrap().len(), 1);
    assert_eq!(c.token().unwrap(), "new");
    assert!(c.feedback.revoked.is_none());
}
