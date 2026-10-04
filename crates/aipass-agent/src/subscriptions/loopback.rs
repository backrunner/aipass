use super::*;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
pub(super) struct Callback {
    pub method: String,
    pub path: String,
    pub fields: Value,
}

pub(super) struct Loopback {
    listener: TcpListener,
    pub port: u16,
}
impl Loopback {
    pub async fn bind(ports: &[u16]) -> Result<Self> {
        for port in ports {
            if let Ok(listener) = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, *port)).await {
                let port = listener
                    .local_addr()
                    .map_err(|_| "callback address unavailable")?
                    .port();
                return Ok(Self { listener, port });
            }
        }
        Err("no loopback sign-in port available".into())
    }
    pub async fn wait(
        &self,
        origins: &[&str],
        accept: impl Fn(&Callback) -> bool,
    ) -> Result<Callback> {
        loop {
            let (mut stream, _) = self
                .listener
                .accept()
                .await
                .map_err(|_| "sign-in callback stopped")?;
            let read = async {
                let mut bytes = zeroize::Zeroizing::new(Vec::new());
                let (header_end, length) = loop {
                    let mut chunk = [0u8; 4096];
                    let n = stream
                        .read(&mut chunk)
                        .await
                        .map_err(|_| "callback read failed")?;
                    if n == 0 {
                        return Err("incomplete callback");
                    }
                    bytes.extend_from_slice(&chunk[..n]);
                    if bytes.len() > 65536 {
                        return Err("callback exceeds limit");
                    }
                    if let Some(i) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let h = std::str::from_utf8(&bytes[..i])
                            .map_err(|_| "invalid callback headers")?;
                        if h.lines()
                            .any(|l| l.to_ascii_lowercase().starts_with("transfer-encoding:"))
                        {
                            return Err("unsupported callback transfer encoding");
                        }
                        let lengths: Vec<_> = h
                            .lines()
                            .filter_map(|l| l.split_once(':'))
                            .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                            .map(|(_, v)| v.trim())
                            .collect();
                        if lengths.len() > 1 {
                            return Err("ambiguous callback length");
                        }
                        let n = lengths
                            .first()
                            .map(|s| s.parse::<usize>())
                            .transpose()
                            .map_err(|_| "invalid callback length")?
                            .unwrap_or(0);
                        if n > 16384 {
                            return Err("callback exceeds limit");
                        }
                        break (i, n);
                    }
                };
                while bytes.len() < header_end + 4 + length {
                    let mut chunk = [0u8; 4096];
                    let n = stream
                        .read(&mut chunk)
                        .await
                        .map_err(|_| "callback read failed")?;
                    if n == 0 {
                        return Err("incomplete callback");
                    }
                    bytes.extend_from_slice(&chunk[..n]);
                    if bytes.len() > 65536 {
                        return Err("callback exceeds limit");
                    }
                }
                let h =
                    std::str::from_utf8(&bytes[..header_end]).map_err(|_| "invalid callback")?;
                let mut first = h.lines().next().unwrap_or("").split_whitespace();
                let method = first.next().unwrap_or("").to_owned();
                let target = first.next().unwrap_or("");
                let url = url::Url::parse(&format!("http://127.0.0.1{target}"))
                    .map_err(|_| "invalid callback URL")?;
                let header = |name: &str| {
                    h.lines()
                        .filter_map(|l| l.split_once(':'))
                        .find(|(k, _)| k.eq_ignore_ascii_case(name))
                        .map(|(_, v)| v.trim())
                        .unwrap_or("")
                };
                let origin = header("origin");
                if !origin.is_empty() && !origins.contains(&origin) {
                    return Err("unexpected callback origin");
                }
                let host = header("host");
                if host != format!("127.0.0.1:{}", self.port)
                    && host != format!("localhost:{}", self.port)
                {
                    return Err("unexpected callback host");
                }
                let mut fields = json!({});
                for (k, v) in url.query_pairs() {
                    fields[k.as_ref()] = json!(v);
                }
                let body = &bytes[header_end + 4..header_end + 4 + length];
                if method == "POST" {
                    if header("content-type").starts_with("application/json") {
                        let v: Value =
                            serde_json::from_slice(body).map_err(|_| "invalid callback JSON")?;
                        fields
                            .as_object_mut()
                            .unwrap()
                            .extend(v.as_object().ok_or("invalid callback JSON")?.clone());
                    } else {
                        for (k, v) in url::form_urlencoded::parse(body) {
                            fields[k.as_ref()] = json!(v);
                        }
                    }
                }
                Ok((
                    Callback {
                        method,
                        path: url.path().into(),
                        fields,
                    },
                    origin.to_owned(),
                ))
            };
            let result = tokio::time::timeout(Duration::from_secs(5), read).await;
            let mut accepted = None;
            let mut cors = String::new();
            let (code, body) = match result {
                Ok(Ok((cb, origin))) => {
                    if !origin.is_empty() {
                        cors=format!("Access-Control-Allow-Origin: {origin}\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nAccess-Control-Allow-Private-Network: true\r\n");
                    }
                    if cb.method == "OPTIONS" {
                        (204, "")
                    } else if accept(&cb) {
                        accepted = Some(cb);
                        (200,"{\"success\":true,\"message\":\"Sign-in received. You can return to AIPass.\"}")
                    } else {
                        (403, "This callback does not belong to the pending sign-in.")
                    }
                }
                _ => (400, "Invalid callback."),
            };
            let response=format!("HTTP/1.1 {code} Callback\r\nConnection: close\r\nCache-Control: no-store\r\nContent-Type: application/json; charset=utf-8\r\n{cors}Content-Length: {}\r\n\r\n{body}",body.len());
            let _ = stream.write_all(response.as_bytes()).await;
            if let Some(cb) = accepted {
                return Ok(cb);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn callback_rejects_foreign_origin_and_state_before_accepting_signin() {
        let callback = Loopback::bind(&[0]).await.unwrap();
        let port = callback.port;
        let waiter = tokio::spawn(async move {
            callback
                .wait(&["https://provider.example"], |r| {
                    r.path == "/callback" && r.method == "POST" && r.fields["state"] == "expected"
                })
                .await
        });
        let client = Client::builder().no_proxy().build().unwrap();
        let url = format!("http://127.0.0.1:{port}/callback");
        let foreign = client
            .post(&url)
            .header("Origin", "https://foreign.example")
            .json(&json!({"state":"expected","code":"private"}))
            .send()
            .await
            .unwrap();
        assert_eq!(foreign.status(), 400);
        let stale = client
            .post(&url)
            .header("Origin", "https://provider.example")
            .json(&json!({"state":"stale","code":"private"}))
            .send()
            .await
            .unwrap();
        assert_eq!(stale.status(), 403);
        let valid = client
            .post(&url)
            .header("Origin", "https://provider.example")
            .json(&json!({"state":"expected","code":"accepted"}))
            .send()
            .await
            .unwrap();
        assert_eq!(valid.status(), 200);
        assert_eq!(valid.json::<Value>().await.unwrap()["success"], true);
        assert_eq!(waiter.await.unwrap().unwrap().fields["code"], "accepted");
    }
}
