//! Wire regressions from the Magpie comparison; independent synthetic payloads.
use super::*;
use serde_json::json;
use ProxyProtocol::{AnthropicMessages as AM, OpenAiChatCompletions as CC, OpenAiResponses as RS};

#[test]
fn unrepresentable_request_content_is_never_silently_removed() {
    for (from, to, input) in [
        (
            AM,
            CC,
            json!({"messages":[{"role":"user","content":[{"type":"document","source":{"type":"text","data":"critical document"}}]}]}),
        ),
        (
            AM,
            RS,
            json!({"messages":[{"role":"user","content":[{"type":"document","source":{"type":"text","data":"critical document"}}]}]}),
        ),
        (
            CC,
            AM,
            json!({"messages":[{"role":"user","content":[{"type":"input_audio","input_audio":{"data":"synthetic","format":"wav"}}]}]}),
        ),
        (
            RS,
            AM,
            json!({"input":[{"role":"user","content":[{"type":"input_file","file_id":"file-synthetic"}]}]}),
        ),
        (
            AM,
            CC,
            json!({"messages":[],"tools":[{"type":"web_search_20250305","name":"web_search"}]}),
        ),
        (AM, RS, json!({"messages":[],"stop_sequences":["END"]})),
        (CC, AM, json!({"messages":[],"n":2})),
    ] {
        assert!(
            BuiltinConversionPlugin
                .convert_request(from, to, input.clone())
                .is_err(),
            "silently changed {from:?}->{to:?}: {input}"
        );
        assert_eq!(
            BuiltinConversionPlugin
                .convert_request(from, from, input.clone())
                .unwrap(),
            input
        );
    }
}

fn events(chunks: Vec<String>) -> Vec<Value> {
    chunks
        .iter()
        .flat_map(|s| s.lines())
        .filter_map(|l| l.strip_prefix("data: "))
        .filter_map(|s| serde_json::from_str(s).ok())
        .collect()
}

#[test]
fn repeated_tool_identity_keeps_one_name_and_id_with_all_argument_fragments() {
    for to in [AM, RS] {
        let mut converter = StreamConverter::new(CC, to).unwrap();
        let mut out = Vec::new();
        for fragment in ["{\"n\":", "7}"] {
            let value = json!({"id":"chatcmpl-fixture","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call-fixture","type":"function","function":{"name":"lookup","arguments":fragment}}]}}]});
            out.extend(converter.push_event(&format!("data: {value}\n\n")).unwrap());
        }
        out.extend(converter.push_event("data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n").unwrap());
        out.extend(converter.finish().unwrap());
        let output = events(out);
        let call = if to == AM {
            output
                .iter()
                .find(|v| v["content_block"]["type"] == "tool_use")
                .unwrap()["content_block"]
                .clone()
        } else {
            output.last().unwrap()["response"]["output"][0].clone()
        };
        assert_eq!(call["name"], "lookup");
        assert_eq!(
            call[if to == AM { "id" } else { "call_id" }],
            "call-fixture"
        );
        let args = if to == AM {
            output
                .iter()
                .filter_map(|v| v["delta"]["partial_json"].as_str())
                .collect::<String>()
        } else {
            call["arguments"].as_str().unwrap().into()
        };
        assert_eq!(
            serde_json::from_str::<Value>(&args).unwrap(),
            json!({"n":7})
        );
    }
}

