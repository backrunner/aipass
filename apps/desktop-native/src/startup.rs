//! Live desktop lifecycle. All readiness and IPC stays off the GPUI thread.
use aipass_desktop_native::{
    runtime::{agent::AgentServices, instance::Instance, Gate, Runtime},
    session::{AgentTransport, Session},
    shell::{self, singleton},
    updates,
};
use gpui::{App, BorrowAppContext, Global, Window};
use std::{
    path::PathBuf,
    sync::{mpsc, Arc},
};

pub struct Lifetime {
    pub handle: shell::Handle,
    events: mpsc::Receiver<shell::Event>,
    jobs: Option<mpsc::Receiver<shell::tray::Command>>,
    tray: Option<shell::Tray>,
    singleton: Option<singleton::DesktopSingleton>,
    replacement: bool,
    _instance: Instance,
}
impl Global for Lifetime {}
impl Drop for Lifetime {
    fn drop(&mut self) {
        self.handle.stop();
    }
}

pub fn connect(
    vault: Option<PathBuf>,
    target: &str,
    links: Vec<aipass_agent_protocol::SensitiveString>,
) -> anyhow::Result<Option<(Session, Lifetime)>> {
    let client = match vault.or_else(|| std::env::var_os("AIPASS_VAULT_DIR").map(PathBuf::from)) {
        Some(path) => aipass_agent::AgentClient::for_vault(path)?,
        None => aipass_agent::AgentClient::default_vault()?,
    };
    let replacement = std::env::var_os("AIPASS_NATIVE_INSTANCE_FD").is_some();
    let singleton = if replacement {
        None
    } else {
        match singleton::acquire(env!("CARGO_PKG_VERSION"), target, links)? {
            singleton::SingletonDecision::Exit => return Ok(None),
            singleton::SingletonDecision::Run(value) => Some(value),
        }
    };
    let instance = Instance::acquire(&aipass_agent::agent_runtime_dir()?.join(format!(
        "native-desktop-{}-{}.lock",
        if cfg!(debug_assertions) {
            "dev"
        } else {
            "release"
        },
        client.config.namespace
    )))?;
    if !aipass_desktop_native::runtime_check::active()
        || aipass_desktop_native::runtime_check::isolated_linux_home()
    {
        aipass_desktop_platform::cli_install::ensure_cli_installed();
    }
    let binary = std::env::current_exe()?;
    let config = updates::production::config(&binary)?;
    let supervise = !aipass_desktop_native::runtime_check::active()
        && singleton::should_install_tray_autostart()
        && config.is_some();
    #[cfg(not(windows))]
    let socket = Some(singleton::socket_path()?);
    #[cfg(windows)]
    let socket = None;
    let services = Arc::new(AgentServices::new(
        client,
        aipass_agent::agent_binary_path()?,
        binary,
        socket,
        supervise,
    )?);
    let gate = Arc::new(Gate::default());
    let (mut handle, events, jobs) = shell::Handle::new(services.clone(), gate.clone(), supervise);
    if let Some(config) = config {
        handle.update = Some(updates::production::Context {
            config,
            runtime: Arc::new(Runtime::new(gate.clone(), services.clone())),
            instance: instance.clone(),
        });
    }
    let session = Session::new(AgentTransport::managed(services, gate))?;
    Ok(Some((
        session,
        Lifetime {
            handle,
            events,
            jobs: Some(jobs),
            tray: None,
            singleton,
            replacement,
            _instance: instance,
        },
    )))
}

pub fn start(cx: &mut App) {
    cx.update_global::<Lifetime, _>(|life, _| {
        match shell::Tray::new(&life.handle) {
            Ok(tray) => life.tray = Some(tray),
            Err(error) => eprintln!("Could not initialize native tray: {error}"),
        }
        if let Some(singleton) = life.singleton.take() {
            singleton::spawn_server(
                life.handle.clone(),
                singleton,
                env!("CARGO_PKG_VERSION").into(),
            );
        } else if life.replacement {
            singleton::spawn_replacement(life.handle.clone(), env!("CARGO_PKG_VERSION").into());
        }
        if let Some(jobs) = life.jobs.take() {
            life.handle.start_services(jobs);
        }
        aipass_desktop_native::runtime_check::start(life.handle.clone());
    });
}

pub fn events(cx: &mut App) -> Vec<shell::Event> {
    if !cx.has_global::<Lifetime>() {
        return Vec::new();
    }
    cx.update_global::<Lifetime, _>(|life, _| {
        if let Some(tray) = &mut life.tray {
            tray.poll();
        }
        let mut result = Vec::new();
        for event in life.events.try_iter().take(32) {
            match event {
                shell::Event::Tray(status) => {
                    if let Some(tray) = &mut life.tray {
                        tray.update(&status);
                    }
                }
                shell::Event::UnlockResult(error) => {
                    if let Some(tray) = &life.tray {
                        tray.unlock_result(error);
                    }
                }
                event => result.push(event),
            }
        }
        result
    })
}
pub fn hide(window: &Window, cx: &mut App) {
    let size = window.viewport_size();
    let size = shell::geometry::Size {
        width: f32::from(size.width),
        height: f32::from(size.height),
    };
    std::thread::spawn(move || {
        let _ = shell::geometry::save(size);
    });
    crate::window_visibility::set(window, false);
    cx.hide();
    cx.set_activation_policy(gpui::ActivationPolicy::Accessory);
}
pub fn activate(window: &Window, cx: &mut App) {
    crate::window_visibility::set(window, true);
    cx.set_activation_policy(gpui::ActivationPolicy::Regular);
    cx.activate(true);
    window.activate_window();
}

pub fn attach(
    view: &gpui::Entity<crate::desktop::Desktop>,
    target: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let shell = cx.global::<Lifetime>().handle.clone();
    if target == "server" {
        view.update(cx, |view, cx| view.activate_server(window, cx));
    }
    #[cfg(not(target_os = "macos"))]
    if target == "tray" {
        crate::window_visibility::set(window, false);
    }
    let owner = view.downgrade();
    let ready = std::env::var_os("AIPASS_NATIVE_UPDATE_READY");
    std::env::remove_var("AIPASS_NATIVE_UPDATE_READY");
    window.on_next_frame(move |_, cx| {
        let _ = aipass_desktop_native::shell::logging::log_event(
            "desktop.startup.stage",
            &[("stage", "complete")],
        );
        if let Some(ready) = ready {
            let services = shell.services.clone();
            std::thread::spawn(move || {
                if services.ensure_ready().is_ok() {
                    let _ = aipass_storage::atomic_write_bytes(
                        PathBuf::from(ready),
                        std::process::id().to_string().as_bytes(),
                    );
                }
            });
        }
        if let Some(context) = &shell.update {
            if !aipass_desktop_native::runtime_check::worker_enabled() {
                return;
            }
            let args = vec![
                "--window-target=main".into(),
                "--vault".into(),
                shell
                    .client()
                    .config
                    .vault_dir
                    .to_string_lossy()
                    .into_owned(),
            ];
            let worker = context
                .updater()
                .and_then(|updater| updates::Worker::with_host(updater, args, context.host()));
            match worker {
                Ok(worker) => {
                    let _ = owner.update(cx, |view, _| {
                        view.enable_updates(worker, Some(shell.gate.clone()))
                    });
                }
                Err(error) => eprintln!("Could not initialize native updater: {error}"),
            }
        }
    });
}

pub fn tray_ready(cx: &App) -> bool {
    cx.try_global::<Lifetime>()
        .is_some_and(|life| life.tray.is_some())
}
