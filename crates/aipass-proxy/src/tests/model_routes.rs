use super::*;
use serde_json::{json, Value};

fn bound_route(targets: Vec<ResolvedTarget>) -> ResolvedRoute {
    conversion_route(
        "group-token",
        ProxyProtocol::OpenAiChatCompletions,
        ProxyProtocol::OpenAiChatCompletions,
        targets,
        3,
    )
}

fn request(bind: &str, body: Value) -> reqwest::blocking::Response {
    reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
        .post(format!("http://{bind}/v1/chat/completions"))
        .bearer_auth("group-token")
        .json(&body)
        .send()
        .unwrap()
}

#[test]
fn group_alias_maps_each_fallback_to_its_actual_model_and_protocol() {
    let (a, first) = mock_upstream(
        "429 Too Many Requests",
        "application/json",
        "{\"error\":\"limited\"}".into(),
    );
    let (b, second) = mock_upstream("200 OK", "application/json", json!({"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[{"type":"text","text":"answer"}],"stop_reason":"end_turn","usage":{"input_tokens":2,"output_tokens":1}}).to_string());
    let mut codex = conversion_target(format!("http://{a}"), 0, ProxyProtocol::OpenAiResponses);
    codex.config.model = Some("gpt-test".into());
    let mut claude = conversion_target(format!("http://{b}"), 1, ProxyProtocol::AnthropicMessages);
    claude.config.model = Some("claude-test".into());
    let route = bound_route(vec![codex, claude]);
    let model = crate::model_routes::model_id(&route).unwrap();
    let proxy = start_proxy(available_addr(), route);
    let response = request(
        &proxy.bind_addr,
        json!({"model":model,"messages":[{"role":"user","content":"hello"}],"max_tokens":100}),
    );
    assert_eq!(response.status(), StatusCode::OK);
    let result: Value = response.json().unwrap();
    assert_eq!(result["choices"][0]["message"]["content"], "answer");
    let (headers, body) = first.join().unwrap();
    assert!(headers.starts_with("POST /v1/responses "));
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["model"],
        "gpt-test"
    );
    let (headers, body) = second.join().unwrap();
    assert!(headers.starts_with("POST /v1/messages "));
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["model"],
        "claude-test"
    );
}

struct SubscriptionCapture(Arc<Mutex<Vec<Value>>>);
impl SubscriptionBackend for SubscriptionCapture {
    fn request(
        &self,
        _target: ResolvedTarget,
        payload: Value,
        _session: Option<String>,
    ) -> SubscriptionFuture {
        self.0.lock().unwrap().push(payload);
        Box::pin(async {
            Ok(SubscriptionResponse {
                status: StatusCode::OK,
                headers: HeaderMap::new(),
                body: Box::pin(stream::once(async {
                    Ok(Bytes::from(json!({"id":"chat_sub","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"subscription"},"finish_reason":"stop"}]}).to_string()))
                })),
            })
        })
    }
    fn revoke(&self) {}
}

#[test]
fn api_refusal_falls_back_to_subscription_using_its_bound_model() {
    let (a, upstream) = mock_upstream(
        "403 Forbidden",
        "application/json",
        "{\"error\":\"balance\"}".into(),
    );
    let mut api = test_target(format!("http://{a}"), 0);
    api.config.protocol = Some(ProxyProtocol::OpenAiChatCompletions);
    api.config.model = Some("api-model".into());
    let mut subscription = test_target("https://unused.invalid".into(), 1);
    subscription.config.protocol = Some(ProxyProtocol::OpenAiChatCompletions);
    subscription.config.model = Some("grok-build-model".into());
    subscription.upstream_kind = UpstreamKind::CommunitySubscription;
    let route = bound_route(vec![api, subscription]);
    let model = crate::model_routes::model_id(&route).unwrap();
    let proxy = start_proxy(available_addr(), route);
    let captured = Arc::new(Mutex::new(Vec::new()));
    *proxy.state.subscription_backend.write().unwrap() =
        Some(Arc::new(SubscriptionCapture(captured.clone())));
    let response = request(
        &proxy.bind_addr,
        json!({"model":model,"messages":[{"role":"user","content":"hello"}]}),
    );
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(captured.lock().unwrap()[0]["model"], "grok-build-model");
    assert_eq!(
        serde_json::from_slice::<Value>(&upstream.join().unwrap().1).unwrap()["model"],
        "api-model"
    );
}

#[test]
fn discovery_serves_only_group_alias_without_an_upstream_request() {
    let mut target = test_target("http://127.0.0.1:1".into(), 0);
    target.config.model = Some("bound-model".into());
    let route = bound_route(vec![target]);
    let model = crate::model_routes::model_id(&route).unwrap();
    let proxy = start_proxy(available_addr(), route);
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .build()
        .unwrap();
    let response = client
        .get(format!("http://{}/v1/models", proxy.bind_addr))
        .bearer_auth("group-token")
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let data: Value = response.json().unwrap();
    assert_eq!(data["data"].as_array().unwrap().len(), 1);
    assert_eq!(data["data"][0]["id"], model);
    assert!(data["data"][0].get("capabilities").is_none());
    assert_eq!(
        request(&proxy.bind_addr, json!({"model":"wrong","messages":[]})).status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            &proxy.bind_addr,
            json!({"model":model,"previous_response_id":"unknown","messages":[]})
        )
        .status(),
        StatusCode::BAD_REQUEST
    );
}

#[test]
fn messages_group_discovery_uses_the_native_anthropic_catalog_shape() {
    let mut target = conversion_target(
        "http://127.0.0.1:1".into(),
        0,
        ProxyProtocol::AnthropicMessages,
    );
    target.config.model = Some("claude-model".into());
    let mut route = bound_route(vec![target]);
    route.config.inbound_protocol = ProxyProtocol::AnthropicMessages;
    let proxy = start_proxy(available_addr(), route);
    let response = reqwest::blocking::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!("http://{}/v1/models", proxy.bind_addr))
        .header("x-api-key", "group-token")
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = response.json().unwrap();
    assert_eq!(payload["data"][0]["type"], "model");
    assert_eq!(payload["data"][0]["display_name"], payload["data"][0]["id"]);
    assert!(payload["data"][0]["created_at"].is_string());
    assert_eq!(payload["has_more"], false);
}

#[test]
fn binding_controls_quota_and_invalidates_cached_target_identity() {
    let mut target = test_target("http://127.0.0.1:1".into(), 0);
    target.config.model = Some("premium".into());
    target.quota = vec![QuotaWindow {
        models: Some(vec!["premium".into()]),
        not_models: vec![],
        used_basis_points: 10_000,
        observed_at: now_unix() as u64,
        resets_at: None,
    }];
    let route = bound_route(vec![target]);
    let proxy = start_proxy(available_addr(), route.clone());
    assert!(
        ordered_route_targets_for_model(&proxy.state, &route, None, Some("group/alias")).is_empty()
    );
    let old = RuntimeConfig::from_routes(proxy.bind_addr.to_string(), vec![route.clone()]);
    let mut new = old.clone();
    assert_eq!(preserved_targets(&old, &new).len(), 1);
    new.routes[0].targets[0].config.model = Some("standard".into());
    assert!(preserved_targets(&old, &new).is_empty());
    assert_ne!(
        websocket_config_key(&old.routes[0].targets[0], &old.upstream_proxy),
        websocket_config_key(&new.routes[0].targets[0], &new.upstream_proxy)
    );
    assert_eq!(
        ordered_route_targets_for_model(&proxy.state, &new.routes[0], None, Some("group/alias"))
            .len(),
        1
    );
}

#[test]
fn opaque_history_retains_recorded_origin_even_when_unavailable() {
    let mut target = test_target("http://127.0.0.1:1".into(), 0);
    target.config.model = Some("model".into());
    let route = bound_route(vec![target]);
    let owner = route.targets[0].config.id;
    let proxy = start_proxy(available_addr(), route.clone());
    let payload = json!({"input":[{"type":"reasoning","encrypted_content":"opaque"}]});
    assert!(crate::model_routes::history_owner(
        &proxy.state,
        &route,
        &payload,
        Some("session"),
        None
    )
    .is_err());
    remember_affinity_target(&proxy.state, route.config.id, Some("session"), owner);
    assert_eq!(
        crate::model_routes::history_owner(&proxy.state, &route, &payload, Some("session"), None)
            .unwrap(),
        Some(owner)
    );
    assert!(crate::model_routes::history_owner(
        &proxy.state,
        &route,
        &payload,
        Some("session"),
        Some(Uuid::new_v4())
    )
    .is_err());
}

#[test]
fn fixed_model_mapping_survives_spooled_native_requests() {
    let (address, upstream) = mock_upstream(
        "200 OK",
        "application/json",
        "{\"id\":\"resp_large\",\"status\":\"completed\",\"output\":[]}".into(),
    );
    let mut route = single_target_route(
        "large-model",
        format!("http://{address}"),
        RetryPolicy::default(),
    );
    route.targets[0].config.model = Some("actual-model".into());
    let alias = crate::model_routes::model_id(&route).unwrap();
    let bind = available_addr();
    let _proxy = start_proxy(bind, route);
    let text = "x".repeat(REQUEST_BODY_MEMORY_THRESHOLD + 1);
    let response = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap()
        .post(format!("http://{bind}/v1/responses"))
        .bearer_auth("large-model")
        .json(&json!({"model":alias,"input":text}))
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&upstream.join().unwrap().1).unwrap();
    assert_eq!(body["model"], "actual-model");
    assert_eq!(body["input"], text);
}

#[test]
fn user_tool_schemas_and_json_content_do_not_become_provider_bound_state() {
    let mut target = test_target("http://127.0.0.1:1".into(), 0);
    target.config.model = Some("model".into());
    let route = bound_route(vec![target]);
    let proxy = start_proxy(available_addr(), route.clone());
    let payload = json!({"tools":[{"type":"function","parameters":{"properties":{"signature":{"type":"string"}}}}],"messages":[{"role":"user","content":[{"type":"text","text":"{\"encrypted_content\":\"user data\"}"}]}]});
    assert_eq!(
        crate::model_routes::history_owner(&proxy.state, &route, &payload, None, None).unwrap(),
        None
    );
}
