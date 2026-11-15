//! Explicit client cache marks, with loss reported at the native CLI boundary.
use super::*;

pub(super) fn strip_marks(payload: &mut Value) {
    if let Some(object) = payload.as_object_mut() {
        object.remove("cache_control");
    }
    for field in ["tools", "system"] {
        for block in payload[field].as_array_mut().into_iter().flatten() {
            if let Some(object) = block.as_object_mut() {
                object.remove("cache_control");
            }
        }
    }
    for message in payload["messages"].as_array_mut().into_iter().flatten() {
        for block in message["content"].as_array_mut().into_iter().flatten() {
            if let Some(object) = block.as_object_mut() {
                object.remove("cache_control");
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ttl {
    Short,
    Hour,
}
impl Ttl {
    fn text(self) -> &'static str {
        match self {
            Self::Short => "5m",
            Self::Hour => "1h",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct Plan {
    pub(super) client: bool,
    tail: Option<Ttl>,
    total: usize,
    tools: usize,
    system: usize,
    historical: usize,
    current: usize,
}
impl Plan {
    pub(super) fn read(payload: &Value) -> Result<Self, String> {
        let mut plan = Self {
            client: payload["_aipass_client_cache"] == true,
            ..Self::default()
        };
        if !plan.client {
            return Ok(plan);
        }
        let mut short = false;
        for tool in payload["tools"].as_array().into_iter().flatten() {
            if plan.mark(tool.get("cache_control"), &mut short)?.is_some() {
                plan.tools += 1;
            }
        }
        for block in payload["system"].as_array().into_iter().flatten() {
            if plan.mark(block.get("cache_control"), &mut short)?.is_some() {
                plan.system += 1;
            }
        }
        let messages = payload["messages"]
            .as_array()
            .ok_or("Claude messages are required")?;
        for (i, message) in messages.iter().enumerate() {
            let blocks = message["content"].as_array();
            for (j, block) in blocks.into_iter().flatten().enumerate() {
                if let Some(ttl) = plan.mark(block.get("cache_control"), &mut short)? {
                    if i + 1 == messages.len() {
                        plan.current += 1;
                        if j + 1 == blocks.unwrap().len() {
                            plan.tail = Some(ttl);
                        }
                    } else {
                        plan.historical += 1;
                    }
                }
            }
        }
        if let Some(ttl) = plan.mark(payload.get("cache_control"), &mut short)? {
            if plan.tail.is_some_and(|tail| tail != ttl) {
                return Err(
                    "automatic cache_control and the final block must use the same ttl".into(),
                );
            }
            plan.tail = Some(ttl);
        }
        Ok(plan)
    }
    fn mark(&mut self, value: Option<&Value>, short: &mut bool) -> Result<Option<Ttl>, String> {
        let Some(value) = value.filter(|v| !v.is_null()) else {
            return Ok(None);
        };
        let ttl = match (
            value["type"].as_str(),
            value.get("ttl").and_then(Value::as_str),
        ) {
            (Some("ephemeral"), None | Some("5m"))
                if value.get("ttl").is_none_or(Value::is_string) =>
            {
                Ttl::Short
            }
            (Some("ephemeral"), Some("1h")) => Ttl::Hour,
            _ => return Err("cache_control must be ephemeral with ttl 5m or 1h".into()),
        };
        if *short && ttl == Ttl::Hour {
            return Err("1h cache marks must precede 5m cache marks".into());
        }
        *short |= ttl == Ttl::Short;
        self.total += 1;
        if self.total > 4 {
            return Err("at most four cache marks are supported".into());
        }
        Ok(Some(ttl))
    }
    pub(super) fn identity(&self) -> Value {
        json!({"client":self.client,"tail":self.tail.map(Ttl::text)})
    }
    pub(super) fn configure(&self, command: &mut Command) -> Result<(), String> {
        // Ordinary runs inherit the CLI's cache settings without overrides.
        if self.client {
            let existing = command
                .get_envs()
                .find(|(key, _)| *key == "CLAUDE_CODE_EXTRA_BODY")
                .map(|(_, value)| value.map(ToOwned::to_owned))
                .unwrap_or_else(|| std::env::var_os("CLAUDE_CODE_EXTRA_BODY"));
            let mut body = match existing {
                Some(value) => {
                    let value = value.to_str().ok_or("invalid Claude extra body encoding")?;
                    if value.len() > MAX_LINE as usize {
                        return Err("Claude extra body exceeds bridge limit".into());
                    }
                    serde_json::from_str::<Value>(value)
                        .map_err(|_| "Claude extra body must be a JSON object")?
                }
                None => json!({}),
            };
            let object = body
                .as_object_mut()
                .ok_or("Claude extra body must be a JSON object")?;
            object.remove("cache_control");
            if let Some(ttl) = self.tail {
                object.insert(
                    "cache_control".into(),
                    json!({"type":"ephemeral","ttl":ttl.text()}),
                );
            }
            command.env("DISABLE_PROMPT_CACHING", "1");
            command.env("CLAUDE_CODE_EXTRA_BODY", body.to_string());
        }
        Ok(())
    }
    pub(super) fn report(&self, full_replay: bool, continuation: bool) -> Option<String> {
        if !self.client {
            return None;
        }
        let mut fields = Vec::new();
        for (key, count) in [
            ("tools", self.tools),
            ("system", self.system),
            (
                "messages",
                self.historical
                    + if full_replay || continuation {
                        self.current
                    } else {
                        0
                    },
            ),
        ] {
            if count > 0 {
                fields.push(format!("{key}={count}"));
            }
        }
        (!fields.is_empty()).then(|| fields.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_preserves_cli_settings_and_opt_in_validates_before_launch() {
        let ordinary = Plan::read(&json!({"messages":[],"cache_control":{"bad":true}})).unwrap();
        let mut command = Command::new("claude");
        ordinary.configure(&mut command).unwrap();
        assert!(command.get_envs().next().is_none());
        for mark in [
            json!({"type":"permanent"}),
            json!({"type":"ephemeral","ttl":"2h"}),
            json!({"type":"ephemeral","ttl":null}),
        ] {
            assert!(Plan::read(
                &json!({"_aipass_client_cache":true,"messages":[],"cache_control":mark})
            )
            .is_err());
        }
        assert!(Plan::read(&json!({"_aipass_client_cache":true,"system":[{"cache_control":{"type":"ephemeral","ttl":"5m"}}],"messages":[],"cache_control":{"type":"ephemeral","ttl":"1h"}})).is_err());
    }
    #[test]
    fn reports_unmapped_marks_and_never_pretends_tools_or_system_were_mapped() {
        let plan=Plan::read(&json!({"_aipass_client_cache":true,"tools":[{"cache_control":{"type":"ephemeral","ttl":"1h"}}],"system":[{"cache_control":{"type":"ephemeral","ttl":"1h"}}],"messages":[{"role":"user","content":[{"type":"text","text":"prefix","cache_control":{"type":"ephemeral"}}]}]})).unwrap();
        assert_eq!(
            plan.report(false, false).as_deref(),
            Some("tools=1,system=1")
        );
        assert_eq!(
            plan.report(true, false).as_deref(),
            Some("tools=1,system=1,messages=1")
        );
        let mut command = Command::new("claude");
        command.env(
            "CLAUDE_CODE_EXTRA_BODY",
            r#"{"safeguards":{"enabled":true},"cache_control":{"ttl":"1h"}}"#,
        );
        plan.configure(&mut command).unwrap();
        let body = command
            .get_envs()
            .find(|(key, _)| *key == "CLAUDE_CODE_EXTRA_BODY")
            .unwrap()
            .1
            .unwrap();
        let body: Value = serde_json::from_str(body.to_str().unwrap()).unwrap();
        assert_eq!(body["cache_control"]["ttl"], "5m");
        assert_eq!(body["safeguards"]["enabled"], true);
        command.env("CLAUDE_CODE_EXTRA_BODY", "[]");
        assert!(plan.configure(&mut command).is_err());
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn invalid_client_marks_return_a_local_rejection_without_starting_a_process() {
        let (_dir, bridge, target) = super::super::tests::fixture("exit 99");
        let response=bridge.request(target,json!({"_aipass_client_cache":true,"messages":[{"role":"user","content":"test"}],"cache_control":{"type":"permanent"}}),None).await.unwrap();
        assert_eq!(response.status, hyper::StatusCode::BAD_REQUEST);
        assert_eq!(response.headers["x-aipass-local-rejection"], "1");
        assert!(bridge.inner.runs.lock().unwrap().is_empty());
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn opt_in_reaches_the_native_process_and_default_leaves_its_cache_alone() {
        for client in [false, true] {
            let script = r#"read -r input
printf '%s\n%s\n%s\n' "$DISABLE_PROMPT_CACHING" "$CLAUDE_CODE_EXTRA_BODY" "$input" > request-cache
printf '%s\n' '{"type":"stream_event","event":{"type":"message_start","message":{"id":"msg_cache","content":[],"usage":{}}}}' '{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}}}' '{"type":"stream_event","event":{"type":"message_stop"}}' '{"type":"result","is_error":false}'
while read -r input; do :; done
"#;
            let (_dir, bridge, target) = super::super::tests::fixture(script);
            let payload = json!({"model":"test","stream":true,"_aipass_client_cache":client,"messages":[{"role":"user","content":[{"type":"text","text":"prompt","cache_control":{"type":"ephemeral","ttl":"1h"}}]}]});
            let (mut stream, report) = bridge.open_report(&target, &payload, None).unwrap();
            while let Some(chunk) = stream.next().await {
                chunk.unwrap();
            }
            let runs = bridge.inner.runs.lock().unwrap();
            let path = runs
                .values()
                .next()
                .unwrap()
                ._directory
                .path()
                .join("request-cache");
            let sent = std::fs::read_to_string(path).unwrap();
            if client {
                let mut lines = sent.lines();
                assert_eq!(lines.next(), Some("1"));
                assert_eq!(
                    serde_json::from_str::<Value>(lines.next().unwrap()).unwrap(),
                    json!({"cache_control":{"ttl":"1h","type":"ephemeral"}})
                );
                assert!(report.is_none());
            } else {
                assert!(!sent.starts_with("1\n"));
            }
            assert_eq!(
                sent.lines().last().unwrap().contains("cache_control"),
                client
            );
            drop(runs);
            bridge.revoke();
        }
    }
}
