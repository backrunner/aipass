mod config;
mod manifest;
mod protocol;
mod request;

pub use config::{
    load_allowed_extension_ids, native_host_settings_path, save_allowed_extension_ids,
    NativeHostConfig, NativeHostSettings,
};
pub use manifest::native_manifest;
pub use protocol::{
    read_message, validate_extension_id, write_message, NativeRequest, NativeResponse,
    NATIVE_PROTOCOL_VERSION,
};
pub use request::{handle_request, handle_request_with_config};

#[cfg(test)]
mod tests {
    use super::*;
    use aipass_agent::{run_server, AgentClient, AgentClientConfig, ServerOptions};
    use aipass_agent_protocol::{AgentRequest, SessionStatus, SessionUnlockMode};
    use aipass_crypto::SecretString;
    use aipass_vault::scan_for_plaintext;
    use std::path::PathBuf;
    use std::thread::{self, JoinHandle};
    use std::time::Duration;
    use tempfile::{tempdir, TempDir};
    use uuid::Uuid;

    struct RunningAgent {
        dir: TempDir,
        password: String,
        client: AgentClient,
        handle: Option<JoinHandle<()>>,
    }

    impl RunningAgent {
        fn start() -> Self {
            let dir = tempdir().unwrap();
            let password = "correct horse battery staple".to_string();
            aipass_vault::Vault::create(dir.path(), &SecretString::new(&password)).unwrap();
            let vault_dir = dir.path().to_path_buf();
            let handle = thread::spawn(move || {
                run_server(ServerOptions { vault_dir }).unwrap();
            });
            let client =
                AgentClient::new(AgentClientConfig::for_vault(dir.path().to_path_buf()).unwrap());
            for _ in 0..50 {
                if client
                    .request::<SessionStatus>(&AgentRequest::SessionStatus)
                    .is_ok()
                {
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }
            Self {
                dir,
                password,
                client,
                handle: Some(handle),
            }
        }

        fn config(&self) -> NativeHostConfig {
            NativeHostConfig {
                vault_dir: self.dir.path().to_path_buf(),
                allowed_extension_ids: vec![],
            }
        }

        fn config_with_allowed_extension(&self, extension_id: &str) -> NativeHostConfig {
            NativeHostConfig {
                vault_dir: self.dir.path().to_path_buf(),
                allowed_extension_ids: vec![extension_id.to_string()],
            }
        }

        fn unlock(&self) {
            let _: SessionStatus = self
                .client
                .request(&AgentRequest::SessionUnlock {
                    mode: SessionUnlockMode::Password {
                        password: self.password.as_str().into(),
                    },
                })
                .unwrap();
        }
    }

    impl Drop for RunningAgent {
        fn drop(&mut self) {
            let _ = self.client.shutdown();
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

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
        match parsed {
            NativeRequest::Ping {
                id: parsed_id,
                protocol_version,
                extension_id,
            } => {
                assert_eq!(parsed_id, id);
                assert_eq!(protocol_version, 1);
                assert_eq!(extension_id, None);
            }
            other => panic!("unexpected request: {other:?}"),
        }
    }

    #[test]
    fn response_includes_camel_case_protocol_version() {
        let response = handle_request_with_config(
            NativeRequest::Ping {
                id: Uuid::new_v4(),
                protocol_version: 99,
                extension_id: None,
            },
            &NativeHostConfig {
                vault_dir: PathBuf::from("/tmp/missing"),
                allowed_extension_ids: vec![],
            },
        );
        let value = serde_json::to_value(response).unwrap();
        assert_eq!(
            value["protocolVersion"],
            serde_json::json!(NATIVE_PROTOCOL_VERSION)
        );
        assert!(value.get("protocol_version").is_none());
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
                allowed_extension_ids: vec!["good-extension-id".to_string()],
            },
        );
        assert!(!response.ok);
        assert_eq!(response.error.as_deref(), Some("extension id missing"));
    }

    #[test]
    fn accepts_native_request_with_allowed_extension_id() {
        let agent = RunningAgent::start();
        let response = handle_request_with_config(
            NativeRequest::Ping {
                id: Uuid::new_v4(),
                protocol_version: 1,
                extension_id: Some("chrome-extension://good-extension-id/".to_string()),
            },
            &agent.config_with_allowed_extension("good-extension-id"),
        );
        assert!(response.ok);
        assert_eq!(response.data["locked"], true);
    }

    #[test]
    fn session_unlock_requires_native_window_flow() {
        let agent = RunningAgent::start();
        let config = agent.config();
        let response = handle_request_with_config(
            NativeRequest::SessionUnlock {
                id: Uuid::new_v4(),
                extension_id: None,
                interactive: None,
            },
            &config,
        );
        assert!(!response.ok);
        assert_eq!(
            response.error.as_deref(),
            Some("interactive unlock via desktop window is required")
        );
    }

    #[test]
    fn lookup_and_fill_uses_short_lived_grant() {
        let agent = RunningAgent::start();
        agent.unlock();
        let config = agent.config();
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
                api_key: "sk-ant-api03-browser-secret".into(),
                environment: Some("work".to_string()),
                tags: vec!["browser".to_string()],
                gateway: None,
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
        let matches =
            scan_for_plaintext(agent.dir.path(), &["sk-ant-api03-browser-secret"]).unwrap();
        assert!(
            matches.is_empty(),
            "native host leaked plaintext to {matches:?}"
        );
    }

