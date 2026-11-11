//! Public HTTP route -> conversion -> real bridge -> synthetic native process.
use super::*;
use aipass_proxy::{
    Protocol, ProxyHandle, ProxyRouteConfig, QuotaWindow, ResolvedRoute, RetryPolicy,
    RouteStrategy, RuntimeConfig, UpstreamKind, UsageStore,
};
use aipass_proxy_conversion::{BuiltinConversionPlugin, ConversionPlugin, StreamConverter};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const SCRIPT: &str = r#"case " $* " in *" --tools default "*) ;; *) exit 92;; esac
printf '%s\n' "$$" >> starts
turn=0
while read -r prompt; do
turn=$((turn+1))
printf '%s\n' "$prompt" >> prompts
if [ -f background-pending ]; then
cat <<'EVENTS'
{"type":"system","subtype":"background_tasks_changed","tasks":[]}
{"type":"system","subtype":"task_notification","task_id":"bg","status":"completed","summary":"background work completed"}
EVENTS
rm background-pending
fi
if [ ! -f permitted ]; then
cat <<'EVENTS'
{"type":"assistant","message":{"id":"write","role":"assistant","content":[{"type":"text","text":"Checking the request."},{"type":"text","text":"Preparing the write."},{"type":"tool_use","id":"native-write","name":"Write","input":{"file_path":"demo","content":"fixture"}}],"stop_reason":"tool_use","usage":{"input_tokens":2,"output_tokens":1}}}
{"type":"control_request","request_id":"permission","request":{"subtype":"can_use_tool","tool_use_id":"native-write","tool_name":"Write","input":{"file_path":"demo","content":"fixture"}}}
EVENTS
read -r reply
printf '%s\n' "$reply" > permitted
printf '%s\n' '{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"native-write","content":"synthetic file write completed"}]}}'
touch background-pending
printf '%s\n' '{"type":"system","subtype":"background_tasks_changed","tasks":[{"task_id":"bg","task_type":"local_bash"}]}'
fi
printf '{"type":"assistant","message":{"id":"answer-%s","role":"assistant","content":[{"type":"text","text":"Claude answer"}],"stop_reason":"end_turn","usage":{"input_tokens":3,"output_tokens":2}}}\n' "$turn"
printf '%s\n' '{"type":"result","is_error":false}'
done
"#;

async fn upstream(response: Value) -> (String, tokio::task::JoinHandle<(String, Value)>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut socket, bytes, header_end, headers) = loop {
            let (mut socket, _) = tokio::time::timeout(Duration::from_secs(15), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut bytes = Vec::new();
            let header_end = loop {
                let mut chunk = [0; 4096];
                let n = socket.read(&mut chunk).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&chunk[..n]);
                if let Some(i) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    break i + 4;
                }
            };
            let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
            if headers.starts_with("GET / HTTP/") {
                socket.write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
                continue;
            }
            break (socket, bytes, header_end, headers);
        };
        let mut bytes = bytes;
        let length: usize = headers
            .lines()
            .find_map(|l| {
                l.split_once(':')
                    .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                    .map(|(_, v)| v.trim().parse().unwrap())
            })
            .unwrap();
        while bytes.len() < header_end + length {
            let mut chunk = [0; 4096];
            let n = socket.read(&mut chunk).await.unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&chunk[..n]);
        }
        let body = serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap();
        // The mock honors the native wire and requested stream mode.
        let payload: Value =
            serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap();
        let streaming = payload["stream"] == true
            || headers
                .lines()
                .next()
                .unwrap()
                .contains("streamGenerateContent");
        let (kind, response) = if streaming {
            let events = if response["object"] == "response" {
                vec![
                    json!({"type":"response.created","response":{"id":"openai","object":"response","model":"openai-model","output":[]}}),
                    json!({"type":"response.output_item.done","output_index":0,"item":response["output"][0]}),
                    json!({"type":"response.completed","response":response}),
                ]
            } else {
                vec![response]
            };
            (
                "text/event-stream",
                events
                    .iter()
                    .map(|e| format!("data: {e}\n\n"))
                    .collect::<String>(),
            )
        } else {
            ("application/json", response.to_string())
        };
        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",response.len()).as_bytes()).await.unwrap();
        (headers, body)
    });
    (url, task)
}

