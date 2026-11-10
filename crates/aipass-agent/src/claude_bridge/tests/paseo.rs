//! Regression scenarios compared with Paseo's native Claude SDK adapter.
//! These processes are handwritten fixtures; they cannot reach a provider.
use super::*;

async fn wait_file(path: &Path) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}
fn append(payload: &mut Value, response: &Value, user: Value) {
    payload["messages"].as_array_mut().unwrap().extend([
        json!({"role":"assistant","content":response["content"]}),
        json!({"role":"user","content":user}),
    ]);
}
fn reply(id: &Value, value: Value) -> Value {
    json!([{"type":"tool_result","tool_use_id":id,"content":value.to_string()}])
}
fn allow() -> Value {
    json!({"outcome":{"outcome":"selected","optionId":"allow_once"}})
}
fn payload(dir: &Path) -> Value {
    json!({"model":"fixture","_aipass_claude_session":{"cwd":dir,"permissionMode":"default"},"messages":[{"role":"user","content":"go"}]})
}

#[tokio::test]
async fn idle_child_permission_does_not_invent_a_main_turn_that_never_completes() {
    let script = r#"read -r input
cat <<'EVENTS'
{"type":"system","subtype":"background_tasks_changed","tasks":[{"task_id":"bg","task_type":"local_agent"}]}
{"type":"result","is_error":false,"result":"background started"}
EVENTS
while [ ! -f .wake ]; do sleep 0.01; done
printf '%s\n' '{"type":"control_request","request_id":"background-permission","request":{"subtype":"can_use_tool","tool_name":"Read","input":{"file_path":"fixture"}}}'
read -r input
printf '%s\n' "$input" > next.json
read -r permission
printf '%s\n' "$permission" > permission.json
cat <<'EVENTS'
{"type":"system","subtype":"background_tasks_changed","tasks":[]}
{"type":"system","subtype":"task_notification","task_id":"bg","status":"completed","summary":"background read finished"}
{"type":"result","is_error":false,"result":"foreground completed"}
EVENTS
while read -r input; do :; done
"#;
    let (dir, bridge, target) = fixture(script);
    let session = Some("idle-permission".into());
    let mut payload = payload(dir.path());
    let first = collect(bridge.open(&target, &payload, session.clone()).unwrap()).await;
    append(&mut payload, &first, json!("follow up"));
    std::fs::write(dir.path().join(".wake"), b"").unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !bridge
            .inner
            .runs
            .lock()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .deferred
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let permission = collect(bridge.open(&target, &payload, session.clone()).unwrap()).await;
    wait_file(&dir.path().join("next.json")).await;
    let input: Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("next.json")).unwrap()).unwrap();
    assert_eq!(input["type"], "user");
    assert!(input["message"]["content"]
        .to_string()
        .contains("follow up"));
    append(
        &mut payload,
        &permission,
        reply(&permission["content"][0]["id"], allow()),
    );
    let done = collect(bridge.open(&target, &payload, session).unwrap()).await;
    assert!(done["content"]
        .to_string()
        .contains("background read finished"));
    assert!(done["content"].to_string().contains("foreground completed"));
    bridge.revoke();
}

#[tokio::test]
async fn question_answers_are_validated_before_consuming_the_permission() {
    let script = r#"read -r input
cat <<'EVENTS'
{"type":"control_cancel_request","request_id":"unknown-old-request"}
{"type":"control_request","request_id":"question","request":{"subtype":"can_use_tool","tool_name":"AskUserQuestion","input":{"questions":[{"question":"Which branch?","header":"Branch","options":[{"label":"main"}],"multiSelect":false}]}}}
EVENTS
read -r response
printf '%s\n' "$response" > reply.json
cat <<'EVENTS'
{"type":"control_cancel_request","request_id":"question"}
{"type":"result","subtype":"success","is_error":false,"result":"answer accepted"}
EVENTS
while read -r input; do :; done
"#;
    let (dir, bridge, target) = fixture(script);
    let mut payload = payload(dir.path());
    let session = Some("question".into());
    let first = collect(bridge.open(&target, &payload, session.clone()).unwrap()).await;
    let id = &first["content"][0]["id"];
    append(&mut payload, &first, reply(id, allow()));
    assert!(bridge.open(&target, &payload, session.clone()).is_err());
    assert!(!dir.path().join("reply.json").exists());
    let mut answered = allow();
    answered["_meta"] =
        json!({"aipass":{"updatedInput":{"answers":{"Branch":"main"},"questions":[]}}});
    payload["messages"][2]["content"] = reply(id, answered);
    let last = collect(bridge.open(&target, &payload, session).unwrap()).await;
    assert!(last["content"].to_string().contains("answer accepted"));
    let response: Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("reply.json")).unwrap()).unwrap();
    let input = &response["response"]["response"]["updatedInput"];
    assert_eq!(input["answers"], json!({"Which branch?":"main"}));
    assert_eq!(input["questions"][0]["question"], "Which branch?");
    assert_eq!(input["questions"][0]["multiSelect"], false);
    bridge.revoke();
}

