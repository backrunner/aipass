use super::*;
use aipass_crypto::SecretString;

#[test]
fn shared_upstream_preserves_keys_models_and_protocols_across_reimports() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::create(temp.path(), &SecretString::new("fixture-password"))
        .unwrap()
        .vault;
    let make = |key: &str, model: &str, interface_type: InterfaceType| CcswitchProvider {
        app_id: "opencode".into(),
        name: "Shared relay".into(),
        key: Some(key.into()),
        base_url: Some("https://relay.example.test/v1".into()),
        model: Some(model.into()),
        website_host: None,
        category: None,
        notes: None,
        is_current: false,
        interface_type,
    };
    let providers = || {
        vec![
            make("fixture-key-a", "model-a", InterfaceType::OpenAiCompatible),
            make("fixture-key-b", "model-b", InterfaceType::OpenAiCompatible),
            make("fixture-key-a", "model-c", InterfaceType::OpenAiCompatible),
            make("fixture-key-a", "model-a", InterfaceType::AnthropicMessages),
        ]
    };
    let results = import_providers(&vault, providers()).unwrap();
    assert!(results.iter().all(|result| result.status == "imported"));
    let entries = vault.list_provider_summaries().unwrap();
    assert_eq!(entries.len(), 4);
    for expected in providers() {
        assert!(entries.iter().any(|entry| entry.fingerprint
            == vault.fingerprint_secret(expected.key.as_deref().unwrap())
            && entry.default_model == expected.model
            && entry.interface_type == expected.interface_type));
    }
    let results = import_providers(&vault, providers()).unwrap();
    assert!(results.iter().all(|result| result.status == "skipped"));
    assert_eq!(vault.list_provider_summaries().unwrap().len(), 4);

    let mut other_endpoint = make("fixture-key-a", "model-a", InterfaceType::OpenAiCompatible);
    other_endpoint.base_url = Some("https://other.example.test/v1".into());
    let new_key = make("fixture-key-c", "model-a", InterfaceType::OpenAiCompatible);
    let results = import_providers(&vault, vec![other_endpoint, new_key]).unwrap();
    assert!(results.iter().all(|result| result.status == "imported"));
    assert_eq!(vault.list_provider_summaries().unwrap().len(), 6);
}
