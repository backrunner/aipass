use super::*;

#[test]
fn proxy_config_defaults_upstream_proxy_to_system_for_legacy_json() {
    let config: ProxyConfig = serde_json::from_str(
        r#"{"enabled":true,"bindAddr":"127.0.0.1:8787","routes":[],"pricing":[]}"#,
    )
    .expect("legacy config without upstreamProxy still deserializes");
    assert_eq!(config.upstream_proxy.mode, UpstreamProxyMode::System);
    assert_eq!(config.upstream_proxy.custom_url, None);
}

#[test]
fn upstream_proxy_config_serde_roundtrip() {
    let config = UpstreamProxyConfig {
        mode: UpstreamProxyMode::Custom,
        custom_url: Some("http://user:pass@127.0.0.1:7890".into()),
    };
    let json = serde_json::to_string(&config).unwrap();
    assert_eq!(
        json,
        r#"{"mode":"custom","customUrl":"http://user:pass@127.0.0.1:7890"}"#
    );
    let parsed: UpstreamProxyConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, config);
}

#[test]
fn apply_upstream_proxy_rejects_custom_mode_without_url() {
    let builder = reqwest::Client::builder();
    let result = apply_upstream_proxy(
        builder,
        &UpstreamProxyConfig {
            mode: UpstreamProxyMode::Custom,
            custom_url: None,
        },
    );
    assert!(result.is_err());
}

#[test]
fn apply_upstream_proxy_rejects_invalid_custom_url() {
    let builder = reqwest::Client::builder();
    let result = apply_upstream_proxy(
        builder,
        &UpstreamProxyConfig {
            mode: UpstreamProxyMode::Custom,
            custom_url: Some("not a url".into()),
        },
    );
    assert!(result.is_err());
}

#[test]
fn apply_upstream_proxy_accepts_valid_modes() {
    for config in [
        UpstreamProxyConfig::default(),
        UpstreamProxyConfig {
            mode: UpstreamProxyMode::Direct,
            custom_url: None,
        },
        UpstreamProxyConfig {
            mode: UpstreamProxyMode::Environment,
            custom_url: None,
        },
        UpstreamProxyConfig {
            mode: UpstreamProxyMode::Custom,
            custom_url: Some("socks5://127.0.0.1:1080".into()),
        },
    ] {
        let builder = reqwest::Client::builder();
        let builder = apply_upstream_proxy(builder, &config).expect("valid proxy config");
        builder.build().expect("client builds");
    }
}

#[tokio::test]
async fn custom_upstream_proxy_routes_http_traffic_through_proxy() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = listener.local_addr().unwrap();
    let capture = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut chunk = [0_u8; 1024];
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            let read = socket.read(&mut chunk).await.unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&chunk[..read]);
        }
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .await
            .unwrap();
        String::from_utf8_lossy(&request).to_string()
    });

    let builder = apply_upstream_proxy(
        reqwest::Client::builder(),
        &UpstreamProxyConfig {
            mode: UpstreamProxyMode::Custom,
            custom_url: Some(format!("http://{proxy_addr}")),
        },
    )
    .expect("custom proxy config");
    let body = builder
        .build()
        .unwrap()
        .get("http://example.com/upstream")
        .send()
        .await
        .expect("request through proxy")
        .text()
        .await
        .unwrap();
    assert_eq!(body, "ok");
    let request = capture.await.unwrap();
    assert!(
        request.starts_with("GET http://example.com/upstream"),
        "proxy received an absolute-URI request, got: {request:?}"
    );
}
