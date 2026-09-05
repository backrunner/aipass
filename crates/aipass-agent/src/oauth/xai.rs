//! Grok (xAI) OAuth device-code flow.
//!
//! A standard OIDC device-authorization grant: endpoints come from xAI's
//! discovery document, the CLI's public client id is used, and the access token
//! is refreshed with the refresh_token grant.

use super::{
    form_body, jwt_claims, DeviceChallenge, OAuthError, OAuthTokenBundle, MAX_OAUTH_RESPONSE_BYTES,
    MAX_POLL_INTERVAL_SECS, OAUTH_HTTP_TIMEOUT,
};
use serde::Deserialize;
use std::sync::{OnceLock, RwLock};

const XAI_ISSUER: &str = "https://auth.x.ai";
const XAI_DISCOVERY_URL: &str = "https://auth.x.ai/.well-known/openid-configuration";
const XAI_CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
const XAI_SCOPE: &str = "openid profile email offline_access grok-cli:access api:access";
const XAI_USER_AGENT: &str = "aipass-xai-oauth";
const DEFAULT_POLL_INTERVAL: u64 = 5;

#[derive(Debug, Deserialize)]
struct DiscoveryDocument {
    issuer: String,
    token_endpoint: String,
    device_authorization_endpoint: String,
}

#[derive(Debug, Clone)]
struct OAuthEndpoints {
    token_endpoint: String,
    device_authorization_endpoint: String,
}

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    interval: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

fn client() -> Result<reqwest::blocking::Client, OAuthError> {
    reqwest::blocking::Client::builder()
        .timeout(OAUTH_HTTP_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| OAuthError::Network(e.to_string()))
}

fn endpoints_cache() -> &'static RwLock<Option<OAuthEndpoints>> {
    static CACHE: OnceLock<RwLock<Option<OAuthEndpoints>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

fn discover_endpoints() -> Result<OAuthEndpoints, OAuthError> {
    if let Some(endpoints) = endpoints_cache().read().unwrap().clone() {
        return Ok(endpoints);
    }
    let response = client()?
        .get(XAI_DISCOVERY_URL)
        .header("User-Agent", XAI_USER_AGENT)
        .send()?;
    let status = response.status();
    if !status.is_success() {
        return Err(OAuthError::Network(format!(
            "xAI discovery failed: HTTP {status}"
        )));
    }
    let value: serde_json::Value = super::read_json_response(response)?;
    let document: DiscoveryDocument = serde_json::from_value(value)
        .map_err(|_| OAuthError::Parse("invalid OAuth response".into()))?;
    if document.issuer.trim_end_matches('/') != XAI_ISSUER {
        return Err(OAuthError::Parse("xAI discovery issuer mismatch".into()));
    }
    validate_endpoint(&document.token_endpoint)?;
    validate_endpoint(&document.device_authorization_endpoint)?;
    let endpoints = OAuthEndpoints {
        token_endpoint: document.token_endpoint,
        device_authorization_endpoint: document.device_authorization_endpoint,
    };
    *endpoints_cache().write().unwrap() = Some(endpoints.clone());
    Ok(endpoints)
}

fn validate_endpoint(url: &str) -> Result<(), OAuthError> {
    let parsed =
        reqwest::Url::parse(url).map_err(|_| OAuthError::Parse("bad xAI endpoint".into()))?;
    if parsed.scheme() != "https" || !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(OAuthError::Parse("xAI endpoint must be https".into()));
    }
    // Endpoints come from a remotely fetched discovery document and refresh
    // tokens are POSTed to them, so pin the host to xAI's domain rather than
    // trusting whatever the (process-wide cached) document says.
    let host = parsed.host_str().unwrap_or_default();
    if host != "x.ai" && !host.ends_with(".x.ai") {
        return Err(OAuthError::Parse("xAI endpoint host is not x.ai".into()));
    }
    Ok(())
}

pub(crate) fn start_device_flow() -> Result<DeviceChallenge, OAuthError> {
    let endpoints = discover_endpoints()?;
    let response = client()?
        .post(&endpoints.device_authorization_endpoint)
        .header("User-Agent", XAI_USER_AGENT)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form_body([
            ("client_id", XAI_CLIENT_ID),
            ("scope", XAI_SCOPE),
        ]))
        .send()?;
    if !response.status().is_success() {
        let status = response.status();
        let text = bounded_text(response);
        return Err(OAuthError::token_fetch_failed(
            "device authorization failed",
            status,
            oauth_error_code_str(&text).as_deref(),
        ));
    }
    let device: DeviceCodeResponse = super::read_json_response(response)?;
    validate_endpoint(&device.verification_uri)?;
    if let Some(uri) = &device.verification_uri_complete {
        validate_endpoint(uri)?;
    }
    Ok(DeviceChallenge {
        device_code: device.device_code,
        user_code: device.user_code,
        verification_uri: device.verification_uri,
        verification_uri_complete: device.verification_uri_complete,
        expires_in: device
            .expires_in
            .unwrap_or(super::DEFAULT_DEVICE_CODE_EXPIRES_IN),
        interval: device
            .interval
            .unwrap_or(DEFAULT_POLL_INTERVAL)
            .clamp(1, MAX_POLL_INTERVAL_SECS),
    })
}

