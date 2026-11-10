//! Every child is an executable shell fixture. No installed CLI or account.
use super::super::tests::{collect, fixture};
use super::*;

#[tokio::test]
async fn native_permission_round_trip_keeps_tools_and_never_reexecutes_them_in_the_client() {
    let script = r#"case " $* " in *" --tools default "*) ;; *) exit 92;; esac
read -r prompt
cat <<'EVENTS'
{"type":"stream_event","event":{"type":"message_start","message":{"id":"native","role":"assistant","usage":{"input_tokens":2}}}}
{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"write1","name":"Write","input":{"file_path":"demo","content":"ok"}}}}
{"type":"stream_event","event":{"type":"content_block_stop","index":0}}
{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":3}}}
{"type":"stream_event","event":{"type":"message_stop"}}
{"type":"assistant","message":{"content":[{"type":"tool_use","id":"write1","name":"Write","input":{"file_path":"demo","content":"ok"}}]}}
{"type":"control_request","request_id":"permission1","request":{"subtype":"can_use_tool","tool_use_id":"write1","tool_name":"Write","input":{"file_path":"demo","content":"ok"}}}
EVENTS
read -r response
printf '%s\n' "$response" > reply.json
cat <<'EVENTS'
{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"write1","content":"wrote demo"}]}}
{"type":"stream_event","event":{"type":"message_start","message":{"id":"final","role":"assistant","usage":{"input_tokens":5}}}}
{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":"Finished."}}}
{"type":"stream_event","event":{"type":"content_block_stop","index":0}}
{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":7}}}
{"type":"stream_event","event":{"type":"message_stop"}}
{"type":"result","is_error":false}
EVENTS
while read -r input; do :; done
"#;
    let (dir, bridge, target) = fixture(script);
    let mut payload = json!({"model":"fixture","stream":true,"_aipass_claude_session":{"cwd":dir.path(),"permissionMode":"default"},"messages":[{"role":"user","content":"write demo"}]});
    let session = Some("permission-session".to_owned());
    let first = collect(bridge.open(&target, &payload, session.clone()).unwrap()).await;
    assert_eq!(first["stop_reason"], "tool_use");
    let block = &first["content"][0];
    assert_eq!(block["name"], "aipass_acp_respond");
    assert_eq!(
        block["input"]["request"]["method"],
        "session/request_permission"
    );
    assert_eq!(
        block["input"]["request"]["params"]["toolCall"]["title"],
        "Write"
    );
    let id = block["id"].as_str().unwrap();
    payload["messages"]
        .as_array_mut()
        .unwrap()
        .push(json!({"role":"assistant","content":first["content"]}));
    payload["messages"].as_array_mut().unwrap().push(json!({"role":"user","content":[{"type":"tool_result","tool_use_id":id,"content":"{\"outcome\":{\"outcome\":\"selected\",\"optionId\":\"invented\"}}"}]}));
    assert!(bridge.open(&target, &payload, session.clone()).is_err());
    assert!(!dir.path().join("reply.json").exists());
    payload["messages"][2]["content"][0]["content"] = json!(json!({"outcome":{"outcome":"selected","optionId":"allow_once"},"_meta":{"aipass":{"updatedInput":{"file_path":"demo","content":"approved"}}}}).to_string());
    let mut tampered = payload.clone();
    tampered["messages"][0]["content"] = json!("changed history");
    assert!(bridge.open(&target, &tampered, session.clone()).is_err());
    let result = collect(bridge.open(&target, &payload, session.clone()).unwrap()).await;
    let reply: Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("reply.json")).unwrap()).unwrap();
    assert_eq!(reply["response"]["response"]["behavior"], "allow");
    assert_eq!(
        reply["response"]["response"]["updatedInput"]["content"],
        "approved"
    );
    assert!(result["content"]
        .as_array()
        .unwrap()
        .iter()
        .all(|b| b["type"] == "text"));
    assert!(result["content"].to_string().contains("already executed"));
    assert!(result["content"].to_string().contains("wrote demo"));
    assert!(result["content"].to_string().contains("Finished."));
    assert!(bridge.open(&target, &payload, session).is_err());
    bridge.revoke();
}

