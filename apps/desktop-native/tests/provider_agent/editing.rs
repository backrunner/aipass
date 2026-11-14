use super::*;
use aipass_agent::{AgentClient, AgentCommandError};
use aipass_agent_protocol::{AgentErrorCode, AgentRequest, ProviderHeaderValues};

fn reveal(client: &AgentClient, id: uuid::Uuid) -> Vec<(String, String)> {
    let result: ProviderHeaderValues = client
        .request(&AgentRequest::SecretRevealHeaders { id })
        .unwrap();
    result
        .headers
        .into_iter()
        .map(|(name, value)| (name, value.expose().to_owned()))
        .collect()
}

pub(super) fn workflow(transport: &mut AgentTransport, root: &Path, id: uuid::Uuid) {
    let client = AgentClient::for_vault(root.join("data/vault")).unwrap();
    let mut edit = Draft::new(Some(snapshot(transport).find(id).unwrap().clone()));
    edit.add_fields.headers = "x-version=1, x-keep=preserved".into();
    edit.add_fields.aliases = r"fast\,cheap=model\=current".into();
    edit.aliases_touched = true;
    applied(transport, edit.into_action().unwrap());
    let original = snapshot(transport).find(id).unwrap().clone();
    assert_eq!(
        original.model_aliases,
        [("fast,cheap".into(), "model=current".into())]
    );
    assert_eq!(original.header_names, ["x-version", "x-keep"]);

    let mut edit = Draft::new(Some(original.clone()));
    edit.add_fields.headers = "X-Version=2, x-new=3".into();
    applied(transport, edit.into_action().unwrap());
    let headers = reveal(&client, id);
    assert_eq!(
        headers,
        [
            ("X-Version".into(), "2".into()),
            ("x-keep".into(), "preserved".into()),
            ("x-new".into(), "3".into())
        ]
    );
    // A normal metadata save keeps the newly merged headers without revealing them.
    let mut edit = Draft::new(Some(snapshot(transport).find(id).unwrap().clone()));
    edit.notes = "headers retained".into();
    edit.aliases_touched = true;
    edit.add_fields.aliases.clear();
    applied(transport, edit.into_action().unwrap());
    let entry = snapshot(transport).find(id).unwrap().clone();
    assert!(entry.model_aliases.is_empty());
    let preserved = reveal(&client, id);
    assert_eq!(headers, preserved);

    // Missing source headers fail before any other edit is written.
    let mut edit = Draft::new(Some(original));
    edit.original.as_mut().unwrap().id = uuid::Uuid::new_v4();
    edit.add_fields.headers = "x-new=discarded".into();
    let result = transport.execute(Command::Provider(edit.into_action().unwrap()));
    assert!(result.is_err());
}

pub(super) fn locked(transport: &mut AgentTransport, entry: aipass_vault::EntrySummary) {
    let mut edit = Draft::new(Some(entry));
    edit.title = "must not save".into();
    edit.add_fields.headers = "x-version=discarded".into();
    let result = transport.execute(Command::Provider(edit.into_action().unwrap()));
    assert!(matches!(
        result,
        Err(AgentCommandError {
            code: Some(AgentErrorCode::Locked),
            ..
        })
    ));
}
