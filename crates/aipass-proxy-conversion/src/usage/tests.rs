use super::*;
use crate::{BuiltinConversionPlugin, ConversionPlugin, StreamConverter};
use serde_json::json;
use ProxyProtocol::{AnthropicMessages as AM, OpenAiChatCompletions as CC, OpenAiResponses as RS};

#[test]
fn nested_ciphertext_is_bound_but_ordinary_tool_json_and_ids_stay_portable() {
    let encrypted = [
        json!({"type":"agent_message","encrypted_content":"sealed-agent","content":[]}),
        json!({"type":"message","role":"assistant","content":[{"type":"encrypted_content","data":"sealed-block"}]}),
        json!({"type":"function_call_output","call_id":"call_1","output":[{"type":"encrypted_content","data":"sealed-result"}]}),
    ];
    for item in encrypted {
        assert_eq!(crate::response_item_ciphertexts(&item).len(), 1);
        let body = json!({"input":[item]});
        assert_eq!(
            BuiltinConversionPlugin
                .convert_request(RS, RS, body.clone())
                .unwrap(),
            body
        );
        for target in [AM, CC] {
            assert!(BuiltinConversionPlugin
                .convert_request(RS, target, body.clone())
                .is_err());
        }
    }
    for item in [
        json!({"type":"function_call","arguments":"{\"type\":\"encrypted_content\",\"data\":\"user JSON\"}"}),
        json!({"type":"function_call_output","call_id":"keep-me","output":{"type":"encrypted_content","data":"user object"}}),
        json!({"role":"user","content":[{"type":"input_text","text":"{\"encrypted_content\":\"user text\"}"}]}),
    ] {
        assert!(crate::response_item_ciphertexts(&item).is_empty());
    }
}

fn converted(from: ProxyProtocol, to: ProxyProtocol, events: &[Value]) -> Vec<Value> {
    let mut converter = StreamConverter::new(from, to).unwrap();
    let mut output = Vec::new();
    for event in events {
        output.extend(converter.push_event(&format!("data: {event}\n\n")).unwrap());
    }
    if from == CC {
        output.extend(converter.push_event("data: [DONE]\n\n").unwrap());
    }
    output.extend(
        converter
            .finish()
            .expect("completed stream must finish cleanly"),
    );
    assert!(converter.finish().unwrap().is_empty());
    output
        .iter()
        .filter_map(|e| e.lines().find_map(|l| l.strip_prefix("data: ")))
        .filter(|s| *s != "[DONE]")
        .map(|s| serde_json::from_str(s).unwrap())
        .collect()
}

fn totals(protocol: ProxyProtocol, events: &[Value]) -> TokenUsage {
    let mut usage = UsageAccumulator::default();
    for event in events {
        usage.observe(protocol, event);
    }
    usage.totals(protocol)
}

#[test]
fn messages_partial_usage_and_late_counters_match_buffered_conversion() {
    let events = [
        json!({"type":"message_start","message":{"id":"msg_usage","model":"m","usage":{"input_tokens":5,"cache_read_input_tokens":80,"cache_creation_input_tokens":15,"output_tokens":1}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"answer"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"max_tokens"},"usage":{"input_tokens":7,"cache_creation_input_tokens":18,"cache_read_input_tokens":null,"output_tokens":11}}),
        json!({"type":"message_delta","usage":{"output_tokens":13}}),
        json!({"type":"message_stop"}),
    ];
    let final_message = json!({"id":"msg_usage","model":"m","content":[{"type":"text","text":"answer"}],"stop_reason":"max_tokens","usage":{"input_tokens":7,"cache_read_input_tokens":80,"cache_creation_input_tokens":18,"output_tokens":13}});
    let expected = TokenUsage {
        input_tokens: 7,
        output_tokens: 13,
        cache_read_tokens: 80,
        cache_creation_tokens: 18,
        cache_creation_1h_tokens: 0,
    };
    for target in [AM, CC, RS] {
        let output = converted(AM, target, &events);
        assert_eq!(totals(target, &output), expected);
        let buffered = BuiltinConversionPlugin
            .convert_response(AM, target, final_message.clone())
            .unwrap();
        assert_eq!(
            BuiltinConversionPlugin.extract_usage(target, &buffered),
            expected
        );
        if target == RS {
            assert_eq!(output.last().unwrap()["type"], "response.incomplete");
            assert_eq!(
                output
                    .iter()
                    .filter(|e| e["type"] == "response.incomplete")
                    .count(),
                1
            );
        } else if target == CC {
            assert_eq!(
                output
                    .iter()
                    .filter_map(|e| e
                        .pointer("/choices/0/finish_reason")
                        .and_then(Value::as_str))
                    .collect::<Vec<_>>(),
                ["length"]
            );
        }
    }
}

#[test]
fn late_cache_details_are_merged_before_subtracting_from_openai_input() {
    let events = [
        json!({"id":"chatcmpl-usage","model":"m","choices":[{"delta":{"content":"answer"}}],"usage":{"prompt_tokens":100,"completion_tokens":1}}),
        json!({"choices":[{"delta":{},"finish_reason":"length"}],"usage":{"prompt_tokens_details":{"cached_tokens":80,"cache_creation_tokens":15}}}),
        json!({"choices":[],"usage":{"completion_tokens":11}}),
        json!({"choices":[],"usage":{"completion_tokens":13,"prompt_tokens_details":{"cache_creation_tokens":0}}}),
    ];
    let expected = TokenUsage {
        input_tokens: 20,
        output_tokens: 13,
        cache_read_tokens: 80,
        cache_creation_tokens: 0,
        cache_creation_1h_tokens: 0,
    };
    for target in [CC, AM, RS] {
        let output = converted(CC, target, &events);
        assert_eq!(totals(target, &output), expected, "{target:?}");
        if target == RS {
            for reverse in [AM, CC] {
                assert_eq!(totals(reverse, &converted(RS, reverse, &output)), expected);
            }
        }
    }
}

