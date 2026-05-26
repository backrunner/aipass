use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use uuid::Uuid;

use aipass_agent_protocol::SensitiveString;
use aipass_provider_registry::{AuthScheme, InterfaceType};
use zeroize::Zeroize;

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
pub struct NativeResponse {
    pub id: Uuid,
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
    if len > 1024 * 1024 {
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
