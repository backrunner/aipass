//! Lifecycle regressions use only disposable shell protocol fixtures.
use super::*;

#[tokio::test]
async fn malformed_native_usage_closes_the_stream_and_releases_the_session() {
    let script = r#"read -r input
printf '%s\n' '{"type":"stream_event","event":{"type":"message_start","message":{"id":"bad","usage":42}}}'
printf '%s\n' '{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}}}'
while read -r input; do :; done
"#;
    let (_dir, bridge, target) = fixture(script);
    let mut output = bridge
        .open(
            &target,
            &json!({"model":"fixture","messages":[{"role":"user","content":"go"}]}),
            None,
        )
        .unwrap();
    loop {
        let next = tokio::time::timeout(Duration::from_secs(3), output.next())
            .await
            .expect("malformed child frame stranded the response");
        if next.expect("missing error").is_err() {
            break;
        }
    }
    assert!(bridge.inner.runs.lock().unwrap().is_empty());
}

#[tokio::test]
async fn dropping_an_old_segment_cannot_cancel_a_reused_session() {
    for backpressured in [false, true] {
        let script = r#"read -r input
printf '%s\n' '{"type":"result","is_error":false,"result":"first answer"}'
read -r input
while [ ! -f .finish ]; do sleep 0.01; done
printf '%s\n' '{"type":"result","is_error":false,"result":"second answer"}'
while read -r input; do :; done
"#;
        let script = if backpressured {
            script.replacen("read -r input", "read -r input\nprintf '%s\\n' '{\"type\":\"assistant\",\"message\":{\"id\":\"first\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"one\"},{\"type\":\"text\",\"text\":\"two\"}],\"stop_reason\":\"end_turn\",\"usage\":{}}}'", 1)
        } else {
            script.to_owned()
        };
        let (_dir, bridge, target) = fixture(&script);
        let session = Some("overlapping-segments".into());
        let mut payload = json!({"model":"fixture","messages":[{"role":"user","content":"first"}]});
        let old = bridge.open(&target, &payload, session.clone()).unwrap();
        // Let the terminal event queue up without consuming it. A later request
        // can arrive while the old HTTP body is still owned by the transport.
        let deadline = Instant::now() + Duration::from_secs(3);
        let (key, path, history) = loop {
            let found = bridge
                .inner
                .runs
                .lock()
                .unwrap()
                .iter()
                .find_map(|(key, run)| {
                    run.terminal.then(|| {
                        (
                            key.clone(),
                            run._directory.path().to_owned(),
                            run.history_messages.clone(),
                        )
                    })
                });
            if let Some(found) = found {
                break found;
            }
            assert!(Instant::now() < deadline, "first segment did not finish");
            tokio::time::sleep(Duration::from_millis(5)).await;
        };
        payload["messages"] = json!(history);
        payload["messages"]
            .as_array_mut()
            .unwrap()
            .push(json!({"role":"user","content":"second"}));
        let next = bridge.open(&target, &payload, session).unwrap();
        drop(old);
        assert!(
            bridge.inner.runs.lock().unwrap().contains_key(&key),
            "old body cancelled the new turn"
        );
        std::fs::write(path.join(".finish"), b"").unwrap();
        let answer = collect(next).await;
        assert!(answer.to_string().contains("second answer"));
        bridge.revoke();
    }
}

