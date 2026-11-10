//! Preserve permission and input requests as bidirectional ACP messages.
use super::*;
mod questions;

pub(super) fn request(state: &mut State, value: &Value) -> Result<Value, String> {
    let id = value["request_id"]
        .as_str()
        .ok_or("invalid Claude control request")?;
    if state.permissions.len() >= 64 || state.permissions.contains_key(id) {
        return Err("too many or duplicate Claude permission requests".into());
    }
    let request = &value["request"];
    let method = match request["subtype"].as_str() {
        Some("can_use_tool") => "session/request_permission",
        // These are not permission booleans: preserve structured user answers.
        Some("elicitation" | "request_user_dialog") => "_aipass/request_input",
        _ => {
            write(
                &mut state.stdin,
                &json!({"type":"control_response","response":{"subtype":"error","request_id":id,"error":"Unsupported Claude control request"}}),
            )?;
            return Ok(Value::Null);
        }
    };
    state.permissions.insert(id.into(), request.clone());
    let allow_name = if request["tool_name"] == "ExitPlanMode" {
        "Approve plan and accept edits"
    } else {
        "Allow once"
    };
    Ok(
        json!({"jsonrpc":"2.0","id":id,"method":method,"params":{"sessionId":state.session,"toolCall":{"toolCallId":request["tool_use_id"],"title":request["tool_name"],"status":"pending","rawInput":request["input"]},"options":[{"optionId":"allow_once","name":allow_name,"kind":"allow_once"},{"optionId":"reject_once","name":"Reject","kind":"reject_once"}],"_meta":{"aipass":{"nativeRequest":request}}}}),
    )
}

pub(super) fn respond(state: &mut State, message: &Value) -> Result<(), String> {
    let response = response(state, message)?;
    let id = message["id"].as_str().ok_or("missing ACP response id")?;
    if state.permissions[id]["tool_name"] == "ExitPlanMode" && response["behavior"] == "allow" {
        // The native host must leave plan mode before resolving ExitPlanMode.
        // Do not acknowledge approval if the mode control request is rejected.
        let mode_id = format!("aipass-mode-{}", Uuid::new_v4());
        write(
            &mut state.stdin,
            &json!({"type":"control_request","request_id":mode_id,"request":{"subtype":"set_permission_mode","mode":"acceptEdits"}}),
        )?;
        state.mode_changes.insert(mode_id, (id.into(), response));
        return Ok(());
    }
    if state.permissions[id]["subtype"] != "can_use_tool"
        && message["result"]["outcome"]["outcome"] == "cancelled"
    {
        write(
            &mut state.stdin,
            &json!({"type":"control_response","response":{"subtype":"error","request_id":id,"error":"Caller cancelled the input request"}}),
        )?;
        state.permissions.remove(id);
        return Ok(());
    }
    write(
        &mut state.stdin,
        &json!({"type":"control_response","response":{"subtype":"success","request_id":id,"response":response}}),
    )?;
    state.permissions.remove(id);
    Ok(())
}
pub(super) fn response(state: &State, message: &Value) -> Result<Value, String> {
    let id = message["id"].as_str().ok_or("missing ACP response id")?;
    let request = state
        .permissions
        .get(id)
        .ok_or("ACP request expired or already answered")?;
    if state
        .mode_changes
        .values()
        .any(|(pending, _)| pending == id)
    {
        return Err("ACP permission reply already in progress".into());
    }
    let result = &message["result"];
    let response = if request["subtype"] == "can_use_tool" {
        match (
            result["outcome"]["outcome"].as_str(),
            result["outcome"]["optionId"].as_str(),
        ) {
            (Some("selected"), Some("allow_once")) => {
                let input = result
                    .pointer("/_meta/aipass/updatedInput")
                    .unwrap_or(&request["input"]);
                if !input.is_object() {
                    return Err("updatedInput must be an object".into());
                }
                let input = if request["tool_name"] == "AskUserQuestion" {
                    questions::answer(&request["input"], input)?
                } else {
                    input.clone()
                };
                json!({"behavior":"allow","updatedInput":input})
            }
            (Some("selected"), Some("reject_once")) | (Some("cancelled"), _) => {
                json!({"behavior":"deny","message":"Denied by the caller"})
            }
            _ if message.get("error").is_some() => {
                json!({"behavior":"deny","message":"Caller could not answer"})
            }
            _ => return Err("select an offered ACP permission option".into()),
        }
    } else {
        if !result.is_object() {
            return Err("Claude input reply must be a JSON object".into());
        }
        result.clone()
    };
    Ok(response)
}

pub(super) fn mode_changed(state: &mut State, value: &Value) -> Result<(), String> {
    let Some(id) = value["response"]["request_id"].as_str() else {
        return Ok(());
    };
    let Some((permission, response)) = state.mode_changes.remove(id) else {
        return Ok(());
    };
    if value["response"]["subtype"] != "success" {
        return Err("Claude rejected the approved plan's permission mode".into());
    }
    if state.permissions.remove(&permission).is_some() {
        write(
            &mut state.stdin,
            &json!({"type":"control_response","response":{"subtype":"success","request_id":permission,"response":response}}),
        )?;
    }
    Ok(())
}