#[tokio::test]
async fn matching_history_reuses_a_session_but_foreign_turns_rebuild_with_full_context() {
    let script = r#"while read -r input; do
printf '%s\n' "$input" > prompt.json
cat <<'EVENTS'
{"type":"stream_event","event":{"type":"message_start","message":{"id":"msg","role":"assistant","usage":{}}}}
{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":"answer"}}}
{"type":"stream_event","event":{"type":"content_block_stop","index":0}}
{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{}}}
{"type":"stream_event","event":{"type":"message_stop"}}
{"type":"result","is_error":false}
EVENTS
done
"#;
    let (dir, bridge, target) = fixture(script);
    let session = Some("handoff-session".to_owned());
    let mut payload = json!({"model":"fixture","_aipass_claude_session":{"cwd":dir.path(),"permissionMode":"plan"},"messages":[{"role":"user","content":"first"}]});
    let one = collect(bridge.open(&target, &payload, session.clone()).unwrap()).await;
    let pid = bridge
        .inner
        .runs
        .lock()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .child
        .id();
    payload["messages"].as_array_mut().unwrap().extend([
        json!({"role":"assistant","content":one["content"]}),
        json!({"role":"user","content":"second"}),
    ]);
    let two = collect(bridge.open(&target, &payload, session.clone()).unwrap()).await;
    assert_eq!(
        bridge
            .inner
            .runs
            .lock()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .child
            .id(),
        pid
    );
    let sent = std::fs::read_to_string(dir.path().join("prompt.json")).unwrap();
    assert!(sent.contains("second"));
    assert!(!sent.contains("first"));
    payload["messages"].as_array_mut().unwrap().extend([
        json!({"role":"assistant","content":two["content"]}),
        json!({"role":"user","content":"ask openai"}),
        json!({"role":"assistant","content":"openai changed the design"}),
        json!({"role":"user","content":"continue with claude"}),
    ]);
    collect(bridge.open(&target, &payload, session).unwrap()).await;
    let runs = bridge.inner.runs.lock().unwrap();
    assert_eq!(runs.len(), 1);
    assert_ne!(runs.values().next().unwrap().child.id(), pid);
    drop(runs);
    let sent = std::fs::read_to_string(dir.path().join("prompt.json")).unwrap();
    assert!(
        sent.contains("first")
            && sent.contains("openai changed the design")
            && sent.contains("continue with claude")
    );
    bridge.revoke();
}

#[tokio::test]
async fn failed_native_result_cannot_be_reported_as_a_successful_turn() {
    let script = r#"read -r input
cat <<'EVENTS'
{"type":"stream_event","event":{"type":"message_start","message":{"id":"failed","role":"assistant","usage":{}}}}
{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{}}}
{"type":"stream_event","event":{"type":"message_stop"}}
{"type":"result","is_error":true}
EVENTS
while read -r input; do :; done
"#;
    let (_dir, bridge, target) = fixture(script);
    let mut stream = bridge
        .open(
            &target,
            &json!({"model":"fixture","messages":[{"role":"user","content":"fail"}]}),
            None,
        )
        .unwrap();
    let mut failed = false;
    while let Some(chunk) = tokio::time::timeout(Duration::from_secs(3), stream.next())
        .await
        .unwrap()
    {
        if chunk.is_err() {
            failed = true;
        } else {
            assert!(!String::from_utf8_lossy(&chunk.unwrap()).contains("message_stop"));
        }
    }
    assert!(failed);
    assert!(bridge.inner.runs.lock().unwrap().is_empty());
}

