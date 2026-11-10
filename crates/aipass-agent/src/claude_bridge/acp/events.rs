use super::*;

pub(super) fn read(stdout: std::process::ChildStdout, client: Client) {
    use std::io::Read;
    let mut reader = BufReader::new(stdout);
    loop {
        let mut line = zeroize::Zeroizing::new(Vec::new());
        let Ok(n) = reader
            .by_ref()
            .take(MAX_LINE + 1)
            .read_until(b'\n', &mut line)
        else {
            break;
        };
        if n == 0 || n as u64 > MAX_LINE {
            break;
        }
        let Ok(value) = serde_json::from_slice::<Value>(&line) else {
            break;
        };
        if dispatch(&client, value).is_err() {
            break;
        }
    }
    let _ = client.emit(json!({"jsonrpc":"2.0","method":"_aipass/closed","params":{}}));
}

fn dispatch(client: &Client, value: Value) -> Result<(), String> {
    let mut state = client.state.lock().map_err(|_| "ACP session unavailable")?;
    state.lifecycle.observe(&value)?;
    let session = state.session.clone();
    let mut outgoing = Vec::new();
    match value["type"].as_str() {
        Some("control_response") => {
            permissions::mode_changed(&mut state, &value)?;
            if value["response"]["request_id"] == "aipass-initialize" {
                let id = state
                    .opening
                    .take()
                    .ok_or("unexpected Claude initialization")?;
                if value["response"]["subtype"] != "success" {
                    outgoing.push(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32603,"message":"Claude initialization rejected"}}));
                } else {
                    state.ready = true;
                    outgoing.push(json!({"jsonrpc":"2.0","id":id,"result":{"sessionId":session,"_meta":{"aipass":{"nativeInitialization":value["response"]["response"]}}}}));
                    let commands = value["response"]["response"]["commands"].as_array().into_iter().flatten().map(|c|json!({"name":c["name"],"description":c["description"],"input":{"hint":c["argumentHint"]}})).collect::<Vec<_>>();
                    outgoing.push(update(&session,json!({"sessionUpdate":"available_commands_update","availableCommands":commands})));
                }
            }
        }
        Some("control_request") => {
            let request = permissions::request(&mut state, &value)?;
            if !request.is_null() {
                if state.main_message {
                    state.deferred_updates.push(request);
                } else {
                    outgoing.push(request);
                }
            }
        }
        Some("control_cancel_request") => {
            if value["request_id"]
                .as_str()
                .is_some_and(|id| state.permissions.remove(id).is_some())
            {
                outgoing.push(json!({"jsonrpc":"2.0","method":"_aipass/request_cancelled","params":{"requestId":value["request_id"]}}));
            }
        }
        Some("result") if value["parent_tool_use_id"].is_null() => {
            state.permissions.clear();
            state.mode_changes.clear();
            let State {
                lifecycle, stdin, ..
            } = &mut *state;
            let autonomous = lifecycle.settled(stdin)?;
            if autonomous {
                let mut event = native(&session, &value);
                event["params"]["autonomous"] = json!(true);
                drop(state);
                return client.emit(event);
            }
            let id = state
                .prompt
                .take()
                .ok_or("Claude result without ACP prompt")?;
            outgoing.push(native(&session, &value));
            if value["is_error"] == true && !state.cancelling {
                outgoing.push(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32603,"message":"Claude prompt failed"}}));
            } else {
                outgoing.push(json!({"jsonrpc":"2.0","id":id,"result":{"stopReason":if state.cancelling {"cancelled"}else{state.stop_reason.as_str()}}}));
            }
        }
        Some("stream_event") => {
            let event = &value["event"];
            let main = value["parent_tool_use_id"].is_null();
            if event["type"] == "message_start" && main {
                state.main_message = true;
                state.streamed_message = event["message"]["id"].as_str().map(str::to_owned);
            }
            let delta = &event["delta"];
            if event["type"] == "message_delta" && main {
                state.stop_reason = match delta["stop_reason"].as_str() {
                    Some("max_tokens") => "max_tokens",
                    Some("refusal") => "refusal",
                    _ => "end_turn",
                }
                .into();
            }
            let text = match delta["type"].as_str() {
                Some("text_delta") => Some(("agent_message_chunk", &delta["text"])),
                Some("thinking_delta") => Some(("agent_thought_chunk", &delta["thinking"])),
                _ => None,
            };
            if let Some((kind, text)) = text.filter(|_| main) {
                outgoing.push(update(
                    &session,
                    json!({"sessionUpdate":kind,"content":{"type":"text","text":text}}),
                ));
            }
            outgoing.push(native(&session, &value));
            if event["type"] == "message_stop" && main {
                state.main_message = false;
                outgoing.append(&mut state.deferred_updates);
            }
        }
        Some("assistant") => {
            if !value["parent_tool_use_id"].is_null() {
                let mut calls = Vec::new();
                for block in value["message"]["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|b| {
                        b["type"] == "tool_use"
                            && b["name"]
                                .as_str()
                                .is_some_and(|n| n.starts_with("mcp__aipass__"))
                    })
                {
                    let id = block["id"]
                        .as_str()
                        .ok_or("child tool call is missing its ID")?;
                    if state.child_calls.len() >= 4096 {
                        return Err("too many child tool calls".into());
                    }
                    if state.child_calls.insert(id.into()) {
                        calls.push(block.clone());
                    }
                }
                if !calls.is_empty() {
                    let request = json!({"jsonrpc":"2.0","method":"_aipass/child_tools","params":{"sessionId":session,"calls":calls}});
                    if state.main_message {
                        state.deferred_updates.push(request);
                    } else {
                        outgoing.push(request);
                    }
                }
            }
            if value["parent_tool_use_id"].is_null() {
                if let Some(id) = value["message"]["id"]
                    .as_str()
                    .filter(|id| state.streamed_message.as_deref() != Some(id))
                {
                    state.streamed_message = Some(id.into());
                    let message = &value["message"];
                    let mut events = vec![json!({"type":"message_start","message":message})];
                    for (index, block) in message["content"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .enumerate()
                    {
                        events.push(json!({"type":"content_block_start","index":index,"content_block":block}));
                        events.push(json!({"type":"content_block_stop","index":index}));
                    }
                    events.push(json!({"type":"message_delta","delta":{"stop_reason":message["stop_reason"]},"usage":message["usage"]}));
                    events.push(json!({"type":"message_stop"}));
                    for event in events {
                        outgoing.push(native(
                            &session,
                            &json!({"type":"stream_event","event":event}),
                        ));
                    }
                }
            }
            for block in value["message"]["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|b| b["type"] == "tool_use")
            {
                outgoing.push(update(&session,json!({"sessionUpdate":"tool_call","toolCallId":block["id"],"title":block["name"],"kind":"other","status":"in_progress","rawInput":block["input"]})));
            }
        }
        Some("user") => {
            for block in value["message"]["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|b| b["type"] == "tool_result")
            {
                outgoing.push(update(&session,json!({"sessionUpdate":"tool_call_update","toolCallId":block["tool_use_id"],"status":if block["is_error"]==true {"failed"}else{"completed"},"rawOutput":block["content"]})));
            }
        }
        Some("keep_alive") => {}
        _ => outgoing.push(native(&session, &value)),
    }
    drop(state);
    for message in outgoing {
        client.emit(message)?;
    }
    Ok(())
}
fn update(session: &str, update: Value) -> Value {
    json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":session,"update":update}})
}
fn native(session: &str, value: &Value) -> Value {
    // Preserve signatures, cache accounting and vendor lifecycle events without
    // pretending the portable ACP schema can represent every provider field.
    json!({"jsonrpc":"2.0","method":"_aipass/native_event","params":{"sessionId":session,"event":value}})
}