#[test]
fn chat_stream_choice_is_stable_across_interleaved_and_string_indices() {
    for to in [AM, RS] {
        let mut converter = StreamConverter::new(CC, to).unwrap();
        let mut out = Vec::new();
        for chunk in [
            json!({"choices":[{"delta":{"role":"assistant"}}]}),
            json!({"choices":[{"index":"1","delta":{"content":"selected "}}]}),
            json!({"choices":[{"index":2,"delta":{"content":"FOREIGN","tool_calls":[{"index":0,"id":"bad","function":{"name":"wrong","arguments":"{}"}}]},"finish_reason":"tool_calls"}]}),
            json!({"choices":[{"index":1.0,"delta":{"content":"answer"},"finish_reason":"stop"}]}),
        ] {
            out.extend(converter.push_event(&format!("data: {chunk}\n\n")).unwrap());
        }
        out.extend(converter.finish().unwrap());
        let wire = out.join("");
        assert!(
            !wire.contains("FOREIGN") && !wire.contains("wrong"),
            "mixed choices {to:?}: {wire}"
        );
        assert!(wire.contains("selected ") && wire.contains("answer"));
    }
}

#[test]
fn refusal_survives_both_buffered_and_streamed_messages_conversion() {
    let response = json!({"id":"chatcmpl-refusal","choices":[{"message":{"role":"assistant","content":null,"refusal":"Cannot do this."},"finish_reason":"stop"}]});
    let converted = BuiltinConversionPlugin
        .convert_response(CC, AM, response)
        .unwrap();
    assert!(converted["content"].to_string().contains("Cannot do this."));
    let mut converter = StreamConverter::new(CC, AM).unwrap();
    let mut out=converter.push_event("data: {\"choices\":[{\"index\":0,\"delta\":{\"refusal\":\"Cannot do this.\"},\"finish_reason\":\"stop\"}]}\n\n").unwrap();
    out.extend(converter.finish().unwrap());
    assert!(out.join("").contains("Cannot do this."));
}

#[test]
fn malformed_successful_responses_do_not_become_empty_successes() {
    for (from, to, payload) in [
        (AM, CC, json!({})),
        (AM, RS, json!({"content":"wrong"})),
        (CC, AM, json!({"choices":[{}]})),
    ] {
        assert!(BuiltinConversionPlugin
            .convert_response(from, to, payload)
            .is_err());
    }
}

#[test]
fn parallel_tool_constraints_survive_all_anthropic_conversion_paths() {
    for openai in [CC, RS] {
        for disabled in [false, true] {
            let am = json!({"messages":[{"role":"user","content":"hello"}],"tools":[{"name":"lookup","input_schema":{"type":"object"}}],"tool_choice":{"type":"auto","disable_parallel_tool_use":disabled}});
            let converted = BuiltinConversionPlugin
                .convert_request(AM, openai, am)
                .unwrap();
            assert_eq!(converted["parallel_tool_calls"], !disabled);
            let restored = BuiltinConversionPlugin
                .convert_request(openai, AM, converted)
                .unwrap();
            assert_eq!(
                restored["tool_choice"]["disable_parallel_tool_use"],
                disabled
            );
        }
    }
}

#[test]
fn openai_target_incompatibilities_are_distinct_from_malformed_requests() {
    for (from, to, payload) in [
        (
            CC,
            RS,
            json!({"messages":[{"role":"user","content":[{"type":"input_audio","input_audio":{"format":"wav","data":"fixture"}}]}]}),
        ),
        (
            RS,
            CC,
            json!({"input":[{"type":"computer_call_output","call_id":"c","output":{}}]}),
        ),
    ] {
        assert!(matches!(
            BuiltinConversionPlugin.convert_request(from, to, payload),
            Err(ConversionError::UnsupportedFeature { .. })
        ));
    }
}

#[test]
fn unknown_native_output_blocks_fail_instead_of_disappearing() {
    for to in [CC, RS] {
        let payload = json!({"content":[{"type":"server_tool_use","id":"native-call","name":"web_search","input":{"query":"fixture"}}]});
        assert!(BuiltinConversionPlugin
            .convert_response(AM, to, payload.clone())
            .is_err());
        let mut converter = StreamConverter::new(AM, to).unwrap();
        assert!(converter.push_event(&format!("data: {}\n\n",json!({"type":"content_block_start","index":0,"content_block":payload["content"][0]}))).is_err());
    }
    assert!(BuiltinConversionPlugin.convert_response(RS,AM,json!({"status":"completed","output":[{"type":"computer_call","call_id":"native","action":{"type":"click"}}]})).is_err());
}

