use super::*;
use aipass_desktop_native::credentials::{Completion, Operation, Outcome, Request, Scope};
use aipass_provider_registry::{BillingRule, InterfaceType};
use aipass_vault::SecretMetadataInput;

fn execute(transport: &mut AgentTransport, id: uuid::Uuid, operation: Operation) -> Completion {
    let scope = Scope {
        entry: id,
        generation: 42,
    };
    let Reply::Credential(completion) = transport
        .execute(Command::Credential(Box::new(Request { scope, operation })))
        .unwrap()
    else {
        panic!("credential reply expected")
    };
    assert!(completion.scope == scope);
    completion
}

fn reveal(transport: &mut AgentTransport, id: uuid::Uuid, secret_id: &str, expected: &str) {
    let Outcome::Revealed(value) = execute(transport, id, Operation::Reveal(secret_id.into()))
        .result
        .unwrap()
    else {
        panic!("read expected")
    };
    assert_eq!(value.expose(), expected);
}

pub(super) fn workflow(transport: &mut AgentTransport, id: uuid::Uuid) -> String {
    let first = snapshot(transport).active[0].secret_refs[0].id.clone();
    reveal(transport, id, &first, "synthetic-native-provider-key");
    assert!(
        execute(transport, id, Operation::Reveal("missing-id".into()))
            .result
            .is_err()
    );
    assert!(matches!(
        execute(
            transport,
            id,
            Operation::Add {
                label: "backup".into(),
                value: "synthetic-backup".into(),
                metadata: SecretMetadataInput {
                    interface_type: Some(InterfaceType::OpenAiCompatible),
                    group: Some("team".into()),
                    endpoint: Some("https://override.invalid/v1".into()),
                    default_model: Some("fixture-model".into()),
                    billing: Some(BillingRule {
                        rate: Some("1.5".into()),
                        note: Some("preserved".into()),
                        ..Default::default()
                    }),
                },
            }
        )
        .result,
        Ok(Outcome::Applied)
    ));
    let second = snapshot(transport).active[0]
        .secret_refs
        .iter()
        .find(|key| key.label == "backup")
        .unwrap()
        .clone();
    assert_ne!(first, second.id);
    assert_eq!(
        second.endpoint.as_deref(),
        Some("https://override.invalid/v1")
    );
    assert_eq!(second.group.as_deref(), Some("team"));
    reveal(transport, id, &second.id, "synthetic-backup");
    assert!(execute(
        transport,
        id,
        Operation::Add {
            label: "backup".into(),
            value: "synthetic-duplicate".into(),
            metadata: Default::default(),
        }
    )
    .result
    .is_err());
    assert_eq!(snapshot(transport).active[0].secret_refs.len(), 2);
    // A label that equals another key's ID must never redirect ID-based actions.
    assert!(matches!(
        execute(
            transport,
            id,
            Operation::Update {
                secret_id: second.id.clone(),
                label: first.clone(),
                value: "synthetic-rotated".into(),
                metadata: SecretMetadataInput {
                    interface_type: Some(InterfaceType::Gemini),
                    group: Some(String::new()),
                    endpoint: Some(String::new()),
                    default_model: Some(String::new()),
                    billing: None
                },
            }
        )
        .result,
        Ok(Outcome::Applied)
    ));
    let current = snapshot(transport).active[0]
        .secret_refs
        .iter()
        .find(|key| key.id == second.id)
        .unwrap()
        .clone();
    assert_eq!(current.interface_type, Some(InterfaceType::Gemini));
    assert!(
        current.endpoint.is_none() && current.default_model.is_none() && current.group.is_none()
    );
    assert_eq!(current.billing.unwrap().note.as_deref(), Some("preserved"));
    reveal(transport, id, &first, "synthetic-native-provider-key");
    reveal(transport, id, &second.id, "synthetic-rotated");
    assert!(matches!(
        execute(transport, id, Operation::Remove(first.clone())).result,
        Ok(Outcome::Applied)
    ));
    let refs = snapshot(transport).active[0].secret_refs.clone();
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].id, second.id);
    // Exact-ID reveal rejects a removed ID, even though another key has that label.
    assert!(execute(transport, id, Operation::Reveal(first))
        .result
        .is_err());
    second.id
}

pub(super) fn locked(transport: &mut AgentTransport, id: uuid::Uuid, secret_id: &str) {
    for operation in [
        Operation::Reveal(secret_id.into()),
        Operation::Remove(secret_id.into()),
    ] {
        assert_eq!(
            execute(transport, id, operation).result.err().unwrap().code,
            Some(aipass_agent_protocol::AgentErrorCode::Locked)
        );
    }
}

pub(super) fn persisted(transport: &mut AgentTransport, id: uuid::Uuid, secret_id: &str) {
    reveal(transport, id, secret_id, "synthetic-rotated");
    assert_eq!(snapshot(transport).active[0].secret_refs.len(), 1);
}
