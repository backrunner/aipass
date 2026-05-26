use crate::paths::agent_service_name;
use crate::ServerOptions;
use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct WindowsServiceStatus {
    pub service_name: String,
    pub registered: bool,
    pub running: bool,
    pub state: Option<String>,
}

#[cfg(target_os = "windows")]
mod imp {
    use super::*;
    use crate::client::AgentClient;
    use aipass_agent_protocol::{AgentRequest, LockReason};
    use anyhow::Context;
    use std::ffi::OsString;
    use std::sync::{mpsc, OnceLock};
    use std::thread;
    use std::time::Duration;
    use windows_service::define_windows_service;
    use windows_service::service::{
        PowerEventParam, ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl,
        ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
        SessionChangeReason,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    use windows_service::service_dispatcher;
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    static SERVICE_RUNTIME: OnceLock<ServiceRuntimeConfig> = OnceLock::new();

    #[derive(Clone, Debug)]
    struct ServiceRuntimeConfig {
        service_name: String,
        options: ServerOptions,
    }

    #[derive(Debug)]
    enum ControlMessage {
        Lock(LockReason),
        Shutdown,
    }

    define_windows_service!(ffi_service_main, service_main);

    pub(super) fn run_dispatcher(service_name: String, options: ServerOptions) -> Result<()> {
        let _ = SERVICE_RUNTIME.set(ServiceRuntimeConfig {
            service_name: service_name.clone(),
            options,
        });
        service_dispatcher::start(service_name, ffi_service_main)
            .context("failed to connect to the Windows Service Control Manager")?;
        Ok(())
    }

    fn service_main(_arguments: Vec<OsString>) {
        let Some(runtime) = SERVICE_RUNTIME.get().cloned() else {
            return;
        };
        let _ = run_service(runtime);
    }

    fn run_service(runtime: ServiceRuntimeConfig) -> Result<()> {
        let (control_tx, control_rx) = mpsc::channel::<ControlMessage>();
        let event_tx = control_tx.clone();
        let status_handle = service_control_handler::register(
            &runtime.service_name,
            move |control_event| -> ServiceControlHandlerResult {
                match control_event {
                    ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
                    ServiceControl::Stop
                    | ServiceControl::Shutdown
                    | ServiceControl::Preshutdown => {
                        let _ = event_tx.send(ControlMessage::Shutdown);
                        ServiceControlHandlerResult::NoError
                    }
                    ServiceControl::PowerEvent(PowerEventParam::Suspend) => {
                        let _ = event_tx.send(ControlMessage::Lock(LockReason::SystemSleep));
                        ServiceControlHandlerResult::NoError
                    }
                    ServiceControl::SessionChange(change) => {
                        match change.reason {
                            SessionChangeReason::SessionLock
                            | SessionChangeReason::SessionLogoff => {
                                let _ = event_tx.send(ControlMessage::Lock(LockReason::ScreenLock));
                            }
                            _ => {}
                        }
                        ServiceControlHandlerResult::NoError
                    }
                    _ => ServiceControlHandlerResult::NotImplemented,
                }
            },
        )?;

        status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::USER_OWN_PROCESS,
            current_state: ServiceState::Running,
            controls_accepted: ServiceControlAccept::STOP
                | ServiceControlAccept::PRESHUTDOWN
                | ServiceControlAccept::POWER_EVENT
                | ServiceControlAccept::SESSION_CHANGE,
            exit_code: windows_service::service::ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        })?;

        let control_vault_dir = runtime.options.vault_dir.clone();
        thread::spawn(move || {
            while let Ok(message) = control_rx.recv() {
                match message {
                    ControlMessage::Lock(reason) => {
                        let _ = send_lock_request(&control_vault_dir, reason);
                    }
                    ControlMessage::Shutdown => {
                        let _ = send_shutdown_request(&control_vault_dir);
                        break;
                    }
                }
            }
        });

        let server_result = crate::server::run_server(runtime.options);

