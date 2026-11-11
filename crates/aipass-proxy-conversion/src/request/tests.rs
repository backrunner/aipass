use super::*;
use serde_json::json;

fn am_request() -> Value {
    json!({
        "model": "claude-sonnet-4-5",
        "system": [{"type": "text", "text": "You are helpful."}],
        "messages": [
            {"role": "user", "content": [
                {"type": "text", "text": "What is in this image?"},
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "aGVsbG8="}}
            ]},
            {"role": "assistant", "content": [
                {"type": "text", "text": "Let me look."},
                {"type": "tool_use", "id": "toolu_1", "name": "describe", "input": {"detail": "high"}}
            ]},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_1", "content": [{"type": "text", "text": "A cat."}]}
            ]}
        ],
        "tools": [{"name": "describe", "description": "Describe an image", "input_schema": {"type": "object", "properties": {"detail": {"type": "string"}}}}],
        "tool_choice": {"type": "tool", "name": "describe"},
        "max_tokens": 1024,
        "stop_sequences": ["END"],
        "temperature": 0.5,
        "top_p": 0.9,
        "stream": true,
        "thinking": {"type": "enabled", "budget_tokens": 1000},
        "metadata": {"user_id": "u1"}
    })
}

#[test]
fn reasoning_and_tool_images_survive_conversion_without_foreign_signatures() {
    let input = json!({"model":"m","thinking":{"type":"adaptive"},"messages":[
        {"role":"assistant","content":[{"type":"thinking","thinking":"prior reasoning"},{"type":"tool_use","id":"call1","name":"look","input":{}}]},
        {"role":"user","content":[{"type":"tool_result","tool_use_id":"call1","content":[{"type":"text","text":"see image"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"YWJj"}}]}]}
    ]});
    let chat = convert(AM, CC, input.clone()).unwrap();
    assert_eq!(chat["messages"][0]["reasoning_content"], "prior reasoning");
    assert_eq!(chat["messages"][1]["role"], "tool");
    assert_eq!(
        chat["messages"][2]["content"][0]["image_url"]["url"],
        "data:image/png;base64,YWJj"
    );
    assert_eq!(chat["reasoning_effort"], "high");
    let responses = convert(AM, RS, input).unwrap();
    assert_eq!(responses["input"][0]["type"], "reasoning");
    let back = convert(RS, AM, responses).unwrap();
    assert!(back.to_string().contains("YWJj"));
    assert!(back.to_string().contains("prior reasoning"));
    for signed in [
        json!({"type":"thinking","thinking":"x","signature":"native-proof"}),
        json!({"type":"redacted_thinking","data":"opaque"}),
    ] {
        assert!(convert(
            AM,
            CC,
            json!({"messages":[{"role":"assistant","content":[signed]}]})
        )
        .is_err());
    }
    assert!(convert(
        RS,
        AM,
        json!({"input":[{"type":"reasoning","encrypted_content":"opaque"}]})
    )
    .is_err());
}

#[test]
fn responses_system_and_developer_items_remain_system_instructions() {
    let out = rs_to_am(json!({
        "model":"m", "instructions":"top-level",
        "input":[
            {"role":"system","content":"system rule"},
            {"type":"message","role":"developer","content":[{"type":"input_text","text":"developer rule"}]},
            {"role":"user","content":"hello"}
        ]
    })).unwrap();
    assert_eq!(out["system"], "top-level\nsystem rule\ndeveloper rule");
    assert_eq!(
        out["messages"],
        json!([{ "role":"user", "content":[{"type":"text","text":"hello"}] }])
    );
}