#[tokio::test]
async fn plan_approval_waits_for_the_native_permission_mode_acknowledgement() {
    for accepted in [true, false] {
        let script = r#"read -r input
printf '%s\n' '{"type":"control_request","request_id":"plan","request":{"subtype":"can_use_tool","tool_name":"ExitPlanMode","input":{"plan":"write test"}}}'
read -r mode
printf '%s\n' "$mode" > mode.json
mode_id=$(printf '%s' "$mode" | sed -n 's/.*"request_id":"\([^"]*\)".*/\1/p')
while [ ! -f .ack ]; do sleep 0.01; done
status=$(cat .ack)
printf '{"type":"control_response","response":{"subtype":"%s","request_id":"%s","response":{}}}\n' "$status" "$mode_id"
read -r response
printf '%s\n' "$response" > reply.json
printf '%s\n' '{"type":"result","is_error":false,"result":"plan executed"}'
while read -r input; do :; done
"#;
        let (dir, bridge, target) = fixture(script);
        let mut payload = payload(dir.path());
        payload["_aipass_claude_session"]["permissionMode"] = json!("plan");
        let session = Some("plan".into());
        let first = collect(bridge.open(&target, &payload, session.clone()).unwrap()).await;
        assert_eq!(
            first["content"][0]["input"]["request"]["params"]["options"][0]["name"],
            "Approve plan and accept edits"
        );
        append(
            &mut payload,
            &first,
            reply(&first["content"][0]["id"], allow()),
        );
        let mut stream = bridge.open(&target, &payload, session).unwrap();
        wait_file(&dir.path().join("mode.json")).await;
        let mode: Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("mode.json")).unwrap()).unwrap();
        assert_eq!(
            mode["request"],
            json!({"subtype":"set_permission_mode","mode":"acceptEdits"})
        );
        assert!(!dir.path().join("reply.json").exists());
        std::fs::write(
            dir.path().join(".ack"),
            if accepted { "success" } else { "error" },
        )
        .unwrap();
        if accepted {
            let final_response = collect(stream).await;
            assert!(final_response["content"]
                .to_string()
                .contains("plan executed"));
            let response: Value =
                serde_json::from_slice(&std::fs::read(dir.path().join("reply.json")).unwrap())
                    .unwrap();
            assert_eq!(response["response"]["request_id"], "plan");
            assert_eq!(response["response"]["response"]["behavior"], "allow");
        } else {
            assert!(tokio::time::timeout(Duration::from_secs(5), stream.next())
                .await
                .unwrap()
                .unwrap()
                .is_err());
            assert!(!dir.path().join("reply.json").exists());
        }
        bridge.revoke();
    }
}

#[tokio::test]
async fn child_caller_tools_survive_interleaved_parent_text_and_child_failure() {
    let script = r#"read -r input
cat <<'EVENTS'
{"type":"stream_event","event":{"type":"message_start","message":{"id":"parent","role":"assistant","usage":{}}}}
{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":"parent before"}}}
{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" and after"}}}
{"type":"stream_event","event":{"type":"content_block_stop","index":0}}
{"type":"stream_event","parent_tool_use_id":"task1","event":{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"child-private-reasoning"}}}
{"type":"assistant","parent_tool_use_id":"task1","message":{"id":"child","role":"assistant","content":[{"type":"text","text":"child-private-text"},{"type":"tool_use","id":"child1","name":"mcp__aipass__lookup","input":{"n":1}},{"type":"tool_use","id":"child2","name":"mcp__aipass__lookup","input":{"n":2}}]}}
{"type":"control_request","request_id":"child-permission","request":{"subtype":"can_use_tool","tool_name":"Read","tool_use_id":"child-read","input":{"file_path":"fixture"}}}
{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{}}}
{"type":"stream_event","event":{"type":"message_stop"}}
EVENTS
read -r permission
printf '%s\n' "$permission" > permission.json
while [ ! -f .resume ]; do sleep 0.01; done
cat <<'EVENTS'
{"type":"result","parent_tool_use_id":"task1","is_error":true}
{"type":"assistant","message":{"id":"recovered","role":"assistant","content":[{"type":"text","text":"parent recovered"}],"stop_reason":"end_turn","usage":{}}}
{"type":"result","is_error":false}
EVENTS
while read -r input; do :; done
"#;
    let (dir, bridge, target) = fixture(script);
    let mut payload = payload(dir.path());
    payload["tools"] = json!([{"name":"lookup","input_schema":{"type":"object"}}]);
    let session = Some("child-tools".into());
    let first = collect(bridge.open(&target, &payload, session.clone()).unwrap()).await;
    assert_eq!(first["stop_reason"], "tool_use");
    assert_eq!(first["content"][0]["text"], "parent before and after");
    assert!(!first.to_string().contains("child-private"));
    let calls = first["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|b| b["type"] == "tool_use")
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 2);
    let key = bridge
        .inner
        .runs
        .lock()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    for (index, call) in calls.iter().enumerate() {
        let claimed = bridge
            .mcp(
                &key,
                ClaudeBridgeRequest::Call {
                    name: "lookup".into(),
                    arguments: json!({"n":index+1}),
                },
            )
            .unwrap();
        assert_eq!(claimed["callId"], call["id"]);
    }
    let results = calls
        .iter()
        .map(|c| json!({"type":"tool_result","tool_use_id":c["id"],"content":"found"}))
        .collect::<Vec<_>>();
    append(&mut payload, &first, json!(results));
    let stream = bridge.open(&target, &payload, session.clone()).unwrap();
    for call in calls {
        let polled = bridge
            .mcp(
                &key,
                ClaudeBridgeRequest::Poll {
                    call_id: call["id"].as_str().unwrap().into(),
                },
            )
            .unwrap();
        assert_eq!(polled["result"]["content"][0]["text"], "found");
    }
    let permission = collect(stream).await;
    assert_eq!(permission["content"][0]["name"], "aipass_acp_respond");
    assert_eq!(
        permission["content"][0]["input"]["request"]["id"],
        "child-permission"
    );
    append(
        &mut payload,
        &permission,
        reply(&permission["content"][0]["id"], allow()),
    );
    let stream = bridge.open(&target, &payload, session).unwrap();
    std::fs::write(dir.path().join(".resume"), b"").unwrap();
    let last = collect(stream).await;
    assert!(last["content"].to_string().contains("parent recovered"));
    let permission: Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("permission.json")).unwrap())
            .unwrap();
    assert_eq!(permission["response"]["response"]["behavior"], "allow");
    bridge.revoke();
}

