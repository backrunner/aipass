//! ChatGPT/Codex OAuth device-code flow.
//!
//! Replicates the official Codex CLI login against OpenAI's device-auth
//! endpoints using the CLI's public client id. The verifier is returned by the
//! server (an OpenAI device-flow quirk), so there is no local PKCE generation.

use super::{
    form_body, jwt_claims, DeviceChallenge, OAuthError, OAuthTokenBundle,
    DEFAULT_DEVICE_CODE_EXPIRES_IN, MAX_OAUTH_RESPONSE_BYTES, OAUTH_HTTP_TIMEOUT,
};
use serde::Deserialize;

const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const DEVICE_AUTH_USERCODE_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/usercode";
const DEVICE_AUTH_TOKEN_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/token";
const OAUTH_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const DEVICE_VERIFICATION_URL: &str = "https://auth.openai.com/codex/device";
/// The device-flow redirect URI is an OpenAI server-side convention, not a
/// loopback listener we host.
const DEVICE_REDIRECT_URI: &str = "https://auth.openai.com/deviceauth/callback";
const CODEX_USER_AGENT: &str = "aipass-codex-oauth";

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_auth_id: String,
    user_code: String,
    #[serde(default)]
    interval: Option<serde_json::Value>,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct DevicePollSuccess {
    authorization_code: String,
    code_verifier: String,
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
        .build()
        .map_err(|e| OAuthError::Network(e.to_string()))
}

fn parse_interval(value: Option<&serde_json::Value>) -> u64 {
    match value {
        Some(serde_json::Value::Number(n)) => n.as_u64().or_else(|| n.as_f64().map(|f| f as u64)),
        Some(serde_json::Value::String(s)) => s.trim().parse::<u64>().ok(),
        _ => None,
    }
    .unwrap_or(5)
    .clamp(1, super::MAX_POLL_INTERVAL_SECS)
}

pub(crate) fn start_device_flow() -> Result<DeviceChallenge, OAuthError> {
    let response = client()?
        .post(DEVICE_AUTH_USERCODE_URL)
        .header("Content-Type", "application/json")
        .header("User-Agent", CODEX_USER_AGENT)
        .json(&serde_json::json!({ "client_id": CODEX_CLIENT_ID }))
        .send()?;
    if !response.status().is_success() {
        let status = response.status();
        let text = bounded_text(response);
        return Err(OAuthError::token_fetch_failed(
            "device usercode failed",
            status,
            oauth_error_code(&text).as_deref(),
        ));
    }
    let device: DeviceCodeResponse = response
        .json()
        .map_err(|e| OAuthError::Parse(e.to_string()))?;
    Ok(DeviceChallenge {
        device_code: device.device_auth_id,
        user_code: device.user_code,
        verification_uri: DEVICE_VERIFICATION_URL.to_string(),
        verification_uri_complete: None,
        expires_in: device.expires_in.unwrap_or(DEFAULT_DEVICE_CODE_EXPIRES_IN),
        interval: parse_interval(device.interval.as_ref()),
    })
}

/// One poll attempt. `user_code` is required by OpenAI's device-token endpoint.
pub(crate) fn poll_for_token(
    device_code: &str,
    user_code: &str,
) -> Result<OAuthTokenBundle, OAuthError> {
    let response = client()?
        .post(DEVICE_AUTH_TOKEN_URL)
        .header("Content-Type", "application/json")
        .header("User-Agent", CODEX_USER_AGENT)
        .json(&serde_json::json!({
            "device_auth_id": device_code,
            "user_code": user_code,
        }))
        .send()?;
    let status = response.status();
    // 403/404: the user has not finished authorizing yet — keep polling.
    if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::NOT_FOUND {
        return Err(OAuthError::AuthorizationPending);
    }
    if status == reqwest::StatusCode::GONE {
        return Err(OAuthError::ExpiredDeviceCode);
    }
    if !status.is_success() {
        let text = bounded_text(response);
        return Err(OAuthError::token_fetch_failed(
            "device poll failed",
            status,
            oauth_error_code(&text).as_deref(),
        ));
    }
    let success: DevicePollSuccess = response
        .json()
        .map_err(|e| OAuthError::Parse(e.to_string()))?;
    let tokens = exchange_code_for_tokens(&success.authorization_code, &success.code_verifier)?;
    bundle_from_tokens(tokens, true)
}

