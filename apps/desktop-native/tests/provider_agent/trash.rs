use super::*;
use aipass_desktop_native::trash::Request;

fn delete(
    transport: &mut AgentTransport,
    target: Option<uuid::Uuid>,
) -> Result<(), aipass_agent::AgentCommandError> {
    let Reply::Trash(completion) = transport
        .execute(Command::Trash(Request {
            generation: 71,
            target,
        }))
        .unwrap()
    else {
        panic!("trash acknowledgement expected");
    };
    assert_eq!(completion.generation, 71);
    completion.result
}

pub(super) fn workflow(transport: &mut AgentTransport) {
    let before = snapshot(transport);
    let create = |transport: &mut AgentTransport, title: &str| {
        let mut draft = Draft::new(None);
        draft.title = title.into();
        draft.api_key = "synthetic-trash-fixture".into();
        draft.endpoint = "https://trash.example.invalid/v1".into();
        applied(transport, draft.into_action().unwrap()).unwrap()
    };
    let first = create(transport, "Delete fixture");
    let second = create(transport, "Empty fixture");
    applied(transport, Action::Trash(first));
    applied(transport, Action::Trash(second));
    delete(transport, Some(first)).unwrap();
    let catalog = snapshot(transport);
    assert!(!catalog.trash.iter().any(|entry| entry.id == first));
    assert!(catalog.trash.iter().any(|entry| entry.id == second));
    transport.execute(Command::Lock).unwrap();
    assert_eq!(
        delete(transport, None).unwrap_err().code,
        Some(aipass_agent_protocol::AgentErrorCode::Locked)
    );
    transport
        .execute(Command::Unlock("synthetic-native-workflow-password".into()))
        .unwrap();
    assert!(snapshot(transport)
        .trash
        .iter()
        .any(|entry| entry.id == second));
    delete(transport, None).unwrap();
    let after = snapshot(transport);
    assert!(after.trash.is_empty());
    assert_eq!(
        before
            .active
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        after
            .active
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        before
            .archived
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        after
            .archived
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>()
    );
    let Reply::Touched(status) = transport.execute(Command::Touch).unwrap() else {
        panic!("activity acknowledgement expected");
    };
    assert!(!status.locked);
}
