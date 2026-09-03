//! In-app OAuth device-code login for official providers (ChatGPT/Codex, Grok).
//!
//! The agent is the only component that ever sees these tokens. This module
//! performs the network half of the flow (device challenge, polling, token
//! exchange, refresh) and returns plain token bundles; the caller in
//! `handlers.rs` persists them to the vault, writes the native CLI credential
//! file, and pushes the access token into the running proxy.
//!
//! These flows reuse the official CLIs' public client ids and undocumented
//! device endpoints, so every constant is isolated here and every network step
//! degrades to a typed error rather than a panic.

pub(crate) mod codex;
pub(crate) mod native_write;
pub(crate) mod refresh_loop;
pub(crate) mod xai;

pub(crate) use refresh_loop::spawn_token_refresh;

use aipass_provider_registry::OAuthProvider;
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Per-request timeout for OAuth device/token endpoints. The shared HTTP client
/// default is far longer; a wedged auth endpoint must not stall a handler.
pub(crate) const OAUTH_HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
/// Refresh the access token this long before it actually expires. Must exceed
/// the refresh loop's sweep interval (120s) so a token is never served expired
/// between sweeps.
pub(crate) const TOKEN_REFRESH_BUFFER_MS: i64 = 300_000;
/// Default device-code lifetime when the provider omits `expires_in`.
pub(crate) const DEFAULT_DEVICE_CODE_EXPIRES_IN: u64 = 900;
/// Cap on how long we keep polling a single device code.
pub(crate) const MAX_DEVICE_CODE_LIFETIME_SECS: u64 = 24 * 60 * 60;
pub(crate) const MAX_POLL_INTERVAL_SECS: u64 = 60;
/// Reject absurd/oversized auth responses.
pub(crate) const MAX_OAUTH_RESPONSE_BYTES: usize = 64 * 1024;

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug)]
pub(crate) enum OAuthError {
    /// The user has not authorized yet; the caller should keep polling.
    AuthorizationPending,
    /// Polling too fast; back off and retry.
    SlowDown,
    /// The device code expired or was abandoned.
    ExpiredDeviceCode,
    /// The user refused the authorization prompt.
    AccessDenied,
    /// The refresh token was rejected; interactive re-login is required.
    RefreshTokenInvalid,
    Network(String),
    Parse(String),
    /// The token endpoint rejected the request. `status` drives retry
    /// classification; `message` is sanitized (context + HTTP status + the
    /// server's machine-readable OAuth error code) so raw response bodies
    /// never cross the IPC boundary.
    TokenFetchFailed {
        status: Option<u16>,
        message: String,
    },
}

impl OAuthError {
    /// Transient failures (network errors, 5xx responses) where the caller
    /// should keep polling/retrying rather than aborting the login.
    pub(crate) fn is_retryable(&self) -> bool {
        match self {
            OAuthError::Network(_) => true,
            OAuthError::TokenFetchFailed { status, .. } => {
                status.is_some_and(|status| (500..=599).contains(&status))
            }
            _ => false,
        }
    }

    /// Build a sanitized token-endpoint failure. `code` is the server's
    /// machine-readable OAuth `error` code, never free-text descriptions.
    pub(crate) fn token_fetch_failed(
        context: &str,
        status: reqwest::StatusCode,
        code: Option<&str>,
    ) -> Self {
        let message = match code {
            // Bound even the "code": a hostile server controls this string.
            Some(code) => format!("{context}: HTTP {status} ({})", truncate(code, 100)),
            None => format!("{context}: HTTP {status}"),
        };
        OAuthError::TokenFetchFailed {
            status: Some(status.as_u16()),
            message,
        }
    }

    /// A token-endpoint failure with no HTTP response (e.g. a malformed body).
    pub(crate) fn token_fetch_failed_plain(message: impl Into<String>) -> Self {
        OAuthError::TokenFetchFailed {
            status: None,
            message: message.into(),
        }
    }
}

/// Clamp a server-supplied token lifetime to a sane range so absurd values can
/// neither overflow the expiry math nor cause perpetual refresh churn.
pub(crate) fn clamp_expires_in(expires_in: i64) -> i64 {
    expires_in.clamp(1, 30 * 24 * 60 * 60)
}

fn truncate(value: &str, max: usize) -> &str {
    match value.char_indices().nth(max) {
        Some((index, _)) => &value[..index],
        None => value,
    }
}

impl std::fmt::Display for OAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OAuthError::AuthorizationPending => write!(f, "authorization pending"),
            OAuthError::SlowDown => write!(f, "polling too fast"),
            OAuthError::ExpiredDeviceCode => write!(f, "device code expired"),
            OAuthError::AccessDenied => write!(f, "access denied"),
            OAuthError::RefreshTokenInvalid => write!(f, "refresh token invalid"),
            OAuthError::Network(message) => write!(f, "network error: {message}"),
            OAuthError::Parse(message) => write!(f, "parse error: {message}"),
            OAuthError::TokenFetchFailed { message, .. } => write!(f, "token error: {message}"),
        }
    }
}

