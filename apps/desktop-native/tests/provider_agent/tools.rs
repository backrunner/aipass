use super::*;
use aipass_agent_protocol::{AgentErrorCode, UsageProbeMode};
use aipass_desktop_native::provider_tools::{Completion, Operation, Outcome, Request, Scope};
fn call(transport: &mut AgentTransport, id: uuid::Uuid, operation: Operation) -> Completion {
    let Reply::ProviderTools(completion) = transport
        .execute(Command::ProviderTools(Box::new(Request {
            scope: Scope {
                entry: id,
                generation: 1,
            },
            operation,
        })))
        .unwrap()
    else {
        panic!("scoped tools response")
    };
    *completion
}
pub fn workflow(transport: &mut AgentTransport) {
    let mut draft = Draft::new(None);
    draft.title = "Maintenance fixture".into();
    draft.api_key = "synthetic-maintenance-key".into();
    // A bound local socket returns deterministic HTTP results. No vendor calls.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let alive = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let running = alive.clone();
    let mock = thread::spawn(move || {
        use std::io::{Read, Write};
        while running.load(std::sync::atomic::Ordering::Relaxed) {
            let Ok((mut socket, _)) = listener.accept() else {
                thread::sleep(Duration::from_millis(10));
                continue;
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = [0; 4096];
            let length = socket.read(&mut request).unwrap_or(0);
            let request = String::from_utf8_lossy(&request[..length]);
            let body = if request.contains("/models") {
                r#"{"object":"list","data":[{"id":"fixture-model","object":"model"}]}"#
            } else {
                r#"{"message":"fixture denied"}"#
            };
            let status = if request.contains("/models") {
                "200 OK"
            } else {
                "401 Unauthorized"
            };
            let _=write!(socket,"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
        }
    });
    draft.endpoint = format!("http://{address}/v1");
    draft.websockets = false;
    let id = applied(transport, draft.into_action().unwrap()).unwrap();
    let original = snapshot(transport).find(id).unwrap().clone();
    let result = call(transport, id, Operation::Probe);
    assert!(matches!(result.result,Ok(Outcome::Probe(p))if p.ok&&p.model_count==Some(1)));
    let result = call(
        transport,
        id,
        Operation::Usage {
            secret_id: Some(original.secret_refs[0].id.clone()),
            mode: UsageProbeMode::NewApi,
            base_url: Some(format!("http://{address}")),
            access_token: None,
            user_id: None,
        },
    );
    assert!(matches!(result.result,Ok(Outcome::Usage(p))if !p.ok));
    alive.store(false, std::sync::atomic::Ordering::Relaxed);
    mock.join().unwrap();
    let result=serde_json::from_value(serde_json::json!({"ok":true,"source":"new_api_token_usage","quota":{"remaining":"75","limit":"100","used":"25","unit":"USD"},"gateway":{"group":"fixture"}})).unwrap();
    assert!(matches!(
        call(
            transport,
            id,
            Operation::Apply {
                entry: Box::new(original.clone()),
                result: Box::new(result)
            }
        )
        .result,
        Ok(Outcome::Applied)
    ));
    let applied_entry = snapshot(transport).find(id).unwrap().clone();
    assert_eq!(
        applied_entry.quota.unwrap().remaining.as_deref(),
        Some("75")
    );
    assert_eq!(
        applied_entry.gateway.unwrap().group.as_deref(),
        Some("fixture")
    );
    let completion = call(
        transport,
        id,
        Operation::Reset {
            secrets: original.secret_refs.clone(),
            archived: false,
        },
    );
    assert!(completion.mutated);
    assert!(
        matches!(completion.result,Ok(Outcome::Reset(e))if e.secret_refs.is_empty()&&e.title==original.title)
    );
    assert!(snapshot(transport).find(id).unwrap().secret_refs.is_empty());
    transport.execute(Command::Lock).unwrap();
    for operation in [
        Operation::Probe,
        Operation::Reset {
            secrets: original.secret_refs,
            archived: false,
        },
    ] {
        assert_eq!(
            call(transport, id, operation).result.err().unwrap().code,
            Some(AgentErrorCode::Locked)
        );
    }
    transport
        .execute(Command::Unlock("synthetic-native-workflow-password".into()))
        .unwrap();
    assert!(snapshot(transport).find(id).unwrap().secret_refs.is_empty());
}
