//! Desktop lifecycle adapters. Agent remains the authority; GPUI owns presentation.
#[cfg(not(target_os = "macos"))]
mod classic;
#[cfg(target_os = "macos")]
mod cloudkit;
pub mod geometry;
mod i18n;
pub mod logging;
#[cfg(target_os = "macos")]
pub mod self_install;
pub mod singleton;
pub mod status;
#[cfg(target_os = "macos")]
mod swift;
pub mod tray;

use crate::runtime::{agent::AgentServices, Gate};
use aipass_agent::{AgentClient, AgentCommandError};
use aipass_agent_protocol::AgentRequest;
use anyhow::Result;
use serde::de::DeserializeOwned;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};

pub enum Event {
    Probe(mpsc::SyncSender<Result<(), String>>),
    Activate(String),
    Links(Vec<aipass_desktop_platform::deeplink::PendingDeepLink>),
    Refresh,
    Locked,
    Quit,
    OpenUrl(String),
    Copy(String),
    Tray(Box<status::TrayStatusDto>),
    UnlockResult(Option<String>),
    Error(String),
}

#[derive(Clone)]
pub struct Handle {
    pub services: Arc<AgentServices>,
    pub gate: Arc<Gate>,
    events: mpsc::Sender<Event>,
    stopped: Arc<AtomicBool>,
    commands: mpsc::SyncSender<tray::Command>,
    pub supervise: bool,
    pub update: Option<crate::updates::production::Context>,
}

impl Handle {
    pub fn new(
        services: Arc<AgentServices>,
        gate: Arc<Gate>,
        supervise: bool,
    ) -> (Self, mpsc::Receiver<Event>, mpsc::Receiver<tray::Command>) {
        // UI callbacks must never block waiting for the UI event pump. Producers
        // are bounded workers; tray status is emitted at most once per command/poll.
        let (events, receiver) = mpsc::channel();
        let (commands, jobs) = mpsc::sync_channel(16);
        (
            Self {
                services,
                gate,
                events,
                commands,
                supervise,
                update: None,
                stopped: Arc::new(AtomicBool::new(false)),
            },
            receiver,
            jobs,
        )
    }
    pub fn event(&self, event: Event) {
        let _ = self.events.send(event);
    }
    pub fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
    }
    pub fn client(&self) -> &AgentClient {
        &self.services.client
    }
    pub fn request<T: DeserializeOwned>(
        &self,
        request: AgentRequest,
    ) -> Result<T, AgentCommandError> {
        let local = |error: anyhow::Error| AgentCommandError {
            code: None,
            message: error.to_string(),
        };
        let _guard = self.gate.enter().map_err(local)?;
        self.services.ensure_ready().map_err(local)?;
        self.client().request(&request)
    }
    pub fn command(&self, command: tray::Command) -> bool {
        !self.stopped() && self.commands.try_send(command).is_ok()
    }
    pub fn refresh(&self) {
        self.command(tray::Command::Refresh);
    }
    pub fn quit(&self) -> Result<()> {
        self.gate.retire_with(|| {
            #[cfg(target_os = "macos")]
            if self.supervise {
                aipass_agent::suspend_tray_autostart(&self.client().config.vault_dir)?;
            }
            aipass_desktop_platform::clipboard::clear_now().map_err(anyhow::Error::msg)?;
            Ok(())
        })?;
        self.stop();
        self.event(Event::Quit);
        Ok(())
    }
    pub fn repair(&self) -> Result<()> {
        let _guard = self.gate.enter()?;
        anyhow::ensure!(
            self.supervise,
            "Autostart requires an installed desktop build"
        );
        let vault = &self.client().config.vault_dir;
        #[cfg(not(windows))]
        aipass_agent::install_agent_autostart(&aipass_agent::agent_binary_path()?, vault)?;
        #[cfg(target_os = "macos")]
        aipass_agent::install_tray_autostart_with_socket(
            &std::env::current_exe()?,
            vault,
            &singleton::socket_path()?,
        )?;
        #[cfg(not(target_os = "macos"))]
        aipass_agent::install_tray_autostart(&std::env::current_exe()?, vault)?;
        self.services.ensure_ready()
    }
    pub fn start_services(&self, jobs: mpsc::Receiver<tray::Command>) {
        tray::start(self.clone(), jobs);
        #[cfg(target_os = "macos")]
        cloudkit::start(self.clone());
        let handle = self.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<()> {
                let _guard = handle.gate.enter()?;
                if handle.supervise {
                    let vault = &handle.client().config.vault_dir;
                    #[cfg(target_os = "macos")]
                    {
                        aipass_agent::ensure_agent_autostart(
                            &aipass_agent::agent_binary_path()?,
                            vault,
                        )?;
                        aipass_agent::ensure_tray_autostart_with_socket(
                            &std::env::current_exe()?,
                            vault,
                            &singleton::socket_path()?,
                        )?;
                    }
                    #[cfg(not(target_os = "macos"))]
                    aipass_agent::install_tray_autostart(&std::env::current_exe()?, vault)?;
                }
                handle.services.ensure_ready()?;
                if handle.supervise {
                    let executable = std::env::current_exe()?;
                    if let Some(path) = aipass_agent::bundled::helper_candidates(
                        &executable,
                        "browser-extension/aipass-extension.json",
                    )
                    .into_iter()
                    .find(|p| p.is_file())
                    {
                        let status =
                            aipass_desktop_platform::browser::browser_extension_status_snapshot(
                                &path,
                            )
                            .map_err(anyhow::Error::msg)?;
                        aipass_desktop_platform::browser::repair_native_host_manifest(vec![
                            status.extension_id,
                        ])
                        .map_err(anyhow::Error::msg)?;
                        aipass_desktop_platform::browser::sync_installed_browser_extensions(&path);
                    }
                }
                Ok(())
            })();
            if result.is_err() {
                handle.event(Event::Error("Desktop startup repair failed".into()));
            }
        });
    }
}

/// Created and updated only on the native main thread.
pub struct Tray {
    #[cfg(not(target_os = "macos"))]
    inner: classic::Tray,
}
impl Tray {
    pub fn new(handle: &Handle) -> Result<Self> {
        #[cfg(target_os = "macos")]
        swift::setup(handle);
        Ok(Self {
            #[cfg(not(target_os = "macos"))]
            inner: classic::Tray::new(handle.clone())?,
        })
    }
    pub fn update(&mut self, status: &status::TrayStatusDto) {
        #[cfg(target_os = "macos")]
        swift::push_status(status);
        #[cfg(not(target_os = "macos"))]
        self.inner.update(status);
    }
    pub fn unlock_result(&self, error: Option<String>) {
        #[cfg(target_os = "macos")]
        swift::report_unlock_result(error);
        #[cfg(not(target_os = "macos"))]
        let _ = error;
    }
    pub fn poll(&mut self) {
        #[cfg(not(target_os = "macos"))]
        self.inner.poll();
    }
}
impl Drop for Tray {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        swift::shutdown();
    }
}
