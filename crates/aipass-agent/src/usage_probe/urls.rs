use reqwest::Url;
use std::net::IpAddr;

pub(super) fn newapi_token_urls(endpoint: &str) -> Vec<String> {
    roots_for_endpoint(endpoint)
        .into_iter()
        .map(|root| join_url(&root, "api/usage/token"))
        .collect()
}

pub(super) fn newapi_user_self_urls(endpoint: &str) -> Vec<String> {
    roots_for_endpoint(endpoint)
        .into_iter()
        .map(|root| join_url(&root, "api/user/self"))
        .collect()
}

pub(super) fn subapi_usage_urls(endpoint: &str) -> Vec<String> {
    roots_for_endpoint(endpoint)
        .into_iter()
        .map(|root| join_url(&root, "v1/usage"))
        .collect()
}

pub(super) fn validate_probe_url(url: &str) -> Result<(), &'static str> {
    let parsed = Url::parse(url).map_err(|_| "usage probe URL is invalid")?;
    if parsed.scheme() == "https" || is_loopback_url(&parsed) {
        Ok(())
    } else {
        Err("usage probe URL must use HTTPS, except localhost")
    }
}

fn roots_for_endpoint(endpoint: &str) -> Vec<String> {
    let trimmed = endpoint.trim().trim_end_matches('/');
    let mut roots = Vec::new();

    if let Ok(parsed) = Url::parse(trimmed) {
        let origin = url_origin(&parsed);
        let path = parsed.path().trim_end_matches('/');
        if let Some(index) = path.find("/v1") {
            push_unique(
                &mut roots,
                format!("{}{}", origin, path[..index].trim_end_matches('/')),
            );
        }
        if !path.is_empty() && path != "/" && !path.contains("/v1") {
            push_unique(&mut roots, format!("{origin}{path}"));
        }
        push_unique(&mut roots, origin);
    } else if !trimmed.is_empty() {
        let root = trimmed
            .find("/v1")
            .map(|index| &trimmed[..index])
            .unwrap_or(trimmed)
            .trim_end_matches('/');
        push_unique(&mut roots, root.to_string());
    }

    roots
}

fn is_loopback_url(url: &Url) -> bool {
    if url
        .host_str()
        .is_some_and(|host| host.eq_ignore_ascii_case("localhost"))
    {
        return true;
    }
    url.host_str()
        .and_then(|host| host.parse::<IpAddr>().ok())
        .is_some_and(|ip| ip.is_loopback())
}

fn url_origin(url: &Url) -> String {
    let mut origin = format!("{}://{}", url.scheme(), url.host_str().unwrap_or_default());
    if let Some(port) = url.port() {
        origin.push(':');
        origin.push_str(&port.to_string());
    }
    origin
}

fn join_url(base: &str, suffix: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        suffix.trim_start_matches('/')
    )
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !value.is_empty() && !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_preserve_prefix_before_v1() {
        let roots = roots_for_endpoint("https://example.com/proxy/v1");
        assert_eq!(roots[0], "https://example.com/proxy");
        assert!(roots.contains(&"https://example.com".to_string()));
    }
}