#[tokio::test]
async fn local_commands_and_nonstreamed_results_are_not_empty_or_duplicated() {
    for command in [false, true] {
        let script = format!("read -r prompt\n{}\nprintf '%s\\n' '{{\"type\":\"result\",\"is_error\":false,\"result\":\"command reply\"}}'\nwhile read -r input; do :; done\n", if command {"printf '%s\\n' '{\"type\":\"system\",\"subtype\":\"local_command_output\",\"content\":\"command reply\"}'"}else{""});
        let (_dir, bridge, target) = fixture(&script);
        let result = collect(
            bridge
                .open(
                    &target,
                    &json!({"model":"fixture","messages":[{"role":"user","content":"/fixture"}]}),
                    None,
                )
                .unwrap(),
        )
        .await;
        assert_eq!(result["content"].as_array().unwrap().len(), 1);
        assert_eq!(result["content"][0]["text"], "command reply");
        bridge.revoke();
    }
}

#[tokio::test]
async fn complete_assistant_frames_preserve_caller_tools_without_partial_events() {
    let script = r#"read -r prompt
printf '%s\n' '{"type":"assistant","message":{"id":"complete","role":"assistant","content":[{"type":"text","text":"lookup"},{"type":"tool_use","id":"native-call","name":"mcp__aipass__lookup","input":{"n":1}}],"stop_reason":"tool_use","usage":{"input_tokens":2,"output_tokens":3}}}'
while read -r input; do :; done
"#;
    let (_dir, bridge, target) = fixture(script);
    let result=collect(bridge.open(&target,&json!({"model":"fixture","tools":[{"name":"lookup","input_schema":{"type":"object"}}],"messages":[{"role":"user","content":"go"}]}),None).unwrap()).await;
    assert_eq!(result["content"][0]["text"], "lookup");
    assert_eq!(result["content"][1]["name"], "lookup");
    assert_eq!(result["content"][1]["input"]["n"], 1);
    assert_eq!(result["stop_reason"], "tool_use");
    bridge.revoke();
}

#[tokio::test]
async fn native_tool_output_waits_for_the_parked_caller_segment() {
    let script = r#"read -r prompt
cat <<'EVENTS'
{"type":"stream_event","event":{"type":"message_start","message":{"id":"parallel","role":"assistant","usage":{}}}}
{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"native","name":"Read","input":{"file_path":"fixture"}}}}
{"type":"stream_event","event":{"type":"content_block_stop","index":0}}
{"type":"stream_event","event":{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"caller","name":"mcp__aipass__lookup","input":{}}}}
{"type":"stream_event","event":{"type":"content_block_stop","index":1}}
{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{}}}
{"type":"stream_event","event":{"type":"message_stop"}}
{"type":"assistant","message":{"content":[{"type":"tool_use","id":"native","name":"Read","input":{"file_path":"fixture"}}]}}
{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"native","content":"parallel read finished"}]}}
EVENTS
while [ ! -f resume ]; do sleep 0.01; done
printf '%s\n' '{"type":"result","is_error":false,"result":"both tools done"}'
while read -r input; do :; done
"#;
    let (dir, bridge, target) = fixture(script);
    let session = Some("parallel".into());
    let mut payload = json!({"model":"fixture","_aipass_claude_session":{"cwd":dir.path(),"permissionMode":"default"},"tools":[{"name":"lookup","input_schema":{"type":"object"}}],"messages":[{"role":"user","content":"parallel"}]});
    let first = collect(bridge.open(&target, &payload, session.clone()).unwrap()).await;
    assert_eq!(first["content"].as_array().unwrap().len(), 1);
    let id = first["content"][0]["id"].clone();
    payload["messages"].as_array_mut().unwrap().extend([json!({"role":"assistant","content":first["content"]}),json!({"role":"user","content":[{"type":"tool_result","tool_use_id":id,"content":"caller complete"}]})]);
    let stream = bridge.open(&target, &payload, session).unwrap();
    std::fs::write(dir.path().join("resume"), "").unwrap();
    let second = collect(stream).await;
    assert!(second["content"]
        .to_string()
        .contains("parallel read finished"));
    assert!(second["content"].to_string().contains("both tools done"));
    bridge.revoke();
}
