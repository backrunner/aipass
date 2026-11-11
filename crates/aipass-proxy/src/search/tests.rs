mod chinese_providers;
mod probes;
use probes::assert_no_api_request;
mod websocket;
use super::*;
use crate::tests::{available_addr, single_target_route};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn request(stream: &mut tokio::net::TcpStream) -> (String, Value) {
    let mut data = Vec::new();
    let (end, len) = loop {
        let mut b = [0; 4096];
        let n = stream.read(&mut b).await.unwrap();
        assert!(n > 0);
        data.extend_from_slice(&b[..n]);
        if let Some(end) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&data[..end]);
            let len = headers
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .map(str::to_owned)
                })
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);
            break (end + 4, len);
        }
    };
    while data.len() < end + len {
        let mut b = [0; 4096];
        let n = stream.read(&mut b).await.unwrap();
        assert!(n > 0);
        data.extend_from_slice(&b[..n]);
    }
    (
        String::from_utf8_lossy(&data[..end]).into(),
        serde_json::from_slice(&data[end..end + len]).unwrap_or(Value::Null),
    )
}
async fn respond(stream: &mut tokio::net::TcpStream, status: &str, body: Value) {
    let body = body.to_string();
    stream.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
}
fn source(base: String) -> ResolvedSearchApi {
    ResolvedSearchApi {
        config: SearchApiConfig {
            id: Uuid::new_v4(),
            vendor: SearchVendor::Tavily,
            provider_entry_id: Some(Uuid::new_v4()),
            secret_id: Some("key".into()),
            base_url: Some(base),
            enabled: true,
        },
        api_key: "synthetic-search-key".into(),
    }
}
fn start(mut config: RuntimeConfig) -> (tempfile::TempDir, ProxyHandle, String) {
    let temp = tempfile::tempdir().unwrap();
    let address = available_addr();
    config.bind_addr = address.to_string();
    config.upstream_proxy.mode = UpstreamProxyMode::Direct;
    let usage = Arc::new(UsageStore::open(temp.path().join("usage.sqlite")).unwrap());
    (
        temp,
        ProxyHandle::start(config, usage).unwrap(),
        format!("http://{address}"),
    )
}
fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap()
}

