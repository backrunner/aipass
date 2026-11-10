use super::*;
use serde_json::json;
use ProxyProtocol::{AnthropicMessages as AM, OpenAiChatCompletions as CC, OpenAiResponses as RS};
fn events(chunks: Vec<String>) -> Vec<Value> {
    chunks
        .iter()
        .flat_map(|s| s.lines())
        .filter_map(|s| s.strip_prefix("data: "))
        .filter_map(|s| serde_json::from_str(s).ok())
        .collect()
}

#[test]
fn converted_chat_stop_reason_matches_actual_tool_calls() {
    for calls in [
        json!(null),
        json!([]),
        json!([{"id":"call_fixture","type":"function","function":{"name":"lookup","arguments":"{}"}}]),
    ] {
        let has_calls = calls.as_array().is_some_and(|calls| !calls.is_empty());
        for finish in ["stop", "tool_calls", "length"] {
            let expected = if finish == "length" {
                "max_tokens"
            } else if has_calls {
                "tool_use"
            } else {
                "end_turn"
            };
            let message = json!({"role":"assistant","content":"answer","tool_calls":calls});
            let converted = BuiltinConversionPlugin
                .convert_response(
                    CC,
                    AM,
                    json!({"choices":[{"message":message,"finish_reason":finish}]}),
                )
                .unwrap();
            assert_eq!(
                converted["stop_reason"], expected,
                "buffered {finish} with {calls}"
            );
            let mut converter = StreamConverter::new(CC, AM).unwrap();
            let mut output = converter
                .push_event(&format!(
                    "data: {}\n\n",
                    json!({"choices":[{"delta":message}]})
                ))
                .unwrap();
            output.extend(
                converter
                    .push_event(&format!(
                        "data: {}\n\n",
                        json!({"choices":[{"delta":{},"finish_reason":finish}]})
                    ))
                    .unwrap(),
            );
            output.extend(converter.finish().unwrap());
            assert_eq!(
                events(output)
                    .iter()
                    .find(|e| e["type"] == "message_delta")
                    .unwrap()["delta"]["stop_reason"],
                expected,
                "stream {finish} with {calls}"
            );
        }
    }
}

#[test]
fn unsupported_legacy_call_output_cannot_disappear_into_text_success() {
    let message = json!({"role":"assistant","content":null,"function_call":{"name":"lookup","arguments":"{}"}});
    for to in [AM, RS] {
        assert!(matches!(
            BuiltinConversionPlugin.convert_response(
                CC,
                to,
                json!({"choices":[{"message":message,"finish_reason":"function_call"}]})
            ),
            Err(ConversionError::UnsupportedFeature { .. })
        ));
        let mut converter = StreamConverter::new(CC, to).unwrap();
        assert!(matches!(
            converter.push_event(&format!(
                "data: {}\n\n",
                json!({"choices":[{"delta":message}]})
            )),
            Err(ConversionError::UnsupportedFeature { .. })
        ));
    }
}

#[test]
fn responses_final_snapshots_complete_partial_text_and_tool_arguments() {
    for to in [CC, AM] {
        let mut converter = StreamConverter::new(RS, to).unwrap();
        let mut out = Vec::new();
        for frame in [
            json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":"hello"}),
            json!({"type":"response.output_item.added","output_index":1,"item":{"type":"function_call","call_id":"c","name":"lookup","arguments":""}}),
            json!({"type":"response.function_call_arguments.delta","output_index":1,"delta":"{\"q\":"}),
            json!({"type":"response.completed","response":{"id":"resp_fixture","status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"hello world"}]},{"type":"function_call","call_id":"c","name":"lookup","arguments":"{\"q\":7}"}]}}),
        ] {
            out.extend(converter.push_event(&format!("data: {frame}\n\n")).unwrap());
        }
        out.extend(converter.finish().unwrap());
        let out = events(out);
        let (text, args) = if to == CC {
            (
                out.iter()
                    .filter_map(|v| v["choices"][0]["delta"]["content"].as_str())
                    .collect::<String>(),
                out.iter()
                    .filter_map(|v| {
                        v["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str()
                    })
                    .collect::<String>(),
            )
        } else {
            (
                out.iter()
                    .filter_map(|v| v["delta"]["text"].as_str())
                    .collect::<String>(),
                out.iter()
                    .filter_map(|v| v["delta"]["partial_json"].as_str())
                    .collect::<String>(),
            )
        };
        assert_eq!(text, "hello world");
        assert_eq!(args, "{\"q\":7}");
    }
}