impl From<reqwest::Error> for OAuthError {
    fn from(err: reqwest::Error) -> Self {
        OAuthError::Network(err.to_string())
    }
}

/// The device-code challenge returned to the desktop so the user can authorize
/// in a browser. Mirrors `OAuthDeviceStart` in the protocol crate.
#[derive(Clone, Debug)]
pub(crate) struct DeviceChallenge {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub expires_in: u64,
    pub interval: u64,
}

/// A full refreshable token bundle obtained from a login or a refresh. The
/// handler persists this; it never crosses the IPC boundary.
#[derive(Clone, Debug)]
pub(crate) struct OAuthTokenBundle {
    pub access_token: String,
    pub refresh_token: String,
    pub id_token: Option<String>,
    /// Codex workspace id (`chatgpt-account-id`); None for Grok.
    pub chatgpt_account_id: Option<String>,
    /// Stable user identity (email or JWT `sub`) for display/dedup.
    pub account_identity: Option<String>,
    /// Access-token lifetime in seconds from now.
    pub expires_in: i64,
}

/// One in-flight device-code login, kept in memory until authorized/expired.
#[derive(Clone, Debug)]
struct PendingDeviceCode {
    provider: OAuthProvider,
    user_code: String,
    expires_at_ms: i64,
    interval_secs: u64,
}

/// The result of one poll attempt: an optional token bundle plus the current
/// server-side poll interval, so the client can back off in step with
/// `slow_down` responses.
#[derive(Clone, Debug)]
pub(crate) struct PollOutcome {
    pub bundle: Option<OAuthTokenBundle>,
    pub interval_secs: u64,
}

/// Process-wide registry of in-flight device codes. Ephemeral by design: a
/// restart simply forces the user to start a new login.
pub(crate) struct OAuthManager {
    pending: RwLock<HashMap<String, PendingDeviceCode>>,
}

impl OAuthManager {
    fn new() -> Self {
        Self {
            pending: RwLock::new(HashMap::new()),
        }
    }

    fn register(&self, device_code: &str, entry: PendingDeviceCode) {
        let mut pending = self.pending.write().unwrap();
        let now = now_ms();
        pending.retain(|_, value| value.expires_at_ms > now);
        pending.insert(device_code.to_string(), entry);
    }

    fn get(&self, device_code: &str) -> Option<PendingDeviceCode> {
        self.pending.read().unwrap().get(device_code).cloned()
    }

    fn bump_interval(&self, device_code: &str) -> Option<u64> {
        let mut pending = self.pending.write().unwrap();
        let mut entry = pending.remove(device_code)?;
        entry.interval_secs = (entry.interval_secs + 5).min(MAX_POLL_INTERVAL_SECS);
        let interval_secs = entry.interval_secs;
        pending.insert(device_code.to_string(), entry);
        Some(interval_secs)
    }

    pub(crate) fn cancel(&self, device_code: &str) -> bool {
        self.pending.write().unwrap().remove(device_code).is_some()
    }

    /// Remove a device-code entry after its tokens were persisted. Kept
    /// separate from `poll` so a failed persistence does not lose the bundle.
    pub(crate) fn consume(&self, device_code: &str) {
        self.pending.write().unwrap().remove(device_code);
    }

    /// The current server-side poll interval for an in-flight login, if known.
    pub(crate) fn current_interval(&self, device_code: &str) -> Option<u64> {
        self.get(device_code).map(|entry| entry.interval_secs)
    }

    /// Kick off a device-code login. Returns the challenge to show the user.
    pub(crate) fn start(&self, provider: OAuthProvider) -> Result<DeviceChallenge, OAuthError> {
        let mut challenge = match provider {
            OAuthProvider::Codex => codex::start_device_flow()?,
            OAuthProvider::Grok => xai::start_device_flow()?,
        };
        // Clamp once so the challenge handed to the client and the registry
        // entry agree on the lifetime and interval.
        challenge.expires_in = challenge.expires_in.clamp(1, MAX_DEVICE_CODE_LIFETIME_SECS);
        challenge.interval = challenge.interval.clamp(1, MAX_POLL_INTERVAL_SECS);
        self.register(
            &challenge.device_code,
            PendingDeviceCode {
                provider,
                user_code: challenge.user_code.clone(),
                expires_at_ms: now_ms().saturating_add((challenge.expires_in as i64) * 1000),
                interval_secs: challenge.interval,
            },
        );
        Ok(challenge)
    }

