//! Untrusted URLs may only reach public addresses. Resolve and pin each hop.
use super::*;
use std::net::{IpAddr, SocketAddr};
pub fn public_fetch_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let [a, b, c, _] = v.octets();
            !v.is_private()
                && !v.is_loopback()
                && !v.is_link_local()
                && !v.is_unspecified()
                && !v.is_broadcast()
                && !v.is_multicast()
                && a != 0
                && a < 224
                && !(a == 100 && (64..=127).contains(&b))
                && !(a == 198 && (b == 18 || b == 19))
                && !(a == 192 && b == 0 && c == 0)
        }
        IpAddr::V6(v) => v
            .to_ipv4_mapped()
            .map(|v| public_fetch_address(v.into()))
            .unwrap_or_else(|| {
                let s = v.segments();
                !v.is_loopback()
                    && !v.is_unspecified()
                    && !v.is_multicast()
                    && (s[0] & 0xfe00 != 0xfc00)
                    && (s[0] & 0xffc0 != 0xfe80)
                    && s[0] & 0xe000 == 0x2000
                    // Transition mechanisms can embed a private IPv4 endpoint.
                    && s[0] != 0x2002
                    && !(s[0] == 0x2001 && s[1] < 0x0200)
            }),
    }
}
pub(super) async fn fetch(url: &str) -> Result<Value, String> {
    tokio::time::timeout(Duration::from_secs(30), fetch_inner(url))
        .await
        .map_err(|_| "web fetch timed out")?
}
async fn fetch_inner(input: &str) -> Result<Value, String> {
    let mut url = reqwest::Url::parse(input).map_err(|_| "invalid page URL")?;
    for _ in 0..5 {
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err("page URL must use HTTP(S) without credentials".into());
        }
        let host = url
            .host_str()
            .ok_or("page URL has no host")?
            .trim_matches(['[', ']']);
        let port = url.port_or_known_default().ok_or("page URL has no port")?;
        if !matches!(port, 80 | 443) {
            return Err("web fetch supports ports 80 and 443".into());
        }
        let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| "page DNS lookup failed")?
            .collect();
        if addresses.is_empty() || addresses.iter().any(|a| !public_fetch_address(a.ip())) {
            return Err("web fetch cannot access local or private addresses".into());
        }
        // Proxy-side DNS would bypass the pinned-address restriction.
        let client = reqwest::Client::builder()
            .no_proxy()
            .resolve_to_addrs(host, &addresses)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| "web fetch unavailable")?;
        let response = client
            .get(url.clone())
            .header(header::ACCEPT, "text/html,text/plain,application/json")
            .send()
            .await
            .map_err(|_| "page request failed")?;
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or("page redirect has no location")?;
            url = url.join(location).map_err(|_| "invalid page redirect")?;
            continue;
        }
        if !response.status().is_success() {
            return Err("page request was rejected".into());
        }
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let html = content_type.contains("html");
        if !(html
            || content_type.starts_with("text/")
            || content_type.starts_with("application/json"))
        {
            return Err("web fetch requires a text page".into());
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| "page response failed")?;
            if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
                return Err("page response too large".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let raw = String::from_utf8_lossy(&bytes);
        let content = if html {
            plain_html(&raw)
        } else {
            raw.chars().take(MAX_RESULT).collect()
        };
        return Ok(json!({"url":url.as_str(),"content":content}));
    }
    Err("too many page redirects".into())
}
fn plain_html(raw: &str) -> String {
    let mut out = String::new();
    let mut tag = String::new();
    let mut inside = false;
    let mut hidden = None;
    for c in raw.chars() {
        if c == '<' {
            inside = true;
            tag.clear();
        } else if c == '>' && inside {
            inside = false;
            let t = tag.trim().to_ascii_lowercase();
            if t.starts_with("script") {
                hidden = Some("script");
            } else if t.starts_with("style") {
                hidden = Some("style");
            } else if hidden.is_some_and(|h| t.starts_with(&format!("/{h}"))) {
                hidden = None;
            }
            if hidden.is_none() {
                out.push(' ');
            }
        } else if inside {
            if tag.len() < 256 {
                tag.push(c);
            }
        } else if hidden.is_none() {
            out.push(c);
        }
        if out.len() >= MAX_RESULT {
            break;
        }
    }
    out.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_destinations_are_rejected() {
        for s in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "::1",
            "::ffff:127.0.0.1",
            "fd00::1",
            "fe80::1",
            "2002:7f00:1::1",
            "2001::ffff:80ff:fffe",
        ] {
            assert!(!public_fetch_address(s.parse().unwrap()), "{s}");
        }
        assert!(public_fetch_address("1.1.1.1".parse().unwrap()));
    }
    #[test]
    fn strips_active_html() {
        assert_eq!(
            plain_html(
                "<p>hello &amp; world</p><script>secret</script><style>hidden</style><p>end</p>"
            ),
            "hello & world end"
        );
    }
}