#[test]
fn empty_or_wrongly_typed_assistant_messages_do_not_turn_into_successes() {
    for to in [AM, RS] {
        for message in [
            json!({}),
            json!({"role":"assistant","content":42}),
            json!({"role":"assistant","content":[{"type":"text"}]}),
        ] {
            assert!(
                BuiltinConversionPlugin
                    .convert_response(
                        CC,
                        to,
                        json!({"choices":[{"message":message,"finish_reason":"stop"}]})
                    )
                    .is_err(),
                "{to:?}: {message}"
            );
            if message.get("content").is_some() {
                let mut converter = StreamConverter::new(CC, to).unwrap();
                assert!(converter
                    .push_event(&format!(
                        "data: {}\n\n",
                        json!({"choices":[{"index":0,"delta":message}]})
                    ))
                    .is_err());
            }
        }
    }
}

#[test]
fn final_tool_identity_preserves_delayed_and_fragmented_ids() {
    for to in [AM, RS] {
        for first_id in [Value::Null, json!("call-")] {
            let mut converter = StreamConverter::new(CC, to).unwrap();
            let mut out = Vec::new();
            for call in [
                json!({"index":0,"id":first_id,"type":"function","function":{"name":"look","arguments":"{"}}),
                json!({"index":0,"id":"call-complete","function":{"name":"lookup","arguments":"}"}}),
                json!({"index":0,"id":"call-complete","function":{"name":"lookup"}}),
            ] {
                out.extend(
                    converter
                        .push_event(&format!(
                            "data: {}\n\n",
                            json!({"choices":[{"index":0,"delta":{"tool_calls":[call]}}]})
                        ))
                        .unwrap(),
                );
            }
            out.extend(converter.push_event("data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n").unwrap());
            out.extend(converter.finish().unwrap());
            let out = events(out);
            let call = if to == AM {
                &out.iter()
                    .find(|v| v["content_block"]["type"] == "tool_use")
                    .unwrap()["content_block"]
            } else {
                &out.last().unwrap()["response"]["output"][0]
            };
            assert_eq!(
                call[if to == AM { "id" } else { "call_id" }],
                "call-complete"
            );
            assert_eq!(call["name"], "lookup");
        }
    }
}

#[test]
fn parallel_chat_tool_indices_do_not_collapse_or_mix_arguments() {
    for to in [AM, RS] {
        let mut converter = StreamConverter::new(CC, to).unwrap();
        let mut out = Vec::new();
        for calls in [
            json!([{"index":"0","id":"call-a","function":{"name":"first","arguments":"{\"n\":"}},{"index":"1","id":"call-b","function":{"name":"second","arguments":"{\"n\":"}}]),
            json!([{"index":1.0,"id":"call-b","function":{"name":"second","arguments":"2}"}}]),
            json!([{"index":0.0,"id":"call-a","function":{"name":"first","arguments":"1}"}}]),
        ] {
            out.extend(
                converter
                    .push_event(&format!(
                        "data: {}\n\n",
                        json!({"choices":[{"index":0,"delta":{"tool_calls":calls}}]})
                    ))
                    .unwrap(),
            );
        }
        out.extend(converter.push_event("data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n").unwrap());
        out.extend(converter.finish().unwrap());
        let events = events(out);
        let mut calls = std::collections::BTreeMap::new();
        if to == RS {
            for item in events.last().unwrap()["response"]["output"]
                .as_array()
                .unwrap()
            {
                calls.insert(
                    item["call_id"].as_str().unwrap().to_owned(),
                    (
                        item["name"].as_str().unwrap().to_owned(),
                        item["arguments"].as_str().unwrap().to_owned(),
                    ),
                );
            }
        } else {
            let mut ids = std::collections::HashMap::new();
            for item in &events {
                if item["type"] == "content_block_start"
                    && item["content_block"]["type"] == "tool_use"
                {
                    let id = item["content_block"]["id"].as_str().unwrap().to_owned();
                    ids.insert(item["index"].as_u64().unwrap(), id.clone());
                    calls.insert(
                        id,
                        (
                            item["content_block"]["name"].as_str().unwrap().to_owned(),
                            String::new(),
                        ),
                    );
                }
                if let Some(partial) = item["delta"]["partial_json"].as_str() {
                    calls
                        .get_mut(&ids[&item["index"].as_u64().unwrap()])
                        .unwrap()
                        .1
                        .push_str(partial);
                }
            }
        }
        assert_eq!(calls.len(), 2);
        assert_eq!(calls["call-a"], ("first".into(), "{\"n\":1}".into()));
        assert_eq!(calls["call-b"], ("second".into(), "{\"n\":2}".into()));
    }
}

