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
                run_server(ServerOptions::without_desktop_tray(vault_dir)).unwrap();
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
        assert!(response.data["vaultNamespace"]
            .as_str()
            .is_some_and(|value| !value.is_empty()));
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
                wait: false,
                timeout_ms: None,
                password: None,
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
    fn session_unlock_accepts_password() {
        let agent = RunningAgent::start();
        let password = agent.password.clone();
        let config = agent.config();
        let response = handle_request_with_config(
            NativeRequest::SessionUnlock {
                id: Uuid::new_v4(),
                extension_id: None,
                interactive: None,
                wait: false,
                timeout_ms: None,
                password: Some(password.as_str().into()),
            },
            &config,
        );
        assert!(response.ok);
        assert_eq!(response.data["locked"], false);
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
                favicon_url: None,
                secret_label: None,
                endpoint: Some("https://api.anthropic.com".to_string()),
                provider_id: Some("anthropic".to_string()),
                interface_type: Some(aipass_provider_registry::InterfaceType::AnthropicMessages),
                auth_scheme: Some(aipass_provider_registry::AuthScheme::XApiKey),
                api_key: "sk-ant-api03-browser-secret".into(),
                tags: vec!["browser".to_string()],
                gateway: None,
                domains: Vec::new(),
                console_endpoint: None,
                default_model: None,
                model_aliases: Vec::new(),
                headers: Vec::new(),
                notes: None,
                group: None,
                billing: None,
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

    /// Two gateway groups on one relay are one entry with two keys — not two
    /// entries — and each key keeps its own group, wire format and billing.
    #[test]
    fn save_detected_groups_same_site_keys_into_one_entry() {
        let agent = RunningAgent::start();
        agent.unlock();
        let config = agent.config();
        for (group, api_key, interface_type) in [
            (
                "default",
                "sk-relay-default-secret",
                aipass_provider_registry::InterfaceType::OpenAiCompatible,
            ),
            (
                "claude",
                "sk-relay-claude-secret",
                aipass_provider_registry::InterfaceType::AnthropicMessages,
            ),
        ] {
            let save = handle_request_with_config(
                NativeRequest::SaveDetected {
                    id: Uuid::new_v4(),
                    extension_id: None,
                    origin: "https://relay.example.test".to_string(),
                    url: "https://relay.example.test/token".to_string(),
                    title: Some("Relay".to_string()),
                    favicon_url: None,
                    secret_label: None,
                    endpoint: Some("https://relay.example.test/v1".to_string()),
                    provider_id: Some("new_api".to_string()),
                    interface_type: Some(interface_type),
                    auth_scheme: Some(aipass_provider_registry::AuthScheme::Bearer),
                    api_key: api_key.into(),
                    tags: vec!["browser".to_string()],
                    gateway: None,
                    domains: Vec::new(),
                    console_endpoint: None,
                    default_model: None,
                    model_aliases: Vec::new(),
                    headers: Vec::new(),
                    notes: None,
                    group: Some(group.to_string()),
                    billing: Some(aipass_provider_registry::BillingRule {
                        rate: Some(format!("{}x", group.len())),
                        ..Default::default()
                    }),
                },
                &config,
            );
            assert!(save.ok, "{save:?}");
        }

        let lookup = handle_request_with_config(
            NativeRequest::ContextLookup {
                id: Uuid::new_v4(),
                extension_id: None,
                origin: "https://relay.example.test".to_string(),
                url: "https://relay.example.test/token".to_string(),
            },
            &config,
        );
        assert!(lookup.ok, "{lookup:?}");
        let entries = lookup.data["entries"].as_array().unwrap();
        let grants = lookup.data["grants"].as_array().unwrap();
        assert_eq!(entries.len(), 1, "both groups belong to one entry");

        let entry = &entries[0];
        let entry_id = Uuid::parse_str(entry["id"].as_str().unwrap()).unwrap();
        let secret_refs = entry["secretRefs"].as_array().unwrap();
        assert_eq!(secret_refs.len(), 2);
        assert_eq!(secret_refs[0]["group"], "default");
        assert_eq!(secret_refs[0]["interfaceType"], "openai_compatible");
        assert_eq!(secret_refs[0]["billing"]["rate"], "7x");
        assert_eq!(secret_refs[1]["group"], "claude");
        assert_eq!(secret_refs[1]["interfaceType"], "anthropic_messages");
        assert_eq!(secret_refs[1]["billing"]["rate"], "6x");

        // One grant per key, so either group's key can be filled.
        assert_eq!(grants.len(), 2);
        for (secret_ref, expected) in secret_refs
            .iter()
            .zip(["sk-relay-default-secret", "sk-relay-claude-secret"])
        {
            let secret_id = secret_ref["id"].as_str().unwrap();
            let grant = grants
                .iter()
                .find(|grant| grant["secretId"].as_str() == Some(secret_id))
                .expect("grant for stored key");
            let fill = handle_request_with_config(
                NativeRequest::SecretFill {
                    id: Uuid::new_v4(),
                    extension_id: None,
                    entry_id,
                    field_id: secret_id.to_string(),
                    grant_id: Uuid::parse_str(grant["id"].as_str().unwrap()).unwrap(),
                },
                &config,
            );
            assert!(fill.ok, "{fill:?}");
            assert_eq!(fill.data["secret"].as_str().unwrap(), expected);
        }
    }

    #[test]
    fn provider_update_and_delete_round_trip() {
        let agent = RunningAgent::start();
        agent.unlock();
        let config = agent.config();
        let add = handle_request_with_config(
            NativeRequest::ProviderAdd {
                id: Uuid::new_v4(),
                extension_id: None,
                title: "OpenRouter".to_string(),
                provider_id: Some("openrouter".to_string()),
                domain: vec!["openrouter.ai".to_string()],
                favicon_url: None,
                endpoint: Some("https://openrouter.ai/api/v1".to_string()),
                endpoints: vec![],
                console_endpoints: vec!["https://openrouter.ai/settings/keys".to_string()],
                interface_type: aipass_provider_registry::InterfaceType::OpenAiCompatible,
                auth_scheme: aipass_provider_registry::AuthScheme::Bearer,
                api_key: "sk-or-v1-native-host-secret".into(),
                secret_label: None,
                default_model: None,
                model_aliases: vec![],
                headers: vec![],
                quota: None,
                gateway: None,
                tags: vec!["browser".to_string()],
                notes: None,
                group: Some("vip".to_string()),
                billing: Some(aipass_provider_registry::BillingRule {
                    rate: Some("0.8x".to_string()),
                    ..Default::default()
                }),
            },
            &config,
        );
        assert!(add.ok, "{add:?}");
        let entry_id = Uuid::parse_str(add.data["entryId"].as_str().unwrap()).unwrap();

        let update = handle_request_with_config(
            NativeRequest::ProviderUpdate {
                id: Uuid::new_v4(),
                extension_id: None,
                entry_id,
                title: "OpenRouter Edited".to_string(),
                provider_id: Some("openrouter".to_string()),
                domain: vec!["openrouter.ai".to_string()],
                favicon_url: None,
                endpoint: Some("https://openrouter.ai/api/v1".to_string()),
                endpoints: vec![],
                console_endpoints: vec!["https://openrouter.ai/settings/keys".to_string()],
                interface_type: aipass_provider_registry::InterfaceType::OpenAiCompatible,
                auth_scheme: aipass_provider_registry::AuthScheme::Bearer,
                api_key: None,
                secret_label: None,
                default_model: Some("openai/gpt-4o-mini".to_string()),
                model_aliases: vec![],
                headers: None,
                quota: None,
                gateway: None,
                tags: vec!["browser".to_string(), "edited".to_string()],
                notes: Some("edited from extension".to_string()),
                // An update that says nothing about the group or billing must
                // leave the key's stored values alone.
                group: None,
                billing: None,
            },
            &config,
        );
        assert!(update.ok, "{update:?}");

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
        assert_eq!(entries[0]["title"], "OpenRouter Edited");
        assert_eq!(entries[0]["notes"], "edited from extension");
        // Group and billing were stored on the key at add time and survive an
        // entry update that does not mention them.
        let secret = &entries[0]["secretRefs"][0];
        assert_eq!(secret["group"], "vip");
        assert_eq!(secret["billing"]["rate"], "0.8x");

        // An update that *does* carry a group and billing applies them to the
        // primary key; these are per-key fields the entry itself cannot hold.
        let regrouped = handle_request_with_config(
            NativeRequest::ProviderUpdate {
                id: Uuid::new_v4(),
                extension_id: None,
                entry_id,
                title: "OpenRouter Edited".to_string(),
                provider_id: Some("openrouter".to_string()),
                domain: vec!["openrouter.ai".to_string()],
                favicon_url: None,
                endpoint: Some("https://openrouter.ai/api/v1".to_string()),
                endpoints: vec![],
                console_endpoints: vec!["https://openrouter.ai/settings/keys".to_string()],
                interface_type: aipass_provider_registry::InterfaceType::OpenAiCompatible,
                auth_scheme: aipass_provider_registry::AuthScheme::Bearer,
                api_key: None,
                secret_label: None,
                default_model: None,
                model_aliases: vec![],
                headers: None,
                quota: None,
                gateway: None,
                tags: vec![],
                notes: None,
                group: Some("enterprise".to_string()),
                billing: Some(aipass_provider_registry::BillingRule {
                    rate: Some("2x".to_string()),
                    ..Default::default()
                }),
            },
            &config,
        );
        assert!(regrouped.ok, "{regrouped:?}");
        let after_regroup = handle_request_with_config(
            NativeRequest::ContextLookup {
                id: Uuid::new_v4(),
                extension_id: None,
                origin: "https://openrouter.ai".to_string(),
                url: "https://openrouter.ai/settings/keys".to_string(),
            },
            &config,
        );
        assert!(after_regroup.ok, "{after_regroup:?}");
        let regrouped_secret = &after_regroup.data["entries"][0]["secretRefs"][0];
        assert_eq!(regrouped_secret["group"], "enterprise");
        assert_eq!(regrouped_secret["billing"]["rate"], "2x");

        let metadata = handle_request_with_config(
            NativeRequest::SecretMetadataSet {
                id: Uuid::new_v4(),
                extension_id: None,
                entry_id,
                secret_id: secret["id"].as_str().unwrap().to_string(),
                group: Some("premium".to_string()),
                interface_type: Some(aipass_provider_registry::InterfaceType::AnthropicMessages),
                billing: Some(aipass_provider_registry::BillingRule {
                    currency: Some("USD".to_string()),
                    ..Default::default()
                }),
            },
            &config,
        );
        assert!(metadata.ok, "{metadata:?}");

        let after_metadata = handle_request_with_config(
            NativeRequest::ContextLookup {
                id: Uuid::new_v4(),
                extension_id: None,
                origin: "https://openrouter.ai".to_string(),
                url: "https://openrouter.ai/settings/keys".to_string(),
            },
            &config,
        );
        assert!(after_metadata.ok, "{after_metadata:?}");
        let updated_secret = &after_metadata.data["entries"][0]["secretRefs"][0];
        assert_eq!(updated_secret["group"], "premium");
        assert_eq!(updated_secret["interfaceType"], "anthropic_messages");
        assert_eq!(updated_secret["billing"]["currency"], "USD");
        // Fields the caller left unset keep their stored value — here the rate
        // the preceding update wrote.
        assert_eq!(updated_secret["billing"]["rate"], "2x");

        let cleared = handle_request_with_config(
            NativeRequest::SecretMetadataSet {
                id: Uuid::new_v4(),
                extension_id: None,
                entry_id,
                secret_id: secret["id"].as_str().unwrap().to_string(),
                group: Some(String::new()),
                interface_type: None,
                billing: Some(aipass_provider_registry::BillingRule {
                    rate: Some(String::new()),
                    currency: Some(String::new()),
                    ..Default::default()
                }),
            },
            &config,
        );
        assert!(cleared.ok, "{cleared:?}");

        let after_clear = handle_request_with_config(
            NativeRequest::ContextLookup {
                id: Uuid::new_v4(),
                extension_id: None,
                origin: "https://openrouter.ai".to_string(),
                url: "https://openrouter.ai/settings/keys".to_string(),
            },
            &config,
        );
        assert!(after_clear.ok, "{after_clear:?}");
        let cleared_secret = &after_clear.data["entries"][0]["secretRefs"][0];
        assert!(cleared_secret["group"].is_null());
        assert!(cleared_secret["billing"].is_null());

        let delete = handle_request_with_config(
            NativeRequest::ProviderDelete {
                id: Uuid::new_v4(),
                extension_id: None,
                entry_id,
            },
            &config,
        );
        assert!(delete.ok, "{delete:?}");

        let after_delete = handle_request_with_config(
            NativeRequest::ContextLookup {
                id: Uuid::new_v4(),
                extension_id: None,
                origin: "https://openrouter.ai".to_string(),
                url: "https://openrouter.ai/settings/keys".to_string(),
            },
            &config,
        );
        assert!(after_delete.ok, "{after_delete:?}");
        assert_eq!(after_delete.data["entries"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn provider_favicon_backfill_forwards_to_agent() {
        let agent = RunningAgent::start();
        agent.unlock();
        let config = agent.config();
        let add = handle_request_with_config(
            NativeRequest::ProviderAdd {
                id: Uuid::new_v4(),
                extension_id: None,
                title: "OpenRouter".to_string(),
                provider_id: Some("openrouter".to_string()),
                domain: vec!["openrouter.ai".to_string()],
                favicon_url: Some("data:image/png;base64,iVBORw0KGgo=".to_string()),
                endpoint: Some("https://openrouter.ai/api/v1".to_string()),
                endpoints: vec![],
                console_endpoints: vec!["https://openrouter.ai".to_string()],
                interface_type: aipass_provider_registry::InterfaceType::OpenAiCompatible,
                auth_scheme: aipass_provider_registry::AuthScheme::Bearer,
                api_key: "sk-or-v1-native-host-secret".into(),
                secret_label: None,
                default_model: None,
                model_aliases: vec![],
                headers: vec![],
                quota: None,
                gateway: None,
                tags: vec![],
                notes: None,
                group: None,
                billing: None,
            },
            &config,
        );
        assert!(add.ok, "{add:?}");
        let entry_id = Uuid::parse_str(add.data["entryId"].as_str().unwrap()).unwrap();

        let backfill = handle_request_with_config(
            NativeRequest::ProviderFaviconBackfill {
                id: Uuid::new_v4(),
                extension_id: None,
                entry_ids: Some(vec![entry_id]),
                limit: Some(4),
            },
            &config,
        );

        assert!(backfill.ok, "{backfill:?}");
        assert_eq!(backfill.data["checked"], 0);
        assert_eq!(backfill.data["updated"], 0);
        assert_eq!(backfill.data["skipped"], 1);
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
                favicon_url: Some("https://gateway.example.test/favicon.svg".to_string()),
                secret_label: Some("Production".to_string()),
                endpoint: Some("https://gateway.example.test/v1".to_string()),
                provider_id: None,
                interface_type: None,
                auth_scheme: None,
                api_key: "sk-gateway-secret-value".into(),
                tags: vec!["browser".to_string()],
                gateway: Some(aipass_provider_registry::GatewayMetadata {
                    group: Some("vip".to_string()),
                    rate: Some("0.8x".to_string()),
                }),
                domains: Vec::new(),
                console_endpoint: None,
                default_model: None,
                model_aliases: Vec::new(),
                headers: Vec::new(),
                notes: None,
                group: None,
                billing: None,
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
        assert_eq!(entries[0].secret_refs[0].label, "Production");
        assert_eq!(
            entries[0].favicon_url.as_deref(),
            Some("https://gateway.example.test/favicon.svg")
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
        // The group and its billing rule also land on the key itself.
        assert_eq!(entries[0].secret_refs[0].group.as_deref(), Some("vip"));
        assert_eq!(
            entries[0].secret_refs[0]
                .billing
                .as_ref()
                .and_then(|billing| billing.rate.as_deref()),
            Some("0.8x")
        );

        let refreshed = handle_request_with_config(
            NativeRequest::SaveDetected {
                id: Uuid::new_v4(),
                extension_id: None,
                origin: "https://gateway.example.test".to_string(),
                url: "https://gateway.example.test/keys".to_string(),
                title: Some("Gateway".to_string()),
                favicon_url: None,
                secret_label: None,
                endpoint: Some("https://gateway.example.test/v1".to_string()),
                provider_id: None,
                interface_type: None,
                auth_scheme: None,
                api_key: "sk-gateway-secret-value".into(),
                tags: vec![],
                gateway: Some(aipass_provider_registry::GatewayMetadata {
                    group: Some("premium".to_string()),
                    rate: Some("1.2x".to_string()),
                }),
                domains: Vec::new(),
                console_endpoint: None,
                default_model: None,
                model_aliases: Vec::new(),
                headers: Vec::new(),
                notes: None,
                group: None,
                billing: None,
            },
            &config,
        );
        assert!(refreshed.ok, "{refreshed:?}");
        let refreshed_entries = aipass_vault::Vault::open(
            agent.dir.path(),
            &SecretString::new("correct horse battery staple"),
        )
        .unwrap()
        .search("gateway")
        .unwrap();
        // Re-detecting the same key updates that key in place; it never forks a
        // second entry, and the group now lives on the key.
        assert_eq!(refreshed_entries.len(), 1);
        let refreshed_entry = &refreshed_entries[0];
        assert_eq!(refreshed_entry.secret_refs.len(), 1);
        assert_eq!(
            refreshed_entry.secret_refs[0].group.as_deref(),
            Some("premium")
        );
        assert_eq!(
            refreshed_entry.secret_refs[0]
                .billing
                .as_ref()
                .and_then(|billing| billing.rate.as_deref()),
            Some("1.2x")
        );

        let preview = handle_request_with_config(
            NativeRequest::PreviewDetected {
                id: Uuid::new_v4(),
                extension_id: None,
                origin: "https://gateway.example.test".to_string(),
                url: "https://gateway.example.test/ui".to_string(),
                title: Some("Gateway".to_string()),
                favicon_url: Some("https://gateway.example.test/favicon.svg".to_string()),
                secret_label: Some("Production".to_string()),
                endpoint: Some("https://gateway.example.test/v1".to_string()),
                provider_id: None,
                interface_type: None,
                auth_scheme: None,
                api_key: "sk-gateway-secret-value".into(),
                tags: vec!["browser".to_string()],
                gateway: None,
                group: None,
                billing: None,
            },
            &config,
        );
        assert!(preview.ok, "{preview:?}");
        assert_eq!(preview.data["isSaved"], true);
        assert_eq!(preview.data["secretLabel"], "Production");
        assert_eq!(
            preview.data["faviconUrl"],
            "https://gateway.example.test/favicon.svg"
        );
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
                favicon_url: Some("https://gateway.example.test/favicon.svg".to_string()),
                secret_label: Some("Preview".to_string()),
                endpoint: Some("https://gateway.example.test/v1".to_string()),
                provider_id: None,
                interface_type: None,
                auth_scheme: None,
                api_key: "sk-gateway-secret-value".into(),
                tags: vec!["browser".to_string()],
                gateway: None,
                group: None,
                billing: None,
            },
            &config,
        );
        assert!(preview.ok, "{preview:?}");
        assert_eq!(preview.data["secretLabel"], "Preview");
        assert_eq!(preview.data["maskedSecret"], "sk-gat...alue");
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
