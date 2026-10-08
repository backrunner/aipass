use super::*;
use std::os::unix::fs::PermissionsExt;
fn fixture(script: &str) -> (tempfile::TempDir, ClaudeBridge, ResolvedTarget) {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("claude");
    std::fs::write(&path, format!("#!/bin/sh\n{script}")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let bridge = ClaudeBridge::new(temp.path());
    *bridge.inner.binary.lock().unwrap() = Some(path);
    let config=serde_json::from_value(json!({"id":Uuid::new_v4(),"providerEntryId":Uuid::new_v4(),"secretId":"test","label":"test","baseUrl":"https://api.anthropic.com","authScheme":"bearer","enabled":true,"priority":0,"weight":1,"headers":[]})).unwrap();
    let target = ResolvedTarget {
        upstream_proxy: None,
        model_override: None,
        profile: Default::default(),
        upstream_kind: aipass_proxy::UpstreamKind::ClaudeSubscription,
        quota: Vec::new(),
        max_concurrent_requests: None,
        supports_websockets: false,
        config,
        api_key: "synthetic-token".into(),
    };
    (temp, bridge, target)
}
async fn collect(mut stream: SubscriptionStream) -> Value {
    let mut collector = Collector::default();
    while let Some(chunk) = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .unwrap()
    {
        collector.push(&chunk.unwrap()).unwrap();
    }
    assert!(collector.complete);
    collector.message
}
#[tokio::test]
async fn parallel_tools_resume_original_process_and_reject_partial_or_replayed_results() {
    let script = r#"read -r input
cat <<'EVENTS'
{"type":"stream_event","event":{"type":"message_start","message":{"id":"msg_1","role":"assistant","usage":{"input_tokens":10}}}}
{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"native_a","name":"mcp__aipass__lookup","input":{"n":1}}}}
{"type":"stream_event","event":{"type":"content_block_stop","index":0}}
{"type":"stream_event","event":{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"native_b","name":"mcp__aipass__lookup","input":{"n":2}}}}
{"type":"stream_event","event":{"type":"content_block_stop","index":1}}
{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":20}}}
{"type":"stream_event","event":{"type":"message_stop"}}
EVENTS
while [ ! -f .resume ]; do sleep 0.01; done
cat <<'EVENTS'
{"type":"stream_event","event":{"type":"message_start","message":{"id":"msg_2","role":"assistant","usage":{"input_tokens":30}}}}
{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":"done"}}}
{"type":"stream_event","event":{"type":"content_block_stop","index":0}}
{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}}}
{"type":"stream_event","event":{"type":"message_stop"}}
EVENTS
while read -r input; do :; done
"#;
    let (_temp, bridge, target) = fixture(script);
    let mut payload = json!({"model":"claude-test","stream":true,"messages":[{"role":"user","content":"go"}],"tools":[{"name":"lookup","input_schema":{"type":"object"}}]});
    let first = collect(bridge.open(&target, &payload, None).unwrap()).await;
    let ids = first["content"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["id"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert!(ids.iter().all(|id| id.starts_with("toolu_aipass_claude_")));
    let (capability, pid, path) = {
        let runs = bridge.inner.runs.lock().unwrap();
        let (key, run) = runs.iter().next().unwrap();
        (
            key.clone(),
            run.child.id(),
            run._directory.path().to_owned(),
        )
    };
    for (index, id) in ids.iter().enumerate() {
        assert_eq!(
            bridge
                .mcp(
                    &capability,
                    ClaudeBridgeRequest::Call {
                        name: "lookup".into(),
                        arguments: json!({"n":index+1})
                    }
                )
                .unwrap()["callId"],
            *id
        );
    }
    payload["messages"]
        .as_array_mut()
        .unwrap()
        .push(json!({"role":"assistant","content":first["content"]}));
    payload["messages"].as_array_mut().unwrap().push(json!({"role":"user","content":[{"type":"tool_result","tool_use_id":ids[0],"content":"one"}]}));
    assert!(bridge.open(&target, &payload, None).is_err());
    assert!(bridge.inner.runs.lock().unwrap()[&capability]
        .sender
        .is_none());
    payload["messages"][2]["content"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"tool_result","tool_use_id":ids[1],"content":"two"}));
    let stream = bridge.open(&target, &payload, None).unwrap();
    let one = bridge
        .mcp(
            &capability,
            ClaudeBridgeRequest::Poll {
                call_id: ids[0].clone(),
            },
        )
        .unwrap();
    assert_eq!(
        one,
        bridge
            .mcp(
                &capability,
                ClaudeBridgeRequest::Poll {
                    call_id: ids[0].clone()
                }
            )
            .unwrap()
    );
    assert_eq!(one["result"]["content"][0]["text"], "one");
    std::fs::write(path.join(".resume"), b"").unwrap();
    let second = collect(stream).await;
    assert_eq!(second["content"][0]["text"], "done");
    assert_eq!(
        bridge.inner.runs.lock().unwrap()[&capability].child.id(),
        pid
    );
    assert!(bridge.open(&target, &payload, None).is_err());
    bridge.revoke();
    assert!(bridge.inner.runs.lock().unwrap().is_empty());
    assert!(!path.exists());
}
#[tokio::test]
async fn cancellation_kills_process_and_capability() {
    let (_temp, bridge, target) = fixture("read -r input\nwhile read -r input; do :; done\n");
    let stream = bridge
        .open(
            &target,
            &json!({"model":"test","messages":[{"role":"user","content":"wait"}]}),
            None,
        )
        .unwrap();
    let capability = bridge
        .inner
        .runs
        .lock()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    drop(stream);
    assert!(bridge.inner.runs.lock().unwrap().is_empty());
    assert!(bridge
        .mcp(&capability, ClaudeBridgeRequest::ListTools)
        .is_err());
}

