use crate::{agent_client, agent_error_to_string, agent_request_no_unlock};
use aipass_agent_protocol::{AgentRequest, LockReason, SessionStatus};
use std::thread;
use std::time::Duration;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, WindowEvent};

#[cfg(target_os = "macos")]
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::process::Command;

const TRAY_ID: &str = "aipass-agent";
const MENU_STATUS: &str = "tray-status";
const MENU_OPEN: &str = "tray-open";
const MENU_HIDE: &str = "tray-hide";
const MENU_REFRESH: &str = "tray-refresh";
const MENU_START_AGENT: &str = "tray-start-agent";
const MENU_LOCK: &str = "tray-lock";
const MENU_INSTALL_LOGIN_AGENT: &str = "tray-install-login-agent";
const MENU_QUIT: &str = "tray-quit";

#[derive(Clone)]
struct TrayMenuItems {
    status: MenuItem<tauri::Wry>,
    start_agent: MenuItem<tauri::Wry>,
    lock: MenuItem<tauri::Wry>,
    install_login_agent: MenuItem<tauri::Wry>,
}

pub(crate) fn setup(app: &App) -> tauri::Result<()> {
    let status = MenuItem::with_id(app, MENU_STATUS, "Agent: checking...", false, None::<&str>)?;
    let open = MenuItem::with_id(app, MENU_OPEN, "Open AIPass", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, MENU_HIDE, "Hide Window", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, MENU_REFRESH, "Refresh Status", true, None::<&str>)?;
    let start_agent = MenuItem::with_id(app, MENU_START_AGENT, "Start Agent", false, None::<&str>)?;
    let lock = MenuItem::with_id(app, MENU_LOCK, "Lock Vault", false, None::<&str>)?;

    #[cfg(target_os = "macos")]
    let install_login_agent = MenuItem::with_id(
        app,
        MENU_INSTALL_LOGIN_AGENT,
        "Install Login Agent",
        true,
        None::<&str>,
    )?;

    #[cfg(not(target_os = "macos"))]
    let install_login_agent = MenuItem::with_id(
        app,
        MENU_INSTALL_LOGIN_AGENT,
        "Login Agent: macOS only",
        false,
        None::<&str>,
    )?;

    let quit = MenuItem::with_id(app, MENU_QUIT, "Quit AIPass", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &status,
            &PredefinedMenuItem::separator(app)?,
            &open,
            &hide,
            &PredefinedMenuItem::separator(app)?,
            &refresh,
            &start_agent,
            &lock,
            &install_login_agent,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    let items = TrayMenuItems {
        status,
        start_agent,
        lock,
        install_login_agent,
    };

    let menu_items = items.clone();
    let tray_items = items.clone();
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("AIPass Agent")
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, event| {
            handle_menu_event(app, event.id().as_ref(), &menu_items);
        })
        .on_tray_icon_event(move |tray, event| {
            if let TrayIconEvent::Click {
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                refresh_status_async(tray.app_handle().clone(), tray_items.clone());
            }
        });

    if let Some(icon) = tray_icon(app) {
        builder = builder.icon(icon.image);
        if icon.is_template {
            builder = builder.icon_as_template(true);
        }
    } else {
        builder = builder.title("AIPass");
    }
    builder.build(app)?;

    install_close_to_tray(app);
    refresh_status_async(app.handle().clone(), items.clone());
    spawn_status_refresher(app.handle().clone(), items);

    Ok(())
}

fn install_close_to_tray(app: &App) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let app_handle = app.handle().clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            hide_main_window(&app_handle);
        }
    });
}

