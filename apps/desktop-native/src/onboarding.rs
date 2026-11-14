//! Existing Agent contracts used by the native setup forms.
use aipass_agent::{AgentClient, AgentCommandError};
use aipass_agent_protocol::{
    AgentRequest, CloudSyncProvider, SensitiveString, SyncMode, SyncSettingsUpdate,
};
#[derive(Clone)]
pub enum Import {
    Archive {
        path: std::path::PathBuf,
        password: SensitiveString,
    },
    Sync {
        settings: SyncSettingsUpdate,
        password: SensitiveString,
    },
}
impl Import {
    pub fn request(self) -> AgentRequest {
        match self {
            Self::Archive { path, password } => AgentRequest::VaultImport {
                input: path,
                export_password: password,
            },
            Self::Sync { settings, password } => {
                AgentRequest::VaultImportSync { settings, password }
            }
        }
    }
}
pub fn check_cloud(client: &AgentClient) -> Result<(), AgentCommandError> {
    client.request::<serde_json::Value>(&AgentRequest::SyncCloud {
        provider: CloudSyncProvider::ICloud,
    })?;
    Ok(())
}
pub fn sync_settings(
    source: &str,
    path: String,
    url: String,
    username: String,
    password: SensitiveString,
) -> SyncSettingsUpdate {
    SyncSettingsUpdate {
        mode: match source {
            "icloud" => SyncMode::ICloud,
            "webdav" => SyncMode::WebDav,
            _ => SyncMode::Local,
        },
        sync_folder: (source == "local").then(|| path.into()),
        webdav_url: (source == "webdav").then_some(url),
        webdav_username: (source == "webdav" && !username.is_empty()).then_some(username),
        webdav_password: (source == "webdav").then_some(password),
        clear_webdav_password: false,
    }
}
