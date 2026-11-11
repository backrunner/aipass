//! Regressions derived from Magpie fixes, using independent wire fixtures.
use super::*;
use serde_json::{json, Value};
use ProxyProtocol::{AnthropicMessages as AM, OpenAiChatCompletions as CC, OpenAiResponses as RS};

const CHAT_FIRST: &str = "data: {\"choices\":[{\"delta\":{\"content\":\"streamed answer\"}}]}\n\n";
const CHAT_END: &str =
    "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";

#[test]
fn mistral_incompatible_reasoning_skips_without_spending_attempt_or_changing_history() {
    let skipped = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let (address, backup) = mock_upstream(
        "200 OK",
        "application/json",
        json!({"choices":[{"message":{"content":"answer"},"finish_reason":"stop"}]}).to_string(),
    );
    let mut mistral = conversion_target(format!("http://{}", skipped.local_addr().unwrap()), 0, CC);
    mistral.profile = ProviderProfile::Mistral;
    let skipped_id = mistral.config.id;
    let proxy = super::local_proxy_review::direct_proxy(conversion_route(
        "fixture",
        CC,
        CC,
        vec![
            mistral,
            conversion_target(format!("http://{address}"), 1, CC),
        ],
        1,
    ));
    let original = json!({"model":"fixture","messages":[{"role":"assistant","reasoning_content":"field reasoning","content":[{"type":"thinking","thinking":"part reasoning"},{"type":"text","text":"answer"}]},{"role":"user","content":"continue"}]});
    let response = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
        .post(format!("http://{}/v1/chat/completions", proxy.bind_addr))
        .bearer_auth("fixture")
        .json(&original)
        .send()
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{}",
        response.text().unwrap()
    );
    let sent: Value = serde_json::from_slice(&backup.join().unwrap().1).unwrap();
    assert_eq!(sent, original);
    mock_upstream::assert_no_api_request(&skipped);
    assert!(!proxy
        .state
        .health
        .lock()
        .unwrap()
        .get(&skipped_id)
        .is_some_and(|health| health.consecutive_failures > 0));
}

fn streaming_request(proxy: &ProxyHandle, protocol: ProxyProtocol) -> reqwest::blocking::Response {
    let body = BuiltinConversionPlugin
        .convert_request(
            CC,
            protocol,
            json!({"model":"fixture","stream":true,"messages":[{"role":"user","content":"hello"}]}),
        )
        .unwrap();
    reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
        .post(format!("http://{}{}", proxy.bind_addr, protocol.path()))
        .bearer_auth("fixture")
        .json(&body)
        .send()
        .unwrap()
}

fn stream_proxy(address: SocketAddr, protocol: ProxyProtocol, silent: bool) -> ProxyHandle {
    let mut route = conversion_route(
        "fixture",
        protocol,
        CC,
        vec![conversion_target(format!("http://{address}"), 0, CC)],
        1,
    );
    route.config.retry.silent_retry = silent;
    super::local_proxy_review::direct_proxy(route)
}

