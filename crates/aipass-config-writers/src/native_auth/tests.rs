use super::*;

fn store(tool: &str, auth: &str) -> (tempfile::TempDir, NativeStore) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("auth.json");
    fs::write(&path, auth).unwrap();
    let store = NativeStore {
        tool: tool.into(),
        home: root.path().to_owned(),
        primary: Resource::File(path),
        fallback: None,
        profile: Some(Resource::File(root.path().join("profile.json"))),
    };
    (root, store)
}

#[test]
fn signed_out_placeholders_do_not_require_an_account_profile() {
    for (tool, auth) in [
        ("codex", "{}"),
        ("codex", r#"{"tokens":null}"#),
        (
            "codex",
            r#"{"tokens":{"access_token":" ","refresh_token":""}}"#,
        ),
        ("claude-code", "{}"),
        ("claude-code", r#"{"claudeAiOauth":null,"mcpOAuth":{}}"#),
        ("claude-code", r#"{"claudeAiOauth":{}}"#),
        (
            "claude-code",
            r#"{"claudeAiOauth":{"accessToken":"","refreshToken":" "}}"#,
        ),
    ] {
        let (_root, store) = store(tool, auth);
        assert_eq!(store.identity().unwrap(), None, "tool={tool}");
        assert_eq!(
            store.primary.read().unwrap().unwrap().as_slice(),
            auth.as_bytes()
        );
    }
}

#[test]
fn existing_native_grants_without_identity_are_not_treated_as_signed_out() {
    for (tool, auth) in [
        ("codex", r#"{"tokens":{"access_token":"fixture"}}"#),
        ("codex", r#"{"tokens":{"refresh_token":"fixture"}}"#),
        (
            "claude-code",
            r#"{"claudeAiOauth":{"accessToken":"fixture"}}"#,
        ),
        (
            "claude-code",
            r#"{"claudeAiOauth":{"refreshToken":"fixture"}}"#,
        ),
    ] {
        let (_root, store) = store(tool, auth);
        assert!(store.identity().is_err(), "tool={tool}");
    }
}

#[test]
fn malformed_or_unreadable_stores_remain_errors() {
    for auth in [
        "invalid json",
        "[]",
        r#"{"claudeAiOauth":[]}"#,
        r#"{"claudeAiOauth":{"accessToken":123}}"#,
    ] {
        let (root, store) = store("claude-code", auth);
        assert!(store.identity().is_err());
        fs::remove_file(root.path().join("auth.json")).unwrap();
        fs::create_dir(root.path().join("auth.json")).unwrap();
        assert!(store.identity().is_err());
    }
}