#[tokio::test]
async fn changing_a_group_model_revokes_the_original_claude_process() {
    let (_temp, bridge, mut target) = fixture("read -r input\nwhile read -r input; do :; done\n");
    target.config.model = Some("original-model".into());
    let stream = bridge
        .open(
            &target,
            &json!({"model":"original-model","messages":[{"role":"user","content":"wait"}]}),
            None,
        )
        .unwrap();
    let capability = bridge
        .inner
        .runs
        .lock()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    bridge.retain_targets(&[&target]);
    assert!(bridge
        .mcp(&capability, ClaudeBridgeRequest::ListTools)
        .is_ok());
    target.config.model = Some("replacement-model".into());
    bridge.retain_targets(&[&target]);
    assert!(bridge.inner.runs.lock().unwrap().is_empty());
    assert!(bridge
        .mcp(&capability, ClaudeBridgeRequest::ListTools)
        .is_err());
    drop(stream);
}

#[tokio::test]
async fn rebinding_a_cli_account_revokes_existing_process_capabilities() {
    let (_temp, bridge, mut target) = fixture("read -r input\nwhile read -r input; do :; done\n");
    let stream = bridge
        .open(
            &target,
            &json!({"model":"test","messages":[{"role":"user","content":"wait"}]}),
            None,
        )
        .unwrap();
    let capability = bridge
        .inner
        .runs
        .lock()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    bridge.retain_targets(&[&target]);
    assert!(bridge
        .mcp(&capability, ClaudeBridgeRequest::ListTools)
        .is_ok());
    target.api_key = "aipass:claude-cli:reconnected".into();
    bridge.retain_targets(&[&target]);
    assert!(bridge.inner.runs.lock().unwrap().is_empty());
    assert!(bridge
        .mcp(&capability, ClaudeBridgeRequest::ListTools)
        .is_err());
    drop(stream);
}
#[test]
fn history_only_ignores_nonportable_assistant_thinking() {
    let a = json!([{"role":"assistant","content":[{"type":"thinking","thinking":"x","signature":"provider"},{"type":"text","text":"answer"}]}]);
    let b = json!([{"role":"assistant","content":[{"type":"text","text":"<reasoning>x</reasoning>"},{"type":"text","text":"answer"}]}]);
    assert_eq!(
        history_hash(a.as_array().unwrap()),
        history_hash(b.as_array().unwrap())
    );
    let c = json!([{"role":"assistant","content":[{"type":"text","text":"changed answer"}]}]);
    assert_ne!(
        history_hash(a.as_array().unwrap()),
        history_hash(c.as_array().unwrap())
    );
}
