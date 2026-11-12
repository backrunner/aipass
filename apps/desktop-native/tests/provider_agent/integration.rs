use super::*;
use aipass_agent_protocol::{AgentErrorCode, ToolConfigMode, ToolConfigRequest, ToolConfigTool};
use aipass_desktop_native::integration::{Completion, Operation, Outcome, Request, Scope};

fn execute(transport: &mut AgentTransport, id: uuid::Uuid, operation: Operation) -> Completion {
    let scope = Scope {
        entry: id,
        generation: 51,
    };
    let Reply::Integration(completion) = transport
        .execute(Command::Integration(Box::new(Request { scope, operation })))
        .unwrap()
    else {
        panic!("integration reply expected")
    };
    assert_eq!(completion.scope, scope);
    *completion
}

pub(super) fn workflow(transport: &mut AgentTransport, root: &Path, id: uuid::Uuid) {
    let secret_id = snapshot(transport).active[0].secret_refs[0].id.clone();
    let mut request = ToolConfigRequest {
        tool: ToolConfigTool::OpenCode,
        id,
        secret_id: Some(secret_id),
        mode: ToolConfigMode::Plaintext,
        codex_api_key_mode: None,
        preview_id: None,
    };
    let Outcome::Preview(first) = execute(transport, id, Operation::Preview(request.clone()))
        .result
        .unwrap()
    else {
        panic!("preview expected")
    };
    let target = Path::new(&first.target_path);
    assert!(
        target.starts_with(std::env::var_os("HOME").unwrap()),
        "write escaped disposable home"
    );
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    let original = br#"{"theme":"system","username":"fixture-user"}"#;
    fs::write(target, original).unwrap();
    let Outcome::Preview(preview) = execute(transport, id, Operation::Preview(request.clone()))
        .result
        .unwrap()
    else {
        panic!("preview expected")
    };
    let redacted = serde_json::to_string(&preview).unwrap();
    assert!(!redacted.contains("synthetic-native-provider-key"));
    assert!(!preview.files.is_empty());
    request.preview_id = preview.preview_id;
    assert!(request.preview_id.is_some());
    let external = br#"{"theme":"dark","username":"externally-edited"}"#;
    fs::write(target, external).unwrap();
    let error = execute(transport, id, Operation::Apply(request.clone()))
        .result
        .err()
        .unwrap();
    assert_eq!(error.code, Some(AgentErrorCode::Conflict));
    assert_eq!(
        fs::read(target).unwrap(),
        external,
        "conflicting write must preserve every byte"
    );
    let Outcome::Preview(fresh) = execute(transport, id, Operation::Preview(request.clone()))
        .result
        .unwrap()
    else {
        panic!("preview expected")
    };
    assert_ne!(fresh.preview_id, request.preview_id);
    request.preview_id = fresh.preview_id;
    match execute(transport, id, Operation::Apply(request.clone())).result {
        Err(error)
            if cfg!(target_os = "macos")
                && error.message == "default Keychain unavailable; unlock Keychain and retry" =>
        {
            assert_eq!(fs::read(target).unwrap(), external);
            eprintln!("EVIDENCE: real IPC preimage conflict passed; fresh apply failed closed without a Keychain in disposable HOME. Apply/rollback success is covered by Agent handler tests.");
        }
        Ok(Outcome::Applied(applied)) => {
            assert_eq!(
                applied.outcome,
                aipass_agent_protocol::ToolConfigOutcome::Applied
            );
            let content = fs::read_to_string(target).unwrap();
            assert!(
                content.contains("synthetic-native-provider-key")
                    && content.contains("externally-edited")
            );
            let Outcome::Applied(restored) =
                execute(transport, id, Operation::Rollback(applied.operation_id))
                    .result
                    .unwrap()
            else {
                panic!("rollback expected")
            };
            assert_eq!(
                restored.outcome,
                aipass_agent_protocol::ToolConfigOutcome::Applied
            );
            assert_eq!(fs::read(target).unwrap(), external);
            eprintln!("EVIDENCE: real IPC preimage conflict, apply and rollback passed.");
        }
        _ => panic!("unexpected integration apply result"),
    }
    transport.execute(Command::Lock).unwrap();
    assert_eq!(
        execute(transport, id, Operation::Preview(request))
            .result
            .err()
            .unwrap()
            .code,
        Some(AgentErrorCode::Locked)
    );
    transport
        .execute(Command::Unlock("synthetic-native-workflow-password".into()))
        .unwrap();
    assert!(!fs::read_to_string(root.join("agent-process.log"))
        .unwrap()
        .contains("synthetic-native-provider-key"));
}