#[test]
fn mistral_replayed_reasoning_uses_content_parts_on_every_inbound_protocol() {
    for inbound in [CC, AM, RS] {
        for streaming in [false, true] {
            let original = json!({"model":"deepseek-fixture","stream":streaming,"messages":[{"role":"user","content":"lookup"},{"role":"assistant","content":"checking","reasoning_content":"preserved thought","tool_calls":[{"id":"call_fixture","type":"function","function":{"name":"lookup","arguments":"{}"}}]},{"role":"tool","tool_call_id":"call_fixture","content":"found"}]});
            let body = if inbound == AM {
                // Author a native Messages history: OpenAI -> Messages request
                // conversion deliberately renders foreign reasoning as text.
                json!({"model":"deepseek-fixture","stream":streaming,"messages":[{"role":"user","content":"lookup"},{"role":"assistant","content":[{"type":"thinking","thinking":"preserved thought"},{"type":"text","text":"checking"},{"type":"tool_use","id":"call_fixture","name":"lookup","input":{}}]},{"role":"user","content":[{"type":"tool_result","tool_use_id":"call_fixture","content":"found"}]}]})
            } else {
                BuiltinConversionPlugin
                    .convert_request(CC, inbound, original)
                    .unwrap()
            };
            let reply = if streaming {
                format!("{CHAT_FIRST}{CHAT_END}")
            } else {
                json!({"choices":[{"message":{"role":"assistant","content":"answer"},"finish_reason":"stop"}]}).to_string()
            };
            let (address, upstream) = mock_upstream(
                "200 OK",
                if streaming {
                    "text/event-stream"
                } else {
                    "application/json"
                },
                reply,
            );
            let mut target = conversion_target(format!("http://{address}"), 0, CC);
            target.profile = ProviderProfile::Mistral;
            let proxy = super::local_proxy_review::direct_proxy(conversion_route(
                "fixture",
                inbound,
                CC,
                vec![target],
                1,
            ));
            let response = reqwest::blocking::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap()
                .post(format!("http://{}{}", proxy.bind_addr, inbound.path()))
                .bearer_auth("fixture")
                .json(&body)
                .send()
                .unwrap();
            let status = response.status();
            let reply = response.text().unwrap();
            let sent: Value = serde_json::from_slice(&upstream.join().unwrap().1).unwrap();
            assert_eq!(status, StatusCode::OK, "{reply}");
            let assistant = sent["messages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|m| m["role"] == "assistant")
                .unwrap();
            assert!(
                assistant.get("reasoning_content").is_none(),
                "{inbound:?}: {assistant}"
            );
            assert_eq!(
                assistant["content"][0],
                json!({"type":"thinking","thinking":[{"type":"text","text":"preserved thought"}]})
            );
            assert_eq!(
                assistant["content"][1],
                json!({"type":"text","text":"checking"})
            );
            assert_eq!(assistant["tool_calls"][0]["id"], "call_fixture");
            assert_eq!(
                sent["messages"].as_array().unwrap().last().unwrap()["content"],
                "found"
            );
        }
    }
}

#[test]
fn mislabeled_event_stream_is_converted_as_a_stream() {
    for content_type in [
        "application/json",
        "application/octet-stream",
        "text/plain",
        "",
        "text/event-stream",
    ] {
        for inbound in [AM, CC, RS] {
            for silent in [false, true] {
                let (address, upstream) =
                    mock_upstream("200 OK", content_type, format!("{CHAT_FIRST}{CHAT_END}"));
                let proxy = stream_proxy(address, inbound, silent);
                let response = streaming_request(&proxy, inbound);
                let status = response.status();
                let content_type = response.headers().get("content-type").cloned();
                let body = response.text().unwrap();
                upstream.join().unwrap();
                assert_eq!(status, StatusCode::OK, "{body}");
                assert_eq!(content_type.unwrap(), "text/event-stream");
                assert!(body.contains("streamed answer"), "{body}");
                assert!(
                    stream_reports_completion(inbound, body.as_bytes()),
                    "{body}"
                );
            }
        }
    }
}

#[test]
fn mislabeled_stream_delivers_content_before_upstream_completion() {
    for content_type in ["application/json", "application/octet-stream", ""] {
        for inbound in [AM, CC, RS] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let (heard, acknowledged) = std::sync::mpsc::channel();
            let upstream = std::thread::spawn(move || {
                let (mut socket, _) = mock_upstream::accept_api_request(&listener);
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{CHAT_FIRST}", CHAT_FIRST.len()+CHAT_END.len()).unwrap();
                socket.flush().unwrap();
                let timely = acknowledged.recv_timeout(Duration::from_secs(3)).is_ok();
                socket.write_all(CHAT_END.as_bytes()).unwrap();
                timely
            });
            let proxy = stream_proxy(address, inbound, false);
            let mut response = streaming_request(&proxy, inbound);
            let status = response.status();
            let mut all = Vec::new();
            let mut sent = false;
            let mut buffer = [0; 256];
            loop {
                let read = response.read(&mut buffer).unwrap();
                if read == 0 {
                    break;
                }
                all.extend_from_slice(&buffer[..read]);
                if !sent && String::from_utf8_lossy(&all).contains("streamed answer") {
                    let _ = heard.send(());
                    sent = true;
                }
            }
            let timely = upstream.join().unwrap();
            assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&all));
            assert!(timely, "{inbound:?} / {content_type}: buffered until EOF");
            assert!(stream_reports_completion(inbound, &all));
        }
    }
}

#[test]
fn mislabeled_stream_errors_preserve_fallback_and_no_replay_boundaries() {
    for inbound in [AM, CC, RS] {
        for silent in [false, true] {
            for output in [false, true] {
                let error = format!(
                    "{}data: {{\"error\":{{\"message\":\"synthetic failure\"}}}}\n\n",
                    if output { CHAT_FIRST } else { "" }
                );
                let (bad, bad_thread) = mock_upstream("200 OK", "application/json", error);
                let backup = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
                let backup_addr = backup.local_addr().unwrap();
                let mut backup_thread = None;
                let mut untouched = None;
                if output {
                    untouched = Some(backup);
                } else {
                    backup_thread = Some(std::thread::spawn(move || {
                        let (mut socket, _) = mock_upstream::accept_api_request(&backup);
                        write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{CHAT_FIRST}{CHAT_END}", CHAT_FIRST.len()+CHAT_END.len()).unwrap();
                    }));
                }
                let mut route = conversion_route(
                    "fixture",
                    inbound,
                    CC,
                    vec![
                        conversion_target(format!("http://{bad}"), 0, CC),
                        conversion_target(format!("http://{backup_addr}"), 1, CC),
                    ],
                    2,
                );
                route.config.retry.silent_retry = silent;
                let proxy = super::local_proxy_review::direct_proxy(route);
                let response = streaming_request(&proxy, inbound);
                let status = response.status();
                let body = response.text().unwrap();
                bad_thread.join().unwrap();
                if let Some(backup_thread) = backup_thread {
                    backup_thread.join().unwrap();
                    assert_eq!(status, StatusCode::OK, "{body}");
                    assert!(stream_reports_completion(inbound, body.as_bytes()));
                }
                if let Some(backup) = untouched {
                    mock_upstream::assert_no_api_request(&backup);
                    assert!(
                        status.is_server_error() || stream_reports_error(body.as_bytes()),
                        "{body}"
                    );
                }
            }
        }
    }
}