#[tokio::test]
async fn autonomous_work_keeps_ownership_and_cannot_complete_the_next_foreground_prompt() {
    let script = r#"read -r input
cat <<'EVENTS'
{"type":"system","subtype":"background_tasks_changed","tasks":[{"task_id":"bg","task_type":"local_bash"}]}
{"type":"result","is_error":false,"result":"started background command"}
EVENTS
while [ ! -f .background ]; do sleep 0.01; done
cat <<'EVENTS'
{"type":"system","subtype":"background_tasks_changed","tasks":[]}
{"type":"system","subtype":"task_notification","task_id":"bg","status":"completed","summary":"wrote background output","output_file":"/synthetic/task"}
{"type":"assistant","message":{"id":"autonomous","role":"assistant","content":[{"type":"text","text":"autonomous answer"}],"stop_reason":"end_turn","usage":{}}}
EVENTS
touch .autonomous
while [ ! -f .settle ]; do sleep 0.01; done
printf '%s\n' '{"type":"result","is_error":false,"result":"autonomous answer"}'
read -r input
printf '%s\n' "$input" > next.json
printf '%s\n' '{"type":"result","is_error":false,"result":"next foreground answer"}'
while read -r input; do :; done
"#;
    let (dir, bridge, target) = fixture(script);
    let mut payload = payload(dir.path());
    let session = Some("background".into());
    let first = collect(bridge.open(&target, &payload, session.clone()).unwrap()).await;
    append(&mut payload, &first, json!("next foreground question"));
    assert!(bridge.active(target.config.provider_entry_id));
    assert_eq!(
        bridge
            .history_owner_for_session(&payload, std::slice::from_ref(&target), session.as_deref())
            .unwrap(),
        Some(target.config.id)
    );
    let mut changed = payload.clone();
    changed["messages"][0]["content"] = json!("rewritten history");
    assert!(bridge.open(&target, &changed, session.clone()).is_err());
    let key = bridge
        .inner
        .runs
        .lock()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    let pid = bridge.inner.runs.lock().unwrap()[&key].child.id();
    std::fs::write(dir.path().join(".background"), b"").unwrap();
    wait_file(&dir.path().join(".autonomous")).await;
    // The reader records autonomous work before publishing its buffered frames.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let ready = {
                let runs = bridge.inner.runs.lock().unwrap();
                let run = &runs[&key];
                run.deferred && run.acp.background_active()
            };
            if ready {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let mut stream = bridge.open(&target, &payload, session.clone()).unwrap();
    let mut collector = Collector::default();
    while !collector.message.to_string().contains("autonomous answer") {
        let bytes = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        collector.push(&bytes).unwrap();
        assert!(!collector.complete);
    }
    assert!(!dir.path().join("next.json").exists());
    std::fs::write(dir.path().join(".settle"), b"").unwrap();
    while let Some(bytes) = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .unwrap()
    {
        collector.push(&bytes.unwrap()).unwrap();
    }
    assert!(collector.complete);
    let text = collector.message["content"].to_string();
    assert!(text.contains("wrote background output"));
    assert!(text.contains("autonomous answer"));
    assert!(text.contains("next foreground answer"));
    assert_eq!(text.matches("autonomous answer").count(), 1);
    assert_eq!(bridge.inner.runs.lock().unwrap()[&key].child.id(), pid);
    assert_eq!(
        bridge
            .history_owner_for_session(&payload, std::slice::from_ref(&target), session.as_deref())
            .unwrap(),
        None
    );
    bridge.revoke();
}
