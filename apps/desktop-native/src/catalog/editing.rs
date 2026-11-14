//! Header assignments follow the shared editor's case-insensitive merge rule.
use aipass_provider_registry::{EndpointKind, ProviderEndpoint};

/// Match the desktop input adapter: an empty override follows only API hosts.
pub(super) fn domains(explicit: &str, endpoints: &[ProviderEndpoint]) -> Vec<String> {
    let explicit = super::lists::parse(explicit);
    if !explicit.is_empty() {
        return explicit;
    }
    let mut hosts = Vec::new();
    for endpoint in endpoints
        .iter()
        .filter(|endpoint| endpoint.kind == EndpointKind::Api)
    {
        if let Some(url) = endpoint
            .url
            .as_deref()
            .and_then(|url| url::Url::parse(url).ok())
            .filter(|url| matches!(url.scheme(), "http" | "https"))
        {
            if let Some(host) = url.host_str() {
                if !hosts.iter().any(|existing| existing == host) {
                    hosts.push(host.to_owned());
                }
            }
        }
    }
    hosts
}

pub(super) fn merge_headers(
    mut stored: Vec<(String, String)>,
    incoming: Vec<(String, String)>,
) -> Vec<(String, String)> {
    for (name, value) in incoming {
        if let Some(pair) = stored
            .iter_mut()
            .find(|(key, _)| key.eq_ignore_ascii_case(&name))
        {
            *pair = (name, value);
        } else {
            stored.push((name, value));
        }
    }
    stored
}

#[cfg(test)]
mod tests;
