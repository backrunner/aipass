use super::*;
use crate::catalog::{draft::Draft, optional, Action, Catalog};
use aipass_agent_protocol::{AgentErrorCode, AgentRequest};

#[test]
fn inferred_domains_follow_changed_api_hosts_and_explicit_overrides_survive() {
    let entry = Catalog::fixture().active.remove(0);
    let mut draft = Draft::new(Some(entry.clone()));
    assert!(draft.domains.is_empty());
    draft.endpoint = "https://new.example.invalid/v1, https://new.example.invalid/v2, https://other.example.invalid/v1".into();
    draft.console = "https://console.example.invalid".into();
    let Action::Update(_, input) = draft.into_action().unwrap() else {
        panic!("update expected")
    };
    assert_eq!(
        input.domains,
        ["new.example.invalid", "other.example.invalid"]
    );
    let mut entry = entry;
    entry.domains = vec!["explicit.example.invalid".into()];
    let mut draft = Draft::new(Some(entry));
    draft.endpoint = "https://changed.example.invalid".into();
    let Action::Update(_, input) = draft.into_action().unwrap() else {
        panic!("update expected")
    };
    assert_eq!(input.domains, ["explicit.example.invalid"]);
}

#[test]
fn headers_merge_case_insensitively_and_preserve_unedited_values() {
    let result = merge_headers(
        vec![
            ("Authorization".into(), "old".into()),
            ("x-version".into(), "1".into()),
        ],
        optional::pairs("authorization=new, x-extra=a=b, X-EXTRA=last"),
    );
    assert_eq!(
        result,
        [
            ("authorization".into(), "new".into()),
            ("x-version".into(), "1".into()),
            ("X-EXTRA".into(), "last".into())
        ]
    );
}

#[test]
fn aliases_round_trip_escaping_and_clear_only_when_edited() {
    let mut entry = Catalog::fixture().active.remove(0);
    entry.model_aliases = vec![("fast,=\\路".into(), "gpt=latest,\\".into())];
    let mut draft = Draft::new(Some(entry.clone()));
    assert_eq!(
        optional::pairs(&draft.add_fields.aliases),
        entry.model_aliases
    );
    draft
        .original
        .as_mut()
        .unwrap()
        .model_aliases
        .push(("background".into(), "fresh".into()));
    let Action::Update(_, input) = draft.into_action().unwrap() else {
        panic!("update expected")
    };
    assert_eq!(input.model_aliases.len(), 2);
    let mut draft = Draft::new(Some(entry));
    draft.add_fields.aliases.clear();
    draft.aliases_touched = true;
    let Action::Update(_, input) = draft.into_action().unwrap() else {
        panic!("update expected")
    };
    assert!(input.model_aliases.is_empty());
}

#[test]
fn header_edit_requires_latest_successful_read_even_with_empty_summary() {
    let mut entry = Catalog::fixture().active.remove(0);
    entry.header_names.clear();
    let make = || {
        let mut draft = Draft::new(Some(entry.clone()));
        draft.add_fields.headers = "x-new=2".into();
        draft.into_action().unwrap()
    };
    let action = make();
    assert!(action.saves_form());
    assert_eq!(action.selected(), Some(entry.id));
    let request = action
        .into_request(|id| {
            assert_eq!(id, entry.id);
            Ok(vec![("x-existing".into(), "1".into())])
        })
        .unwrap();
    let AgentRequest::ProviderUpdate { input, .. } = request else {
        panic!("update expected")
    };
    assert_eq!(
        input.headers.unwrap(),
        [
            ("x-existing".into(), "1".into()),
            ("x-new".into(), "2".into())
        ]
    );
    let result = make().into_request(|_| {
        Err(aipass_agent::AgentCommandError {
            code: Some(AgentErrorCode::Locked),
            message: "locked".into(),
        })
    });
    assert!(result.is_err());
    // Blank edits never reveal headers and retain the protocol's preserve marker.
    let request = Draft::new(Some(entry))
        .into_action()
        .unwrap()
        .into_request(|_| panic!("unnecessary reveal"))
        .unwrap();
    let AgentRequest::ProviderUpdate { input, .. } = request else {
        panic!("update expected")
    };
    assert!(input.headers.is_none());
}