pub(crate) fn poll_for_token(device_code: &str) -> Result<OAuthTokenBundle, OAuthError> {
    let endpoints = discover_endpoints()?;
    let response = client()?
        .post(&endpoints.token_endpoint)
        .header("User-Agent", XAI_USER_AGENT)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form_body([
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", XAI_CLIENT_ID),
            ("device_code", device_code),
        ]))
        .send()?;
    let status = response.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(OAuthError::SlowDown);
    }
    // Parse opportunistically: an unreadable/empty body on an error status is
    // still just that HTTP error, not a parse failure.
    let value: serde_json::Value =
        super::read_json_response(response).unwrap_or(serde_json::Value::Null);
    if let Some(code) = oauth_error_code(&value) {
        return match code.as_str() {
            "authorization_pending" => Err(OAuthError::AuthorizationPending),
            "slow_down" => Err(OAuthError::SlowDown),
            "access_denied" => Err(OAuthError::AccessDenied),
            "expired_token" => Err(OAuthError::ExpiredDeviceCode),
            _ => Err(OAuthError::token_fetch_failed(
                "device poll failed",
                status,
                Some(&code),
            )),
        };
    }
    if !status.is_success() {
        return Err(OAuthError::token_fetch_failed(
            "device poll failed",
            status,
            None,
        ));
    }
    let tokens: OAuthTokenResponse = serde_json::from_value(value)
        .map_err(|_| OAuthError::Parse("invalid OAuth response".into()))?;
    bundle_from_tokens(tokens, true)
}

pub(crate) fn refresh_with_token(refresh_token: &str) -> Result<OAuthTokenBundle, OAuthError> {
    let endpoints = discover_endpoints()?;
    let response = client()?
        .post(&endpoints.token_endpoint)
        .header("User-Agent", XAI_USER_AGENT)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form_body([
            ("grant_type", "refresh_token"),
            ("client_id", XAI_CLIENT_ID),
            ("refresh_token", refresh_token),
            ("scope", XAI_SCOPE),
        ]))
        .send()?;
    let status = response.status();
    // An unreadable/empty body on a 4xx still means the grant is gone.
    let value: serde_json::Value =
        super::read_json_response(response).unwrap_or(serde_json::Value::Null);
    let code = oauth_error_code(&value);
    if matches!(
        code.as_deref(),
        Some("invalid_grant" | "invalid_token" | "refresh_token_expired")
    ) || status == reqwest::StatusCode::UNAUTHORIZED
        || status == reqwest::StatusCode::FORBIDDEN
    {
        return Err(OAuthError::RefreshTokenInvalid);
    }
    if !status.is_success() || code.is_some() {
        return Err(OAuthError::token_fetch_failed(
            "refresh failed",
            status,
            code.as_deref(),
        ));
    }
    let tokens: OAuthTokenResponse = serde_json::from_value(value)
        .map_err(|_| OAuthError::Parse("invalid OAuth response".into()))?;
    bundle_from_tokens(tokens, false)
}

fn bundle_from_tokens(
    tokens: OAuthTokenResponse,
    require_refresh: bool,
) -> Result<OAuthTokenBundle, OAuthError> {
    let refresh_token = tokens.refresh_token.unwrap_or_default();
    if require_refresh && refresh_token.trim().is_empty() {
        return Err(OAuthError::token_fetch_failed_plain(
            "xAI response missing refresh_token",
        ));
    }
    if tokens.access_token.trim().is_empty() {
        return Err(OAuthError::token_fetch_failed_plain(
            "xAI response missing access_token",
        ));
    }
    // Identity comes from the id_token (OIDC): prefer email, fall back to sub.
    let claims = tokens.id_token.as_deref().and_then(jwt_claims);
    let account_identity = claims.as_ref().and_then(|claims| {
        ["email", "sub"].iter().find_map(|key| {
            claims
                .get(*key)
                .and_then(|v| v.as_str())
                .filter(|v| !v.trim().is_empty())
                .map(str::to_string)
        })
    });
    if require_refresh && account_identity.is_none() {
        return Err(OAuthError::Parse(
            "xAI token missing a stable identity claim".into(),
        ));
    }
    Ok(OAuthTokenBundle {
        access_token: tokens.access_token,
        refresh_token,
        id_token: tokens.id_token.filter(|v| !v.trim().is_empty()),
        chatgpt_account_id: None,
        account_identity,
        expires_in: tokens.expires_in.unwrap_or(3600),
    })
}

fn oauth_error_code(value: &serde_json::Value) -> Option<String> {
    value
        .get("error")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// The machine-readable OAuth `error` code from a raw body, used for
/// sanitized IPC-facing messages (never the free-text `error_description`).
fn oauth_error_code_str(text: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .as_ref()
        .and_then(oauth_error_code)
}

/// Read a response body with the read itself capped, so an oversized error
/// body cannot exhaust memory before truncation.
fn bounded_text(response: reqwest::blocking::Response) -> String {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    let _ = response
        .take(MAX_OAUTH_RESPONSE_BYTES as u64)
        .read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_endpoint_pins_the_xai_host() {
        assert!(validate_endpoint("https://auth.x.ai/oauth/token").is_ok());
        assert!(validate_endpoint("https://accounts.x.ai/device").is_ok());
        assert!(validate_endpoint("https://x.ai/token").is_ok());
        assert!(validate_endpoint("http://auth.x.ai/token").is_err());
        assert!(validate_endpoint("https://evil.example.com/token").is_err());
        assert!(validate_endpoint("https://auth.x.ai.evil.example.com/token").is_err());
        assert!(validate_endpoint("not a url").is_err());
    }
}
