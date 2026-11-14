use super::draft::Draft;
use super::*;
use aipass_provider_registry::{AuthScheme, InterfaceType};

#[test]
fn catalog_keeps_archived_and_deleted_entries_out_of_active_filters() {
    let catalog = Catalog::fixture();
    assert_eq!(catalog.entries(0, "").len(), 2);
    assert_eq!(catalog.entries(1, "").len(), 1);
    assert_eq!(catalog.entries(2, "")[0].title, "Anthropic fixture");
    assert_eq!(
        catalog.entries(3, "")[0].provider_id.as_deref(),
        Some("anthropic")
    );
    assert_eq!(catalog.entries(9, "")[0].title, "Archived account");
    assert_eq!(catalog.entries(10, "")[0].title, "Deleted account");
    assert_eq!(catalog.entries(0, "gateway WORK").len(), 1);
    assert!(catalog.entries(0, "gateway anthropic").is_empty());
    assert!(catalog.entries(7, "").is_empty());
    assert!(catalog.entries(8, "").is_empty());
}

#[test]
fn quota_parser_matches_the_shared_forms_first_numeric_value() {
    for (text, expected) in [
        ("$1,200.50 credits", Some(1200.5)),
        ("12.3.4", Some(12.3)),
        ("12.", Some(12.)),
        ("-2", Some(2.)),
        ("unlimited", None),
    ] {
        assert_eq!(numeric(Some(text)), expected);
    }
}

#[test]
fn edit_preserves_credential_metadata_and_untouched_websocket_detection() {
    let catalog = Catalog::fixture();
    let entry = catalog.active[0].clone();
    let mut draft = Draft::new(Some(entry.clone()));
    draft.title = "Renamed provider".into();
    let Action::Update(id, input) = draft.into_action().unwrap() else {
        panic!("update expected")
    };
    assert_eq!(id, entry.id);
    assert_eq!(input.title, "Renamed provider");
    assert_eq!(input.provider_kind, entry.provider_kind);
    assert_eq!(input.model_aliases, entry.model_aliases);
    assert_eq!(input.endpoints, entry.endpoints);
    assert_eq!(input.supports_websockets, None);
    assert_eq!(input.max_concurrent_requests, None);
    assert!(input.headers.is_none());
    assert!(input.api_key.is_none());
    assert!(input.credential_kind.is_none());
    assert!(input.secret_label.is_none());
    assert!(input.subscription.is_none());
}

#[test]
fn clearing_optional_fields_explicitly_removes_all_api_endpoints() {
    let mut entry = Catalog::fixture().active.remove(0);
    let mut secondary = entry.endpoints[0].clone();
    secondary.id = "secondary-api".into();
    secondary.url = Some("https://secondary.example.invalid/v1".into());
    entry.endpoints.push(secondary.clone());
    let mut draft = Draft::new(Some(entry));
    draft.endpoint.clear();
    draft.concurrency.clear();
    draft.concurrency_touched = true;
    draft.tags.clear();
    draft.notes.clear();
    draft.websockets = true;
    draft.websocket_touched = true;
    let Action::Update(_, input) = draft.into_action().unwrap() else {
        panic!("update expected")
    };
    assert!(input.endpoints.is_empty());
    assert_eq!(input.max_concurrent_requests, Some(0));
    assert_eq!(input.supports_websockets, Some(true));
    assert!(input.tags.is_empty());
    assert!(input.notes.is_none());
}

#[test]
fn add_requires_a_key_and_rejects_invalid_endpoints_and_limits() {
    let mut draft = Draft::new(None);
    assert_eq!(draft.validate(), Err("providerForm.apiKeyRequired"));
    draft.api_key = " fixture-key ".into();
    for endpoint in [
        "ftp://example.invalid",
        "https://user:secret@example.invalid",
        "not a URL",
    ] {
        draft.endpoint = endpoint.into();
        assert_eq!(draft.validate(), Err("providers.invalidEndpoint"));
    }
    draft.endpoint = " https://api.example.invalid/v1 ".into();
    for limit in ["-1", "1.5", "4294967296", "invalid"] {
        draft.concurrency = limit.into();
        assert_eq!(
            draft.validate(),
            Err("providerForm.invalidConcurrencyLimit")
        );
    }
    draft.concurrency = "12".into();
    draft.tags = "one, two\nthree".into();
    let Action::Add(input) = draft.into_action().unwrap() else {
        panic!("add expected")
    };
    assert_eq!(input.api_key, "fixture-key");
    assert_eq!(input.tags, ["one", "two\nthree"]);
    assert_eq!(
        input.endpoints[0].url.as_deref(),
        Some("https://api.example.invalid/v1")
    );
    assert_eq!(input.max_concurrent_requests, Some(12));
    assert_eq!(input.credential_kind, CredentialKind::Api);
}

#[test]
fn protocol_changes_keep_compatible_auth_and_replace_incompatible_auth() {
    let mut draft = Draft::new(None);
    draft.choose_interface(InterfaceType::AnthropicMessages);
    assert_eq!(draft.auth, AuthScheme::XApiKey);
    draft.choose_interface(InterfaceType::Gemini);
    assert_eq!(draft.auth, AuthScheme::GoogleApiKey);
    draft.choose_interface(InterfaceType::AnthropicMessages);
    assert_eq!(draft.auth, AuthScheme::XApiKey);
    draft.choose_provider("anthropic".into());
    assert_eq!(draft.interface, InterfaceType::AnthropicMessages);
    assert_eq!(draft.auth, AuthScheme::XApiKey);
}

#[test]
fn multiple_endpoints_and_escaped_lists_preserve_metadata_without_duplicate_ids() {
    let mut entry = Catalog::fixture().active.remove(0);
    entry.tags = vec!["work, team".into(), "path\\label".into()];
    entry.endpoints[0].region = Some("fixture-region".into());
    let original = entry.endpoints[0].clone();
    let mut draft = Draft::new(Some(entry));
    draft.endpoint = format!(
        "https://new.example.invalid/v1?models=a,b, {}",
        original.url.as_ref().unwrap()
    );
    let Action::Update(_, input) = draft.into_action().unwrap() else {
        panic!("update expected")
    };
    assert_eq!(input.endpoints.len(), 2);
    assert_eq!(input.endpoints[1], original);
    assert_ne!(input.endpoints[0].id, input.endpoints[1].id);
    assert_eq!(input.tags, ["work, team", "path\\label"]);
}
