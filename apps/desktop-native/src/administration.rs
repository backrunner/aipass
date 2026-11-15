//! Scoped settings and proxy requests. The Agent remains the durable authority.
use aipass_agent::{AgentClient, AgentCommandError};
use aipass_agent_protocol::*;
use aipass_desktop_platform::preferences::AppPreferences;
use aipass_proxy::{ProxyConfig, ProxyLogEntry, ProxyStatus, UsageTimeseriesPoint};
use std::path::PathBuf;
mod preferences;
#[cfg(test)]
mod tests;

trait Client {
    fn request<T: serde::de::DeserializeOwned>(
        &self,
        request: &AgentRequest,
    ) -> Result<T, AgentCommandError>;
}
impl Client for AgentClient {
    fn request<T: serde::de::DeserializeOwned>(
        &self,
        request: &AgentRequest,
    ) -> Result<T, AgentCommandError> {
        AgentClient::request(self, request)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    Desktop,
    Settings,
    Proxy,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scope {
    pub surface: Surface,
    pub generation: u64,
}
pub enum Operation {
    Preferences {
        path: Option<PathBuf>,
        previous: AppPreferences,
        preferences: AppPreferences,
    },
    LoadPreferences {
        path: Option<PathBuf>,
        preferences: AppPreferences,
    },
    DetectImport,
    BrowserExtension {
        install: bool,
    },
    Tools,
    ToolPreview(ToolConfigProxyRequest),
    ToolApply(ToolConfigProxyRequest),
    LoadSync,
    SaveSync {
        settings: SyncSettingsUpdate,
        run: bool,
    },
    LoadServer,
    LoadProxy {
        days: u32,
        hourly: bool,
        offset: i32,
    },
    Logs,
    Apply(Box<AgentRequest>),
    AccessCode {
        allow_remote_unlock: bool,
    },
}
pub struct Request {
    pub scope: Scope,
    pub operation: Operation,
}
pub enum Outcome {
    Preferences(AppPreferences),
    PreferencesSaved {
        preferences: AppPreferences,
        error: Option<AgentCommandError>,
    },
    SyncSaved {
        settings: SyncSettings,
        error: Option<AgentCommandError>,
    },
    Detection(CcSwitchDetection),
    BrowserExtension(Box<aipass_desktop_platform::browser::BrowserExtensionStatus>),
    Tools(Vec<aipass_config_writers::ToolDetection>),
    ToolPreview(ToolConfigPreviewResponse),
    ToolApplied(ToolConfigApplyResponse),
    Sync {
        settings: SyncSettings,
        conflicts: Vec<SyncConflictResponse>,
        devices: Vec<aipass_vault::DeviceRecord>,
    },
    Server {
        config: Box<ProxyConfig>,
        panel: ControlPanelStatus,
    },
    Proxy {
        config: Box<ProxyConfig>,
        status: ProxyStatus,
        usage: ServerUsageSummary,
        series: Vec<UsageTimeseriesPoint>,
    },
    Logs(Vec<ProxyLogEntry>),
    AccessCode(ControlPanelAccessCode),
    Panel(ControlPanelStatus),
    Applied,
}
pub struct Completion {
    pub scope: Scope,
    pub result: Result<Outcome, AgentCommandError>,
}
impl Completion {
    /// A later operation can fail after an earlier durable write succeeded.
    pub fn error(&self) -> Option<&AgentCommandError> {
        match &self.result {
            Err(error) => Some(error),
            Ok(Outcome::PreferencesSaved { error, .. } | Outcome::SyncSaved { error, .. }) => {
                error.as_ref()
            }
            _ => None,
        }
    }
}
impl Request {
    pub fn execute(self, agent: &AgentClient) -> Completion {
        Completion {
            scope: self.scope,
            result: self.operation.execute(agent),
        }
    }
}
impl Operation {
    fn execute(self, agent: &impl Client) -> Result<Outcome, AgentCommandError> {
        match self {
            Self::LoadPreferences {
                path,
                mut preferences,
            } => {
                if let Some(path) = path {
                    preferences =
                        aipass_desktop_platform::preferences::load(&path).map_err(local_error)?;
                }
                let policy: SessionPolicy = agent.request(&AgentRequest::SessionPolicyGet)?;
                let language: LanguageSettings =
                    agent.request(&AgentRequest::LanguageSettingsGet {
                        legacy_locale: Some(preferences.locale),
                    })?;
                preferences.auto_lock_minutes = policy.idle_lock_minutes;
                preferences.lock_on_sleep = policy.lock_on_sleep;
                preferences.lock_on_screen_lock = policy.lock_on_screen_lock;
                preferences.clipboard_clear_seconds =
                    aipass_desktop_platform::clipboard::normalize_seconds(
                        preferences.clipboard_clear_seconds,
                    );
                preferences.locale = language.locale;
                preferences.resolved_locale = Some(language.resolved_locale);
                Ok(Outcome::Preferences(preferences))
            }
            Self::Preferences {
                path,
                previous,
                preferences,
            } => Ok(preferences::save(
                agent,
                path.as_deref(),
                previous,
                preferences,
            )),
            Self::DetectImport => agent
                .request(&AgentRequest::CcSwitchDetect)
                .map(Outcome::Detection),
            Self::SaveSync { settings, run } => {
                let settings = agent.request(&AgentRequest::SyncSettingsSet { settings })?;
                let error = if run {
                    agent
                        .request::<serde_json::Value>(&AgentRequest::SyncConfigured)
                        .err()
                } else {
                    None
                };
                Ok(Outcome::SyncSaved { settings, error })
            }
            Self::LoadSync => {
                let settings: SyncSettings = agent.request(&AgentRequest::SyncSettingsGet)?;
                let (dir, provider) = sync_target(&settings);
                Ok(Outcome::Sync {
                    settings,
                    conflicts: agent.request(&AgentRequest::SyncConflicts { dir, provider })?,
                    devices: agent.request(&AgentRequest::DevicesList)?,
                })
            }
            Self::BrowserExtension { install } => {
                let path = extension_metadata()?;
                let status = if install {
                    aipass_desktop_platform::browser::install_browser_extension(&path)
                        .map(|v| v.status)
                } else {
                    aipass_desktop_platform::browser::browser_extension_status_snapshot(&path)
                };
                status
                    .map(Box::new)
                    .map(Outcome::BrowserExtension)
                    .map_err(local_error)
            }
            Self::Tools => Ok(Outcome::Tools(aipass_config_writers::detect_tools())),
            Self::ToolPreview(request) => agent
                .request(&AgentRequest::ToolConfigProxyPreview { request })
                .map(Outcome::ToolPreview),
            Self::ToolApply(request) => agent
                .request(&AgentRequest::ToolConfigProxyApply { request })
                .map(Outcome::ToolApplied),
            Self::LoadServer => Ok(Outcome::Server {
                config: Box::new(agent.request(&AgentRequest::ServerConfigGet)?),
                panel: agent.request(&AgentRequest::ControlPanelStatus)?,
            }),
            Self::LoadProxy {
                days,
                hourly,
                offset,
            } => {
                let granularity = if hourly {
                    aipass_proxy::UsageGranularity::Hour
                } else {
                    aipass_proxy::UsageGranularity::Day
                };
                Ok(Outcome::Proxy {
                    config: Box::new(agent.request(&AgentRequest::ServerConfigGet)?),
                    status: agent.request(&AgentRequest::ServerStatus)?,
                    usage: agent.request(&AgentRequest::ServerUsageSummary {
                        days: Some(days),
                        timezone_offset_minutes: offset,
                        granularity,
                    })?,
                    series: agent.request(&AgentRequest::ServerUsageTimeseries {
                        days,
                        timezone_offset_minutes: offset,
                        granularity,
                    })?,
                })
            }
            Self::Logs => agent.request(&AgentRequest::ServerLogs).map(Outcome::Logs),
            Self::AccessCode {
                allow_remote_unlock,
            } => agent
                .request(&AgentRequest::ControlPanelRotateAccessCode {
                    allow_remote_unlock,
                })
                .map(Outcome::AccessCode),
            Self::Apply(request) => {
                if matches!(
                    *request,
                    AgentRequest::ControlPanelConfigure { .. }
                        | AgentRequest::ControlPanelStop
                        | AgentRequest::ControlPanelDisableRemoteUnlock
                ) {
                    return agent.request(&request).map(Outcome::Panel);
                }
                agent.request::<serde_json::Value>(&request)?;
                Ok(Outcome::Applied)
            }
        }
    }
}
fn local_error(message: String) -> AgentCommandError {
    AgentCommandError {
        code: None,
        message,
    }
}

fn extension_metadata() -> Result<PathBuf, AgentCommandError> {
    let exe = std::env::current_exe().map_err(|e| local_error(e.to_string()))?;
    let mut paths = vec![];
    if let Some(parent) = exe.parent() {
        paths.push(parent.join("../Resources/browser-extension/aipass-extension.json"));
    }
    paths.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../extension/build/aipass-extension.json"),
    );
    paths
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| local_error("Bundled browser extension metadata is missing.".into()))
}
pub fn sync_target(settings: &SyncSettings) -> (Option<PathBuf>, Option<CloudSyncProvider>) {
    match settings.mode {
        SyncMode::Local => (settings.sync_folder.clone(), None),
        SyncMode::ICloud => (None, Some(CloudSyncProvider::ICloud)),
        SyncMode::OneDrive => (None, Some(CloudSyncProvider::OneDrive)),
        SyncMode::WebDav => (None, None),
    }
}