#[tokio::test]
async fn claude_capability_rejections_are_local_target_skips() {
    for control in [
        json!({"temperature":0.2}),
        json!({"top_p":0.8}),
        json!({"top_k":20}),
        json!({"stop_sequences":["stop"]}),
        json!({"tool_choice":{"type":"any"}}),
        json!({"tool_choice":{"type":"auto","disable_parallel_tool_use":true}}),
        json!({"output_config":{"format":{"type":"json_schema","schema":{"type":"object"}}}}),
        json!({"tools":[{"type":"web_search_20250305","name":"web_search","max_uses":1}]}),
        json!({"tools":[{"type":"computer_20250124","name":"computer"}]}),
        json!({"tools":[{"name":"browser","input_schema":{"oneOf":[{"type":"object","properties":{"code":{"type":"string"}},"required":["code"]},{"type":"object","properties":{"completion_summary":{"type":"string"}},"required":["completion_summary"]}]}}]}),
        json!({"tools":[{"name":"browser","input_schema":{"type":"object","anyOf":[{"required":["code"]},{"required":["completion_summary"]}]}}]}),
        json!({"tools":[{"name":"browser","input_schema":{"type":"object","allOf":[{"required":["code"]}]}}]}),
        json!({"tools":[{"name":"list","input_schema":{"type":"array","items":{"type":"string"}}}]}),
    ] {
        let (_dir, bridge, target) = fixture("exit 99");
        let mut body = json!({"model":"fixture","messages":[{"role":"user","content":"go"}]});
        body.as_object_mut()
            .unwrap()
            .extend(control.as_object().unwrap().clone());
        let response = bridge.request(target, body, None).await.unwrap();
        assert_eq!(
            response
                .headers
                .get("x-aipass-target-incompatible")
                .and_then(|v| v.to_str().ok()),
            Some("1"),
            "{control}"
        );
        assert!(!response.headers.contains_key("x-aipass-local-rejection"));
        assert!(bridge.inner.runs.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn tool_result_siblings_are_rejected_before_consuming_the_pending_call() {
    let script = r#"read -r input
printf '%s\n' '{"type":"assistant","message":{"id":"call","role":"assistant","content":[{"type":"tool_use","id":"native","name":"mcp__aipass__lookup","input":{}}],"stop_reason":"tool_use","usage":{}}}'
while read -r input; do :; done
"#;
    let (_dir, bridge, target) = fixture(script);
    let mut body = json!({"model":"fixture","messages":[{"role":"user","content":"go"}],"tools":[{"name":"lookup","input_schema":{"type":"object"}}]});
    let answer = collect(bridge.open(&target, &body, None).unwrap()).await;
    let id = answer["content"][0]["id"].clone();
    body["messages"]
        .as_array_mut()
        .unwrap()
        .push(json!({"role":"assistant","content":answer["content"]}));
    let result = json!({"type":"tool_result","tool_use_id":id,"content":"found"});
    for extra in [
        json!({"type":"text","text":"stop, read the other file"}),
        json!({"type":"image","source":{"type":"base64","media_type":"image/png","data":"YWJj"}}),
    ] {
        let mut candidate = body.clone();
        candidate["messages"]
            .as_array_mut()
            .unwrap()
            .push(json!({"role":"user","content":[result,extra]}));
        let response = tokio::time::timeout(
            Duration::from_secs(3),
            bridge.request(target.clone(), candidate, None),
        )
        .await
        .expect("mixed continuation was consumed instead of rejected")
        .unwrap();
        assert_eq!(response.status, hyper::StatusCode::BAD_REQUEST);
        assert!(response.headers.contains_key("x-aipass-local-rejection"));
        let runs = bridge.inner.runs.lock().unwrap();
        let run = runs.values().next().unwrap();
        assert!(run.parked && run.sender.is_none());
        assert!(run.calls.iter().all(|call| call.result.is_none()));
    }
    // Embedded tool-result images retain their ordinary MCP representation.
    body["messages"].as_array_mut().unwrap().push(json!({"role":"user","content":[{"type":"tool_result","tool_use_id":id,"content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"YWJj"}}]}]}));
    let next = bridge.open(&target, &body, None).unwrap();
    assert_eq!(
        bridge
            .inner
            .runs
            .lock()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .calls[0]
            .result
            .as_ref()
            .unwrap()["content"][0]["data"],
        "YWJj"
    );
    drop(next);
}

#[tokio::test]
async fn malformed_preflight_does_not_skip_targets_or_replace_existing_sessions() {
    let (_dir, bridge, target) = fixture("while read -r prompt; do printf '%s\\n' '{\"type\":\"result\",\"is_error\":false,\"result\":\"answer\"}'; done");
    let session = Some("preserved-session".to_owned());
    let body = json!({"model":"fixture","messages":[{"role":"user","content":"first"}]});
    collect(bridge.open(&target, &body, session.clone()).unwrap()).await;
    let key = bridge
        .inner
        .runs
        .lock()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    for invalid in [
        json!({"temperature":"bad"}),
        json!({"temperature":0.1,"stop_sequences":[42]}),
        json!({"tools":{}}),
        json!({"tools":[{"name":"lookup","input_schema":[]}]}),
        json!({"tools":[{"name":"aipass_acp_respond","input_schema":{}}]}),
        json!({"tools":[{"name":"duplicate"},{"name":"duplicate"}]}),
        json!({"tool_choice":{"type":"tool"}}),
        json!({"tool_choice":{"type":"auto","disable_parallel_tool_use":"true"}}),
    ] {
        let mut payload = body.clone();
        payload
            .as_object_mut()
            .unwrap()
            .extend(invalid.as_object().unwrap().clone());
        let response = bridge
            .request(target.clone(), payload, session.clone())
            .await
            .unwrap();
        assert_eq!(response.status, hyper::StatusCode::BAD_REQUEST);
        assert!(
            response.headers.contains_key("x-aipass-local-rejection"),
            "{invalid}"
        );
        assert!(!response
            .headers
            .contains_key("x-aipass-target-incompatible"));
        assert!(bridge.inner.runs.lock().unwrap().contains_key(&key));
    }
    for optional in [
        json!({"temperature":null,"top_p":null,"stop_sequences":null,"tool_choice":null}),
        json!({"stop_sequences":[],"tool_choice":{"type":"auto","disable_parallel_tool_use":false}}),
    ] {
        let mut payload = body.clone();
        payload
            .as_object_mut()
            .unwrap()
            .extend(optional.as_object().unwrap().clone());
        assert!(preflight::check(&payload, session.as_deref()).is_ok());
    }
    bridge.revoke();
}
