//! Desktop update control shares the private singleton transport; mutations
//! additionally require the selected vault's Agent authentication token.
use crate::SensitiveString;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const UPDATE_CONTROL_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    #[default]
    Official,
    Beta,
    Nightly,
}

impl UpdateChannel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Official => "official",
            Self::Beta => "beta",
            Self::Nightly => "nightly",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "camelCase")]
pub enum DesktopUpdateOperation {
    Check {
        channel: UpdateChannel,
    },
    Download {
        channel: UpdateChannel,
    },
    Install {
        channel: UpdateChannel,
        version: String,
    },
    Status,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticatedDesktopUpdate {
    pub protocol_version: u32,
    pub vault_dir: PathBuf,
    pub auth_token: SensitiveString,
    pub request: DesktopUpdateOperation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOutcome {
    pub target: String,
    #[serde(default)]
    pub process_id: Option<u32>,
    pub current_version: String,
    pub latest_version: Option<String>,
    pub available: bool,
    pub status: String,
    pub error: Option<String>,
}

impl UpdateOutcome {
    pub fn new(target: &str, version: &str) -> Self {
        Self {
            target: target.into(),
            process_id: None,
            current_version: version.into(),
            latest_version: None,
            available: false,
            status: "upToDate".into(),
            error: None,
        }
    }
}
