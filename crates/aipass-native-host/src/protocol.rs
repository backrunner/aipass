use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use uuid::Uuid;

use aipass_agent_protocol::SensitiveString;
use aipass_provider_registry::{AuthScheme, GatewayMetadata, InterfaceType};
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
        endpoint: Option<String>,
        provider_id: Option<String>,
        interface_type: Option<InterfaceType>,
        auth_scheme: Option<AuthScheme>,
        api_key: SensitiveString,
        environment: Option<String>,
        tags: Vec<String>,
        gateway: Option<GatewayMetadata>,
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
}
