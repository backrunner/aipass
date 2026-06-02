use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use uuid::Uuid;

use aipass_agent_protocol::SensitiveString;
use aipass_provider_registry::{AuthScheme, GatewayMetadata, InterfaceType, QuotaInfo};
use zeroize::Zeroize;

pub const MAX_NATIVE_MESSAGE_BYTES: usize = 1024 * 1024;
pub const NATIVE_PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum NativeRequest {
    #[serde(rename = "ping")]
    Ping {
        id: Uuid,
        protocol_version: u32,
        extension_id: Option<String>,
    },
    #[serde(rename = "context.lookup")]
    ContextLookup {
        id: Uuid,
        extension_id: Option<String>,
        origin: String,
        url: String,
    },
    #[serde(rename = "entries.list")]
    EntriesList {
        id: Uuid,
        extension_id: Option<String>,
    },
    #[serde(rename = "entries.search")]
    EntriesSearch {
        id: Uuid,
        extension_id: Option<String>,
        origin: String,
        query: String,
    },
    #[serde(rename = "settings.isOriginIgnored")]
    IsOriginIgnored {
        id: Uuid,
        extension_id: Option<String>,
        origin: String,
    },
    #[serde(rename = "settings.ignoreOrigin")]
    IgnoreOrigin {
        id: Uuid,
        extension_id: Option<String>,
        origin: String,
    },
    #[serde(rename = "secret.fill")]
    SecretFill {
        id: Uuid,
        extension_id: Option<String>,
        entry_id: Uuid,
        field_id: String,
        grant_id: Uuid,
    },
    #[serde(rename = "secret.saveDetected")]
    SaveDetected {
        id: Uuid,
        extension_id: Option<String>,
        origin: String,
        url: String,
        title: Option<String>,
        favicon_url: Option<String>,
        #[serde(default)]
        secret_label: Option<String>,
        endpoint: Option<String>,
        provider_id: Option<String>,
        interface_type: Option<InterfaceType>,
        auth_scheme: Option<AuthScheme>,
        api_key: SensitiveString,
        environment: Option<String>,
        tags: Vec<String>,
        gateway: Option<GatewayMetadata>,
    },
    #[serde(rename = "secret.previewDetected")]
    PreviewDetected {
        id: Uuid,
        extension_id: Option<String>,
        origin: String,
        url: String,
        title: Option<String>,
        favicon_url: Option<String>,
        #[serde(default)]
        secret_label: Option<String>,
        endpoint: Option<String>,
        provider_id: Option<String>,
        interface_type: Option<InterfaceType>,
        auth_scheme: Option<AuthScheme>,
        api_key: SensitiveString,
        environment: Option<String>,
        tags: Vec<String>,
        gateway: Option<GatewayMetadata>,
    },
    #[serde(rename = "provider.add")]
    ProviderAdd {
        id: Uuid,
        extension_id: Option<String>,
        title: String,
        provider_id: Option<String>,
        #[serde(default)]
        domain: Vec<String>,
        favicon_url: Option<String>,
        endpoint: Option<String>,
        #[serde(default)]
        endpoints: Vec<String>,
        #[serde(default)]
        console_endpoints: Vec<String>,
        interface_type: InterfaceType,
        auth_scheme: AuthScheme,
        api_key: SensitiveString,
        default_model: Option<String>,
        #[serde(default)]
        model_aliases: Vec<(String, String)>,
        #[serde(default)]
        headers: Vec<(String, String)>,
        quota: Option<QuotaInfo>,
        gateway: Option<GatewayMetadata>,
        #[serde(default)]
        tags: Vec<String>,
        environment: String,
        notes: Option<String>,
    },
    #[serde(rename = "provider.update")]
    ProviderUpdate {
        id: Uuid,
        extension_id: Option<String>,
        entry_id: Uuid,
        title: String,
        provider_id: Option<String>,
        #[serde(default)]
        domain: Vec<String>,
        favicon_url: Option<String>,
        endpoint: Option<String>,
        #[serde(default)]
        endpoints: Vec<String>,
        #[serde(default)]
        console_endpoints: Vec<String>,
        interface_type: InterfaceType,
        auth_scheme: AuthScheme,
        api_key: Option<SensitiveString>,
        default_model: Option<String>,
        #[serde(default)]
        model_aliases: Vec<(String, String)>,
        #[serde(default)]
        headers: Option<Vec<(String, String)>>,
        quota: Option<QuotaInfo>,
        gateway: Option<GatewayMetadata>,
        #[serde(default)]
        tags: Vec<String>,
        environment: String,
        notes: Option<String>,
    },
    #[serde(rename = "provider.delete")]
    ProviderDelete {
        id: Uuid,
        extension_id: Option<String>,
        entry_id: Uuid,
    },
    #[serde(rename = "unlock.request")]
    UnlockRequest {
        id: Uuid,
        extension_id: Option<String>,
        reason: String,
    },
    #[serde(rename = "session.unlock")]
    SessionUnlock {
        id: Uuid,
        extension_id: Option<String>,
        interactive: Option<String>,
        #[serde(default)]
        password: Option<SensitiveString>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeResponse {
    pub id: Uuid,
    pub protocol_version: u32,
    pub ok: bool,
    pub error: Option<String>,
    pub data: serde_json::Value,
}

pub fn validate_extension_id(actual: &str, allowed: &[String]) -> Result<()> {
    let actual = normalize_extension_id(actual);
    if allowed.is_empty()
        || allowed
            .iter()
            .any(|id| normalize_extension_id(id) == actual)
    {
        Ok(())
    } else {
        bail!("extension id is not allowed")
    }
}

pub fn read_message(mut reader: impl Read) -> Result<NativeRequest> {
    let mut len = [0_u8; 4];
    reader.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_NATIVE_MESSAGE_BYTES {
        bail!("native message too large");
    }
    let mut body = vec![0_u8; len];
    reader.read_exact(&mut body)?;
    let parsed = serde_json::from_slice(&body);
    body.zeroize();
    Ok(parsed?)
}

pub fn write_message(mut writer: impl Write, response: &NativeResponse) -> Result<()> {
    let mut body = serde_json::to_vec(response)?;
    if body.len() > MAX_NATIVE_MESSAGE_BYTES {
        body.zeroize();
        bail!("native message too large");
    }
    let result = (|| {
        writer.write_all(&(body.len() as u32).to_le_bytes())?;
        writer.write_all(&body)?;
        Ok(())
    })();
    body.zeroize();
    result
}

fn normalize_extension_id(value: &str) -> String {
    value
        .trim()
        .trim_start_matches("chrome-extension://")
        .trim_start_matches("chrome://")
        .trim_end_matches('/')
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_message_rejects_oversized_lengths_before_allocating_body() {
        let bytes = ((MAX_NATIVE_MESSAGE_BYTES + 1) as u32).to_le_bytes();
        let err = read_message(bytes.as_slice()).unwrap_err();
        assert_eq!(err.to_string(), "native message too large");
    }

    #[test]
    fn write_message_rejects_oversized_payloads() {
        let response = NativeResponse {
            id: Uuid::new_v4(),
            protocol_version: NATIVE_PROTOCOL_VERSION,
            ok: true,
            error: None,
            data: serde_json::json!({ "value": "x".repeat(MAX_NATIVE_MESSAGE_BYTES) }),
        };
        let err = write_message(Vec::new(), &response).unwrap_err();
        assert_eq!(err.to_string(), "native message too large");
    }

    #[test]
    fn provider_add_deserializes_from_snake_case_payload() {
        let request: NativeRequest = serde_json::from_str(
            r#"{
                "type": "provider.add",
                "id": "00000000-0000-0000-0000-000000000000",
                "title": "My Gateway",
                "interface_type": "openai_compatible",
                "auth_scheme": "bearer",
                "api_key": "sk-test",
                "endpoints": ["https://gw.example.com/v1"],
                "environment": "work"
            }"#,
        )
        .unwrap();
        match request {
            NativeRequest::ProviderAdd {
                title,
                interface_type,
                endpoints,
                ..
            } => {
                assert_eq!(title, "My Gateway");
                assert_eq!(interface_type, InterfaceType::OpenAiCompatible);
                assert_eq!(endpoints, vec!["https://gw.example.com/v1".to_string()]);
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn entries_list_deserializes_without_payload() {
        let request: NativeRequest = serde_json::from_str(
            r#"{
                "type": "entries.list",
                "id": "00000000-0000-0000-0000-000000000000"
            }"#,
        )
        .unwrap();
        match request {
            NativeRequest::EntriesList { id, .. } => {
                assert_eq!(id.to_string(), "00000000-0000-0000-0000-000000000000");
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn provider_update_deserializes_entry_id_and_optional_secret() {
        let request: NativeRequest = serde_json::from_str(
            r#"{
                "type": "provider.update",
                "id": "00000000-0000-0000-0000-000000000000",
                "entry_id": "11111111-1111-1111-1111-111111111111",
                "title": "Edited Gateway",
                "interface_type": "openai_compatible",
                "auth_scheme": "bearer",
                "api_key": "sk-updated",
                "environment": "work"
            }"#,
        )
        .unwrap();
        match request {
            NativeRequest::ProviderUpdate {
                entry_id,
                title,
                api_key,
                ..
            } => {
                assert_eq!(entry_id.to_string(), "11111111-1111-1111-1111-111111111111");
                assert_eq!(title, "Edited Gateway");
                assert_eq!(api_key.unwrap().into_inner(), "sk-updated");
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }
}
