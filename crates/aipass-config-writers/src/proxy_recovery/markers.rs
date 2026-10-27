//! Only exact route identifiers, credentials and endpoints written by AIPass
//! prove that an unchanged value still belongs to its proxy.
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Markers(Vec<String>);
impl Drop for Markers {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}
impl Markers {
    pub fn from_entry(entry: &crate::ToolEntry) -> Self {
        let mut result = Self::default();
        result.add_route(entry.id, entry.api_key.as_deref().unwrap_or_default());
        if let Some(endpoint) = &entry.endpoint {
            result.add_endpoint(endpoint);
        }
        result
    }
    pub fn add_route(&mut self, id: uuid::Uuid, token: &str) {
        self.0.extend([id.to_string(), id.simple().to_string()]);
        if !token.is_empty() {
            self.0.push(token.to_owned());
        }
    }
    pub fn add_endpoint(&mut self, endpoint: &str) {
        let endpoint = endpoint.trim_end_matches('/');
        if !endpoint.is_empty() {
            self.0.push(endpoint.to_owned());
        }
        if let Some(base) = endpoint.strip_suffix("/v1") {
            if !base.is_empty() {
                self.0.push(base.to_owned());
            }
        }
    }
    pub(super) fn contains_bytes(&self, bytes: &[u8]) -> bool {
        self.0.iter().any(|value| {
            !value.is_empty()
                && bytes.windows(value.len()).enumerate().any(|(index, v)| {
                    v == value.as_bytes()
                        && endpoint_boundary(value, bytes.get(index + value.len()).copied())
                })
        })
    }
    pub(super) fn contains(&self, text: &str) -> bool {
        self.0.iter().any(|value| {
            !value.is_empty()
                && text.match_indices(value).any(|(index, _)| {
                    endpoint_boundary(value, text.as_bytes().get(index + value.len()).copied())
                })
        })
    }
    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub(super) fn extend(&mut self, other: &Self) {
        for value in &other.0 {
            if !self.0.contains(value) {
                self.0.push(value.clone());
            }
        }
    }
    pub(super) fn contains_json(&self, value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::String(value) => self.contains(value),
            serde_json::Value::Array(values) => values.iter().any(|v| self.contains_json(v)),
            serde_json::Value::Object(values) => values
                .iter()
                .any(|(k, v)| self.contains(k) || self.contains_json(v)),
            _ => false,
        }
    }
    #[cfg(test)]
    pub(super) fn fixture() -> Self {
        Self(vec![
            "proxy".into(),
            "localhost".into(),
            "http://127.0.0.1:1".into(),
        ])
    }
}

fn endpoint_boundary(value: &str, next: Option<u8>) -> bool {
    if !(value.starts_with("http://") || value.starts_with("https://")) {
        return true;
    }
    next.is_none_or(|c| {
        matches!(
            c,
            b'/' | b'?' | b'#' | b'"' | b'\'' | b' ' | b'\t' | b'\r' | b'\n' | b'\\'
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_markers_do_not_claim_a_different_port_or_host() {
        let mut markers = Markers::default();
        markers.add_endpoint("http://127.0.0.1:8787/v1");
        for value in [
            "http://127.0.0.1:8787/v1",
            "http://127.0.0.1:8787/v1/messages",
            "base_url = \"http://127.0.0.1:8787\"",
            "http://127.0.0.1:8787?route=x",
        ] {
            assert!(markers.contains(value));
            assert!(markers.contains_bytes(value.as_bytes()));
        }
        for value in [
            "http://127.0.0.1:87870/v1",
            "http://127.0.0.1:8787.evil/v1",
            "http://127.0.0.1:8787@other.invalid/v1",
        ] {
            assert!(!markers.contains(value));
            assert!(!markers.contains_bytes(value.as_bytes()));
        }
    }
}
