use super::*;

#[test]
fn multimodal_round_trip_preserves_native_fields_and_rejects_unknown_blocks() {
    let content = json!([
        {"type":"text","text":"inspect","cache_control":{"type":"ephemeral"}},
        {"type":"image","source":{"type":"base64","media_type":"image/png","data":"synthetic"}},
        {"type":"document","title":"spec","source":{"type":"base64","media_type":"application/pdf","data":"synthetic"}}
    ]);
    let acp = content::to_acp(&content).unwrap();
    assert_eq!(acp[1]["type"], "image");
    assert_eq!(content::to_native(&acp).unwrap(), content);
    assert!(content::to_acp(&json!([{"type":"unknown","secret":"never silently drop"}])).is_err());
}

#[test]
fn handoff_keeps_roles_results_and_images_without_replaying_tool_calls() {
    let messages = json!([
        {"role":"user","content":"inspect"},
        {"role":"assistant","content":[{"type":"thinking","thinking":"private-reasoning"},{"type":"tool_use","id":"call_openai","name":"Read","input":{"path":"file"}}]},
        {"role":"user","content":[{"type":"tool_result","tool_use_id":"call_openai","content":[{"type":"text","text":"already read"},{"type":"image","source":{"type":"base64","data":"fake-image","media_type":"image/png"}}]}]}
    ]);
    let prompt = handoff::prompt(messages.as_array().unwrap(), false).unwrap();
    assert!(!prompt.to_string().contains("private-reasoning"));
    assert!(prompt.to_string().contains("already read"));
    assert!(prompt.to_string().contains("Historical assistant"));
    assert!(prompt
        .as_array()
        .unwrap()
        .iter()
        .any(|b| b["type"] == "image"));
    assert!(!prompt
        .as_array()
        .unwrap()
        .iter()
        .any(|b| b["type"] == "tool_use" || b["type"] == "tool_result"));
    validate_prompt(&prompt).unwrap();
}

#[test]
fn only_the_current_tool_round_pins_an_account() {
    let mut payload = json!({"messages":[{"role":"user","content":"go"},{"role":"assistant","content":[{"type":"tool_use","id":"toolu_aipass_claude_1"}]},{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_aipass_claude_1","content":"done"}]}]});
    assert_eq!(
        handoff::pending_ids(&payload),
        vec!["toolu_aipass_claude_1"]
    );
    payload["messages"].as_array_mut().unwrap().extend([
        json!({"role":"assistant","content":"finished"}),
        json!({"role":"user","content":"next model"}),
    ]);
    assert!(handoff::pending_ids(&payload).is_empty());
    let responses = json!({"input":[{"type":"function_call","call_id":"toolu_aipass_claude_1"},{"type":"function_call_output","call_id":"toolu_aipass_claude_1","output":"ok"},{"type":"message","role":"assistant","content":[]},{"type":"message","role":"user","content":[]}]});
    assert!(handoff::pending_ids(&responses).is_empty());
}

#[test]
fn native_execution_requires_explicit_directory_permissions_and_session() {
    let temp = tempfile::tempdir().unwrap();
    let options = json!({"_aipass_claude_session":{"cwd":temp.path(),"permissionMode":"default","settingSources":["user","project","local"]}});
    assert!(session_options::Options::read(&options, None).is_err());
    let parsed = session_options::Options::read(&options, Some("s"))
        .unwrap()
        .unwrap();
    let mut command = Command::new("not-a-real-claude");
    parsed.configure(&mut command);
    let args = command
        .get_args()
        .map(|s| s.to_str().unwrap())
        .collect::<Vec<_>>();
    assert!(args.windows(2).any(|a| a == ["--tools", "default"]));
    assert!(args
        .windows(2)
        .any(|a| a == ["--setting-sources", "user,project,local"]));
    assert!(!args.contains(&"--strict-mcp-config"));
    let invalid = json!({"_aipass_claude_session":{"cwd":"relative","permissionMode":"default"}});
    assert!(session_options::Options::read(&invalid, Some("s")).is_err());
}