#[test]
fn populated_parts_and_done_snapshots_preserve_text_refusal_and_reasoning_once() {
    for (part, delta, done, field, key) in [
        (
            "output_text",
            "response.output_text.delta",
            "response.output_text.done",
            "content",
            "text",
        ),
        (
            "refusal",
            "response.refusal.delta",
            "response.refusal.done",
            "refusal",
            "refusal",
        ),
        (
            "summary_text",
            "response.reasoning_summary_text.delta",
            "response.reasoning_summary_text.done",
            "reasoning_content",
            "text",
        ),
    ] {
        let mut c = StreamConverter::new(RS, CC).unwrap();
        let mut out = Vec::new();
        let added = if part == "summary_text" {
            "response.reasoning_summary_part.added"
        } else {
            "response.content_part.added"
        };
        for frame in [
            json!({"type":added,"output_index":0,"content_index":0,"summary_index":0,"part":{"type":part,key:"prefix"}}),
            json!({"type":delta,"output_index":0,"content_index":0,"summary_index":0,"delta":" middle"}),
            json!({"type":done,"output_index":0,"content_index":0,"summary_index":0,key:"prefix middle tail"}),
            json!({"type":done,"output_index":0,"content_index":0,"summary_index":0,key:"prefix middle tail"}),
            json!({"type":"response.completed","response":{"status":"completed","output":[]}}),
        ] {
            out.extend(c.push_event(&format!("data: {frame}\n\n")).unwrap());
        }
        out.extend(c.finish().unwrap());
        let text = events(out)
            .iter()
            .filter_map(|v| v["choices"][0]["delta"][field].as_str())
            .collect::<String>();
        assert_eq!(text, "prefix middle tail");
    }
}

#[test]
fn conflicting_snapshots_and_unowned_tool_deltas_fail_explicitly() {
    for to in [CC, AM] {
        let mut c = StreamConverter::new(RS, to).unwrap();
        c.push_event("data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"content_index\":0,\"delta\":\"delivered\"}\n\n").unwrap();
        assert!(c.push_event("data: {\"type\":\"response.output_text.done\",\"output_index\":0,\"content_index\":0,\"text\":\"different\"}\n\n").is_err());
        let mut c = StreamConverter::new(RS, to).unwrap();
        assert!(c.push_event("data: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":9,\"delta\":\"{}\"}\n\n").is_err());
    }
}

#[test]
fn refusal_history_survives_each_openai_handoff_path() {
    let cc = json!({"messages":[{"role":"user","content":"old request"},{"role":"assistant","content":null,"refusal":"Previous refusal must remain context"},{"role":"user","content":"new request"}]});
    for to in [AM, RS] {
        let converted = BuiltinConversionPlugin
            .convert_request(CC, to, cc.clone())
            .unwrap();
        assert!(
            converted
                .to_string()
                .contains("Previous refusal must remain context"),
            "{to:?}: {converted}"
        );
    }
    let rs = json!({"input":[{"role":"assistant","type":"message","content":[{"type":"refusal","refusal":"Previous refusal must remain context"}]},{"role":"user","content":"new request"}]});
    for to in [AM, CC] {
        let converted = BuiltinConversionPlugin
            .convert_request(RS, to, rs.clone())
            .unwrap();
        assert!(converted
            .to_string()
            .contains("Previous refusal must remain context"));
    }
}

