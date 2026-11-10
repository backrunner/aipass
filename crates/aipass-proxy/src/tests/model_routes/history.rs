use super::*;

fn mixed_route() -> ResolvedRoute {
    let mut a = test_target("http://127.0.0.1:1".into(), 0);
    a.config.model = Some("one".into());
    let mut b = test_target("http://127.0.0.1:2".into(), 1);
    b.config.model = Some("two".into());
    bound_route(vec![a, b])
}

#[test]
fn completed_native_calls_are_portable_in_all_three_protocols() {
    let route = mixed_route();
    for prefix in [
        "toolu_aipass_claude_",
        "call_aipass_gemini_",
        "call_aipass_cursor_",
    ] {
        let id = format!("{prefix}fixture");
        for mut payload in [
            json!({"messages":[{"role":"user","content":"first"},{"role":"assistant","content":[{"type":"thinking","thinking":"private","signature":"signed"},{"type":"tool_use","id":id,"name":"lookup","input":{"signature":"user input"}}]},{"role":"user","content":[{"type":"tool_result","tool_use_id":id,"content":"answer"}]},{"role":"assistant","content":"done"},{"role":"user","content":"next"}]}),
            json!({"messages":[{"role":"user","content":"first"},{"role":"assistant","tool_calls":[{"id":id,"type":"function","function":{"name":"lookup","arguments":"{}"}}]},{"role":"tool","tool_call_id":id,"content":"answer"},{"role":"assistant","content":"done"},{"role":"user","content":"next"}]}),
            json!({"input":[{"role":"user","content":"first"},{"type":"reasoning","encrypted_content":"private"},{"type":"function_call","call_id":id,"name":"lookup","arguments":"{}"},{"type":"function_call_output","call_id":id,"output":"answer"},{"role":"assistant","content":"done"},{"role":"user","content":"next"}]}),
        ] {
            assert!(crate::model_routes::make_portable(&route, &mut payload));
            let ids = subscription_history_call_ids(&payload);
            assert_eq!(ids.len(), 2);
            assert_eq!(ids[0], ids[1]);
            assert!(ids[0].starts_with("call_history_") && ids[0].len() <= 64);
            assert!(!payload.to_string().contains("private"));
            assert!(payload.to_string().contains("answer"));
            assert!(!crate::model_routes::make_portable(&route, &mut payload));
        }
    }
}