fn route(
    protocol: Protocol,
    mut claude: ResolvedTarget,
    openai: String,
    gemini: String,
) -> ResolvedRoute {
    claude.config.protocol = Some(Protocol::AnthropicMessages);
    claude.config.model = Some("claude-fixture".into());
    let mut api = claude.clone();
    api.config.id = Uuid::new_v4();
    api.config.provider_entry_id = Uuid::new_v4();
    api.config.base_url = openai;
    api.config.protocol = Some(Protocol::OpenAiResponses);
    api.config.model = Some("openai-model".into());
    api.config.priority = 1;
    api.upstream_kind = UpstreamKind::Standard;
    let mut google = api.clone();
    google.config.id = Uuid::new_v4();
    google.config.provider_entry_id = Uuid::new_v4();
    google.config.base_url = gemini;
    google.config.protocol = Some(Protocol::OpenAiChatCompletions);
    google.config.model = Some("gemini-fixture".into());
    google.config.priority = 2;
    google.upstream_kind = UpstreamKind::GeminiNative;
    ResolvedRoute {
        config: ProxyRouteConfig {
            id: Uuid::new_v4(),
            name: "mixed".into(),
            token: String::new(),
            inbound_protocol: protocol,
            upstream_protocol: Protocol::AnthropicMessages,
            conversion_enabled: true,
            strategy: RouteStrategy::Fallback,
            targets: Vec::new(),
            retry: RetryPolicy {
                max_attempts: 3,
                ..Default::default()
            },
            enabled: true,
        },
        local_token: "synthetic-proxy-token".into(),
        targets: vec![claude, api, google],
    }
}

