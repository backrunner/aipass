use super::*;
use serde_json::{json, Value};

#[test]
fn gemini_native_generation_uses_native_path_auth_and_normalizes_response() {
    let (address,upstream)=mock_upstream("200 OK","application/json",json!({"modelVersion":"gemini-3","candidates":[{"content":{"parts":[{"text":"answer"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":2,"thoughtsTokenCount":3}}).to_string());
    let mut target = conversion_target(
        format!("http://{address}"),
        0,
        ProxyProtocol::OpenAiChatCompletions,
    );
    target.upstream_kind = UpstreamKind::GeminiNative;
    target.config.auth_scheme = "google_api_key".into();
    let bind = available_addr();
    let token = "gemini-route";
    let mut route = conversion_route(
        token,
        ProxyProtocol::OpenAiChatCompletions,
        ProxyProtocol::OpenAiChatCompletions,
        vec![target],
        1,
    );
    route.config.conversion_enabled = false;
    let _proxy = start_proxy(bind, route);
    let response=reqwest::blocking::Client::builder().no_proxy().build().unwrap().post(format!("http://{bind}/v1/chat/completions")).bearer_auth(token).json(&json!({"model":"gemini-3","messages":[{"role":"system","content":"rules"},{"role":"user","content":"question"}],"reasoning_effort":"low"})).send().unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value = response.json().unwrap();
    assert_eq!(value["choices"][0]["message"]["content"], "answer");
    assert_eq!(value["usage"]["completion_tokens"], 5);
    let (headers, body) = upstream.join().unwrap();
    let headers = headers.to_ascii_lowercase();
    assert!(headers.starts_with("post /v1beta/models/gemini-3:generatecontent "));
    assert!(headers.contains("x-goog-api-key: upstream-secret"));
    assert!(!headers.contains(token));
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["systemInstruction"]["parts"][0]["text"], "rules");
    assert_eq!(
        body["generationConfig"]["thinkingConfig"]["thinkingLevel"],
        "low"
    );
}

#[test]
fn quota_order_excludes_only_fresh_exhaustion_and_affinity_stays_eligible() {
    let now = now_unix() as u64;
    let mut a = test_target("http://127.0.0.1:1".into(), 0);
    let mut b = test_target("http://127.0.0.1:2".into(), 1);
    let mut c = test_target("http://127.0.0.1:3".into(), 2);
    a.quota = vec![QuotaWindow {
        models: None,
        not_models: Vec::new(),
        used_basis_points: 10_000,
        observed_at: now,
        resets_at: Some(now + 100),
    }];
    b.quota = vec![QuotaWindow {
        models: None,
        not_models: Vec::new(),
        used_basis_points: 9_000,
        observed_at: now,
        resets_at: None,
    }];
    c.quota = vec![QuotaWindow {
        models: None,
        not_models: Vec::new(),
        used_basis_points: 1_000,
        observed_at: now,
        resets_at: None,
    }];
    let mut route = conversion_route(
        "quota",
        ProxyProtocol::OpenAiChatCompletions,
        ProxyProtocol::OpenAiChatCompletions,
        vec![a.clone(), b.clone(), c.clone()],
        3,
    );
    route.config.strategy = RouteStrategy::QuotaAware;
    let proxy = start_proxy(available_addr(), route.clone());
    remember_affinity_target(&proxy.state, route.config.id, Some("s"), a.config.id);
    assert_eq!(
        ordered_route_targets(&proxy.state, &route, Some("s"))
            .iter()
            .map(|t| t.config.id)
            .collect::<Vec<_>>(),
        vec![c.config.id, b.config.id]
    );
    remember_affinity_target(&proxy.state, route.config.id, Some("s"), b.config.id);
    assert_eq!(
        ordered_route_targets(&proxy.state, &route, Some("s"))[0]
            .config
            .id,
        b.config.id
    );
    route.targets[0].quota[0].observed_at = now - 301;
    assert!(ordered_route_targets(&proxy.state, &route, None)
        .iter()
        .any(|t| t.config.id == a.config.id));
    route.targets[0].quota[0].observed_at = now;
    route.targets[0].quota[0].resets_at = Some(now);
    assert!(ordered_route_targets(&proxy.state, &route, None)
        .iter()
        .any(|t| t.config.id == a.config.id));
}

#[test]
fn quota_scope_follows_the_effective_model_and_preserves_unrelated_models() {
    let now = now_unix() as u64;
    let mut target = test_target("http://127.0.0.1:1".into(), 0);
    target.quota = vec![
        QuotaWindow {
            models: Some(vec!["premium".into()]),
            not_models: vec![],
            used_basis_points: 10_000,
            observed_at: now,
            resets_at: None,
        },
        QuotaWindow {
            models: None,
            not_models: vec!["premium".into()],
            used_basis_points: 2_000,
            observed_at: now,
            resets_at: None,
        },
    ];
    assert_eq!(target.quota_usage(now, Some("PREMIUM")), Some(10_000));
    assert_eq!(target.quota_usage(now, Some("standard")), Some(2_000));
    assert_eq!(target.quota_usage(now, None), None);
    target.model_override = Some("premium".into());
    assert_eq!(target.quota_usage(now, Some("standard")), Some(10_000));
    target.model_override = None;
    let mut route = conversion_route(
        "quota-scope",
        ProxyProtocol::OpenAiChatCompletions,
        ProxyProtocol::OpenAiChatCompletions,
        vec![target],
        1,
    );
    route.config.strategy = RouteStrategy::QuotaAware;
    let proxy = start_proxy(available_addr(), route.clone());
    assert!(crate::selection::ordered_route_targets_for_model(
        &proxy.state,
        &route,
        None,
        Some("premium")
    )
    .is_empty());
    assert_eq!(
        crate::selection::ordered_route_targets_for_model(
            &proxy.state,
            &route,
            None,
            Some("standard")
        )
        .len(),
        1
    );
}

#[test]
fn rate_limit_retry_after_is_temporary_and_does_not_change_quota() {
    let route = single_target_route("rate", "http://127.0.0.1:1".into(), RetryPolicy::default());
    let target = route.targets[0].config.id;
    let proxy = start_proxy(available_addr(), route.clone());
    let mut headers = HeaderMap::new();
    headers.insert(header::RETRY_AFTER, HeaderValue::from_static("120"));
    mark_rate_limited(&proxy.state, target, &headers, &route.config.retry);
    assert!(ordered_route_targets(&proxy.state, &route, None).is_empty());
    assert!(proxy.state.config.read().unwrap().routes[0].targets[0]
        .quota
        .is_empty());
    proxy
        .state
        .health
        .lock()
        .unwrap()
        .get_mut(&target)
        .unwrap()
        .open_until = Some(Instant::now() - Duration::from_secs(1));
    assert_eq!(ordered_route_targets(&proxy.state, &route, None).len(), 1);
}
