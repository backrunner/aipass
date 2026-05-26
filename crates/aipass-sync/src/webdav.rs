use crate::local::SyncStatus;
use anyhow::Result;
use reqwest::blocking::Client;
use reqwest::header::{ETAG, IF_MATCH};
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebDavEntry {
    pub path: String,
    pub etag: Option<String>,
    pub len: u64,
}

pub trait WebDavClient {
    fn list(&self, prefix: &str) -> Result<Vec<WebDavEntry>>;
    fn get(&self, path: &str) -> Result<Vec<u8>>;
    fn put(&self, path: &str, bytes: &[u8], etag: Option<&str>) -> Result<Option<String>>;
    fn delete(&self, path: &str, etag: Option<&str>) -> Result<()>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebDavErrorKind {
    Conflict,
    AuthFailed,
    Offline,
    ServerError,
    Other,
}

#[derive(Debug)]
pub struct WebDavError {
    kind: WebDavErrorKind,
    message: String,
}

impl WebDavError {
    fn new(kind: WebDavErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> WebDavErrorKind {
        self.kind
    }
}

impl std::fmt::Display for WebDavError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for WebDavError {}

#[derive(Clone)]
pub struct HttpWebDavClient {
    base_url: String,
    username: Option<String>,
    password: Option<String>,
    client: Client,
}

impl HttpWebDavClient {
    pub fn new(base_url: &str, username: Option<String>, password: Option<String>) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("AIPass/1.0")
            .build()?;
        Ok(Self {
            base_url: format!("{}/", base_url.trim_end_matches('/')),
            username,
            password,
            client,
        })
    }

    fn request(&self, method: Method, path: &str) -> reqwest::blocking::RequestBuilder {
        let request = self.client.request(method, self.url_for(path));
        if let Some(username) = &self.username {
            request.basic_auth(username, self.password.as_deref())
        } else {
            request
        }
    }

    fn url_for(&self, path: &str) -> String {
        let path = path.trim_start_matches('/');
        if path.is_empty() {
            self.base_url.clone()
        } else {
            format!("{}{}", self.base_url, encode_relative_path(path))
        }
    }

    fn ensure_collection(&self, path: &str) -> Result<()> {
        let mut current = String::new();
        let parts = path
            .trim_matches('/')
            .split('/')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        for part in parts {
            if !current.is_empty() {
                current.push('/');
            }
            current.push_str(part);
            let response = self
                .request(Method::from_bytes(b"MKCOL")?, &current)
                .send()
                .map_err(|err| request_error("MKCOL", &current, err))?;
            match response.status() {
                StatusCode::CREATED
                | StatusCode::OK
                | StatusCode::METHOD_NOT_ALLOWED
                | StatusCode::CONFLICT => {}
                status if status.is_success() => {}
                status => return Err(status_error("MKCOL", &current, status)),
            }
        }
        Ok(())
    }
}

impl WebDavClient for HttpWebDavClient {
    fn list(&self, prefix: &str) -> Result<Vec<WebDavEntry>> {
        let prefix = prefix.trim_matches('/');
        if !prefix.is_empty() {
            self.ensure_collection(prefix)?;
        }
        let body = r#"<?xml version="1.0" encoding="utf-8" ?>
<D:propfind xmlns:D="DAV:">
  <D:prop>
    <D:getetag/>
    <D:getcontentlength/>
  </D:prop>
</D:propfind>"#;
        let response = self
            .request(Method::from_bytes(b"PROPFIND")?, prefix)
            .header("Depth", "1")
            .body(body)
            .send()
            .map_err(|err| request_error("PROPFIND", prefix, err))?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(Vec::new());
        }
        if !response.status().is_success() && response.status().as_u16() != 207 {
            return Err(status_error("PROPFIND", prefix, response.status()));
        }
        let text = response.text()?;
        parse_propfind_response(&text, prefix)
    }

    fn get(&self, path: &str) -> Result<Vec<u8>> {
        let response = self
            .request(Method::GET, path)
            .send()
            .map_err(|err| request_error("GET", path, err))?;
        if response.status() == StatusCode::NOT_FOUND {
            return Err(status_error("GET", path, response.status()));
        }
        if !response.status().is_success() {
            return Err(status_error("GET", path, response.status()));
        }
        Ok(response.bytes()?.to_vec())
    }

    fn put(&self, path: &str, bytes: &[u8], etag: Option<&str>) -> Result<Option<String>> {
        if let Some(parent) = Path::new(path).parent() {
            self.ensure_collection(&parent.display().to_string())?;
        }
        let mut request = self.request(Method::PUT, path).body(bytes.to_vec());
        if let Some(etag) = etag {
            request = request.header(IF_MATCH, etag);
        }
        let response = request
            .send()
            .map_err(|err| request_error("PUT", path, err))?;
        if response.status() == StatusCode::PRECONDITION_FAILED {
            return Err(status_error("PUT", path, response.status()));
        }
        if !response.status().is_success() {
            return Err(status_error("PUT", path, response.status()));
        }
        Ok(response
            .headers()
            .get(ETAG)
            .and_then(|value| value.to_str().ok())
            .map(ToString::to_string))
    }