#[tokio::test]
async fn claude_preflight_incompatibility_falls_back_without_spawning_or_spending_an_attempt() {
    for protocol in [
        Protocol::AnthropicMessages,
        Protocol::OpenAiChatCompletions,
        Protocol::OpenAiResponses,
    ] {
        for streaming in [false, true] {
            for control in [
                json!({"temperature":0.2}),
                json!({"tool_choice":{"type":"auto","disable_parallel_tool_use":true}}),
                json!({"tools":[{"name":"lookup","input_schema":{"oneOf":[{"type":"object","properties":{"code":{"type":"string"}},"required":["code"]},{"type":"object","properties":{"summary":{"type":"string"}},"required":["summary"]}]}}]}),
            ] {
                let (dir, bridge, target) = fixture("touch unexpectedly-started\nexit 99");
                let (url, upstream_task) = upstream(json!({"id":"openai","object":"response","model":"openai-model","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"compatible answer"}]}]})).await;
                let mut route = route(protocol, target, url, "http://127.0.0.1:1".into());
                route.targets.truncate(2);
                route.config.retry.max_attempts = 1;
                let claude_id = route.targets[0].config.id;
                let usage = Arc::new(UsageStore::open(dir.path().join("usage.sqlite")).unwrap());
                let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
                let bind = listener.local_addr().unwrap().to_string();
                drop(listener);
                let proxy = ProxyHandle::start(
                    RuntimeConfig::from_routes(&bind, vec![route.clone()]),
                    usage.clone(),
                )
                .unwrap();
                proxy.set_subscription_backend(Arc::new(crate::community::Dispatch {
                    claude: Arc::new(bridge.clone()),
                    community: Arc::new(crate::community::CommunityBridge::new(dir.path())),
                }));
                let mut payload = json!({"model":format!("group/{}",route.config.id),"stream":streaming,"messages":[{"role":"user","content":"go"}],"tools":[{"name":"lookup","input_schema":{"type":"object"}}]});
                payload
                    .as_object_mut()
                    .unwrap()
                    .extend(control.as_object().unwrap().clone());
                let payload = BuiltinConversionPlugin
                    .convert_request(Protocol::AnthropicMessages, protocol, payload)
                    .unwrap();
                let response = reqwest::Client::builder()
                    .no_proxy()
                    .timeout(Duration::from_secs(5))
                    .build()
                    .unwrap()
                    .post(format!("http://{bind}{}", protocol.path()))
                    .bearer_auth("synthetic-proxy-token")
                    .json(&payload)
                    .send()
                    .await
                    .unwrap();
                let answer = message(response, protocol, streaming).await;
                assert!(answer.to_string().contains("compatible answer"));
                let (_, forwarded) = upstream_task.await.unwrap();
                assert_eq!(forwarded["model"], "openai-model");
                if control.get("temperature").is_some() {
                    assert_eq!(forwarded["temperature"], 0.2);
                } else if control.get("tools").is_some() {
                    assert_eq!(
                        forwarded["tools"][0]["parameters"],
                        control["tools"][0]["input_schema"]
                    );
                } else {
                    assert_eq!(forwarded["parallel_tool_calls"], false);
                }
                assert!(bridge.inner.runs.lock().unwrap().is_empty());
                assert!(!dir.path().join("unexpectedly-started").exists());
                assert!(!proxy.status().degraded_target_ids.contains(&claude_id));
                let summary = usage.summary(|_| 0).unwrap();
                assert_eq!(summary.attempt_count, 1);
                assert_eq!(summary.successful_attempts, 1);
            }
        }
    }
}
fn select(proxy: &ProxyHandle, route: &mut ResolvedRoute, bind: &str, index: usize) {
    for (i, target) in route.targets.iter_mut().enumerate() {
        target.quota = if i == index {
            Vec::new()
        } else {
            vec![QuotaWindow {
                model_family: None,
                models: None,
                not_models: Vec::new(),
                used_basis_points: 10000,
                span_seconds: None,
                observed_at: time::OffsetDateTime::now_utc().unix_timestamp() as u64,
                resets_at: None,
            }]
        };
    }
    proxy
        .update_config(RuntimeConfig::from_routes(bind, vec![route.clone()]))
        .unwrap();
}
async fn send(
    bind: &str,
    route: &ResolvedRoute,
    cwd: &Path,
    messages: &[Value],
    streaming: bool,
) -> reqwest::Response {
    let protocol = route.config.inbound_protocol;
    let payload=BuiltinConversionPlugin.convert_request(Protocol::AnthropicMessages,protocol,json!({"model":format!("group/{}",route.config.id),"max_tokens":1000,"messages":messages,"stream":streaming})).unwrap();
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap()
        .post(format!("http://{bind}{}", protocol.path()))
        .bearer_auth("synthetic-proxy-token")
        .header("x-api-key", "synthetic-proxy-token")
        .header("x-aipass-session-id", "mixed-session")
        .header(
            "x-aipass-claude-session",
            json!({"cwd":cwd,"permissionMode":"default"}).to_string(),
        )
        .json(&payload)
        .send()
        .await
        .unwrap()
}
async fn message(response: reqwest::Response, protocol: Protocol, streaming: bool) -> Value {
    let status = response.status();
    let bytes = response.bytes().await.unwrap();
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&bytes));
    if !streaming {
        return BuiltinConversionPlugin
            .convert_response(
                protocol,
                Protocol::AnthropicMessages,
                serde_json::from_slice(&bytes).unwrap(),
            )
            .unwrap();
    }
    if protocol == Protocol::OpenAiResponses {
        // Responses clients replay the actual output items. Converting through
        // Chat here would discard their message/part boundaries in the fixture.
        let completed = std::str::from_utf8(&bytes)
            .unwrap()
            .lines()
            .filter_map(|l| l.strip_prefix("data: "))
            .filter_map(|s| serde_json::from_str::<Value>(s).ok())
            .find(|e| e["type"] == "response.completed")
            .expect("completed Responses stream");
        return BuiltinConversionPlugin
            .convert_response(
                protocol,
                Protocol::AnthropicMessages,
                completed["response"].clone(),
            )
            .unwrap();
    }
    let mut converter = StreamConverter::new(protocol, Protocol::AnthropicMessages).unwrap();
    let mut collector = Collector::default();
    for event in std::str::from_utf8(&bytes)
        .unwrap()
        .split("\n\n")
        .filter(|s| !s.trim().is_empty())
    {
        for converted in converter.push_event(event).unwrap() {
            collector.push(converted.as_bytes()).unwrap();
        }
    }
    for converted in converter.finish().unwrap() {
        collector.push(converted.as_bytes()).unwrap();
    }
    assert!(collector.complete, "{}", String::from_utf8_lossy(&bytes));
    collector.message
}

#[tokio::test]
async fn mixed_group_preserves_permission_and_context_across_claude_openai_gemini_and_back() {
    for protocol in [
        Protocol::AnthropicMessages,
        Protocol::OpenAiChatCompletions,
        Protocol::OpenAiResponses,
    ] {
        for streaming in [false, true] {
            eprintln!("routing matrix: {protocol:?} streaming={streaming}");
            let (dir, bridge, target) = fixture(SCRIPT);
            let (openai,openai_task)=upstream(json!({"id":"openai","object":"response","model":"openai-model","status":"completed","output":[{"type":"message","id":"openai-msg","role":"assistant","content":[{"type":"output_text","text":"OpenAI design change"}]}],"usage":{"input_tokens":2,"output_tokens":2}})).await;
            let (gemini,gemini_task)=upstream(json!({"responseId":"gemini","modelVersion":"gemini-fixture","candidates":[{"content":{"role":"model","parts":[{"text":"Gemini reviewed the change"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":3,"candidatesTokenCount":2}})).await;
            let mut route = route(protocol, target, openai, gemini);
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let bind = listener.local_addr().unwrap().to_string();
            drop(listener);
            let proxy = ProxyHandle::start(
                RuntimeConfig::from_routes(&bind, vec![route.clone()]),
                Arc::new(UsageStore::open(dir.path().join("usage.sqlite")).unwrap()),
            )
            .unwrap();
            proxy.set_subscription_backend(Arc::new(crate::community::Dispatch {
                claude: Arc::new(bridge.clone()),
                community: Arc::new(crate::community::CommunityBridge::new(dir.path())),
            }));
            let mut history = vec![json!({"role":"user","content":"write a fixture"})];
            let first = message(
                send(&bind, &route, dir.path(), &history, streaming).await,
                protocol,
                streaming,
            )
            .await;
            assert_eq!(first["stop_reason"], "tool_use");
            let call = first["content"]
                .as_array()
                .unwrap()
                .iter()
                .find(|b| b["type"] == "tool_use")
                .unwrap();
            assert_eq!(call["name"], "aipass_acp_respond");
            history.extend([json!({"role":"assistant","content":first["content"]}),json!({"role":"user","content":[{"type":"tool_result","tool_use_id":call["id"],"content":json!({"outcome":{"outcome":"selected","optionId":"allow_once"}}).to_string()}]})]);
            // With its owner unavailable, an approval must not be sent elsewhere.
            select(&proxy, &mut route, &bind, 1);
            let blocked = send(&bind, &route, dir.path(), &history, streaming).await;
            assert!(blocked.status().is_server_error());
            assert!(!dir.path().join("permitted").exists());
            select(&proxy, &mut route, &bind, 0);
            let response = send(&bind, &route, dir.path(), &history, streaming).await;
            assert!(
                response.status().is_success(),
                "permission continuation: {:?}",
                proxy.status().last_error
            );
            let done = message(response, protocol, streaming).await;
            assert!(
                done["content"]
                    .to_string()
                    .contains("synthetic file write completed"),
                "{protocol:?} streaming={streaming}: {done}"
            );
            assert_eq!(
                std::fs::read_to_string(dir.path().join("starts"))
                    .unwrap()
                    .lines()
                    .count(),
                1
            );
            history.push(json!({"role":"assistant","content":done["content"]}));
            // Even a fresh user turn remains on Claude while native background
            // work is outstanding. Portable old call IDs must still match its
            // existing process instead of discarding it or rejecting history.
            history.push(json!({"role":"user","content":"collect background work"}));
            select(&proxy, &mut route, &bind, 1);
            assert!(send(&bind, &route, dir.path(), &history, streaming)
                .await
                .status()
                .is_server_error());
            select(&proxy, &mut route, &bind, 0);
            let background = message(
                send(&bind, &route, dir.path(), &history, streaming).await,
                protocol,
                streaming,
            )
            .await;
            assert!(background["content"]
                .to_string()
                .contains("background work completed"));
            assert!(background["content"].to_string().contains("Claude answer"));
            assert_eq!(
                std::fs::read_to_string(dir.path().join("starts"))
                    .unwrap()
                    .lines()
                    .count(),
                1
            );
            history.push(json!({"role":"assistant","content":background["content"]}));
            for (index, prompt, expected) in [
                (1, "ask OpenAI", "OpenAI design change"),
                (2, "ask Gemini", "Gemini reviewed the change"),
                (0, "return to Claude", "Claude answer"),
            ] {
                select(&proxy, &mut route, &bind, index);
                history.push(json!({"role":"user","content":prompt}));
                let answer = message(
                    send(&bind, &route, dir.path(), &history, streaming).await,
                    protocol,
                    streaming,
                )
                .await;
                assert!(answer["content"].to_string().contains(expected), "{answer}");
                history.push(json!({"role":"assistant","content":answer["content"]}));
            }
            let (headers, body) = openai_task.await.unwrap();
            assert!(headers.starts_with("POST /v1/responses "));
            assert!(!headers.to_lowercase().contains("x-aipass-claude-session"));
            assert!(body.to_string().contains("synthetic file write completed"));
            assert!(body.to_string().contains("background work completed"));
            assert_eq!(body["model"], "openai-model");
            let (headers, body) = gemini_task.await.unwrap();
            assert!(headers.contains("/v1beta/models/gemini-fixture:"));
            assert!(body.to_string().contains("OpenAI design change"));
            let prompts = std::fs::read_to_string(dir.path().join("prompts")).unwrap();
            let last = prompts.lines().last().unwrap();
            for context in [
                "write a fixture",
                "synthetic file write completed",
                "OpenAI design change",
                "Gemini reviewed the change",
                "return to Claude",
            ] {
                assert!(last.contains(context), "missing {context}: {last}");
            }
            assert_eq!(
                std::fs::read_to_string(dir.path().join("starts"))
                    .unwrap()
                    .lines()
                    .count(),
                2
            );
            bridge.revoke();
        }
    }
}