fn handle_menu_event(app: &AppHandle, id: &str, items: &TrayMenuItems) {
    match id {
        MENU_OPEN => {
            show_main_window(app);
            refresh_status_async(app.clone(), items.clone());
        }
        MENU_HIDE => hide_main_window(app),
        MENU_REFRESH => refresh_status_async(app.clone(), items.clone()),
        MENU_START_AGENT => start_agent_async(app.clone(), items.clone()),
        MENU_LOCK => lock_vault_async(app.clone(), items.clone()),
        MENU_INSTALL_LOGIN_AGENT => install_login_agent_async(app.clone(), items.clone()),
        MENU_QUIT => app.exit(0),
        _ => {}
    }
}

fn show_main_window(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.show();

    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn hide_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

fn refresh_status_async(app: AppHandle, items: TrayMenuItems) {
    thread::spawn(move || refresh_status(&app, &items));
}

fn spawn_status_refresher(app: AppHandle, items: TrayMenuItems) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(30));
        refresh_status(&app, &items);
    });
}

fn refresh_status(app: &AppHandle, items: &TrayMenuItems) {
    let status = agent_client(app)
        .and_then(|client| {
            client
                .request::<SessionStatus>(&AgentRequest::SessionStatus)
                .map_err(agent_error_to_string)
        })
        .map(TrayStatus::Running)
        .unwrap_or_else(TrayStatus::Unavailable);

    let _ = items.status.set_text(status.menu_text());
    let _ = items.start_agent.set_enabled(status.can_start());
    let _ = items.lock.set_enabled(status.can_lock());

    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(status.tooltip()));
    }
}

fn start_agent_async(app: AppHandle, items: TrayMenuItems) {
    thread::spawn(move || {
        let _ = items.status.set_text("Agent: starting...");
        let _ = items.start_agent.set_enabled(false);
        if let Err(err) = agent_client(&app).and_then(|client| {
            client.ensure_running().map_err(|err| err.to_string())?;
            Ok(())
        }) {
            eprintln!("failed to start AIPass agent from tray: {err}");
            let _ = items.status.set_text("Agent: start failed");
        }
        refresh_status(&app, &items);
    });
}

fn lock_vault_async(app: AppHandle, items: TrayMenuItems) {
    thread::spawn(move || {
        let result = agent_request_no_unlock::<SessionStatus>(
            &app,
            AgentRequest::SessionLock {
                reason: LockReason::Manual,
            },
        );
        if let Err(err) = result {
            eprintln!("failed to lock AIPass vault from tray: {err}");
            let _ = items.status.set_text("Agent: lock failed");
        }
        refresh_status(&app, &items);
    });
}

