use super::Services;
use aipass_agent::{ipc, AgentClient};
use aipass_agent_protocol::{AgentRequest, ProxyStatus, SessionStatus};
use anyhow::{ensure, Context, Result};
use std::{
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};

/// Uses only existing typed/authenticated Agent IPC. Supervisor mutation is
/// allowed only when its recorded executable belongs to this installation.
pub struct AgentServices {
    pub client: AgentClient,
    agent_binary: PathBuf,
    desktop_binary: PathBuf,
    singleton_socket: Option<PathBuf>,
    supervise: bool,
    suspended: Mutex<Suspended>,
}

#[derive(Default)]
struct Suspended {
    agent: bool,
    tray: bool,
    stopped: bool,
}

impl AgentServices {
    pub fn new(
        client: AgentClient,
        agent_binary: PathBuf,
        desktop_binary: PathBuf,
        singleton_socket: Option<PathBuf>,
        supervise: bool,
    ) -> Result<Self> {
        let agent_binary = agent_binary.canonicalize()?;
        let desktop_binary = desktop_binary.canonicalize()?;
        ensure!(
            aipass_agent::agent_binary_path()?.canonicalize()? == agent_binary,
            "Agent resolver points to another installation"
        );
        Ok(Self {
            client,
            agent_binary,
            desktop_binary,
            singleton_socket,
            supervise,
            suspended: Mutex::new(Suspended::default()),
        })
    }

    pub fn ensure_ready(&self) -> Result<()> {
        ensure!(
            aipass_agent::agent_binary_path()?.canonicalize()? == self.agent_binary,
            "Agent resolver changed installation"
        );
        self.client.ensure_running_for_desktop_companion()
    }

    fn listener_closed(&self) -> Result<bool> {
        match ipc::connect(&self.client.config.vault_dir) {
            Ok(_) => Ok(false),
            Err(error)
                if error.downcast_ref::<std::io::Error>().is_some_and(|io| {
                    matches!(
                        io.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                    )
                }) =>
            {
                Ok(true)
            }
            Err(error) => Err(error).context("could not establish Agent listener shutdown"),
        }
    }

    fn shutdown(&self) -> Result<()> {
        self.client.shutdown()?;
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.listener_closed()? {
            ensure!(
                Instant::now() < deadline,
                "Agent listener remained open during update"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
        Ok(())
    }
}

impl Services for AgentServices {
    fn proxy_running(&self) -> Result<bool> {
        // Cached installation can win the race with the first status refresh.
        // Establish readiness inside the same exclusion boundary before probing.
        self.ensure_ready()?;
        // A connection/authorization error is not evidence that disconnect is safe.
        Ok(self
            .client
            .request::<ProxyStatus>(&AgentRequest::ServerStatus)?
            .running)
    }

    fn suspend(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        if self.supervise {
            let vault = &self.client.config.vault_dir;
            let agent = aipass_agent::query_agent_autostart(vault)?;
            let tray = aipass_agent::query_tray_autostart(vault)?;
            if agent.registered {
                ensure!(
                    agent
                        .agent_binary
                        .as_ref()
                        .and_then(|p| p.canonicalize().ok())
                        .as_ref()
                        == Some(&self.agent_binary),
                    "refusing to suspend another installation's Agent"
                );
            }
            if tray.registered {
                ensure!(
                    tray.desktop_binary
                        .as_ref()
                        .and_then(|p| p.canonicalize().ok())
                        .as_ref()
                        == Some(&self.desktop_binary),
                    "refusing to suspend another installation's tray"
                );
                ensure!(
                    self.singleton_socket.is_some(),
                    "tray singleton is not configured"
                );
            }
            let mut state = self.suspended.lock().unwrap();
            if tray.registered {
                state.tray = true;
                aipass_agent::suspend_tray_autostart(vault)?;
            }
            if agent.registered {
                state.agent = true;
                aipass_agent::suspend_agent_autostart(vault)?;
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (&self.desktop_binary, &self.singleton_socket);
            ensure!(
                !self.supervise,
                "native supervisor update support is macOS-only"
            );
        }
        Ok(())
    }

    fn stop(&self) -> Result<()> {
        // Failure of either authenticated command blocks bundle replacement.
        self.client
            .request::<SessionStatus>(&AgentRequest::SessionStatus)?;
        self.client
            .request::<ProxyStatus>(&AgentRequest::ServerStop)?;
        self.suspended.lock().unwrap().stopped = true;
        self.shutdown()
    }

    fn restore(&self) -> Result<()> {
        let mut state = self.suspended.lock().unwrap();
        // A failed replacement can have launched its own Agent before failing
        // the readiness check. Retire it before starting the restored sidecar.
        if state.stopped && !self.listener_closed()? {
            self.client
                .request::<ProxyStatus>(&AgentRequest::ServerStop)?;
            self.shutdown()?;
        }
        #[cfg(target_os = "macos")]
        if state.agent {
            aipass_agent::ensure_agent_autostart(
                &self.agent_binary,
                &self.client.config.vault_dir,
            )?;
            state.agent = false;
        }
        if state.stopped {
            self.ensure_ready()?;
            state.stopped = false;
        }
        #[cfg(target_os = "macos")]
        if state.tray {
            aipass_agent::ensure_tray_autostart_with_socket(
                &self.desktop_binary,
                &self.client.config.vault_dir,
                self.singleton_socket
                    .as_ref()
                    .context("missing tray socket")?,
            )?;
            state.tray = false;
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (state.agent, state.tray);
        Ok(())
    }
}

impl Services for std::sync::Arc<AgentServices> {
    fn proxy_running(&self) -> Result<bool> {
        Services::proxy_running(self.as_ref())
    }
    fn suspend(&self) -> Result<()> {
        Services::suspend(self.as_ref())
    }
    fn stop(&self) -> Result<()> {
        Services::stop(self.as_ref())
    }
    fn restore(&self) -> Result<()> {
        Services::restore(self.as_ref())
    }
}
