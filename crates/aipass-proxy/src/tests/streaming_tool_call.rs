use super::*;

#[test]
fn anthropic_client_to_chat_completions_upstream_streaming_tool_call() {
    let sse = concat!(
            "data: {\"id\":\"chatcmpl_1\",\"model\":\"gpt-test\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Let me check.\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"lookup\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"city\\\"\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\":\\\"Paris\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":20,\"completion_tokens\":9}}\n\n",
            "data: [DONE]\n\n",
        );
    let (upstream_addr, upstream) = mock_upstream("200 OK", "text/event-stream", sse.to_string());
    let bind_addr = available_addr();
    let token = "aipass_conv_am_cc_stream_test";
    let route = conversion_route(
        token,
        ProxyProtocol::AnthropicMessages,
        ProxyProtocol::AnthropicMessages,
        vec![conversion_target(
            format!("http://{upstream_addr}/v1"),
            0,
            ProxyProtocol::OpenAiChatCompletions,
        )],
        1,
    );
    let _proxy = start_proxy(bind_addr, route);

    let response = reqwest::blocking::Client::new()
        .post(format!("http://{bind_addr}/v1/messages"))
        .bearer_auth(token)
        .json(&serde_json::json!({
            "model": "claude-test",
            "messages": [{"role": "user", "content": "weather?"}],
            "max_tokens": 64,
            "stream": true
        }))
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().unwrap();

    for expected in [
        "event: message_start",
        "event: content_block_start",
        "event: content_block_delta",
        "event: content_block_stop",
        "event: message_delta",
        "event: message_stop",
    ] {
        assert!(body.contains(expected), "missing {expected} in:\n{body}");
    }
    assert!(!body.contains("[DONE]"));
    let events: Vec<serde_json::Value> = body
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .map(|data| serde_json::from_str(data).unwrap())
        .collect();
    assert!(
        events.iter().any(|event| event["delta"]
            == serde_json::json!({
                "type":"text_delta", "text":"Let me check."
            })),
        "{body}"
    );
    assert!(
        events.iter().any(|event| event["content_block"]
            == serde_json::json!({
                "type":"tool_use", "id":"call_1", "name":"lookup", "input":{}
            })),
        "{body}"
    );
    assert!(
        events.iter().any(|event| event["delta"]
            == serde_json::json!({
                "type":"input_json_delta", "partial_json":r#"{"city":"Paris"}"#
            })),
        "{body}"
    );
    let completion = events
        .iter()
        .find(|event| event["type"] == "message_delta")
        .unwrap();
    assert_eq!(completion["delta"]["stop_reason"], "tool_use");
    assert_eq!(completion["usage"]["output_tokens"], 9);

    let (headers, body) = upstream.join().unwrap();
    let sent: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(sent["stream"], true);
    // Usage reporting is requested on the converted CC stream.
    assert_eq!(sent["stream_options"]["include_usage"], true);
    assert!(headers.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"));
}