#[test]
fn am_to_cc_full_request() {
    let out = am_to_cc(am_request()).unwrap();
    assert_eq!(out["model"], "claude-sonnet-4-5");
    assert_eq!(out["max_tokens"], 1024);
    assert_eq!(out["stop"], json!(["END"]));
    assert_eq!(out["temperature"], 0.5);
    assert_eq!(out["top_p"], 0.9);
    assert_eq!(out["stream"], true);
    assert!(out.get("thinking").is_none());
    assert!(out.get("metadata").is_none());

    let messages = out["messages"].as_array().unwrap();
    assert_eq!(
        messages[0],
        json!({"role": "system", "content": "You are helpful."})
    );
    assert_eq!(
        messages[1],
        json!({"role": "user", "content": [
            {"type": "text", "text": "What is in this image?"},
            {"type": "image_url", "image_url": {"url": "data:image/png;base64,aGVsbG8="}}
        ]})
    );
    assert_eq!(
        messages[2],
        json!({"role": "assistant", "content": "Let me look.", "tool_calls": [
            {"id": "toolu_1", "type": "function", "function": {"name": "describe", "arguments": "{\"detail\":\"high\"}"}}
        ]})
    );
    assert_eq!(
        messages[3],
        json!({"role": "tool", "tool_call_id": "toolu_1", "content": "A cat."})
    );
    assert_eq!(
        out["tools"],
        json!([{"type": "function", "function": {"name": "describe", "description": "Describe an image", "parameters": {"type": "object", "properties": {"detail": {"type": "string"}}}}}])
    );
    assert_eq!(
        out["tool_choice"],
        json!({"type": "function", "function": {"name": "describe"}})
    );
}

#[test]
fn am_to_cc_tool_choice_variants_and_string_system() {
    let base =
        json!({"model": "m", "system": "sys", "messages": [{"role": "user", "content": "hi"}]});
    let mut with_auto = base.clone();
    with_auto["tool_choice"] = json!({"type": "auto"});
    let out = am_to_cc(with_auto).unwrap();
    assert_eq!(out["tool_choice"], json!("auto"));
    assert_eq!(
        out["messages"][0],
        json!({"role": "system", "content": "sys"})
    );

    let mut with_any = base.clone();
    with_any["tool_choice"] = json!({"type": "any"});
    assert_eq!(
        am_to_cc(with_any).unwrap()["tool_choice"],
        json!("required")
    );

    let mut with_none = base;
    with_none["tool_choice"] = json!({"type": "none"});
    assert_eq!(am_to_cc(with_none).unwrap()["tool_choice"], json!("none"));
}

#[test]
fn am_to_rs_full_request() {
    let out = am_to_rs(am_request()).unwrap();
    assert_eq!(out["instructions"], "You are helpful.");
    assert_eq!(out["max_output_tokens"], 1024);
    assert_eq!(out["model"], "claude-sonnet-4-5");
    assert_eq!(out["stream"], true);
    assert!(out.get("thinking").is_none());

    let input = out["input"].as_array().unwrap();
    assert_eq!(
        input[0],
        json!({"type": "message", "role": "user", "content": [
            {"type": "input_text", "text": "What is in this image?"},
            {"type": "input_image", "image_url": "data:image/png;base64,aGVsbG8="}
        ]})
    );
    assert_eq!(
        input[1],
        json!({"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Let me look."}]})
    );
    assert_eq!(
        input[2],
        json!({"type": "function_call", "call_id": "toolu_1", "name": "describe", "arguments": "{\"detail\":\"high\"}"})
    );
    assert_eq!(
        input[3],
        json!({"type": "function_call_output", "call_id": "toolu_1", "output": "A cat."})
    );
    assert_eq!(
        out["tools"],
        json!([{"type": "function", "name": "describe", "description": "Describe an image", "parameters": {"type": "object", "properties": {"detail": {"type": "string"}}}}])
    );
    assert_eq!(
        out["tool_choice"],
        json!({"type": "function", "name": "describe"})
    );
}

