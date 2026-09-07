use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use tokio_tungstenite::tungstenite::{accept, Message};

#[test]
fn recovery_requires_empty_ws_completion_and_revalidates_after_unlocked_probe() {
    for mode in [
        "completed",
        "credential_interface",
        "staged_interface",
        "failed",
        "nonempty",
        "changed",
    ] {
        let accepted = matches!(
            mode,
            "completed" | "credential_interface" | "staged_interface"
        );
        let temp = tempfile::tempdir().unwrap();
        let password = SecretString::new("recovery-test-password");
        let vault = Vault::create(temp.path(), &password).unwrap().vault;
        let mut input = tests::sync_test_provider("original title", "old-key");
        input.supports_websockets = Some(false);
        input.default_model = Some("test-model".into());
        if mode == "credential_interface" {
            input.interface_type = InterfaceType::AnthropicMessages;
            input.secret_metadata.interface_type = Some(InterfaceType::OpenAiCompatible);
        } else if mode == "staged_interface" {
            input.secret_metadata.interface_type = Some(InterfaceType::AnthropicMessages);
        }
        let id = vault.add_provider(input.clone()).unwrap();
        let state = tests::sync_test_state(temp.path().to_path_buf());
        crate::session::set_session_vault(&state, vault);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let mut edit: aipass_vault::ProviderEntryUpdateInput =
            serde_json::from_value(serde_json::to_value(input).unwrap()).unwrap();
        edit.title = "saved draft".into();
        edit.api_key = Some("pending-new-key".into());
        edit.headers = Some(vec![("x-pending-header".into(), "pending-value".into())]);
        edit.endpoints = vec![ProviderEndpoint::api(format!("http://{address}/v1"))];
        edit.supports_websockets = Some(true);
        edit.secret_metadata.interface_type = match mode {
            "staged_interface" => Some(InterfaceType::OpenAiCompatible),
            _ => None,
        };
        let server_state = state.clone();
        let server = thread::spawn(move || {
            let (mut http, _) = listener.accept().unwrap();
            http.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                http.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
            assert!(request.starts_with("get /v1/models"));
            assert!(request.contains("authorization: bearer pending-new-key"));
            assert!(request.contains("x-pending-header: pending-value"));
            let body = r#"{"data":[{"id":"test-model"}]}"#;
            write!(http, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            drop(http);
            let (socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut ws = accept(socket).unwrap();
            let payload: serde_json::Value =
                serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(payload["generate"], false);
            assert_eq!(payload["input"], serde_json::json!([]));
            // This would deadlock if the network probe retained the vault lock.
            with_vault(&server_state, false, |vault| {
                assert_eq!(
                    vault.get_provider_summary(id).unwrap().supports_websockets,
                    Some(false)
                );
                if mode == "changed" {
                    vault.set_provider_favorite(id, true).unwrap();
                }
                Ok(())
            })
            .unwrap();
            let result = match mode {
                "failed" => serde_json::json!({"type":"response.failed","response":{"id":"probe"}}),
                "nonempty" => {
                    serde_json::json!({"type":"response.completed","response":{"id":"probe","output":[{"type":"message"}]}})
                }
                _ => {
                    serde_json::json!({"type":"response.completed","response":{"id":"probe","output":[]}})
                }
            };
            ws.send(Message::text(result.to_string())).unwrap();
        });
        let result = handle_request(&state, AgentRequest::ProviderUpdate { id, input: edit });
        server.join().unwrap();
        assert_eq!(result.ok, accepted, "{mode}: {result:?}");
        with_vault(&state, false, |vault| {
            let summary = vault.get_provider_summary(id).unwrap();
            assert_eq!(summary.supports_websockets, Some(accepted));
            assert_eq!(
                summary.title,
                if accepted {
                    "saved draft"
                } else {
                    "original title"
                }
            );
            assert_eq!(
                vault.reveal_secret(id).unwrap(),
                if accepted {
                    "pending-new-key"
                } else {
                    "old-key"
                }
            );
            if mode == "changed" {
                assert!(summary.favorite);
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(
            state.sync_revision.load(Ordering::Relaxed),
            u64::from(accepted)
        );
    }
}