#[test]
fn unfinished_turns_permissions_and_opaque_compaction_stay_bound() {
    let route = mixed_route();
    for payload in [
        json!({"messages":[{"role":"user","content":"first"},{"role":"assistant","content":[{"type":"tool_use","id":"toolu_aipass_claude_pending","name":"aipass_acp_respond","input":{}}]},{"role":"user","content":"new text is not permission"}]}),
        json!({"messages":[{"role":"user","content":"first"},{"role":"assistant","content":[{"type":"thinking","signature":"private"},{"type":"tool_use","id":"toolu_aipass_claude_pending","name":"aipass_acp_respond","input":{}}]},{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_aipass_claude_pending","content":"reply"}]},{"role":"user","content":"still awaiting completion"}]}),
        json!({"input":[{"type":"compaction","encrypted_content":"private"},{"role":"assistant","content":"done"},{"role":"user","content":"next"}]}),
        json!({"input":[{"type":"item_reference","id":"private"},{"role":"assistant","content":"done"},{"role":"user","content":"next"}]}),
        json!({"previous_response_id":"server-state","input":[{"type":"reasoning","encrypted_content":"private"},{"role":"assistant","content":"done"},{"role":"user","content":"next"}]}),
    ] {
        let mut candidate = payload.clone();
        assert!(!crate::model_routes::make_portable(&route, &mut candidate));
        assert_eq!(payload, candidate);
    }
}

#[test]
fn gemini_signed_tool_round_finishes_on_its_owner_then_can_switch_members() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let native = std::thread::spawn(move || {
        let mut bodies = Vec::new();
        for parts in [
            json!([{"functionCall":{"name":"lookup","args":{"q":"fixture"}},"thoughtSignature":"private-signature"}]),
            json!([{"text":"Gemini completed"}]),
        ] {
            let (mut socket, (_, body)) =
                crate::tests::mock_upstream::accept_api_request(&listener);
            bodies.push(serde_json::from_slice::<Value>(&body).unwrap());
            let response =
                json!({"candidates":[{"content":{"parts":parts},"finishReason":"STOP"}]})
                    .to_string();
            write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",response.len()).unwrap();
        }
        bodies
    });
    let (other,upstream)=mock_upstream("200 OK","application/json",json!({"id":"next","object":"chat.completion","choices":[{"message":{"role":"assistant","content":"Other provider continued"},"finish_reason":"stop"}]}).to_string());
    let mut route = mixed_route();
    route.targets[0].config.base_url = format!("http://{address}");
    route.targets[0].config.model = Some("gemini-fixture".into());
    route.targets[0].upstream_kind = UpstreamKind::GeminiNative;
    route.targets[1].config.base_url = format!("http://{other}");
    let proxy = start_proxy(available_addr(), route.clone());
    let mut body = json!({"model":crate::model_routes::model_id(&route).unwrap(),"messages":[{"role":"user","content":"lookup fixture"}]});
    let first = request(&proxy.bind_addr, body.clone());
    assert_eq!(first.status(), StatusCode::OK);
    let first: Value = first.json().unwrap();
    let assistant = &first["choices"][0]["message"];
    let id = assistant["tool_calls"][0]["id"].as_str().unwrap();
    assert!(id.starts_with("call_aipass_gemini_"));
    body["messages"].as_array_mut().unwrap().extend([
        assistant.clone(),
        json!({"role":"tool","tool_call_id":id,"content":"lookup result"}),
    ]);
    let second = request(&proxy.bind_addr, body.clone());
    assert_eq!(
        second.status(),
        StatusCode::OK,
        "{}",
        second.text().unwrap()
    );
    let native = native.join().unwrap();
    assert!(native[1].to_string().contains("private-signature"));
    body["messages"].as_array_mut().unwrap().extend([
        json!({"role":"assistant","content":"Gemini completed"}),
        json!({"role":"user","content":"continue elsewhere"}),
    ]);
    route.targets[0].quota = vec![QuotaWindow {
        model_family: None,
        models: None,
        not_models: Vec::new(),
        used_basis_points: 10000,
        span_seconds: None,
        observed_at: now_unix() as u64,
        resets_at: None,
    }];
    proxy
        .update_config(RuntimeConfig::from_routes(&proxy.bind_addr, vec![route]))
        .unwrap();
    let third = request(&proxy.bind_addr, body);
    assert_eq!(third.status(), StatusCode::OK, "{}", third.text().unwrap());
    let body: Value = serde_json::from_slice(&upstream.join().unwrap().1).unwrap();
    let ids = subscription_history_call_ids(&body);
    assert_eq!(ids[0], ids[1]);
    assert!(ids[0].starts_with("call_history_"));
    assert!(body.to_string().contains("lookup result"));
    assert!(!body.to_string().contains("private-signature"));
}

#[test]
fn completed_prefix_does_not_rewrite_current_tool_round_or_user_payloads() {
    let route = mixed_route();
    let input =
        json!({"type":"tool_use","id":"call_aipass_gemini_user_data","signature":"untouched"});
    let mut payload = json!({"messages":[
        {"role":"assistant","content":[{"type":"thinking","signature":"old"},{"type":"text","text":"completed"}]},
        {"role":"user","content":"new turn"},
        {"role":"assistant","content":[{"type":"thinking","signature":"current"},{"type":"tool_use","id":"toolu_aipass_claude_pending","name":"lookup","input":input}]},
        {"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_aipass_claude_pending","content":[{"type":"text","text":input.to_string()}]}]}
    ]});
    let current = payload["messages"].as_array().unwrap()[1..].to_vec();
    assert!(crate::model_routes::make_portable(&route, &mut payload));
    assert_eq!(&payload["messages"].as_array().unwrap()[1..], current);
    let mut thinking_only = json!({"messages":[{"role":"assistant","content":[{"type":"thinking","signature":"old"}]},{"role":"user","content":"next"}]});
    assert!(crate::model_routes::make_portable(
        &route,
        &mut thinking_only
    ));
    assert_eq!(
        thinking_only["messages"],
        json!([{"role":"user","content":"next"}])
    );
}

#[test]
fn portable_history_reaches_native_same_protocol_http_after_primary_fails() {
    let (a, first) = mock_upstream("429 Too Many Requests", "application/json", "{}".into());
    let (b, second)=mock_upstream("200 OK","application/json",json!({"id":"r","object":"chat.completion","choices":[{"message":{"role":"assistant","content":"continued"},"finish_reason":"stop"}]}).to_string());
    let mut route = mixed_route();
    route.targets[0].config.base_url = format!("http://{a}");
    route.targets[1].config.base_url = format!("http://{b}");
    let model = crate::model_routes::model_id(&route).unwrap();
    let proxy = start_proxy(available_addr(), route);
    let response = request(
        &proxy.bind_addr,
        json!({"model":model,"messages":[{"role":"user","content":"lookup"},{"role":"assistant","tool_calls":[{"id":"call_aipass_gemini_old","type":"function","function":{"name":"lookup","arguments":"{}"}}]},{"role":"tool","tool_call_id":"call_aipass_gemini_old","content":"tool fact"},{"role":"assistant","content":"done"},{"role":"user","content":"continue"}]}),
    );
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{}",
        response.text().unwrap()
    );
    for upstream in [first, second] {
        let body: Value = serde_json::from_slice(&upstream.join().unwrap().1).unwrap();
        let ids = subscription_history_call_ids(&body);
        assert_eq!(ids[0], ids[1]);
        assert!(ids[0].starts_with("call_history_"));
        assert!(body.to_string().contains("tool fact"));
    }
}