    #[test]
    fn save_detected_allows_multiple_keys_for_same_platform() {
        let agent = RunningAgent::start();
        agent.unlock();
        let config = agent.config();
        for (title, api_key) in [
            ("OpenRouter Product A", "sk-or-v1-product-a-secret"),
            ("OpenRouter Product B", "sk-or-v1-product-b-secret"),
        ] {
            let save = handle_request_with_config(
                NativeRequest::SaveDetected {
                    id: Uuid::new_v4(),
                    extension_id: None,
                    origin: "https://openrouter.ai".to_string(),
                    url: "https://openrouter.ai/settings/keys".to_string(),
                    title: Some(title.to_string()),
                    endpoint: Some("https://openrouter.ai/api/v1".to_string()),
                    provider_id: Some("openrouter".to_string()),
                    interface_type: Some(aipass_provider_registry::InterfaceType::OpenAiCompatible),
                    auth_scheme: Some(aipass_provider_registry::AuthScheme::Bearer),
                    api_key: api_key.into(),
                    environment: Some("browser".to_string()),
                    tags: vec!["browser".to_string()],
                    gateway: None,
                },
                &config,
            );
            assert!(save.ok, "{save:?}");
        }

        let lookup = handle_request_with_config(
            NativeRequest::ContextLookup {
                id: Uuid::new_v4(),
                extension_id: None,
                origin: "https://openrouter.ai".to_string(),
                url: "https://openrouter.ai/settings/keys".to_string(),
            },
            &config,
        );
        assert!(lookup.ok, "{lookup:?}");
        let entries = lookup.data["entries"].as_array().unwrap();
        let grants = lookup.data["grants"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(grants.len(), 2);

        for entry in entries {
            let entry_id = Uuid::parse_str(entry["id"].as_str().unwrap()).unwrap();
            let entry_id_string = entry_id.to_string();
            let grant = grants
                .iter()
                .find(|grant| grant["entryId"].as_str() == Some(entry_id_string.as_str()))
                .expect("grant for saved entry");
            let grant_id = Uuid::parse_str(grant["id"].as_str().unwrap()).unwrap();
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
            let title = entry["title"].as_str().unwrap();
            let secret = fill.data["secret"].as_str().unwrap();
            if title.ends_with("A") {
                assert_eq!(secret, "sk-or-v1-product-a-secret");
            } else {
                assert_eq!(secret, "sk-or-v1-product-b-secret");
            }
        }
    }

    #[test]
    fn save_detected_infers_endpoint_interface_and_auth() {
        let agent = RunningAgent::start();
        agent.unlock();
        let config = agent.config();
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
                api_key: "sk-gateway-secret-value".into(),
                environment: Some("work".to_string()),
                tags: vec!["browser".to_string()],
                gateway: Some(aipass_provider_registry::GatewayMetadata {
                    group: Some("vip".to_string()),
                    rate: Some("0.8x".to_string()),
                }),
            },
            &config,
        );
        assert!(save.ok, "{save:?}");
        let vault = aipass_vault::Vault::open(
            agent.dir.path(),
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
        assert_eq!(
            entries[0]
                .gateway
                .as_ref()
                .and_then(|gateway| gateway.group.as_deref()),
            Some("vip")
        );
        assert_eq!(
            entries[0]
                .gateway
                .as_ref()
                .and_then(|gateway| gateway.rate.as_deref()),
            Some("0.8x")
        );

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
                api_key: "sk-gateway-secret-value".into(),
                environment: Some("work".to_string()),
                tags: vec!["browser".to_string()],
                gateway: None,
            },
            &config,
        );
        assert!(preview.ok, "{preview:?}");
        assert_eq!(preview.data["isSaved"], true);
        assert!(preview.data["existingEntryId"].as_str().is_some());
    }

    #[test]
    fn preview_detected_reports_preview_without_persisting() {
        let agent = RunningAgent::start();
        agent.unlock();
        let config = agent.config();
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
                api_key: "sk-gateway-secret-value".into(),
                environment: Some("work".to_string()),
                tags: vec!["browser".to_string()],
                gateway: None,
            },
            &config,
        );
        assert!(preview.ok, "{preview:?}");
        assert_eq!(preview.data["maskedSecret"], "•••• alue");
        assert!(!preview.data["fingerprint"].as_str().unwrap().is_empty());
        let vault = aipass_vault::Vault::open(
            agent.dir.path(),
            &SecretString::new("correct horse battery staple"),
        )
        .unwrap();
        assert!(vault.search("Gateway").unwrap().is_empty());
    }

    #[test]
    fn ignored_origins_are_persisted_in_native_host_storage() {
        let agent = RunningAgent::start();
        let config = agent.config();

        let ignored = handle_request_with_config(
            NativeRequest::IgnoreOrigin {
                id: Uuid::new_v4(),
                extension_id: None,
                origin: "https://console.anthropic.com".to_string(),
            },
            &config,
        );
        assert!(ignored.ok, "{ignored:?}");
        assert_eq!(ignored.data["ignoredOrigins"].as_array().unwrap().len(), 1);

        let check = handle_request_with_config(
            NativeRequest::IsOriginIgnored {
                id: Uuid::new_v4(),
                extension_id: None,
                origin: "https://console.anthropic.com".to_string(),
            },
            &config,
        );
        assert!(check.ok, "{check:?}");
        assert_eq!(check.data["ignored"], true);
    }
}