#[test]
fn cc_to_am_full_request() {
    let request = json!({
        "model": "gpt-5",
        "messages": [
            {"role": "system", "content": "Be terse."},
            {"role": "user", "content": [
                {"type": "text", "text": "Look:"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,aGVsbG8="}}
            ]},
            {"role": "assistant", "content": null, "tool_calls": [
                {"id": "call_1", "type": "function", "function": {"name": "describe", "arguments": "{\"detail\":\"high\"}"}}
            ]},
            {"role": "tool", "tool_call_id": "call_1", "content": "A cat."},
            {"role": "tool", "tool_call_id": "call_2", "content": "A dog."}
        ],
        "tools": [{"type": "function", "function": {"name": "describe", "description": "d", "parameters": {"type": "object"}}}],
        "tool_choice": "required",
        "stop": "END",
        "temperature": 0.2
    });
    let out = cc_to_am(request).unwrap();
    assert_eq!(out["model"], "gpt-5");
    assert_eq!(out["system"], "Be terse.");
    assert_eq!(out["max_tokens"], DEFAULT_MAX_TOKENS);
    assert_eq!(out["stop_sequences"], json!(["END"]));
    assert_eq!(out["temperature"], 0.2);

    let messages = out["messages"].as_array().unwrap();
    assert_eq!(
        messages[0],
        json!({"role": "user", "content": [
            {"type": "text", "text": "Look:"},
            {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "aGVsbG8="}}
        ]})
    );
    assert_eq!(
        messages[1],
        json!({"role": "assistant", "content": [
            {"type": "tool_use", "id": "call_1", "name": "describe", "input": {"detail": "high"}}
        ]})
    );
    // consecutive tool messages merge into one user message
    assert_eq!(
        messages[2],
        json!({"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "call_1", "content": "A cat."},
            {"type": "tool_result", "tool_use_id": "call_2", "content": "A dog."}
        ]})
    );
    assert_eq!(
        out["tools"],
        json!([{"name": "describe", "description": "d", "input_schema": {"type": "object"}}])
    );
    assert_eq!(out["tool_choice"], json!({"type": "any"}));
}

#[test]
fn cc_to_am_preserves_max_tokens_and_named_tool_choice() {
    let request = json!({
        "messages": [{"role": "user", "content": "hi"}],
        "max_completion_tokens": 512,
        "tool_choice": {"type": "function", "function": {"name": "f"}}
    });
    let out = cc_to_am(request).unwrap();
    assert_eq!(out["max_tokens"], 512);
    assert_eq!(out["tool_choice"], json!({"type": "tool", "name": "f"}));
}

#[test]
fn rs_to_am_full_request() {
    let request = json!({
        "model": "gpt-5",
        "instructions": "Be terse.",
        "input": [
            {"type": "message", "role": "user", "content": [
                {"type": "input_text", "text": "Look:"},
                {"type": "input_image", "image_url": "data:image/png;base64,aGVsbG8="}
            ]},
            {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "One moment."}]},
            {"type": "function_call", "call_id": "call_1", "name": "describe", "arguments": "{\"detail\":\"high\"}"},
            {"type": "function_call_output", "call_id": "call_1", "output": "A cat."},
            {"type": "reasoning", "summary": []}
        ],
        "tools": [{"type": "function", "name": "describe", "description": "d", "parameters": {"type": "object"}}],
        "max_output_tokens": 256
    });
    let out = rs_to_am(request).unwrap();
    assert_eq!(out["system"], "Be terse.");
    assert_eq!(out["max_tokens"], 256);

    let messages = out["messages"].as_array().unwrap();
    assert_eq!(
        messages[0],
        json!({"role": "user", "content": [
            {"type": "text", "text": "Look:"},
            {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "aGVsbG8="}}
        ]})
    );
    assert_eq!(
        messages[1],
        json!({"role": "assistant", "content": [
            {"type": "text", "text": "One moment."},
            {"type": "tool_use", "id": "call_1", "name": "describe", "input": {"detail": "high"}}
        ]})
    );
    assert_eq!(
        messages[2],
        json!({"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "call_1", "content": "A cat."}
        ]})
    );
    assert_eq!(
        out["tools"],
        json!([{"name": "describe", "description": "d", "input_schema": {"type": "object"}}])
    );
}