#[test]
fn initial_text_and_thinking_in_anthropic_blocks_survive_stream_conversion() {
    for to in [CC, RS] {
        let mut converter = StreamConverter::new(AM, to).unwrap();
        let mut out = Vec::new();
        for payload in [
            json!({"type":"message_start","message":{"id":"msg_initial","content":[]}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"initial thought"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":"initial text"}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":" continued"}}),
            json!({"type":"content_block_stop","index":1}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":5}}),
            json!({"type":"message_stop"}),
        ] {
            out.extend(
                converter
                    .push_event(&format!("data: {payload}\n\n"))
                    .unwrap(),
            );
        }
        out.extend(converter.finish().unwrap());
        let events = events(out);
        let (text, thinking) = if to == CC {
            (
                events
                    .iter()
                    .filter_map(|v| v["choices"][0]["delta"]["content"].as_str())
                    .collect::<String>(),
                events
                    .iter()
                    .filter_map(|v| v["choices"][0]["delta"]["reasoning_content"].as_str())
                    .collect::<String>(),
            )
        } else {
            (
                events
                    .iter()
                    .filter(|v| v["type"] == "response.output_text.delta")
                    .filter_map(|v| v["delta"].as_str())
                    .collect::<String>(),
                events
                    .iter()
                    .filter(|v| v["type"] == "response.reasoning_summary_text.delta")
                    .filter_map(|v| v["delta"].as_str())
                    .collect::<String>(),
            )
        };
        assert_eq!(text, "initial text continued");
        assert_eq!(thinking, "initial thought");
    }
}

#[test]
fn freeform_chat_tool_response_preserves_its_plain_input() {
    let response = json!({"choices":[{"message":{"role":"assistant","tool_calls":[{"type":"custom","id":"patch-call","custom":{"name":"apply_patch","input":"*** Begin Patch\nfixture\n*** End Patch"}}]},"finish_reason":"tool_calls"}]});
    let converted = BuiltinConversionPlugin
        .convert_response(CC, AM, response)
        .unwrap();
    assert_eq!(converted["content"][0]["name"], "apply_patch");
    assert_eq!(
        converted["content"][0]["input"]["input"],
        "*** Begin Patch\nfixture\n*** End Patch"
    );
}

#[test]
fn tool_results_precede_followup_user_text_on_both_openai_wires() {
    let input = json!({"messages":[{"role":"assistant","content":[{"type":"tool_use","id":"call-order","name":"lookup","input":{}}]},{"role":"user","content":[{"type":"tool_result","tool_use_id":"call-order","content":"result first"},{"type":"text","text":"then follow up"}]}]});
    for to in [CC, RS] {
        let converted = BuiltinConversionPlugin
            .convert_request(AM, to, input.clone())
            .unwrap();
        let messages = converted[if to == CC { "messages" } else { "input" }]
            .as_array()
            .unwrap();
        assert_eq!(messages.len(), 3);
        assert!(messages[0].to_string().contains("call-order"));
        assert!(
            messages[1].to_string().contains("result first"),
            "{converted}"
        );
        assert!(
            messages[2].to_string().contains("then follow up"),
            "{converted}"
        );
    }
}

#[test]
fn empty_reasoning_alias_does_not_hide_the_other_field() {
    for to in [AM, RS] {
        for primary in [Value::Null, json!("")] {
            let mut converter = StreamConverter::new(CC, to).unwrap();
            let chunk = json!({"choices":[{"index":0,"delta":{"reasoning_content":primary,"reasoning":"kept thought"},"finish_reason":"stop"}]});
            let mut out = converter.push_event(&format!("data: {chunk}\n\n")).unwrap();
            out.extend(converter.finish().unwrap());
            assert!(out.join("").contains("kept thought"));
        }
    }
}
