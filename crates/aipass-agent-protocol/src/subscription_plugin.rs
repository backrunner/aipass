//! Version 1. Each private stdio session serves one operation.
use crate::SensitiveString;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum PluginCall<'a> {
    Initialize {
        protocol: u8,
        nonce: &'a str,
        provider: &'a str,
        proxy_mode: &'a str,
    },
    Login {
        method: usize,
        inputs: BTreeMap<String, SensitiveString>,
        key: Option<SensitiveString>,
    },
    Code {
        code: SensitiveString,
    },
    Refresh {
        account_id: &'a str,
        state: &'a SensitiveString,
    },
    Generate {
        account_id: &'a str,
        state: &'a SensitiveString,
        model: &'a str,
        wire: &'a str,
        body: serde_json::Value,
    },
    HttpResult {
        status: u16,
        body: SensitiveString,
    },
    Ack,
    Shutdown,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginModel {
    pub id: String,
    pub name: String,
    pub wire: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginWindow {
    pub name: String,
    pub used: f64,
    pub span: Option<u64>,
    pub resets_at: Option<String>,
}
#[derive(Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PluginEvent {
    Ready {
        protocol: u8,
        nonce: String,
    },
    Challenge {
        url: String,
        instructions: String,
        manual: bool,
    },
    Http(PluginHttp),
    Connected {
        account_id: String,
        state: SensitiveString,
        models: Vec<PluginModel>,
    },
    Rotated {
        state: SensitiveString,
    },
    Usage {
        plan: Option<String>,
        balance: Option<String>,
        until: Option<String>,
        windows: Vec<PluginWindow>,
        #[serde(default)]
        sections: Vec<aipass_provider_registry::plugin_ui::Section>,
        models: Option<Vec<PluginModel>>,
    },
    Headers {
        status: u16,
        content_type: String,
    },
    Data {
        base64: String,
    },
    End,
    Error {
        code: PluginError,
    },
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginError {
    Reconnect,
    Unavailable,
    RateLimited,
    Unsupported,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginHttp {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, SensitiveString>,
    pub body: Option<SensitiveString>,
}