#[test]
fn rs_to_am_string_input_and_default_max_tokens() {
    let out = rs_to_am(json!({"model": "gpt-5", "input": "hello"})).unwrap();
    assert_eq!(out["max_tokens"], DEFAULT_MAX_TOKENS);
    assert_eq!(
        out["messages"],
        json!([{"role": "user", "content": [{"type": "text", "text": "hello"}]}])
    );
}

#[test]
fn responses_conversion_rejects_unsupported_tools_and_history_without_silently_dropping_them() {
    for kind in ["custom", "shell", "local_shell", "namespace", "web_search"] {
        let result = rs_to_am(json!({"model":"test", "input":"run a command",
            "tools":[{"type":"function","name":"supported","parameters":{}},
                {"type":kind,"name":"exec"}]}));
        assert!(
            result.is_err(),
            "unsupported tool {kind} must not disappear"
        );
    }
    for kind in [
        "custom_tool_call",
        "custom_tool_call_output",
        "shell_call",
        "shell_call_output",
        "local_shell_call",
    ] {
        let result = rs_to_am(json!({"model":"test", "input":[
            {"type":kind,"name":"exec","call_id":"call-1","input":"pwd","output":"/tmp"}]}));
        assert!(
            result.is_err(),
            "unsupported history {kind} must not disappear"
        );
    }
}

#[test]
fn missing_tool_call_ids_are_synthesized() {
    // History items without call ids still need valid tool_use ids so the
    // pairing survives the next round-trip.
    let out = rs_to_am(json!({
        "model": "m",
        "input": [{"type": "function_call", "name": "f", "arguments": "{}"}]
    }))
    .unwrap();
    assert_eq!(out["messages"][0]["content"][0]["id"], "toolu_conv_1");

    // Ids stay unique across messages in the same request.
    let out = am_to_cc(json!({
        "model": "m",
        "messages": [
            {"role": "assistant", "content": [
                {"type": "tool_use", "name": "f", "input": {}}
            ]},
            {"role": "assistant", "content": [
                {"type": "tool_use", "name": "g", "input": {}}
            ]}
        ]
    }))
    .unwrap();
    assert_eq!(out["messages"][0]["tool_calls"][0]["id"], "call_conv_0");
    assert_eq!(out["messages"][1]["tool_calls"][0]["id"], "call_conv_1");

    let out = am_to_rs(json!({
        "model": "m",
        "messages": [{"role": "assistant", "content": [
            {"type": "tool_use", "name": "f", "input": {}}
        ]}]
    }))
    .unwrap();
    assert_eq!(out["input"][0]["call_id"], "call_conv_0");
}

#[test]
fn cc_object_arguments_and_custom_tools_are_preserved() {
    // Providers may emit arguments as an object, and Codex-style custom
    // tools must map to a single-string-parameter function shape.
    let out = cc_to_am(json!({
        "model": "m",
        "messages": [{"role": "assistant", "tool_calls": [
            {"id": "c1", "type": "function", "function": {"name": "f", "arguments": {"a": 1}}},
            {"id": "c2", "type": "custom", "custom": {"name": "apply_patch", "input": "*** patch"}}
        ]}],
        "tools": [
            {"type": "function", "function": {"name": "f", "parameters": {"type": "object"}}},
            {"type": "custom", "custom": {"name": "apply_patch", "description": "edit files"}}
        ]
    }))
    .unwrap();
    let blocks = out["messages"][0]["content"].as_array().unwrap();
    assert_eq!(blocks[0]["input"], json!({"a": 1}));
    assert_eq!(
        blocks[1],
        json!({"type": "tool_use", "id": "c2", "name": "apply_patch", "input": {"input": "*** patch"}})
    );
    assert_eq!(
        out["tools"][1],
        json!({"name": "apply_patch", "description": "edit files", "input_schema": {"type": "object", "properties": {"input": {"type": "string"}}, "required": ["input"]}})
    );

    // Responses function_call items may also carry object arguments.
    let out = rs_to_am(json!({
        "model": "m",
        "input": [{"type": "function_call", "call_id": "c", "name": "f", "arguments": {"a": 1}}]
    }))
    .unwrap();
    assert_eq!(out["messages"][0]["content"][0]["input"], json!({"a": 1}));
}