#[test]
fn tool_start_input_survives_without_argument_deltas_and_terminal_is_once() {
    for input in [json!({}), json!({"path":"file"})] {
        let events = [
            json!({"type":"message_start","message":{"id":"msg_tool","model":"m","usage":{}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"read","input":input}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":4}}),
            json!({"type":"message_stop"}),
        ];
        for target in [CC, RS] {
            let output = converted(AM, target, &events);
            let args = if target == CC {
                output
                    .iter()
                    .filter_map(|e| {
                        e.pointer("/choices/0/delta/tool_calls/0/function/arguments")
                            .and_then(Value::as_str)
                    })
                    .collect::<String>()
            } else {
                output.last().unwrap()["response"]["output"][0]["arguments"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            };
            assert_eq!(serde_json::from_str::<Value>(&args).unwrap(), input);
            let mut converter = StreamConverter::new(AM, target).unwrap();
            for event in &events {
                converter.push_event(&format!("data: {event}\n\n")).unwrap();
            }
            assert!(converter
                .push_event("data: {\"type\":\"message_stop\"}\n\n")
                .unwrap()
                .is_empty());
            assert!(converter.finish().unwrap().is_empty());
        }
    }
}

#[test]
fn mixed_ttl_usage_survives_late_updates_and_every_protocol_round_trip() {
    let events = [
        json!({"type":"message_start","message":{"id":"msg_ttl","model":"m","usage":{"input_tokens":5,"cache_read_input_tokens":80,"cache_creation_input_tokens":15,"cache_creation":{"ephemeral_1h_input_tokens":10,"ephemeral_5m_input_tokens":5},"output_tokens":1}}}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"cache_creation_input_tokens":18,"cache_creation":{"ephemeral_5m_input_tokens":8},"output_tokens":11}}),
        json!({"type":"message_stop"}),
    ];
    let expected = TokenUsage {
        input_tokens: 5,
        output_tokens: 11,
        cache_read_tokens: 80,
        cache_creation_tokens: 18,
        cache_creation_1h_tokens: 10,
    };
    for target in [AM, CC, RS] {
        let output = converted(AM, target, &events);
        assert_eq!(totals(target, &output), expected, "{target:?}");
        for reverse in [AM, CC, RS] {
            assert_eq!(
                totals(reverse, &converted(target, reverse, &output)),
                expected,
                "{target:?} -> {reverse:?}"
            );
        }
    }
    let mut usage = UsageAccumulator::default();
    usage.update(&json!({"input_tokens":5,"cache_creation_input_tokens":18,"cache_creation":{"ephemeral_1h_input_tokens":10}}));
    usage.update(&json!({"cache_creation":{"ephemeral_1h_input_tokens":null}}));
    assert_eq!(usage.totals(AM).cache_creation_1h_tokens, 10);
    usage.update(&json!({"cache_creation":{"ephemeral_1h_input_tokens":0}}));
    assert_eq!(usage.totals(AM).cache_creation_1h_tokens, 0);
}

#[test]
fn thinking_token_details_survive_all_wire_conversions_without_double_counting() {
    for thinking in [None, Some(0), Some(7)] {
        let mut usage = json!({"input_tokens":11,"output_tokens":23});
        if let Some(thinking) = thinking {
            usage["output_tokens_details"] = json!({"thinking_tokens":thinking});
        }
        let message = json!({"id":"msg_usage","model":"fixture","content":[{"type":"text","text":"answer"}],"stop_reason":"end_turn","usage":usage});
        let frames = [
            json!({"type":"message_start","message":{"id":"msg_usage","model":"fixture","usage":{"input_tokens":11,"output_tokens":1}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":"answer"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":usage}),
            // A sparse late observation must not erase reasoning details.
            json!({"type":"message_delta","usage":{"output_tokens":23}}),
            json!({"type":"message_stop"}),
        ];
        for via in [AM, CC, RS] {
            let intermediate = BuiltinConversionPlugin
                .convert_response(AM, via, message.clone())
                .unwrap();
            let streamed = converted(AM, via, &frames);
            for to in [AM, CC, RS] {
                let buffered = BuiltinConversionPlugin
                    .convert_response(via, to, intermediate.clone())
                    .unwrap();
                let mut accumulated = UsageAccumulator::default();
                for frame in converted(via, to, &streamed) {
                    accumulated.observe(to, &frame);
                }
                let pointer = match to {
                    AM => "/output_tokens_details/thinking_tokens",
                    CC => "/completion_tokens_details/reasoning_tokens",
                    RS => "/output_tokens_details/reasoning_tokens",
                };
                for value in [&buffered["usage"], accumulated.wire()] {
                    assert_eq!(
                        value.pointer(pointer).and_then(Value::as_u64),
                        thinking,
                        "{via:?} -> {to:?}: {value}"
                    );
                    let totals = BuiltinConversionPlugin.extract_usage(to, value);
                    assert_eq!((totals.input_tokens, totals.output_tokens), (11, 23));
                }
            }
        }
    }
}
