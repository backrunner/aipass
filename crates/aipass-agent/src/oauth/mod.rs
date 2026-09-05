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
use zeroize::ZeroizeOnDrop;

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
                status.is_some_and(|status| status == 429 || (500..=599).contains(&status))
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
#[derive(Clone, ZeroizeOnDrop)]
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

impl std::fmt::Debug for OAuthTokenBundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OAuthTokenBundle([REDACTED])")
    }
}

/// One in-flight device-code login, kept in memory until authorized/expired.
#[derive(Clone, Debug)]
struct PendingDeviceCode {
    provider: OAuthProvider,
    user_code: String,
    expires_at_ms: i64,
    interval_secs: u64,
    next_poll_at_ms: i64,
    polling: bool,
    bundle: Option<OAuthTokenBundle>,
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

    #[cfg(test)]
    fn bump_interval(&self, device_code: &str) -> Option<u64> {
        let mut pending = self.pending.write().unwrap();
        let mut entry = pending.remove(device_code)?;
        entry.interval_secs = (entry.interval_secs + 5).min(MAX_POLL_INTERVAL_SECS);
        let interval_secs = entry.interval_secs;
        pending.insert(device_code.to_string(), entry);
        Some(interval_secs)
    }

    pub(crate) fn clear(&self) {
        self.pending.write().unwrap().clear();
    }

    pub(crate) fn cancel(&self, device_code: &str) -> bool {
        self.pending.write().unwrap().remove(device_code).is_some()
    }