    /// One poll attempt. `bundle: None` means still pending; `Some(bundle)`
    /// means the user authorized and we exchanged for tokens. On success the
    /// pending entry is kept until the caller persists the bundle and calls
    /// `consume`, so a persistence failure cannot lose the tokens.
    pub(crate) fn poll(
        &self,
        provider: OAuthProvider,
        device_code: &str,
    ) -> Result<PollOutcome, OAuthError> {
        let entry = match self.get(device_code) {
            Some(entry) if entry.provider == provider => entry,
            Some(_) => return Err(OAuthError::Parse("device code/provider mismatch".into())),
            None => return Err(OAuthError::ExpiredDeviceCode),
        };
        if entry.expires_at_ms <= now_ms() {
            self.cancel(device_code);
            return Err(OAuthError::ExpiredDeviceCode);
        }
        let result = match provider {
            OAuthProvider::Codex => codex::poll_for_token(device_code, &entry.user_code),
            OAuthProvider::Grok => xai::poll_for_token(device_code),
        };
        match result {
            Ok(bundle) => Ok(PollOutcome {
                bundle: Some(bundle),
                interval_secs: entry.interval_secs,
            }),
            Err(OAuthError::AuthorizationPending) => Ok(PollOutcome {
                bundle: None,
                interval_secs: entry.interval_secs,
            }),
            Err(OAuthError::SlowDown) => Ok(PollOutcome {
                bundle: None,
                interval_secs: self
                    .bump_interval(device_code)
                    .unwrap_or(entry.interval_secs),
            }),
            Err(err @ (OAuthError::ExpiredDeviceCode | OAuthError::AccessDenied)) => {
                self.cancel(device_code);
                Err(err)
            }
            Err(err) => Err(err),
        }
    }

    /// Refresh an account's access token using its refresh token.
    pub(crate) fn refresh(
        provider: OAuthProvider,
        refresh_token: &str,
    ) -> Result<OAuthTokenBundle, OAuthError> {
        match provider {
            OAuthProvider::Codex => codex::refresh_with_token(refresh_token),
            OAuthProvider::Grok => xai::refresh_with_token(refresh_token),
        }
    }
}

fn manager() -> &'static OAuthManager {
    static MANAGER: OnceLock<OAuthManager> = OnceLock::new();
    MANAGER.get_or_init(OAuthManager::new)
}

pub(crate) fn oauth_manager() -> &'static OAuthManager {
    manager()
}

/// Decode a JWT payload's claims without verifying the signature. Used only to
/// read display identity / workspace id.
///
/// WARNING: the claims are unverified attacker-controllable input. They must
/// never be used for an authorization or trust decision.
pub(crate) fn jwt_claims(token: &str) -> Option<serde_json::Value> {
    use base64::Engine as _;
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Percent-encode pairs into an `application/x-www-form-urlencoded` body.
/// reqwest here is built without default features, so `RequestBuilder::form`
/// is unavailable and we serialize the body ourselves.
pub(crate) fn form_body<'a, I>(pairs: I) -> String
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs)
        .finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_and_5xx_failures_are_retryable() {
        assert!(OAuthError::Network("boom".into()).is_retryable());
        assert!(OAuthError::TokenFetchFailed {
            status: Some(503),
            message: "refresh failed: HTTP 503".into(),
        }
        .is_retryable());
    }

    #[test]
    fn hard_failures_are_not_retryable() {
        for err in [
            OAuthError::AuthorizationPending,
            OAuthError::SlowDown,
            OAuthError::ExpiredDeviceCode,
            OAuthError::AccessDenied,
            OAuthError::RefreshTokenInvalid,
            OAuthError::Parse("bad".into()),
            OAuthError::TokenFetchFailed {
                status: Some(400),
                message: "refresh failed: HTTP 400 (invalid_grant)".into(),
            },
            OAuthError::TokenFetchFailed {
                status: None,
                message: "login response missing refresh_token".into(),
            },
        ] {
            assert!(!err.is_retryable(), "{err} must not be retryable");
        }
    }

    #[test]
    fn clamp_expires_in_bounds_absurd_server_values() {
        assert_eq!(clamp_expires_in(0), 1);
        assert_eq!(clamp_expires_in(-5), 1);
        assert_eq!(clamp_expires_in(3600), 3600);
        assert_eq!(clamp_expires_in(i64::MAX), 30 * 24 * 60 * 60);
    }

    #[test]
    fn bump_interval_raises_and_returns_the_stored_interval() {
        let manager = OAuthManager::new();
        manager.register(
            "code",
            PendingDeviceCode {
                provider: OAuthProvider::Codex,
                user_code: "user".into(),
                expires_at_ms: now_ms() + 60_000,
                interval_secs: 5,
            },
        );
        assert_eq!(manager.bump_interval("code"), Some(10));
        assert_eq!(manager.current_interval("code"), Some(10));
        manager.cancel("code");
        assert_eq!(manager.bump_interval("code"), None);
        assert_eq!(manager.current_interval("code"), None);
    }
}