#[test]
fn tool_selection_restrictions_are_preserved_or_rejected() {
    for (from, to, body) in [
        (
            RS,
            AM,
            json!({"input":"hello","tool_choice":{"type":"allowed_tools","mode":"required","tools":[{"type":"function","name":"read"}]}}),
        ),
        (
            AM,
            CC,
            json!({"messages":[],"tool_choice":{"type":"unknown_restriction"}}),
        ),
        (
            CC,
            AM,
            json!({"messages":[],"tool_choice":"unknown_restriction"}),
        ),
    ] {
        assert!(matches!(
            BuiltinConversionPlugin.convert_request(from, to, body),
            Err(ConversionError::UnsupportedFeature { .. })
        ));
    }
    let converted=BuiltinConversionPlugin.convert_request(CC,AM,json!({"messages":[],"tools":[{"type":"custom","custom":{"name":"patch"}}],"tool_choice":{"type":"custom","custom":{"name":"patch"}}})).unwrap();
    assert_eq!(
        converted["tool_choice"],
        json!({"type":"tool","name":"patch"})
    );
}

#[test]
fn sse_control_frames_are_harmless_in_every_conversion_direction() {
    for from in [AM, CC, RS] {
        for to in [AM, CC, RS] {
            let mut converter = StreamConverter::new(from, to).unwrap();
            for frame in [
                ": keep-alive\r\n\r\n",
                "id: fixture\nretry: 1000\n\n",
                "event: ping\n\n",
                "unknown: ignored\n\n",
            ] {
                assert!(
                    converter.push_event(frame).is_ok(),
                    "{from:?}->{to:?}: {frame}"
                );
            }
            let terminal = match from {
                AM => json!({"type":"message_stop"}),
                CC => {
                    json!({"id":"chatcmpl_fixture","choices":[{"index":0,"delta":{"content":"answer"},"finish_reason":"stop"}]})
                }
                RS => {
                    json!({"type":"response.completed","response":{"id":"resp_fixture","status":"completed","output":[]}})
                }
            };
            converter
                .push_event(&format!("data: {terminal}\n\n"))
                .unwrap();
            assert!(converter.finish().is_ok(), "{from:?}->{to:?}");
        }
    }
}

#[test]
fn streams_ignore_null_errors_but_reject_failed_envelopes() {
    for from in [AM, CC, RS] {
        let terminal = match from {
            AM => json!({"type":"message_stop","error":null}),
            CC => {
                json!({"id":"chatcmpl_fixture","choices":[{"index":0,"delta":{"content":"answer"},"finish_reason":"stop"}],"error":null})
            }
            RS => {
                json!({"type":"response.completed","response":{"status":"completed"},"error":null})
            }
        };
        for to in [AM, CC, RS] {
            for prefix in [
                "",
                "event: message\n",
                "event: error\nevent:\n",
                "event: error\nevent\n",
            ] {
                let mut converter = StreamConverter::new(from, to).unwrap();
                let mut output = converter
                    .push_event(&format!("{prefix}data: {terminal}\n\n"))
                    .unwrap();
                output.extend(converter.finish().unwrap());
                assert!(
                    !events(output).iter().any(|v| v["type"] == "error"
                        || v["type"] == "response.failed"
                        || v.get("error").is_some_and(|e| !e.is_null())),
                    "{from:?}->{to:?}"
                );
            }
        }
    }
}

#[test]
fn explicit_failures_with_missing_details_still_deliver_non_null_errors() {
    for from in [AM, CC, RS] {
        for to in [AM, CC, RS] {
            for frame in [
                "event: error\ndata: {\"message\":\"upstream failed\"}\n\n",
                "event: message\ndata: {\"type\":\"error\",\"error\":null}\n\n",
                "data: {\"type\":\"error\",\"error\":null}\n\n",
                "data: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"error\":null}}\n\n",
            ] {
                let mut converter = StreamConverter::new(from, to).unwrap();
                if let Ok(output) = converter.push_event(frame) {
                    let values = events(output);
                    assert!(values.iter().any(|v| v.get("error").is_some_and(|e| !e.is_null()) || v.pointer("/response/error").is_some_and(|e| !e.is_null())), "{from:?}->{to:?}: {values:?}");
                    assert!(!values.iter().any(|v| v["type"] == "response.completed" || v["type"] == "message_stop"));
                }
            }
        }
    }
}
