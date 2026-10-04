//! Copilot editor grants exchange sessions; CLI grants use their own integration.
use super::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const USER_URL: &str = "https://api.github.com/copilot_internal/user";
const TOKEN_URL: &str = "https://api.github.com/copilot_internal/v2/token";
const REFRESH_LEAD_SECS: u64 = 120;
#[derive(Clone)]
struct Session {
    token: String,
    api: String,
    expires: u64,
    models: Value,
}
impl Drop for Session {
    fn drop(&mut self) {
        self.token.zeroize();
    }
}
#[derive(Default)]
pub(crate) struct Sessions {
    entries: HashMap<[u8; 32], Session>,
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn headers(cli: bool) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (k, v) in [
        ("editor-version", "vscode/1.104.0"),
        ("editor-plugin-version", "copilot-chat/0.31.0"),
        ("copilot-integration-id", "vscode-chat"),
        ("user-agent", "GitHubCopilotChat/0.31.0"),
        ("x-github-api-version", "2025-10-01"),
    ] {
        headers.insert(
            header::HeaderName::from_static(k),
            HeaderValue::from_static(v),
        );
    }
    if cli {
        headers.remove("editor-plugin-version");
        for (key, value) in [
            ("editor-version", "copilot/1.0.88"),
            ("user-agent", "copilot/1.0.88"),
            ("copilot-integration-id", "copilot-developer-cli"),
            ("x-github-api-version", "2026-07-01"),
        ] {
            headers.insert(
                header::HeaderName::from_static(key),
                HeaderValue::from_static(value),
            );
        }
    }
    headers
}
fn trusted_api(value: &str) -> Result<String, String> {
    let url = reqwest::Url::parse(value).map_err(|_| "invalid Copilot API endpoint")?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some_and(|p| p != 443)
        || !url
            .host_str()
            .is_some_and(|h| h == "githubcopilot.com" || h.ends_with(".githubcopilot.com"))
    {
        return Err("Copilot returned an untrusted API endpoint".into());
    }
    Ok(value.trim_end_matches('/').to_owned())
}
async fn read(response: reqwest::Response) -> Result<Value, String> {
    let status = response.status();
    if !status.is_success() {
        return Err(format!("Copilot account request returned HTTP {status}"));
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "Copilot account response failed")?;
        if bytes.len() + chunk.len() > 4 * 1024 * 1024 {
            bytes.zeroize();
            return Err("Copilot account response exceeds limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let parsed =
        serde_json::from_slice(&bytes).map_err(|_| "invalid Copilot account response".into());
    bytes.zeroize();
    parsed
}
fn session_key(token: &str, cli: bool) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update([u8::from(cli)]);
    digest.update(token.as_bytes());
    digest.finalize().into()
}
async fn session(
    state: &RuntimeState,
    client: &reqwest::Client,
    token: &str,
    cli: bool,
) -> Result<Session, String> {
    let key: [u8; 32] = session_key(token, cli);
    cached_session(
        &state.copilot_sessions,
        key,
        exchange_session(client, token, cli),
    )
    .await
}

async fn cached_session(
    cache: &tokio::sync::Mutex<Sessions>,
    key: [u8; 32],
    load: impl std::future::Future<Output = Result<Session, String>>,
) -> Result<Session, String> {
    let mut sessions = cache.lock().await;
    sessions
        .entries
        .retain(|_, s| s.expires > now() + REFRESH_LEAD_SECS);
    if let Some(session) = sessions.entries.get(&key) {
        return Ok(session.clone());
    }
    let session = load.await?;
    if session.expires <= now() {
        return Err("Copilot session expired during model discovery".into());
    }
    if sessions.entries.len() >= 32 {
        sessions.entries.clear();
    }
    sessions.entries.insert(key, session.clone());
    Ok(session)
}

async fn exchange_session(
    client: &reqwest::Client,
    token: &str,
    cli: bool,
) -> Result<Session, String> {
    let mut value = read(
        client
            .get(if cli { USER_URL } else { TOKEN_URL })
            .headers(headers(cli))
            .header(header::AUTHORIZATION, format!("token {token}"))
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(|_| "Copilot session exchange failed")?,
    )
    .await?;
    let access = if cli {
        token.to_owned()
    } else {
        value["token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("Copilot session token missing")?
            .to_owned()
    };
    let api = trusted_api(
        value
            .pointer("/endpoints/api")
            .and_then(Value::as_str)
            .unwrap_or("https://api.githubcopilot.com"),
    )?;
    let expires = if cli {
        now() + 1800
    } else {
        value["expires_at"]
            .as_u64()
            .filter(|v| *v > now())
            .ok_or("Copilot session expiry missing or expired")?
    };
    if let Some(Value::String(token)) = value.get_mut("token") {
        token.zeroize();
    }
    let mut session = Session {
        token: access,
        api,
        expires,
        models: Value::Null,
    };
    session.models = read(
        client
            .get(format!("{}/models", session.api))
            .headers(headers(cli))
            .bearer_auth(&session.token)
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(|_| "Copilot model discovery failed")?,
    )
    .await?;
    if session.expires <= now() {
        return Err("Copilot session expired during model discovery".into());
    }
    Ok(session)
}

pub(crate) fn models(payload: Bytes) -> Result<Bytes, String> {
    let mut root: Value =
        serde_json::from_slice(&payload).map_err(|_| "invalid Copilot model catalog")?;
    let data = root["data"]
        .as_array_mut()
        .ok_or("missing Copilot model list")?;
    if !data.iter().any(|model| model["id"] == "auto") {
        data.insert(
            0,
            json!({"id":"auto","object":"model","owned_by":"github-copilot","name":"Auto"}),
        );
    }
    serde_json::to_vec(&root)
        .map(Bytes::from)
        .map_err(|_| "could not encode Copilot catalog".into())
}

pub(crate) async fn resolve(
    state: &RuntimeState,
    target: &mut ResolvedTarget,
    requested: Option<&str>,
    protocol: ProxyProtocol,
    body: Option<&Value>,
    timeout: u64,
) -> Result<(), String> {
    let client = target_client(state, target, timeout)?;
    let cli = target.upstream_kind == UpstreamKind::CopilotCli;
    let session = session(state, &client, &target.api_key, cli).await?;
    let mut model = requested.map(str::to_owned);
    let mut extra = headers(cli);
    if requested == Some("auto") {
        let value = read(
            client
                .post(format!("{}/models/session", session.api))
                .headers(headers(cli))
                .bearer_auth(&session.token)
                .json(&json!({"auto_mode":{"model_hints":["auto"]}}))
                .timeout(Duration::from_secs(15))
                .send()
                .await
                .map_err(|_| "Copilot Auto session failed")?,
        )
        .await?;
        model = value["selected_model"]
            .as_str()
            .or_else(|| value.pointer("/selected_model/id").and_then(Value::as_str))
            .or_else(|| value.pointer("/available_models/0").and_then(Value::as_str))
            .map(str::to_owned);
        if model.is_none() {
            return Err("Copilot Auto did not select a model".into());
        }
        {
            let token = value["session_token"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or("Copilot Auto session token missing")?;
            let mut value =
                HeaderValue::from_str(token).map_err(|_| "invalid Copilot Auto session")?;
            value.set_sensitive(true);
            extra.insert("copilot-session-token", value);
        }
    }
    if let Some(model) = model.as_deref() {
        let entry = session.models["data"]
            .as_array()
            .and_then(|models| models.iter().find(|m| m["id"] == model));
        if let Some(entry) = entry {
            if entry.pointer("/policy/state").and_then(Value::as_str) == Some("disabled") {
                return Err("Copilot model is disabled by account policy; enable it in GitHub Copilot settings".into());
            }
            if let Some(paths) = entry["supported_endpoints"].as_array() {
                let supported = |p: ProxyProtocol| {
                    paths.iter().any(|v| {
                        v.as_str().is_some_and(|v| {
                            v.trim_start_matches("/v1") == p.path().trim_start_matches("/v1")
                        })
                    })
                };
                target.config.protocol = Some(if supported(protocol) {
                    protocol
                } else {
                    [
                        ProxyProtocol::OpenAiResponses,
                        ProxyProtocol::OpenAiChatCompletions,
                        ProxyProtocol::AnthropicMessages,
                    ]
                    .into_iter()
                    .find(|p| supported(*p))
                    .ok_or("Copilot model has no supported text endpoint")?
                });
            }
        } else if requested != Some("auto") {
            return Err("model is not listed for this Copilot account".into());
        }
    }
    extra.insert(
        "openai-intent",
        HeaderValue::from_static("conversation-panel"),
    );
    let user = body
        .and_then(|b| b["messages"].as_array().and_then(|m| m.last()))
        .is_some_and(|m| {
            m["role"] == "user"
                && !m["content"]
                    .as_array()
                    .is_some_and(|a| a.iter().all(|b| b["type"] == "tool_result"))
        })
        || body.is_some_and(|b| {
            b["input"].is_string()
                || b["input"]
                    .as_array()
                    .and_then(|i| i.last())
                    .is_some_and(|i| i["role"] == "user")
        });
    extra.insert(
        "x-initiator",
        HeaderValue::from_static(if user { "user" } else { "agent" }),
    );
    if body.is_some_and(|b| {
        let s = b.to_string();
        s.contains("image_url") || s.contains("input_image") || s.contains("\"type\":\"image\"")
    }) {
        extra.insert("copilot-vision-request", HeaderValue::from_static("true"));
    }
    for (name, value) in extra.iter() {
        target
            .config
            .headers
            .retain(|(n, _)| !n.eq_ignore_ascii_case(name.as_str()));
        target.config.headers.push((
            name.to_string(),
            value
                .to_str()
                .map_err(|_| "invalid Copilot header")?
                .to_owned(),
        ));
    }
    target.api_key.zeroize();
    target.api_key = session.token.clone();
    target.config.base_url = session.api.clone();
    target.config.auth_scheme = "bearer".into();
    target.model_override = model;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_session(expires: u64) -> Session {
        Session {
            token: "test-session-token".into(),
            api: "https://api.githubcopilot.com".into(),
            expires,
            models: json!({"data":[]}),
        }
    }
    #[tokio::test]
    async fn concurrent_session_refresh_spends_one_exchange_and_separates_accounts() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let cache = tokio::sync::Mutex::new(Sessions::default());
        let calls = AtomicUsize::new(0);
        let futures = (0..8).map(|_| {
            cached_session(&cache, session_key("alice", false), async {
                calls.fetch_add(1, Ordering::Relaxed);
                tokio::task::yield_now().await;
                Ok(test_session(now() + 3600))
            })
        });
        for result in futures_util::future::join_all(futures).await {
            result.unwrap();
        }
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        cached_session(&cache, session_key("bob", false), async {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(test_session(now() + 3600))
        })
        .await
        .unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }
    #[tokio::test]
    async fn sessions_renew_before_expiry_and_never_cache_expired_exchange_results() {
        let cache = tokio::sync::Mutex::new(Sessions::default());
        let key = session_key("alice", false);
        cache
            .lock()
            .await
            .entries
            .insert(key, test_session(now() + 90));
        let fresh = cached_session(&cache, key, async { Ok(test_session(now() + 3600)) })
            .await
            .unwrap();
        assert!(fresh.expires > now() + 120);
        let key = session_key("expired", false);
        assert!(cached_session(&cache, key, async { Ok(test_session(1)) })
            .await
            .is_err());
        assert!(!cache.lock().await.entries.contains_key(&key));
    }
    #[tokio::test]
    async fn account_catalog_controls_model_protocol_and_policy() {
        let route = crate::tests::single_target_route(
            "local",
            "http://127.0.0.1:1".into(),
            RetryPolicy::default(),
        );
        let proxy = crate::tests::start_proxy(crate::tests::available_addr(), route.clone());
        let mut target = route.targets[0].clone();
        target.upstream_kind = UpstreamKind::Copilot;
        let key = session_key(&target.api_key, false);
        proxy.state.copilot_sessions.lock().await.entries.insert(key,Session{token:"session-not-github-token".into(),api:"https://api.individual.githubcopilot.com".into(),expires:now()+3600,models:json!({"data":[{"id":"claude","supported_endpoints":["/v1/messages"],"policy":{"state":"enabled"}},{"id":"blocked","policy":{"state":"disabled"}}]})});
        let original = target.clone();
        resolve(
            &proxy.state,
            &mut target,
            Some("claude"),
            ProxyProtocol::OpenAiChatCompletions,
            Some(&json!({"messages":[{"role":"user","content":"hi"}]})),
            1000,
        )
        .await
        .unwrap();
        assert_eq!(
            target.config.protocol,
            Some(ProxyProtocol::AnthropicMessages)
        );
        assert_eq!(target.api_key, "session-not-github-token");
        assert!(target
            .config
            .headers
            .iter()
            .any(|(k, v)| k == "x-initiator" && v == "user"));
        let mut blocked = original.clone();
        assert!(resolve(
            &proxy.state,
            &mut blocked,
            Some("blocked"),
            ProxyProtocol::OpenAiChatCompletions,
            None,
            1000
        )
        .await
        .unwrap_err()
        .contains("disabled"));
        let mut missing = original;
        assert!(resolve(
            &proxy.state,
            &mut missing,
            Some("unknown"),
            ProxyProtocol::OpenAiChatCompletions,
            None,
            1000
        )
        .await
        .unwrap_err()
        .contains("not listed"));
    }
    #[tokio::test]
    async fn cli_grants_keep_their_own_session_and_client_headers() {
        let route = crate::tests::single_target_route(
            "local",
            "http://127.0.0.1:1".into(),
            RetryPolicy::default(),
        );
        let proxy = crate::tests::start_proxy(crate::tests::available_addr(), route.clone());
        let mut target = route.targets[0].clone();
        target.upstream_kind = UpstreamKind::CopilotCli;
        let key = session_key(&target.api_key, true);
        assert_ne!(key, session_key(&target.api_key, false));
        proxy.state.copilot_sessions.lock().await.entries.insert(
            key,
            Session {
                token: target.api_key.clone(),
                api: "https://api.githubcopilot.com".into(),
                expires: now() + 3600,
                models: json!({"data":[{"id":"model","supported_endpoints":["/responses"]}]}),
            },
        );
        let original = target.api_key.clone();
        resolve(
            &proxy.state,
            &mut target,
            Some("model"),
            ProxyProtocol::OpenAiResponses,
            Some(&json!({"input":[{"type":"function_call_output","call_id":"a","output":"done"}]})),
            1000,
        )
        .await
        .unwrap();
        assert_eq!(target.api_key, original);
        assert!(target
            .config
            .headers
            .iter()
            .any(|(k, v)| k == "copilot-integration-id" && v == "copilot-developer-cli"));
        assert!(target
            .config
            .headers
            .iter()
            .any(|(k, v)| k == "x-initiator" && v == "agent"));
        assert!(!target
            .config
            .headers
            .iter()
            .any(|(k, _)| k == "editor-plugin-version"));
    }

    #[test]
    fn exchange_cannot_redirect_a_github_grant_to_arbitrary_hosts() {
        assert!(trusted_api("https://api.individual.githubcopilot.com").is_ok());
        for url in [
            "http://api.githubcopilot.com",
            "https://api.githubcopilot.com.evil.test",
            "https://evil.test",
            "https://user@api.githubcopilot.com",
        ] {
            assert!(trusted_api(url).is_err());
        }
    }
}