#[test]
fn native_tool_mapping_preserves_caller_tools_and_disabled_selection() {
    for protocol in [
        ProxyProtocol::OpenAiResponses,
        ProxyProtocol::AnthropicMessages,
        ProxyProtocol::OpenAiChatCompletions,
    ] {
        let mut body = match protocol {
            ProxyProtocol::OpenAiResponses => {
                json!({"tools":[{"type":"web_search"},{"type":"function","name":"aipass_web_search"}]})
            }
            ProxyProtocol::AnthropicMessages => {
                json!({"tools":[{"type":"web_search_20250305","name":"web_search"},{"name":"aipass_web_search","input_schema":{}}]})
            }
            _ => {
                json!({"web_search_options":{},"tools":[{"type":"function","function":{"name":"aipass_web_search"}}]})
            }
        };
        assert!(wire::asked(protocol, &body));
        let plan = wire::Plan::prepare(protocol, &mut body).unwrap();
        assert_eq!(plan.name, "aipass_aipass_web_search");
        assert!(plan.enabled);
        assert_eq!(body["tools"].as_array().unwrap().len(), 2);
    }
    let mut body = json!({"tools":[{"type":"web_search"}],"tool_choice":"none"});
    let plan = wire::Plan::prepare(ProxyProtocol::OpenAiResponses, &mut body).unwrap();
    assert!(!plan.enabled);
    assert!(body["tools"].as_array().unwrap().is_empty());
    assert_eq!(body["tool_choice"], "none");
    let mut body =
        json!({"tools":[{"type":"web_search"}],"tool_choice":{"type":"function","name":"other"}});
    assert!(
        !wire::Plan::prepare(ProxyProtocol::OpenAiResponses, &mut body)
            .unwrap()
            .enabled
    );
    let mut body = json!({"tools":[{"type":"web_search","external_web_access":false}]});
    assert!(wire::Plan::prepare(ProxyProtocol::OpenAiResponses, &mut body).is_err());
}
#[test]
fn response_decoders_and_domain_filters_retain_real_sources() {
    for (vendor, value) in [
        (
            SearchVendor::Tavily,
            json!({"results":[{"url":"https://example.com/a","title":"a","content":"fact"}]}),
        ),
        (
            SearchVendor::Brave,
            json!({"web":{"results":[{"url":"https://example.com/a","description":"fact"}]}}),
        ),
        (
            SearchVendor::Exa,
            json!({"results":[{"url":"https://example.com/a","text":"fact"}]}),
        ),
        (
            SearchVendor::Firecrawl,
            json!({"data":{"web":[{"url":"https://example.com/a","description":"fact"}]}}),
        ),
        (
            SearchVendor::You,
            json!({"results":{"web":[{"url":"https://example.com/a","snippets":["fact"]}]}}),
        ),
        (
            SearchVendor::Deepseek,
            json!({"results":[{"url":"https://example.com/a","snippet":"fact"}]}),
        ),
        (
            SearchVendor::Searxng,
            json!({"results":[{"url":"https://example.com/a","content":"fact"}]}),
        ),
    ] {
        let result = api::decode(vendor, &value).unwrap();
        assert_eq!(result["results"][0]["content"], "fact");
    }
    let mut body =
        json!({"tools":[{"type":"web_search","filters":{"allowed_domains":["example.com"]}}]});
    let plan = wire::Plan::prepare(ProxyProtocol::OpenAiResponses, &mut body).unwrap();
    let result=plan.filter(json!({"results":[{"url":"https://a.example.com/a"},{"url":"https://example.com.attacker.test/a"}]}));
    assert_eq!(result["results"].as_array().unwrap().len(), 1);
}
#[tokio::test]
async fn search_mcp_auth_notifications_and_private_fetch_are_enforced() {
    let route = single_target_route(
        "local-search-token",
        "http://127.0.0.1:1".into(),
        RetryPolicy::default(),
    );
    let (_temp, _handle, url) = start(RuntimeConfig::from_routes("", vec![route]));
    let client = client();
    let endpoint = format!("{url}{MCP_PATH}");
    assert_eq!(
        client
            .post(&endpoint)
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .post(&endpoint)
            .bearer_auth("local-search-token")
            .header("Origin", "http://localhost")
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let listed: Value = client
        .post(&endpoint)
        .bearer_auth("local-search-token")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 2);
    let notification=client.post(&endpoint).bearer_auth("local-search-token").json(&json!({"jsonrpc":"2.0","method":"tools/call","params":{"name":"web_search","arguments":{"query":"must not run"}}})).send().await.unwrap();
    assert_eq!(notification.status(), StatusCode::ACCEPTED);
    let fetch:Value=client.post(&endpoint).bearer_auth("local-search-token").json(&json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"web_fetch","arguments":{"url":"http://127.0.0.1/private"}}})).send().await.unwrap().json().await.unwrap();
    assert_eq!(fetch["result"]["isError"], true);
    let body = vec![b' '; 1024 * 1024 + 1];
    assert_eq!(
        client
            .post(&endpoint)
            .bearer_auth("local-search-token")
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
}
#[tokio::test]
async fn responses_to_chat_search_rounds_are_executed_and_history_restored() {
    let search = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let search_url = format!("http://{}", search.local_addr().unwrap());
    let search_task = tokio::spawn(async move {
        let (mut stream, _) = search.accept().await.unwrap();
        let (headers, body) = request(&mut stream).await;
        assert!(headers
            .to_ascii_lowercase()
            .contains("authorization: bearer synthetic-search-key"));
        assert_eq!(body["query"], "latest release");
        respond(&mut stream,"200 OK",json!({"results":[{"title":"release","url":"https://example.com/release","content":"version 2"}]})).await;
    });
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_url = format!("http://{}", upstream.local_addr().unwrap());
    let model_task = tokio::spawn(async move {
        for round in 0..5 {
            let (mut stream, _) = upstream.accept().await.unwrap();
            let (headers, body) = request(&mut stream).await;
            assert!(headers.starts_with("POST /v1/chat/completions "));
            if round < 2 {
                assert_eq!(body["tools"][0]["function"]["name"], "aipass_web_search");
            } else {
                assert!(!body["tools"].to_string().contains("aipass_web_search"));
            }
            if round == 0 {
                respond(&mut stream,"200 OK",json!({"id":"first","object":"chat.completion","model":"test","choices":[{"index":0,"message":{"role":"assistant","content":null,"tool_calls":[{"id":"search_1","type":"function","function":{"name":"aipass_web_search","arguments":"{\"query\":\"latest release\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":10,"completion_tokens":2}})).await;
            } else {
                assert!(body["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|m| m["role"] == "tool" && m["tool_call_id"] == "search_1"));
                respond(&mut stream,"200 OK",json!({"id":format!("final_{round}"),"object":"chat.completion","model":"test","choices":[{"index":0,"message":{"role":"assistant","content":"version 2 https://example.com/release"},"finish_reason":"stop"}],"usage":{"prompt_tokens":20,"completion_tokens":5}})).await;
            }
        }
    });
    let mut route = single_target_route("local-search-token", upstream_url, RetryPolicy::default());
    route.config.upstream_protocol = ProxyProtocol::OpenAiChatCompletions;
    route.config.conversion_enabled = true;
    let mut config = RuntimeConfig::from_routes("", vec![route]);
    let source = source(search_url);
    config.search.apis.push(source.config.clone());
    config.search_apis.push(source);
    let (_temp, handle, url) = start(config);
    let client = client();
    let input = json!([{"role":"user","content":"find release"}]);
    let reply: Value = client
        .post(format!("{url}/v1/responses"))
        .bearer_auth("local-search-token")
        .json(&json!({"model":"test","input":input,"tools":[{"type":"web_search"}],"stream":false}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(reply["status"], "completed", "{reply}");
    assert!(!reply.to_string().contains("aipass_web_search"));
    let mut next = input.as_array().unwrap().clone();
    next.extend(reply["output"].as_array().unwrap().iter().cloned());
    next.push(json!({"role":"user","content":"repeat"}));
    for tools in [
        json!({"tools":[{"type":"web_search"}],"tool_choice":"none"}),
        json!({}),
        json!({"tools":[{"type":"web_search"},{"type":"function","name":"other","parameters":{"type":"object"}}],"tool_choice":{"type":"function","name":"other"}}),
    ] {
        let mut body = json!({"model":"test","input":next,"stream":false});
        body.as_object_mut()
            .unwrap()
            .extend(tools.as_object().unwrap().clone());
        let response = client
            .post(format!("{url}/v1/responses"))
            .bearer_auth("local-search-token")
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let reply: Value = response.json().await.unwrap();
        assert_eq!(reply["status"], "completed", "{reply}");
        assert!(!reply.to_string().contains("aipass_web_search"));
    }
    assert!(handle
        .logs()
        .unwrap_or_default()
        .iter()
        .all(|log| !log.message.contains("latest release")
            && !log.message.contains("synthetic-search-key")));
    search_task.await.unwrap();
    model_task.await.unwrap();
}
#[test]
fn search_history_is_route_scoped_and_revoked_with_its_target() {
    let mut ledger = History::default();
    let route = Uuid::new_v4();
    let target = Uuid::new_v4();
    let protocol = ProxyProtocol::OpenAiChatCompletions;
    let original = json!({"messages":[{"role":"user","content":"hello"}]});
    let private = json!({"messages":[{"role":"user","content":"hello"},{"role":"assistant","content":"internal"}]});
    let response = json!({"choices":[{"message":{"role":"assistant","content":"public"}}]});
    ledger.remember(route, target, protocol, &original, &private, &response);
    let mut next = json!({"messages":[{"role":"user","content":"hello"},{"role":"assistant","content":"public"},{"role":"user","content":"next"}]});
    assert!(ledger
        .restore(Uuid::new_v4(), protocol, &original, &mut next)
        .is_none());
    assert_eq!(
        ledger.restore(route, protocol, &original, &mut next),
        Some(target)
    );
    assert_eq!(next["messages"].as_array().unwrap().len(), 4);
    ledger.retain(&HashSet::new());
    assert!(ledger
        .restore(route, protocol, &original, &mut next)
        .is_none());
}

#[tokio::test]
async fn api_fallback_survives_rejection_and_never_forwards_keys_on_redirect() {
    let first = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let first_url = format!("http://{}", first.local_addr().unwrap());
    let redirect = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let redirect_url = format!("http://{}", redirect.local_addr().unwrap());
    let last = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let last_url = format!("http://{}", last.local_addr().unwrap());
    let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let destination_url = format!("http://{}", destination.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut stream, _) = first.accept().await.unwrap();
        let _ = request(&mut stream).await;
        respond(
            &mut stream,
            "429 Too Many Requests",
            json!({"error":"synthetic-search-key"}),
        )
        .await;
        let (mut stream, _) = redirect.accept().await.unwrap();
        let _ = request(&mut stream).await;
        stream.write_all(format!("HTTP/1.1 302 Found\r\nLocation: {destination_url}/steal\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        let (mut stream, _) = last.accept().await.unwrap();
        let _ = request(&mut stream).await;
        respond(&mut stream,"200 OK",json!({"results":[{"title":"source","url":"https://example.com","content":"verified"}]})).await;
        assert_no_api_request(&destination).await;
    });
    let route = single_target_route(
        "local-search-token",
        "http://127.0.0.1:1".into(),
        RetryPolicy::default(),
    );
    let mut config = RuntimeConfig::from_routes("", vec![route]);
    for url in [first_url, redirect_url, last_url] {
        let source = source(url);
        config.search.apis.push(source.config.clone());
        config.search_apis.push(source);
    }
    let (_temp, handle, url) = start(config);
    let reply:Value=client().post(format!("{url}{MCP_PATH}")).bearer_auth("local-search-token").json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"web_search","arguments":{"query":"test"}}})).send().await.unwrap().json().await.unwrap();
    assert_eq!(
        reply["result"]["structuredContent"]["results"][0]["content"],
        "verified"
    );
    assert!(handle
        .logs()
        .unwrap()
        .iter()
        .all(|l| !l.message.contains("synthetic-search-key")));
    task.await.unwrap();
}
#[tokio::test]
async fn rotating_the_mcp_token_cancels_an_inflight_search() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let source = source(format!("http://{}", listener.local_addr().unwrap()));
    let route = single_target_route(
        "local-search-token",
        "http://127.0.0.1:1".into(),
        RetryPolicy::default(),
    );
    let mut config = RuntimeConfig::from_routes("", vec![route]);
    config.search.apis.push(source.config.clone());
    config.search_apis.push(source);
    let (_temp, handle, url) = start(config.clone());
    let call = tokio::spawn(async move {
        client().post(format!("{url}{MCP_PATH}")).bearer_auth("local-search-token").json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"web_search","arguments":{"query":"test"}}})).send().await.unwrap()
    });
    let (mut stream, _) = listener.accept().await.unwrap();
    let _ = request(&mut stream).await;
    config.bind_addr = handle.bind_addr.clone();
    config.routes[0].local_token = "rotated".into();
    handle.update_config(config).unwrap();
    assert_eq!(
        call.await.unwrap().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn configured_native_search_model_uses_the_subscription_backend() {
    struct Backend(std::sync::atomic::AtomicU64);
    impl SubscriptionBackend for Backend {
        fn request(
            &self,
            _target: ResolvedTarget,
            payload: Value,
            _session: Option<String>,
        ) -> SubscriptionFuture {
            self.0.fetch_add(1, Ordering::Relaxed);
            assert_eq!(payload["tools"][0]["type"], "web_search_20250305");
            assert_eq!(payload["model"], "search-model");
            Box::pin(async {
                let bytes=Bytes::from(json!({"id":"msg_search","type":"message","role":"assistant","model":"search-model","content":[{"type":"text","text":"Retrieved fact https://example.com/source"}],"stop_reason":"end_turn","usage":{"input_tokens":12,"output_tokens":6,"server_tool_use":{"web_search_requests":1}}}).to_string());
                let mut headers = HeaderMap::new();
                headers.insert(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json"),
                );
                Ok(SubscriptionResponse {
                    status: StatusCode::OK,
                    headers,
                    body: Box::pin(stream::once(async move { Ok(bytes) })),
                })
            })
        }
        fn revoke(&self) {}
    }
    let mut route = single_target_route(
        "local-search-token",
        "https://api.anthropic.com".into(),
        RetryPolicy::default(),
    );
    route.config.inbound_protocol = ProxyProtocol::AnthropicMessages;
    route.config.upstream_protocol = ProxyProtocol::AnthropicMessages;
    route.targets[0].upstream_kind = UpstreamKind::ClaudeSubscription;
    let mut config = RuntimeConfig::from_routes("", vec![route.clone()]);
    config.search.model = Some(SearchModelConfig {
        route_id: route.config.id,
        model: "search-model".into(),
    });
    let (_temp, handle, url) = start(config);
    let backend = Arc::new(Backend(std::sync::atomic::AtomicU64::new(0)));
    handle.set_subscription_backend(backend.clone());
    let result:Value=client().post(format!("{url}{MCP_PATH}")).bearer_auth("local-search-token").json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"web_search","arguments":{"query":"new information"}}})).send().await.unwrap().json().await.unwrap();
    assert_eq!(result["result"]["isError"], false, "{result}");
    assert!(result.to_string().contains("https://example.com/source"));
    assert_eq!(backend.0.load(Ordering::Relaxed), 1);
}

#[test]
fn absent_search_and_unsupported_execution_constraints_do_not_grant_search() {
    assert!(!wire::asked(
        ProxyProtocol::OpenAiChatCompletions,
        &json!({"messages":[],"web_search_options":null})
    ));
    let mut body = json!({"tools":[{"type":"web_search_20250305","name":"web_search","allowed_callers":["code_execution_20260120"]}]});
    assert!(wire::Plan::prepare(ProxyProtocol::AnthropicMessages, &mut body).is_err());
    let mut body = json!({"web_search_options":[]});
    assert!(wire::Plan::prepare(ProxyProtocol::OpenAiChatCompletions, &mut body).is_err());
    for filters in [
        json!("example.com"),
        json!({"blocked_domains":["example.com"]}),
    ] {
        let mut body = json!({"tools":[{"type":"web_search","filters":filters}]});
        assert!(wire::Plan::prepare(ProxyProtocol::OpenAiResponses, &mut body).is_err());
    }
    let mut body = json!({"web_search_options":{"unknown_constraint":true}});
    assert!(wire::Plan::prepare(ProxyProtocol::OpenAiChatCompletions, &mut body).is_err());
}
