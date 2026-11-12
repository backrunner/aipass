use super::*;
use crate::catalog::Catalog;

#[test]
fn installation_requires_runtime_and_cursor_requires_local_agent() {
    for (tool, binary, local, expected) in [
        (Tool::Codex, false, false, false),
        (Tool::Codex, true, false, true),
        (Tool::Cursor, true, false, false),
        (Tool::Cursor, false, true, true),
    ] {
        let detection = ToolDetection {
            tool: tool_id(&tool),
            binary_found: binary,
            local_runtime_found: local,
            extension_found: false,
            config_path: Some("/synthetic/stale".into()),
        };
        assert_eq!(installed(&tool, &[detection]), expected, "{tool:?}");
    }
}

#[test]
fn capability_uses_key_protocol_and_auth_and_keeps_missing_model_visible() {
    let mut entry = Catalog::fixture().active.remove(0);
    let mut secret = entry.secret_refs[0].clone();
    entry.default_model = None;
    assert!(compatible(&Tool::Pi, &entry, &secret));
    assert!(!ready(&Tool::Pi, &entry, &secret));
    assert!(ready(&Tool::Codex, &entry, &secret));
    secret.default_model = Some("fixture".into());
    assert!(ready(&Tool::Pi, &entry, &secret));
    secret.interface_type = Some(Interface::AnthropicMessages);
    assert!(!compatible(&Tool::Codex, &entry, &secret));
    assert!(compatible(&Tool::ClaudeCode, &entry, &secret));
    entry.interface_type = Interface::AnthropicMessages;
    entry.auth_scheme = Auth::Bearer;
    assert!(compatible(&Tool::ClaudeCode, &entry, &secret));
    assert!(!compatible(&Tool::OpenCode, &entry, &secret));
    secret.interface_type = Some(Interface::Gemini);

    assert!(compatible(&Tool::GeminiCli, &entry, &secret));
    assert!(!compatible(&Tool::ClaudeCode, &entry, &secret));
}

#[test]
fn muse_requires_vendor_endpoint_and_oauth_never_enters_api_writer() {
    let mut entry = Catalog::fixture().active.remove(0);
    let mut secret = entry.secret_refs[0].clone();
    assert!(!compatible(&Tool::MuseCode, &entry, &secret));
    secret.endpoint = Some("https://api.meta.ai/v1/".into());
    assert!(compatible(&Tool::MuseCode, &entry, &secret));
    entry.credential_kind = CredentialKind::OAuth;
    assert!(AUTOMATIC
        .iter()
        .all(|(tool, _)| !compatible(tool, &entry, &secret)));
}

#[test]
fn all_automatic_tools_map_to_installed_detection_ids() {
    for (tool, _) in AUTOMATIC {
        let d = ToolDetection {
            tool: tool_id(&tool),
            binary_found: true,
            local_runtime_found: true,
            extension_found: false,
            config_path: None,
        };
        assert!(installed(&tool, &[d]), "{tool:?}");
    }
}
