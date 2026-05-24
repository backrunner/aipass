mod config;
mod manifest;
mod preview;
mod protocol;
mod request;

pub use config::NativeHostConfig;
pub use manifest::native_manifest;
pub use protocol::{
    read_message, validate_extension_id, write_message, NativeRequest, NativeResponse,
};
pub use request::{handle_request, handle_request_with_config};

#[cfg(test)]
mod tests {
    use super::*;
    use aipass_crypto::SecretString;
    use aipass_vault::scan_for_plaintext;
    use std::path::PathBuf;
    use tempfile::tempdir;
    use uuid::Uuid;

    #[test]
    fn rejects_unknown_extension() {
        assert!(validate_extension_id("bad", &["good".to_string()]).is_err());
        assert!(validate_extension_id("any", &[]).is_ok());
    }

    #[test]
    fn round_trip_message() {
        let id = Uuid::new_v4();
        let request = NativeRequest::Ping {
            id,
            protocol_version: 1,
            extension_id: None,
        };
        let body = serde_json::to_vec(&request).unwrap();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&body);
        let parsed = read_message(bytes.as_slice()).unwrap();
        let response = handle_request_with_config(
            parsed,
            &NativeHostConfig {
                vault_dir: PathBuf::from("/tmp/missing"),
                master_password: None,
                allowed_extension_ids: vec![],
            },
        );
        assert!(response.ok);
    }

    #[test]
    fn rejects_native_request_without_allowed_extension_id() {
        let response = handle_request_with_config(
            NativeRequest::Ping {
                id: Uuid::new_v4(),
                protocol_version: 1,
                extension_id: None,
            },
            &NativeHostConfig {
                vault_dir: PathBuf::from("/tmp/missing"),
                master_password: None,
                allowed_extension_ids: vec!["good-extension-id".to_string()],
            },
        );
        assert!(!response.ok);
        assert_eq!(response.error.as_deref(), Some("extension id missing"));
    }

    #[test]
    fn accepts_native_request_with_allowed_extension_id() {
        let response = handle_request_with_config(
            NativeRequest::Ping {
                id: Uuid::new_v4(),
                protocol_version: 1,
                extension_id: Some("chrome-extension://good-extension-id/".to_string()),
            },
            &NativeHostConfig {
                vault_dir: PathBuf::from("/tmp/missing"),
                master_password: None,
                allowed_extension_ids: vec!["good-extension-id".to_string()],
            },
        );
        assert!(response.ok);
    }

    #[test]
    fn lookup_and_fill_uses_short_lived_grant() {
        let dir = tempdir().unwrap();
        let password = "correct horse battery staple".to_string();
        aipass_vault::Vault::create(dir.path(), &SecretString::new(&password)).unwrap();
        let config = NativeHostConfig {
            vault_dir: dir.path().to_path_buf(),
            master_password: Some(password),
            allowed_extension_ids: vec![],
        };
        let save = handle_request_with_config(
            NativeRequest::SaveDetected {
                id: Uuid::new_v4(),
                extension_id: None,
                origin: "https://console.anthropic.com".to_string(),
                url: "https://console.anthropic.com/settings/keys".to_string(),
                title: Some("Anthropic Browser".to_string()),
                endpoint: Some("https://api.anthropic.com".to_string()),
                provider_id: Some("anthropic".to_string()),
                interface_type: Some(aipass_provider_registry::InterfaceType::AnthropicMessages),
                auth_scheme: Some(aipass_provider_registry::AuthScheme::XApiKey),
                api_key: "sk-ant-api03-browser-secret".to_string(),
                environment: Some("work".to_string()),
                tags: vec!["browser".to_string()],
            },
            &config,
        );
        assert!(save.ok, "{save:?}");
        let lookup = handle_request_with_config(
            NativeRequest::ContextLookup {
                id: Uuid::new_v4(),
                extension_id: None,
                origin: "https://console.anthropic.com".to_string(),
                url: "https://console.anthropic.com/settings/keys".to_string(),
            },
            &config,
        );
        assert!(lookup.ok, "{lookup:?}");
        let grants = lookup.data["grants"].as_array().unwrap();
        let entries = lookup.data["entries"].as_array().unwrap();
        let grant_id = Uuid::parse_str(grants[0]["id"].as_str().unwrap()).unwrap();
        let entry_id = Uuid::parse_str(entries[0]["id"].as_str().unwrap()).unwrap();
        let fill = handle_request_with_config(
            NativeRequest::SecretFill {
                id: Uuid::new_v4(),
                extension_id: None,
                entry_id,
                field_id: "primary".to_string(),
                grant_id,
            },
            &config,
        );
        assert!(fill.ok, "{fill:?}");
        assert_eq!(fill.data["secret"], "sk-ant-api03-browser-secret");
        let matches = scan_for_plaintext(dir.path(), &["sk-ant-api03-browser-secret"]).unwrap();
        assert!(
            matches.is_empty(),
            "native host leaked plaintext to {matches:?}"
        );
    }

    #[test]
    fn save_detected_infers_endpoint_interface_and_auth() {
        let dir = tempdir().unwrap();
        let password = "correct horse battery staple".to_string();
        aipass_vault::Vault::create(dir.path(), &SecretString::new(&password)).unwrap();
        let config = NativeHostConfig {
            vault_dir: dir.path().to_path_buf(),
            master_password: Some(password),
            allowed_extension_ids: vec![],
        };
        let save = handle_request_with_config(
            NativeRequest::SaveDetected {
                id: Uuid::new_v4(),
                extension_id: None,
                origin: "https://gateway.example.test".to_string(),
                url: "https://gateway.example.test/ui".to_string(),
                title: Some("Gateway".to_string()),
                endpoint: Some("https://gateway.example.test/v1".to_string()),
                provider_id: None,
                interface_type: None,
                auth_scheme: None,
                api_key: "sk-gateway-secret-value".to_string(),
                environment: Some("work".to_string()),
                tags: vec!["browser".to_string()],
            },
            &config,
        );
        assert!(save.ok, "{save:?}");
        let vault = aipass_vault::Vault::open(
            dir.path(),
            &SecretString::new("correct horse battery staple"),
        )
        .unwrap();
        let entries = vault.search("gateway").unwrap();
        assert_eq!(
            entries[0].interface_type,
            aipass_provider_registry::InterfaceType::OpenAiCompatible
        );
        assert_eq!(
            entries[0].auth_scheme,
            aipass_provider_registry::AuthScheme::Bearer
        );
    }

    #[test]
    fn preview_detected_reports_preview_without_persisting() {
        let dir = tempdir().unwrap();
        let password = "correct horse battery staple".to_string();
        aipass_vault::Vault::create(dir.path(), &SecretString::new(&password)).unwrap();
        let config = NativeHostConfig {
            vault_dir: dir.path().to_path_buf(),
            master_password: Some(password),
            allowed_extension_ids: vec![],
        };
        let preview = handle_request_with_config(
            NativeRequest::PreviewDetected {
                id: Uuid::new_v4(),
                extension_id: None,
                origin: "https://gateway.example.test".to_string(),
                url: "https://gateway.example.test/ui".to_string(),
                title: Some("Gateway".to_string()),
                endpoint: Some("https://gateway.example.test/v1".to_string()),
                provider_id: None,
                interface_type: None,
                auth_scheme: None,
                api_key: "sk-gateway-secret-value".to_string(),
                environment: Some("work".to_string()),
                tags: vec!["browser".to_string()],
            },
            &config,
        );
        assert!(preview.ok, "{preview:?}");
        assert_eq!(preview.data["maskedSecret"], "•••• alue");
        assert!(!preview.data["fingerprint"].as_str().unwrap().is_empty());
        let vault = aipass_vault::Vault::open(
            dir.path(),
            &SecretString::new("correct horse battery staple"),
        )
        .unwrap();
        assert!(vault.search("Gateway").unwrap().is_empty());
    }
}