    fn delete(&self, path: &str, etag: Option<&str>) -> Result<()> {
        let mut request = self.request(Method::DELETE, path);
        if let Some(etag) = etag {
            request = request.header(IF_MATCH, etag);
        }
        let response = request
            .send()
            .map_err(|err| request_error("DELETE", path, err))?;
        if response.status() != StatusCode::NOT_FOUND && !response.status().is_success() {
            return Err(status_error("DELETE", path, response.status()));
        }
        Ok(())
    }
}

pub fn classify_webdav_error(err: &anyhow::Error) -> SyncStatus {
    if let Some(webdav) = err.downcast_ref::<WebDavError>() {
        return match webdav.kind() {
            WebDavErrorKind::Conflict => SyncStatus::Conflict,
            WebDavErrorKind::AuthFailed => SyncStatus::AuthFailed,
            WebDavErrorKind::Offline => SyncStatus::Offline,
            WebDavErrorKind::ServerError => SyncStatus::ServerError,
            WebDavErrorKind::Other => SyncStatus::Offline,
        };
    }
    SyncStatus::Offline
}

fn request_error(method: &str, path: &str, err: reqwest::Error) -> anyhow::Error {
    let kind = if err.is_timeout() || err.is_connect() || err.is_request() {
        WebDavErrorKind::Offline
    } else {
        WebDavErrorKind::Other
    };
    WebDavError::new(kind, format!("webdav {method} {path} failed: {err}")).into()
}

fn status_error(method: &str, path: &str, status: StatusCode) -> anyhow::Error {
    let kind = match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => WebDavErrorKind::AuthFailed,
        StatusCode::PRECONDITION_FAILED | StatusCode::CONFLICT => WebDavErrorKind::Conflict,
        _ if status.is_server_error() => WebDavErrorKind::ServerError,
        _ => WebDavErrorKind::Other,
    };
    WebDavError::new(kind, format!("webdav {method} {path} failed with {status}")).into()
}

fn encode_relative_path(path: &str) -> String {
    path.split('/')
        .map(percent_encode_segment)
        .collect::<Vec<_>>()
        .join("/")
}

fn percent_encode_segment(segment: &str) -> String {
    let mut encoded = String::new();
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

pub(crate) fn parse_propfind_response(xml: &str, prefix: &str) -> Result<Vec<WebDavEntry>> {
    let mut reader = quick_xml::reader::Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut entries = Vec::new();
    let mut current_href: Option<String> = None;
    let mut current_etag: Option<String> = None;
    let mut current_len: u64 = 0;
    let mut current_tag = String::new();

    loop {
        match reader.read_event()? {
            quick_xml::events::Event::Start(start) => {
                current_tag =
                    String::from_utf8_lossy(start.local_name().as_ref()).to_ascii_lowercase();
                if current_tag == "response" {
                    current_href = None;
                    current_etag = None;
                    current_len = 0;
                }
            }
            quick_xml::events::Event::Text(text) => {
                let value = text.decode()?.trim().to_string();
                match current_tag.as_str() {
                    "href" => current_href = Some(value),
                    "getetag" => current_etag = Some(value),
                    "getcontentlength" => {
                        current_len = value.parse::<u64>().unwrap_or_default();
                    }
                    _ => {}
                }
            }
            quick_xml::events::Event::End(end) => {
                let tag = String::from_utf8_lossy(end.local_name().as_ref()).to_ascii_lowercase();
                if tag == "response" {
                    if let Some(path) = current_href
                        .as_deref()
                        .and_then(|href| propfind_href_to_relative_path(href, prefix))
                    {
                        entries.push(WebDavEntry {
                            path,
                            etag: current_etag.clone(),
                            len: current_len,
                        });
                    }
                }
                current_tag.clear();
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
    }

    Ok(entries)
}

fn propfind_href_to_relative_path(href: &str, prefix: &str) -> Option<String> {
    let trimmed = href.split(['?', '#']).next()?.trim_matches('/');
    let prefix = prefix.trim_matches('/');
    if prefix.is_empty() {
        return (!trimmed.is_empty()).then(|| trimmed.to_string());
    }
    if trimmed == prefix {
        return None;
    }
    let relative = trimmed.strip_prefix(prefix)?.trim_start_matches('/');
    (!relative.is_empty()).then(|| relative.trim_matches('/').to_string())
}