fn install_login_agent_async(app: AppHandle, items: TrayMenuItems) {
    thread::spawn(move || {
        #[cfg(target_os = "macos")]
        match install_macos_login_agent() {
            Ok(_) => {
                let _ = items.install_login_agent.set_text("Repair Login Agent");
                refresh_status(&app, &items);
            }
            Err(err) => {
                eprintln!("failed to install AIPass login agent: {err}");
                let _ = items.status.set_text("Agent: login install failed");
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = app;
            let _ = items;
        }
    });
}

enum TrayStatus {
    Running(SessionStatus),
    Unavailable(String),
}

struct TrayIconAsset {
    image: tauri::image::Image<'static>,
    is_template: bool,
}

impl TrayStatus {
    fn menu_text(&self) -> String {
        match self {
            Self::Running(status) if !status.exists => "Agent: running (no vault)".to_string(),
            Self::Running(status) if status.locked => "Agent: running (locked)".to_string(),
            Self::Running(_) => "Agent: running (unlocked)".to_string(),
            Self::Unavailable(_) => "Agent: not reachable".to_string(),
        }
    }

    fn tooltip(&self) -> String {
        match self {
            Self::Running(status) if !status.exists => {
                "AIPass Agent is running; no vault exists".to_string()
            }
            Self::Running(status) if status.locked => {
                "AIPass Agent is running; vault is locked".to_string()
            }
            Self::Running(_) => "AIPass Agent is running; vault is unlocked".to_string(),
            Self::Unavailable(err) => {
                format!("AIPass Agent is not reachable: {}", short_error(err))
            }
        }
    }

    fn can_lock(&self) -> bool {
        matches!(self, Self::Running(status) if status.exists && !status.locked)
    }

    fn can_start(&self) -> bool {
        matches!(self, Self::Unavailable(_))
    }
}

#[cfg(target_os = "macos")]
fn tray_icon(app: &App) -> Option<TrayIconAsset> {
    if let Ok(icon) = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-template.png"))
    {
        Some(TrayIconAsset {
            image: icon.to_owned(),
            is_template: true,
        })
    } else {
        app.default_window_icon()
            .cloned()
            .map(|icon| TrayIconAsset {
                image: icon.to_owned(),
                is_template: false,
            })
    }
}

#[cfg(not(target_os = "macos"))]
fn tray_icon(app: &App) -> Option<TrayIconAsset> {
    app.default_window_icon()
        .cloned()
        .map(|icon| TrayIconAsset {
            image: icon.to_owned(),
            is_template: false,
        })
}

fn short_error(value: &str) -> String {
    let line = value.lines().next().unwrap_or(value).trim();
    const MAX_LEN: usize = 160;
    let mut chars = line.chars();
    let shortened = chars.by_ref().take(MAX_LEN).collect::<String>();
    if chars.next().is_some() {
        format!("{shortened}...")
    } else {
        shortened
    }
}

#[cfg(target_os = "macos")]
fn install_macos_login_agent() -> Result<PathBuf, String> {
    let vault_dir = configured_vault_dir()?;
    let agent_binary = aipass_agent::agent_binary_path().map_err(|err| err.to_string())?;
    let namespace =
        aipass_agent::namespace_for_vault_dir(&vault_dir).map_err(|err| err.to_string())?;
    let label = format!("dev.aipass.agent.{namespace}");
    let plist_path = launch_agent_path(&label)?;
    let home = home_dir()?;
    let log_dir = home.join("Library").join("Logs").join("AIPass");
    std::fs::create_dir_all(&log_dir).map_err(|err| err.to_string())?;

    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
    <string>--vault</string>
    <string>{}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{}</string>
  <key>StandardErrorPath</key>
  <string>{}</string>
</dict>
</plist>
"#,
        xml_escape(&label),
        xml_escape(&agent_binary.display().to_string()),
        xml_escape(&vault_dir.display().to_string()),
        xml_escape(
            &log_dir
                .join(format!("agent-{namespace}.out.log"))
                .display()
                .to_string()
        ),
        xml_escape(
            &log_dir
                .join(format!("agent-{namespace}.err.log"))
                .display()
                .to_string()
        ),
    );

    if let Some(parent) = plist_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    aipass_storage::atomic_write_bytes(&plist_path, plist.as_bytes())
        .map_err(|err| err.to_string())?;

    let path_text = plist_path.to_string_lossy().into_owned();
    let _ = Command::new("launchctl")
        .args(["unload", path_text.as_str()])
        .status();
    let status = Command::new("launchctl")
        .args(["load", "-w", path_text.as_str()])
        .status()
        .map_err(|err| err.to_string())?;
    if !status.success() {
        return Err("launchctl load -w failed".to_string());
    }

    Ok(plist_path)
}

#[cfg(target_os = "macos")]
fn configured_vault_dir() -> Result<PathBuf, String> {
    if let Some(explicit) = std::env::var_os("AIPASS_VAULT_DIR") {
        Ok(PathBuf::from(explicit))
    } else {
        aipass_agent::default_vault_dir().map_err(|err| err.to_string())
    }
}

#[cfg(target_os = "macos")]
fn launch_agent_path(label: &str) -> Result<PathBuf, String> {
    Ok(home_dir()?
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{label}.plist")))
}

#[cfg(target_os = "macos")]
fn home_dir() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set".to_string())
}

#[cfg(target_os = "macos")]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
