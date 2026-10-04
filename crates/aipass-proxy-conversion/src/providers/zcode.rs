use serde_json::{json, Value};
pub struct Environment<'a> {
    pub cwd: &'a str,
    pub platform: &'a str,
    pub shell: &'a str,
    pub os_version: &'a str,
    pub date: &'a str,
}
pub fn dress(
    body: &mut Value,
    site: &str,
    device: &str,
    context: &Environment<'_>,
) -> Result<(), String> {
    let prompt: Value = serde_json::from_str(include_str!("zcode_prompt.json"))
        .map_err(|_| "invalid ZCode metadata")?;
    let mut own = match &body["system"] {
        Value::Null => vec![],
        Value::String(s) => vec![json!({"type":"text","text":s})],
        Value::Array(a) => a.clone(),
        _ => return Err("invalid ZCode system prompt".into()),
    };
    for b in &mut own {
        if let Some(o) = b.as_object_mut() {
            o.remove("cache_control");
        }
    }
    let cached =
        |text: String| json!({"type":"text","text":text,"cache_control":{"type":"ephemeral"}});
    let stable = prompt["stableSections"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("\n\n");
    let e = &prompt["environment"];
    let powered = e["poweredByLine"]
        .as_str()
        .unwrap_or("")
        .replace(
            "{provider}",
            if site == "bigmodel" {
                "bigmodel-api"
            } else {
                "zai-api"
            },
        )
        .replace("{model}", body["model"].as_str().unwrap_or(""));
    let mut lines = vec![
        e["heading"].as_str().unwrap_or("").to_owned(),
        e["invokedLine"].as_str().unwrap_or("").to_owned(),
    ];
    for (key, value) in [
        ("cwdLabel", context.cwd),
        ("gitLabel", e["gitNo"].as_str().unwrap_or("")),
        ("platformLabel", context.platform),
        ("shellLabel", context.shell),
        ("osVersionLabel", context.os_version),
    ] {
        lines.push(format!("- {}: {value}", e[key].as_str().unwrap_or("")));
    }
    lines.push(powered);
    let env = lines.join("\n");
    let mut system = vec![
        cached(prompt["cliPrefix"].as_str().unwrap_or("").into()),
        cached(stable),
        cached(format!(
            "\n\n{}\n\n{env}\n\n{}",
            prompt["beforeEnvironment"].as_str().unwrap_or(""),
            prompt["afterEnvironment"].as_str().unwrap_or("")
        )),
    ];
    system.extend(own);
    body["system"] = json!(system);
    let messages = body["messages"]
        .as_array_mut()
        .ok_or("ZCode messages must be an array")?;
    let c = &prompt["context"];
    let reminder = format!(
        "<system-reminder>{}\n{}\n{}\n\n{}</system-reminder>",
        c["intro"].as_str().unwrap_or(""),
        c["currentDateHeading"].as_str().unwrap_or(""),
        c["currentDateLine"]
            .as_str()
            .unwrap_or("")
            .replace("{date}", context.date),
        c["outro"].as_str().unwrap_or("")
    );
    messages.insert(
        0,
        json!({"role":"user","content":[{"type":"text","text":reminder}]}),
    );
    for m in messages.iter_mut() {
        for b in m["content"].as_array_mut().into_iter().flatten() {
            if let Some(o) = b.as_object_mut() {
                o.remove("cache_control");
            }
        }
    }
    if let Some(last) = messages.last_mut() {
        if let Some(text) = last["content"].as_str() {
            last["content"] = json!([cached(text.into())]);
        } else if let Some(block) = last["content"].as_array_mut().and_then(|a| a.last_mut()) {
            block["cache_control"] = json!({"type":"ephemeral"});
        }
    }
    for tool in body["tools"].as_array_mut().into_iter().flatten() {
        if let Some(o) = tool.as_object_mut() {
            o.remove("cache_control");
        }
    }
    if !body["metadata"].is_object() {
        body["metadata"] = json!({});
    }
    body["metadata"]["user_id"] =
        json!(json!({"device_id":device,"account_uuid":"","session_id":""}).to_string());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn start_plan_preserves_caller_prompt_and_limits_cache_boundaries() {
        let mut body = json!({"model":"glm","system":[{"type":"text","text":"caller rules","cache_control":{"type":"ephemeral"}}],"messages":[{"role":"user","content":"hello"}],"tools":[{"name":"search","cache_control":{"type":"ephemeral"}}]});
        dress(
            &mut body,
            "zai",
            "device",
            &Environment {
                cwd: "/tmp",
                platform: "darwin",
                shell: "zsh",
                os_version: "Darwin",
                date: "2026-10-03",
            },
        )
        .unwrap();
        assert_eq!(body["system"].as_array().unwrap().len(), 4);
        assert_eq!(body["system"][3]["text"], "caller rules");
        assert!(body["system"][3].get("cache_control").is_none());
        assert!(body["messages"][0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("2026-10-03"));
        assert_eq!(
            body["messages"][1]["content"][0]["cache_control"]["type"],
            "ephemeral"
        );
        assert!(body["tools"][0].get("cache_control").is_none());
    }
}
