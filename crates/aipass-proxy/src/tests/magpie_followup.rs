//! Regressions derived from Magpie fixes, using independent wire fixtures.
use super::*;
use serde_json::{json, Value};

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