#[test]
fn am_cc_am_round_trip_preserves_text_and_tools() {
    let request = json!({
        "model": "claude-sonnet-4-5",
        "system": "You are helpful.",
        "messages": [
            {"role": "user", "content": [{"type": "text", "text": "Hi"}]},
            {"role": "assistant", "content": [
                {"type": "text", "text": "Working."},
                {"type": "tool_use", "id": "toolu_1", "name": "f", "input": {"a": 1}}
            ]},
            {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_1", "content": "done"}]}
        ],
        "tools": [{"name": "f", "description": "d", "input_schema": {"type": "object", "properties": {"a": {"type": "number"}}}}],
        "max_tokens": 100
    });
    let back = cc_to_am(am_to_cc(request).unwrap()).unwrap();
    assert_eq!(back["system"], "You are helpful.");
    let messages = back["messages"].as_array().unwrap();
    assert_eq!(
        messages[0]["content"][0],
        json!({"type": "text", "text": "Hi"})
    );
    assert_eq!(
        messages[1]["content"][1],
        json!({"type": "tool_use", "id": "toolu_1", "name": "f", "input": {"a": 1}})
    );
    assert_eq!(
        messages[2]["content"][0],
        json!({"type": "tool_result", "tool_use_id": "toolu_1", "content": "done"})
    );
    assert_eq!(
        back["tools"][0]["input_schema"]["properties"]["a"],
        json!({"type": "number"})
    );
    assert_eq!(back["max_tokens"], 100);
}

#[test]
fn am_rs_am_round_trip_preserves_text_and_tools() {
    let request = json!({
        "model": "claude-sonnet-4-5",
        "system": "You are helpful.",
        "messages": [
            {"role": "user", "content": [{"type": "text", "text": "Hi"}]},
            {"role": "assistant", "content": [
                {"type": "tool_use", "id": "toolu_1", "name": "f", "input": {"a": 1}}
            ]},
            {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_1", "content": "done"}]}
        ],
        "tools": [{"name": "f", "description": "d", "input_schema": {"type": "object"}}],
        "max_tokens": 100
    });
    let back = rs_to_am(am_to_rs(request).unwrap()).unwrap();
    assert_eq!(back["system"], "You are helpful.");
    let messages = back["messages"].as_array().unwrap();
    assert_eq!(
        messages[0]["content"][0],
        json!({"type": "text", "text": "Hi"})
    );
    assert_eq!(
        messages[1]["content"][0],
        json!({"type": "tool_use", "id": "toolu_1", "name": "f", "input": {"a": 1}})
    );
    assert_eq!(
        messages[2]["content"][0],
        json!({"type": "tool_result", "tool_use_id": "toolu_1", "content": "done"})
    );
    assert_eq!(back["max_tokens"], 100);
}

#[test]
fn malformed_requests_are_invalid_payload() {
    assert!(matches!(
        am_to_cc(json!("nope")),
        Err(ConversionError::InvalidPayload { .. })
    ));
    assert!(matches!(
        am_to_cc(json!({"model": "m"})),
        Err(ConversionError::InvalidPayload { .. })
    ));
    assert!(matches!(
        cc_to_am(json!({"model": "m"})),
        Err(ConversionError::InvalidPayload { .. })
    ));
    assert!(matches!(
        cc_to_am(
            json!({"messages": [{"role": "assistant", "tool_calls": [{"function": {"name": "f", "arguments": "{oops"}}]}]})
        ),
        Err(ConversionError::InvalidPayload { .. })
    ));
    assert!(matches!(
        rs_to_am(
            json!({"input": [{"type": "function_call", "call_id": "c", "name": "f", "arguments": "not json"}]})
        ),
        Err(ConversionError::InvalidPayload { .. })
    ));
}