#[test]
fn subscriptions_and_official_anthropic_share_thinking_cleanup() {
    let codec = SubscriptionCodec::default();
    let mut body = json!({"model":"claude-fixture","messages":[{"role":"assistant","content":[{"type":"thinking","thinking":"foreign","signature":" "},{"type":"text","text":"retained"}]}]});
    let cleaned = codec
        .prepare_protocol(
            "anthropic",
            ProxyProtocol::AnthropicMessages,
            body.clone(),
            Uuid::new_v4(),
        )
        .unwrap();
    assert_eq!(
        cleaned["messages"][0]["content"],
        json!([{"type":"text","text":"retained"}])
    );
    body["model"] = json!("model-alias");
    let bytes = Bytes::from(body.to_string());
    assert_eq!(
        crate::provider::prepare(
            ProviderProfile::Generic,
            ProxyProtocol::AnthropicMessages,
            bytes.clone()
        )
        .unwrap(),
        bytes
    );
    let official: Value = serde_json::from_slice(
        &crate::provider::prepare(
            ProviderProfile::Anthropic,
            ProxyProtocol::AnthropicMessages,
            bytes,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(official["messages"], cleaned["messages"]);
}

#[test]
fn unsigned_thinking_does_not_bind_a_tool_round_or_reach_claude() {
    for signature in [Value::Null, json!(""), json!(" \t")] {
        for streaming in [false, true] {
            let reply = json!({"id":"msg","type":"message","role":"assistant","content":[{"type":"text","text":"done"}],"stop_reason":"end_turn","usage":{"input_tokens":2,"output_tokens":1}});
            let (content_type, response_body) = if streaming {
                let events = [
                    json!({"type":"message_start","message":{"id":"msg","type":"message","role":"assistant","content":[],"usage":{"input_tokens":2,"output_tokens":0}}}),
                    json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
                    json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"done"}}),
                    json!({"type":"content_block_stop","index":0}),
                    json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}}),
                    json!({"type":"message_stop"}),
                ];
                (
                    "text/event-stream",
                    events
                        .iter()
                        .map(|event| format!("data: {event}\n\n"))
                        .collect(),
                )
            } else {
                ("application/json", reply.to_string())
            };
            let (address, upstream) = mock_upstream("200 OK", content_type, response_body);
            let mut target = conversion_target(
                format!("http://{address}"),
                0,
                ProxyProtocol::AnthropicMessages,
            );
            target.config.model = Some("claude-fixture".into());
            let route = conversion_route(
                "token",
                ProxyProtocol::AnthropicMessages,
                ProxyProtocol::AnthropicMessages,
                vec![target],
                1,
            );
            let model = crate::model_routes::model_id(&route).unwrap();
            let proxy = start_proxy(available_addr(), route);
            let body = json!({"model":model,"stream":streaming,"messages":[{"role":"user","content":"lookup"},{"role":"assistant","content":[{"type":"thinking","thinking":"foreign reasoning","signature":signature},{"type":"tool_use","id":"call","name":"lookup","input":{"signature":"user-owned"}}]},{"role":"user","content":[{"type":"tool_result","tool_use_id":"call","content":"found"}]}]});
            let response = reqwest::blocking::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap()
                .post(format!("http://{}/v1/messages", proxy.bind_addr))
                .bearer_auth("token")
                .json(&body)
                .send()
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "{}",
                response.text().unwrap()
            );
            assert_eq!(response.headers()["content-type"], content_type);
            assert!(response.text().unwrap().contains("done"));
            let sent: Value = serde_json::from_slice(&upstream.join().unwrap().1).unwrap();
            assert_eq!(
                sent["messages"][1]["content"],
                json!([{"type":"tool_use","id":"call","name":"lookup","input":{"signature":"user-owned"}}])
            );
            assert_eq!(sent["messages"][2], body["messages"][2]);
        }
    }
}
