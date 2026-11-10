//! Local service discovery must not count as a leaked search/API request.
use super::*;

pub(super) async fn assert_no_api_request(listener: &TcpListener) {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(50);
    loop {
        let Ok(accepted) = tokio::time::timeout_at(deadline, listener.accept()).await else {
            return;
        };
        let (mut stream, _) = accepted.unwrap();
        let mut bytes = Vec::new();
        loop {
            let mut part = [0; 1024];
            let n = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut part))
                .await
                .unwrap()
                .unwrap();
            if n == 0 {
                assert!(bytes.is_empty(), "incomplete unexpected request");
                break;
            }
            bytes.extend_from_slice(&part[..n]);
            assert!(bytes.len() <= 16 * 1024, "oversized unexpected request");
            if bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        if bytes.is_empty() {
            continue;
        }
        let headers = String::from_utf8_lossy(&bytes);
        let first = headers.lines().next().unwrap_or("");
        let safe = first.starts_with("GET / HTTP/")
            && !headers.lines().any(|line| {
                line.split_once(':').is_some_and(|(name, value)| {
                    name.eq_ignore_ascii_case("authorization")
                        || name.eq_ignore_ascii_case("x-api-key")
                        || name.eq_ignore_ascii_case("transfer-encoding")
                        || name.eq_ignore_ascii_case("content-length") && value.trim() != "0"
                })
            });
        assert!(safe, "unexpected API request: {first}");
        stream
            .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn no_request_guard_ignores_only_discovery() {
    for (request, allowed) in [
        ("GET / HTTP/1.1\r\nHost: localhost\r\n\r\n", true),
        ("POST /search HTTP/1.1\r\nHost: localhost\r\n\r\n", false),
        ("GET /steal HTTP/1.1\r\nHost: localhost\r\n\r\n", false),
        (
            "GET / HTTP/1.1\r\nAuthorization: Bearer synthetic\r\n\r\n",
            false,
        ),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        client.write_all(request.as_bytes()).await.unwrap();
        let guard = tokio::spawn(async move {
            assert_no_api_request(&listener).await;
        });
        let mut reply = Vec::new();
        tokio::time::timeout(Duration::from_secs(2), client.read_to_end(&mut reply))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(guard.await.is_ok(), allowed);
        if allowed {
            assert!(reply.starts_with(b"HTTP/1.1 204"));
        }
    }
}
