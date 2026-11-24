//! Host-mediated HTTPS for sandboxed providers. DNS and redirects cannot widen a grant.
use super::Result;
use aipass_agent_protocol::{subscription_plugin::PluginHttp, SensitiveString};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use reqwest::{header, Client, Method, Url};
use std::{net::SocketAddr, time::Duration};

fn endpoint(input: &str) -> Option<Url> {
    let url = Url::parse(input).ok()?;
    let host = url.domain()?;
    (url.scheme() == "https"
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.'))
        && host.contains('.')
        && !host.ends_with('.')
        && !host.ends_with(".localhost")
        && !host.ends_with(".local"))
    .then_some(url)
}
pub(super) fn valid_origin(value: &str) -> bool {
    value.len() <= 256 && endpoint(value).is_some_and(|u| u.origin().ascii_serialization() == value)
}
pub(crate) fn approved(origins: &[String], url: &str) -> bool {
    endpoint(url).is_some_and(|u| origins.contains(&u.origin().ascii_serialization()))
}

pub(crate) async fn request(
    origins: &[String],
    request: PluginHttp,
) -> Result<(u16, SensitiveString)> {
    tokio::time::timeout(Duration::from_secs(45), fetch(origins, request))
        .await
        .map_err(|_| "Plugin HTTPS request timed out")?
}
async fn fetch(origins: &[String], r: PluginHttp) -> Result<(u16, SensitiveString)> {
    const MAX: usize = 2 * 1024 * 1024;
    if r.url.len() > 8192 || !approved(origins, &r.url) {
        return Err("Plugin HTTPS origin not authorized".into());
    }
    let url = endpoint(&r.url).ok_or("Invalid plugin URL")?;
    let host = url.domain().ok_or("Invalid plugin host")?;
    let method = match r.method.as_str() {
        "GET" => Method::GET,
        "POST" => Method::POST,
        "PUT" => Method::PUT,
        "PATCH" => Method::PATCH,
        "DELETE" => Method::DELETE,
        _ => return Err("Invalid plugin HTTP method".into()),
    };
    if r.headers.len() > 24 {
        return Err("Invalid plugin HTTP headers".into());
    }
    let mut headers = header::HeaderMap::new();
    for (key, value) in r.headers {
        if !(matches!(
            key.as_str(),
            "authorization"
                | "accept"
                | "content-type"
                | "api-key"
                | "anthropic-version"
                | "anthropic-beta"
        ) || key.starts_with("x-"))
            || key.len() > 128
            || value.expose().len() > 16384
        {
            return Err("Invalid plugin HTTP header".into());
        }
        headers.insert(
            header::HeaderName::from_bytes(key.as_bytes())
                .map_err(|_| "Invalid plugin HTTP header")?,
            header::HeaderValue::from_str(value.expose())
                .map_err(|_| "Invalid plugin HTTP header")?,
        );
    }
    let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host, 443))
        .await
        .map_err(|_| "Plugin host unavailable")?
        .take(65)
        .collect();
    if addresses.is_empty()
        || addresses.len() > 64
        || addresses
            .iter()
            .any(|a| !aipass_proxy::public_fetch_address(a.ip()))
    {
        return Err("Plugin HTTPS destination forbidden".into());
    }
    let client = Client::builder()
        .https_only(true)
        .no_proxy()
        .resolve_to_addrs(host, &addresses)
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(Duration::from_secs(8))
        .build()
        .map_err(|_| "Plugin HTTPS unavailable")?;
    let mut req = client.request(method, url).headers(headers);
    if let Some(body) = r.body {
        if body.expose().len() > MAX.div_ceil(3) * 4 {
            return Err("Plugin HTTPS body exceeds limit".into());
        }
        let data = STANDARD
            .decode(body.expose())
            .map_err(|_| "Invalid plugin HTTPS body")?;
        if data.len() > MAX {
            return Err("Plugin HTTPS body exceeds limit".into());
        }
        req = req.body(data);
    }
    let mut response = req
        .send()
        .await
        .map_err(|_| "Plugin HTTPS request failed")?;
    let status = response.status().as_u16();
    let mut data = zeroize::Zeroizing::new(Vec::new());
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Plugin HTTPS response failed")?
    {
        if data.len() + chunk.len() > MAX {
            return Err("Plugin HTTPS response exceeds limit".into());
        }
        data.extend_from_slice(&chunk);
    }
    Ok((status, SensitiveString::new(STANDARD.encode(&*data))))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn network_grants_match_exact_origins_before_io() {
        let origins = vec!["https://api.example.com".into()];
        assert!(approved(
            &origins,
            "https://api.example.com/quota?period=day"
        ));
        for url in [
            "http://api.example.com",
            "https://api.example.com.evil.test/",
            "https://user@api.example.com/",
            "https://api.example.com:444/",
            "https://127.0.0.1/",
            "file:///test",
        ] {
            assert!(!approved(&origins, url));
            let event = PluginHttp {
                method: "GET".into(),
                url: url.into(),
                headers: Default::default(),
                body: None,
            };
            assert!(
                matches!(request(&origins,event).await,Err(e) if e=="Plugin HTTPS origin not authorized")
            );
        }
    }
}
