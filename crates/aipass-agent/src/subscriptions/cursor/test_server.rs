//! The local discovery probe is HTTP/1; it must not consume the H2 fixture.
use std::io;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub(super) async fn accept(listener: &TcpListener) -> io::Result<TcpStream> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let (mut socket, _) = listener.accept().await?;
            let mut first = [0];
            // Only HTTP/1 GET probes are candidates for exclusion. Everything
            // else reaches the real H2 parser, including malformed prefaces.
            if socket.peek(&mut first).await? != 1 || first[0] != b'G' {
                return Ok(socket);
            }
            let mut headers = Vec::new();
            while !headers.ends_with(b"\r\n\r\n") {
                if headers.len() >= 8192 {
                    return Err(io::Error::other("oversized HTTP/1 fixture request"));
                }
                headers.push(socket.read_u8().await?);
            }
            if !discovery_probe(&headers) {
                return Err(io::Error::other("unexpected HTTP/1 request to H2 fixture"));
            }
            socket
                .write_all(
                    b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await?;
            socket.shutdown().await?;
        }
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "H2 fixture accept timed out"))?
}

fn discovery_probe(headers: &[u8]) -> bool {
    let Ok(headers) = std::str::from_utf8(headers) else {
        return false;
    };
    let mut lines = headers.split("\r\n");
    if !matches!(lines.next(), Some("GET / HTTP/1.1" | "GET / HTTP/1.0")) {
        return false;
    }
    for line in lines.filter(|line| !line.is_empty()) {
        let Some((name, value)) = line.split_once(':') else {
            return false;
        };
        if name.trim() != name {
            return false;
        }
        match name.to_ascii_lowercase().as_str() {
            "authorization" | "x-api-key" | "api-key" | "x-goog-api-key" | "transfer-encoding" => {
                return false
            }
            "content-length" if value.trim() != "0" => return false,
            _ => {}
        }
    }
    true
}

/// Inject the observed competing request before the actual HTTP/2 client.
pub(super) async fn probe(base: &str) {
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap()
        .get(base)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn only_unauthenticated_root_probes_are_excluded() {
    for request in [
        "GET /v1/messages HTTP/1.1\r\nHost: localhost\r\n\r\n",
        "GET / HTTP/1.1\r\nAuthorization: Bearer fixture\r\n\r\n",
        "GET / HTTP/1.1\r\nx-api-key: fixture\r\n\r\n",
        "GET / HTTP/1.1\r\nContent-Length: 1\r\n\r\n",
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        client.write_all(request.as_bytes()).await.unwrap();
        assert!(accept(&listener).await.is_err(), "accepted {request:?}");
    }
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut client = TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    client.write_all(b"PRI malformed preface").await.unwrap();
    let mut accepted = accept(&listener).await.unwrap();
    let mut received = [0; 21];
    accepted.read_exact(&mut received).await.unwrap();
    assert_eq!(&received, b"PRI malformed preface");
}
