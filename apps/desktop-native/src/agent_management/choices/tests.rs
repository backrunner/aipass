use super::*;
#[test]
fn managed_modes_and_native_credentials_follow_reference_contract() {
    let mut entries = crate::catalog::Catalog::fixture().active;
    let codex = choices(&entries, &Tool::Codex);
    assert_eq!(codex.len(), 2);
    assert_eq!(codex[0].request.mode, ToolConfigMode::Plaintext);
    assert_eq!(
        codex[0].request.codex_api_key_mode,
        Some(CodexApiKeyMode::AuthJson)
    );
    assert_eq!(
        choices(&entries, &Tool::ClaudeCode)[0].request.mode,
        ToolConfigMode::Plaintext
    );
    assert_eq!(
        choices(&entries, &Tool::Zed)[0].request.mode,
        ToolConfigMode::Helper
    );
    entries[0].credential_kind = CredentialKind::OAuth;
    entries[0].provider_kind = aipass_provider_registry::ProviderKind::Official;
    entries[0].provider_id = Some("codex".into());
    let native = choices(&entries, &Tool::Codex);
    assert_eq!(native[0].request.mode, ToolConfigMode::Official);
    assert_eq!(native[0].request.codex_api_key_mode, None);
    assert!(choices(&entries, &Tool::Cline)
        .iter()
        .all(|c| c.request.id != entries[0].id));
    entries[1].archived_at = Some(time::OffsetDateTime::now_utc());
    assert!(choices(&entries, &Tool::ClaudeCode).is_empty());
}
#[test]
fn reconcile_prefers_valid_user_choice_then_current_binding_and_keeps_duplicate_labels_distinct() {
    let mut entries = crate::catalog::Catalog::fixture().active;
    entries[0].secret_refs[1].label = entries[0].secret_refs[0].label.clone();
    let values = choices(&entries, &Tool::Codex);
    assert_ne!(values[0].label, values[1].label);
    let mut status = empty_status(Tool::Codex, "ready");
    status.entry_id = Some(entries[0].id);
    status.secret_id = Some("key-a".into());
    assert_eq!(
        reconcile(&values, &values[1].value, Some(&status)),
        values[1].value
    );
    assert_eq!(
        reconcile(&values, "removed-key", Some(&status)),
        values[0].value
    );
    assert_eq!(reconcile(&values, "", None), "");
    entries[0].deleted_at = Some(time::OffsetDateTime::now_utc());
    assert!(choices(&entries, &Tool::Codex).is_empty());
}