    /// Serialize persistence with cancellation and concurrent polls. Retain the
    /// exchanged bundle on failure: authorization codes can only be used once.
    pub(crate) fn complete<T, E>(
        &self,
        device_code: &str,
        persist: impl FnOnce() -> Result<T, E>,
    ) -> Option<Result<T, E>> {
        let mut pending = self.pending.write().unwrap();
        let entry = pending.get(device_code)?;
        if entry.bundle.is_none() || entry.expires_at_ms <= now_ms() {
            return None;
        }
        let result = persist();
        if result.is_ok() {
            pending.remove(device_code);
        }
        Some(result)
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
                next_poll_at_ms: 0,
                polling: false,
                bundle: None,
            },
        );
        Ok(challenge)
    }

    /// One poll attempt. `bundle: None` means still pending; `Some(bundle)`
    /// means the user authorized and we exchanged for tokens. On success the
    /// pending entry is kept until the caller persists the bundle and completes, so a persistence failure cannot lose the tokens.
    pub(crate) fn poll(
        &self,
        provider: OAuthProvider,
        device_code: &str,
    ) -> Result<PollOutcome, OAuthError> {
        self.poll_with(provider, device_code, |user_code| match provider {
            OAuthProvider::Codex => codex::poll_for_token(device_code, user_code),
            OAuthProvider::Grok => xai::poll_for_token(device_code),
        })
    }

    fn poll_with(
        &self,
        provider: OAuthProvider,
        device_code: &str,
        fetch: impl FnOnce(&str) -> Result<OAuthTokenBundle, OAuthError>,
    ) -> Result<PollOutcome, OAuthError> {
        let user_code = {
            let mut pending = self.pending.write().unwrap();
            let entry = pending
                .get_mut(device_code)
                .ok_or(OAuthError::ExpiredDeviceCode)?;
            if entry.provider != provider {
                return Err(OAuthError::Parse("device code/provider mismatch".into()));
            }
            if entry.expires_at_ms <= now_ms() {
                pending.remove(device_code);
                return Err(OAuthError::ExpiredDeviceCode);
            }
            if entry.bundle.is_some() || entry.polling || entry.next_poll_at_ms > now_ms() {
                return Ok(PollOutcome {
                    bundle: entry.bundle.clone(),
                    interval_secs: entry.interval_secs,
                });
            }
            entry.polling = true;
            entry.user_code.clone()
        };
        let result = fetch(&user_code);
        let mut pending = self.pending.write().unwrap();
        let entry = pending
            .get_mut(device_code)
            .ok_or(OAuthError::ExpiredDeviceCode)?;
        if entry.expires_at_ms <= now_ms() {
            pending.remove(device_code);
            return Err(OAuthError::ExpiredDeviceCode);
        }
        entry.polling = false;
        if matches!(result, Err(OAuthError::SlowDown)) {
            entry.interval_secs = (entry.interval_secs + 5).min(MAX_POLL_INTERVAL_SECS);
        }
        entry.next_poll_at_ms = now_ms().saturating_add(entry.interval_secs as i64 * 1000);
        match result {
            Ok(bundle) => {
                entry.bundle = Some(bundle.clone());
                Ok(PollOutcome {
                    bundle: Some(bundle),
                    interval_secs: entry.interval_secs,
                })
            }
            Err(OAuthError::AuthorizationPending | OAuthError::SlowDown) => Ok(PollOutcome {
                bundle: None,
                interval_secs: entry.interval_secs,
            }),
            Err(err @ (OAuthError::ExpiredDeviceCode | OAuthError::AccessDenied)) => {
                pending.remove(device_code);
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

/// Bound successful bodies too: token responses contain secrets and must never
/// be echoed by serde errors or allowed to allocate an unlimited body.
pub(crate) fn read_json_response<T: serde::de::DeserializeOwned>(
    response: reqwest::blocking::Response,
) -> Result<T, OAuthError> {
    use std::io::Read;
    let mut bytes = zeroize::Zeroizing::new(Vec::new());
    response
        .take(MAX_OAUTH_RESPONSE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| OAuthError::Network("could not read OAuth response".into()))?;
    if bytes.len() > MAX_OAUTH_RESPONSE_BYTES {
        return Err(OAuthError::Parse(
            "OAuth response exceeds size limit".into(),
        ));
    }
    serde_json::from_slice(&bytes).map_err(|_| OAuthError::Parse("invalid OAuth response".into()))
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
                next_poll_at_ms: 0,
                polling: false,
                bundle: None,
            },
        );
        assert_eq!(manager.bump_interval("code"), Some(10));
        assert_eq!(manager.current_interval("code"), Some(10));
        manager.cancel("code");
        assert_eq!(manager.bump_interval("code"), None);
        assert_eq!(manager.current_interval("code"), None);
    }
    fn pending_manager() -> OAuthManager {
        let manager = OAuthManager::new();
        manager.register(
            "code",
            PendingDeviceCode {
                provider: OAuthProvider::Codex,
                user_code: "user".into(),
                expires_at_ms: now_ms() + 60_000,
                interval_secs: 5,
                next_poll_at_ms: 0,
                polling: false,
                bundle: None,
            },
        );
        manager
    }

    fn tokens() -> OAuthTokenBundle {
        OAuthTokenBundle {
            access_token: "fake-access-secret".into(),
            refresh_token: "fake-refresh-secret".into(),
            id_token: None,
            chatgpt_account_id: Some("workspace".into()),
            account_identity: Some("user".into()),
            expires_in: 3600,
        }
    }

    #[test]
    fn persistence_retry_reuses_exchanged_tokens_and_consumes_once() {
        let manager = pending_manager();
        assert!(manager
            .poll_with(OAuthProvider::Codex, "code", |_| Ok(tokens()))
            .unwrap()
            .bundle
            .is_some());
        assert_eq!(
            manager.complete("code", || Err::<(), _>("disk full")),
            Some(Err("disk full"))
        );
        let cached = manager
            .poll_with(OAuthProvider::Codex, "code", |_| {
                panic!("must not spend code twice")
            })
            .unwrap();
        assert_eq!(cached.bundle.unwrap().access_token, "fake-access-secret");
        assert_eq!(manager.complete("code", || Ok::<_, ()>(())), Some(Ok(())));
        assert_eq!(
            manager.complete("code", || -> Result<(), ()> {
                panic!("must not persist twice")
            }),
            None
        );
    }

    #[test]
    fn cancellation_during_network_discards_tokens_and_prevents_persistence() {
        let manager = pending_manager();
        let result = manager.poll_with(OAuthProvider::Codex, "code", |_| {
            manager.cancel("code");
            Ok(tokens())
        });
        assert!(matches!(result, Err(OAuthError::ExpiredDeviceCode)));
        assert!(manager.complete("code", || Ok::<_, ()>(())).is_none());
    }

    #[test]
    fn overlapping_and_early_polls_do_not_call_the_provider() {
        let manager = pending_manager();
        manager
            .poll_with(OAuthProvider::Codex, "code", |_| {
                assert!(manager
                    .poll_with(OAuthProvider::Codex, "code", |_| panic!("overlapping poll"))
                    .unwrap()
                    .bundle
                    .is_none());
                Err(OAuthError::SlowDown)
            })
            .unwrap();
        let outcome = manager
            .poll_with(OAuthProvider::Codex, "code", |_| panic!("early poll"))
            .unwrap();
        assert!(outcome.bundle.is_none());
        assert_eq!(outcome.interval_secs, 10);
        assert!(matches!(
            manager.poll_with(OAuthProvider::Grok, "code", |_| panic!("wrong provider")),
            Err(OAuthError::Parse(_))
        ));
    }

    #[test]
    fn token_debug_output_is_redacted() {
        assert!(!format!("{:?}", tokens()).contains("secret"));
    }
    #[test]
    fn oauth_response_reads_are_bounded_and_parse_errors_do_not_echo_tokens() {
        use std::io::{Read, Write};
        let response = |body: String| {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let _ = stream.read(&mut [0; 4096]);
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
            });
            let result = reqwest::blocking::Client::builder()
                .no_proxy()
                .build()
                .unwrap()
                .get(format!("http://{address}"))
                .send()
                .unwrap();
            server.join().unwrap();
            result
        };
        let error =
            read_json_response::<u64>(response(r#""fake-token-secret""#.into())).unwrap_err();
        assert!(!error.to_string().contains("fake-token-secret"));
        let error = read_json_response::<serde_json::Value>(response(
            " ".repeat(MAX_OAUTH_RESPONSE_BYTES + 1),
        ))
        .unwrap_err();
        assert!(error.to_string().contains("size limit"));
        assert!(OAuthError::token_fetch_failed(
            "poll",
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            None
        )
        .is_retryable());
    }
}