        status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::USER_OWN_PROCESS,
            current_state: ServiceState::Stopped,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: windows_service::service::ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        })?;

        server_result
    }

    fn send_lock_request(vault_dir: &Path, reason: LockReason) -> Result<()> {
        let client = AgentClient::for_vault(vault_dir.to_path_buf())?;
        for _ in 0..20 {
            match client.request::<serde_json::Value>(&AgentRequest::SessionLock {
                reason: reason.clone(),
            }) {
                Ok(_) => return Ok(()),
                Err(err) if err.code.is_some() => thread::sleep(Duration::from_millis(100)),
                Err(err) => return Err(anyhow!(err)),
            }
        }
        Err(anyhow!("agent service did not accept lock request in time"))
    }

    fn send_shutdown_request(vault_dir: &Path) -> Result<()> {
        let client = AgentClient::for_vault(vault_dir.to_path_buf())?;
        for _ in 0..40 {
            match client.shutdown() {
                Ok(_) => return Ok(()),
                Err(_) => thread::sleep(Duration::from_millis(100)),
            }
        }
        Err(anyhow!(
            "agent service did not accept shutdown request in time"
        ))
    }

    pub(super) fn install(binary: &Path, vault_dir: &Path) -> Result<WindowsServiceStatus> {
        let service_name = agent_service_name(vault_dir)?;
        let service_manager = ServiceManager::local_computer(
            None::<&str>,
            ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
        )?;
        let launch_arguments = vec![
            OsString::from("--service"),
            OsString::from("--service-name"),
            OsString::from(service_name.clone()),
            OsString::from("--vault"),
            vault_dir.as_os_str().to_os_string(),
        ];
        let service_info = ServiceInfo {
            name: OsString::from(service_name.clone()),
            display_name: OsString::from(format!("AIPass Agent ({service_name})")),
            service_type: ServiceType::USER_OWN_PROCESS,
            start_type: ServiceStartType::AutoStart,
            error_control: ServiceErrorControl::Normal,
            executable_path: binary.to_path_buf(),
            launch_arguments,
            dependencies: vec![],
            account_name: None,
            account_password: None,
        };
        let access = ServiceAccess::QUERY_STATUS
            | ServiceAccess::START
            | ServiceAccess::STOP
            | ServiceAccess::CHANGE_CONFIG;
        let service = match service_manager.open_service(&service_name, access) {
            Ok(service) => {
                service.change_config(&service_info)?;
                service
            }
            Err(_) => service_manager.create_service(&service_info, access)?,
        };
        let _ = service.set_description("AIPass background agent");
        if !matches!(service.query_status()?.current_state, ServiceState::Running) {
            let _ = service.start(&[]);
        }
        query(vault_dir)
    }

    pub(super) fn start(vault_dir: &Path) -> Result<()> {
        let service_name = agent_service_name(vault_dir)?;
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
        let service = manager.open_service(
            &service_name,
            ServiceAccess::START | ServiceAccess::QUERY_STATUS,
        )?;
        if !matches!(service.query_status()?.current_state, ServiceState::Running) {
            service.start(&[])?;
        }
        Ok(())
    }

    pub(super) fn stop(vault_dir: &Path) -> Result<()> {
        let service_name = agent_service_name(vault_dir)?;
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
        let service = manager.open_service(
            &service_name,
            ServiceAccess::STOP | ServiceAccess::QUERY_STATUS,
        )?;
        if !matches!(service.query_status()?.current_state, ServiceState::Stopped) {
            let _ = service.stop()?;
        }
        Ok(())
    }

    pub(super) fn query(vault_dir: &Path) -> Result<WindowsServiceStatus> {
        let service_name = agent_service_name(vault_dir)?;
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
        match manager.open_service(&service_name, ServiceAccess::QUERY_STATUS) {
            Ok(service) => {
                let status = service.query_status()?;
                Ok(WindowsServiceStatus {
                    service_name,
                    registered: true,
                    running: matches!(status.current_state, ServiceState::Running),
                    state: Some(format!("{:?}", status.current_state).to_lowercase()),
                })
            }
            Err(_) => Ok(WindowsServiceStatus {
                service_name,
                registered: false,
                running: false,
                state: None,
            }),
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    use super::*;

    pub(super) fn run_dispatcher(_service_name: String, _options: ServerOptions) -> Result<()> {
        Err(anyhow!("Windows services are only available on Windows"))
    }

    pub(super) fn install(_binary: &Path, _vault_dir: &Path) -> Result<WindowsServiceStatus> {
        Err(anyhow!("Windows services are only available on Windows"))
    }

    pub(super) fn start(_vault_dir: &Path) -> Result<()> {
        Err(anyhow!("Windows services are only available on Windows"))
    }

    pub(super) fn stop(_vault_dir: &Path) -> Result<()> {
        Err(anyhow!("Windows services are only available on Windows"))
    }

    pub(super) fn query(vault_dir: &Path) -> Result<WindowsServiceStatus> {
        Ok(WindowsServiceStatus {
            service_name: agent_service_name(vault_dir)?,
            registered: false,
            running: false,
            state: None,
        })
    }
}

pub fn run_dispatcher(service_name: String, options: ServerOptions) -> Result<()> {
    imp::run_dispatcher(service_name, options)
}

pub fn install_service(binary: &Path, vault_dir: &Path) -> Result<WindowsServiceStatus> {
    imp::install(binary, vault_dir)
}

pub fn start_service(vault_dir: &Path) -> Result<()> {
    imp::start(vault_dir)
}

pub fn stop_service(vault_dir: &Path) -> Result<()> {
    imp::stop(vault_dir)
}

pub fn query_service(vault_dir: &Path) -> Result<WindowsServiceStatus> {
    imp::query(vault_dir)
}

pub fn binary_for_service(current_exe: &Path) -> PathBuf {
    let file_name = if cfg!(target_os = "windows") {
        "aipass-agent.exe"
    } else {
        "aipass-agent"
    };
    let sibling = current_exe.with_file_name(file_name);
    if sibling.exists() {
        sibling
    } else {
        PathBuf::from(file_name)
    }
}