fn exchange_code_for_tokens(
    code: &str,
    code_verifier: &str,
) -> Result<OAuthTokenResponse, OAuthError> {
    let response = client()?
        .post(OAUTH_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("User-Agent", CODEX_USER_AGENT)
        .body(form_body([
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", DEVICE_REDIRECT_URI),
            ("client_id", CODEX_CLIENT_ID),
            ("code_verifier", code_verifier),
        ]))
        .send()?;
    if !response.status().is_success() {
        let status = response.status();
        let text = bounded_text(response);
        return Err(OAuthError::token_fetch_failed(
            "token exchange failed",
            status,
            oauth_error_code(&text).as_deref(),
        ));
    }
    response
        .json()
        .map_err(|e| OAuthError::Parse(e.to_string()))
}

pub(crate) fn refresh_with_token(refresh_token: &str) -> Result<OAuthTokenBundle, OAuthError> {
    let response = client()?
        .post(OAUTH_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("User-Agent", CODEX_USER_AGENT)
        .body(form_body([
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", CODEX_CLIENT_ID),
        ]))
        .send()?;
    let status = response.status();
    if !status.is_success() {
        let text = bounded_text(response);
        if status == reqwest::StatusCode::UNAUTHORIZED
            || status == reqwest::StatusCode::FORBIDDEN
            || matches!(
                extract_error_code(&text).as_deref(),
                Some(
                    "refresh_token_expired" | "refresh_token_reused" | "refresh_token_invalidated"
                )
            )
        {
            return Err(OAuthError::RefreshTokenInvalid);
        }
        return Err(OAuthError::token_fetch_failed(
            "refresh failed",
            status,
            oauth_error_code(&text).as_deref(),
        ));
    }
    let tokens: OAuthTokenResponse = response
        .json()
        .map_err(|e| OAuthError::Parse(e.to_string()))?;
    // A refresh may omit a new refresh token; the caller keeps the old one.
    bundle_from_tokens(tokens, false)
}

/// Assemble a token bundle, pulling workspace id + identity from the id_token.
/// `require_id_token` is true for a fresh login (we need a stable identity to
/// dedup accounts and to write the native auth.json), false for a refresh.
fn bundle_from_tokens(
    tokens: OAuthTokenResponse,
    require_id_token: bool,
) -> Result<OAuthTokenBundle, OAuthError> {
    let refresh_token = tokens.refresh_token.unwrap_or_default();
    if require_id_token && refresh_token.is_empty() {
        return Err(OAuthError::token_fetch_failed_plain(
            "login response missing refresh_token",
        ));
    }
    if tokens.access_token.trim().is_empty() {
        return Err(OAuthError::token_fetch_failed_plain(
            "login response missing access_token",
        ));
    }
    let id_token = tokens
        .id_token
        .clone()
        .filter(|value| !value.trim().is_empty());
    let claims = id_token.as_deref().and_then(jwt_claims);
    let chatgpt_account_id = claims.as_ref().and_then(|claims| {
        claims
            .get("chatgpt_account_id")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| {
                // The workspace id may be nested under the literal claim key
                // "https://api.openai.com/auth" (slashes in the key rule out a
                // JSON Pointer lookup).
                claims
                    .get("https://api.openai.com/auth")
                    .and_then(|auth| auth.get("chatgpt_account_id"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            })
    });
    let account_identity = claims.as_ref().and_then(|claims| {
        claims
            .get("email")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| {
                claims
                    .get("sub")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            })
    });
    if require_id_token && (id_token.is_none() || chatgpt_account_id.is_none()) {
        return Err(OAuthError::token_fetch_failed_plain(
            "login response missing id_token or chatgpt_account_id",
        ));
    }
    Ok(OAuthTokenBundle {
        access_token: tokens.access_token,
        refresh_token,
        id_token,
        chatgpt_account_id,
        account_identity,
        expires_in: tokens.expires_in.unwrap_or(3600),
    })
}

/// The machine-readable OAuth `error` code from a JSON error body, used for
/// sanitized IPC-facing messages (never the free-text `error_description`).
fn oauth_error_code(text: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(|e| e.as_str())
                .map(str::to_string)
        })
}

fn extract_error_code(text: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(|e| e.as_str())
                .map(str::to_string)
                .or_else(|| {
                    value
                        .get("error_description")
                        .and_then(|e| e.as_str())
                        .map(str::to_string)
                })
        })
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
    use base64::Engine as _;

    fn fake_jwt(claims: serde_json::Value) -> String {
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&claims).unwrap());
        format!("header.{payload}.signature")
    }

    fn response(id_token: Option<String>, expires_in: Option<i64>) -> OAuthTokenResponse {
        OAuthTokenResponse {
            access_token: "access".to_string(),
            refresh_token: Some("refresh".to_string()),
            id_token,
            expires_in,
        }
    }

    #[test]
    fn extracts_workspace_id_from_nested_openai_auth_claim() {
        let id_token = fake_jwt(serde_json::json!({
            "email": "user@example.com",
            "https://api.openai.com/auth": { "chatgpt_account_id": "ws_nested" }
        }));
        let bundle = bundle_from_tokens(response(Some(id_token), Some(3600)), true).unwrap();
        assert_eq!(bundle.chatgpt_account_id.as_deref(), Some("ws_nested"));
        assert_eq!(bundle.account_identity.as_deref(), Some("user@example.com"));
    }

    #[test]
    fn prefers_top_level_workspace_id_and_defaults_expiry() {
        let id_token = fake_jwt(serde_json::json!({
            "sub": "subject-id",
            "chatgpt_account_id": "ws_top"
        }));
        let bundle = bundle_from_tokens(response(Some(id_token), None), true).unwrap();
        assert_eq!(bundle.chatgpt_account_id.as_deref(), Some("ws_top"));
        assert_eq!(bundle.account_identity.as_deref(), Some("subject-id"));
        assert_eq!(bundle.expires_in, 3600);
    }

    #[test]
    fn rejects_empty_access_token() {
        let mut tokens = response(None, None);
        tokens.access_token = "   ".to_string();
        let err = bundle_from_tokens(tokens, false).unwrap_err();
        assert!(matches!(err, OAuthError::TokenFetchFailed { .. }));
    }
}
