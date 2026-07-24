use crate::{
    agent_client, agent_error_to_string, agent_request_no_unlock, ensure_agent_running_for_desktop,
};
use aipass_agent_protocol::{AgentRequest, LockReason, ProxyStatus, SessionStatus};
#[cfg(target_os = "macos")]
use aipass_agent_protocol::{SensitiveString, SessionUnlockMode};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tauri::{App, AppHandle, Emitter, Listener, Manager, WindowEvent};

#[cfg(not(target_os = "macos"))]
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
#[cfg(not(target_os = "macos"))]
use tauri::tray::{MouseButtonState, TrayIconBuilder, TrayIconEvent};

#[cfg(not(target_os = "macos"))]
const TRAY_ID: &str = "aipass-agent";
#[cfg(not(target_os = "macos"))]
const MENU_STATUS: &str = "tray-status";
#[cfg(not(target_os = "macos"))]
const MENU_OPEN: &str = "tray-open";
#[cfg(not(target_os = "macos"))]
const MENU_HIDE: &str = "tray-hide";
#[cfg(not(target_os = "macos"))]
const MENU_REFRESH: &str = "tray-refresh";
#[cfg(not(target_os = "macos"))]
const MENU_START_AGENT: &str = "tray-start-agent";
#[cfg(not(target_os = "macos"))]
const MENU_LOCK: &str = "tray-lock";
#[cfg(not(target_os = "macos"))]
const MENU_INSTALL_LOGIN_AGENT: &str = "tray-install-login-agent";
#[cfg(not(target_os = "macos"))]
const MENU_PROXY_STATUS: &str = "tray-proxy-status";
#[cfg(not(target_os = "macos"))]
const MENU_PROXY_OPEN: &str = "tray-proxy-open";
#[cfg(not(target_os = "macos"))]
const MENU_PROXY_START: &str = "tray-proxy-start";
#[cfg(not(target_os = "macos"))]
const MENU_PROXY_STOP: &str = "tray-proxy-stop";
#[cfg(not(target_os = "macos"))]
const MENU_PROXY_REFRESH: &str = "tray-proxy-refresh";
#[cfg(not(target_os = "macos"))]
const MENU_QUIT: &str = "tray-quit";
const PROXY_STATUS_CHANGED_EVENT: &str = "proxy-status-changed";
pub(crate) const REFRESH_PROXY_TRAY_EVENT: &str = "refresh-proxy-tray-status";
const OPEN_SERVER_WORKSPACE_EVENT: &str = "open-server-workspace";
const STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(30);

static AGENT_START_LOCK: Mutex<()> = Mutex::new(());

/// Canonical tray action ids shared by the classic menu and the Swift panel.
mod action {
    pub(crate) const OPEN: &str = "open";
    pub(crate) const HIDE: &str = "hide";
    pub(crate) const REFRESH: &str = "refresh";
    pub(crate) const START_AGENT: &str = "start-agent";
    pub(crate) const LOCK_VAULT: &str = "lock-vault";
    pub(crate) const REPAIR_AUTOSTART: &str = "repair-autostart";
    pub(crate) const PROXY_OPEN: &str = "proxy-open";
    pub(crate) const PROXY_START: &str = "proxy-start";
    pub(crate) const PROXY_STOP: &str = "proxy-stop";
    pub(crate) const QUIT: &str = "quit";
    /// Sent by the Swift panel whenever it is shown, to trigger a status refresh.
    #[cfg(target_os = "macos")]
    pub(crate) const PANEL_OPEN: &str = "panel-open";
}

/// Platform-specific rendering target for tray status updates. All tray logic
/// (polling, IPC, actions) is shared; only the presentation differs.
#[derive(Clone)]
enum TrayFeedback {
    #[cfg(target_os = "macos")]
    Swift,
    #[cfg(not(target_os = "macos"))]
    Menu(TrayMenuItems),
}

impl TrayFeedback {
    fn apply(&self, #[allow(unused_variables)] app: &AppHandle, snapshot: &TraySnapshot) {
        match self {
            #[cfg(target_os = "macos")]
            Self::Swift => crate::tray_swift::push_status(&snapshot.dto()),
            #[cfg(not(target_os = "macos"))]
            Self::Menu(items) => {
                let _ = items.status.set_text(snapshot.agent.menu_text());
                let _ = items.start_agent.set_enabled(snapshot.agent.can_start());
                let _ = items.lock.set_enabled(snapshot.agent.can_lock());
                let _ = items.proxy_status.set_text(snapshot.proxy_menu_text());
                let _ = items.proxy_open.set_enabled(snapshot.can_open_proxy());
                let _ = items.proxy_start.set_enabled(snapshot.can_start_proxy());
                let _ = items.proxy_stop.set_enabled(snapshot.can_stop_proxy());
                let _ = items.proxy_refresh.set_enabled(true);

                if let Some(tray) = app.tray_by_id(TRAY_ID) {
                    let _ = tray.set_tooltip(Some(snapshot.tooltip()));
                }
            }
        }
    }

    /// Transient text shown while a long-running action is in flight. The
    /// Swift panel renders its own busy indicator instead, so this is a no-op
    /// there; the next `apply` call always republishes the authoritative state.
    fn agent_transient(&self, #[allow(unused_variables)] text: &str) {
        #[cfg(not(target_os = "macos"))]
        if let Self::Menu(items) = self {
            let _ = items.status.set_text(text);
        }
    }

    fn set_agent_start_enabled(&self, #[allow(unused_variables)] enabled: bool) {
        #[cfg(not(target_os = "macos"))]
        if let Self::Menu(items) = self {
            let _ = items.start_agent.set_enabled(enabled);
        }
    }

    fn proxy_transient(&self, #[allow(unused_variables)] text: &str) {
        #[cfg(not(target_os = "macos"))]
        if let Self::Menu(items) = self {
            let _ = items.proxy_status.set_text(text);
        }
    }

    fn set_proxy_start_enabled(&self, #[allow(unused_variables)] enabled: bool) {
        #[cfg(not(target_os = "macos"))]
        if let Self::Menu(items) = self {
            let _ = items.proxy_start.set_enabled(enabled);
        }
    }

    fn set_proxy_stop_enabled(&self, #[allow(unused_variables)] enabled: bool) {
        #[cfg(not(target_os = "macos"))]
        if let Self::Menu(items) = self {
            let _ = items.proxy_stop.set_enabled(enabled);
        }
    }

    fn reset_repair_text(&self) {
        #[cfg(not(target_os = "macos"))]
        if let Self::Menu(items) = self {
            let _ = items.install_login_agent.set_text("Repair Auto-Start");
        }
    }
}

#[cfg(not(target_os = "macos"))]
#[derive(Clone)]
struct TrayMenuItems {
    status: MenuItem<tauri::Wry>,
    start_agent: MenuItem<tauri::Wry>,
    lock: MenuItem<tauri::Wry>,
    install_login_agent: MenuItem<tauri::Wry>,
    proxy_status: MenuItem<tauri::Wry>,
    proxy_open: MenuItem<tauri::Wry>,
    proxy_start: MenuItem<tauri::Wry>,
    proxy_stop: MenuItem<tauri::Wry>,
    proxy_refresh: MenuItem<tauri::Wry>,
}

pub(crate) fn setup(app: &App) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        crate::tray_swift::setup(app)?;
        start_shared(app, TrayFeedback::Swift);
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        setup_menu(app)
    }
}

#[cfg(not(target_os = "macos"))]
fn setup_menu(app: &App) -> tauri::Result<()> {
    let status = MenuItem::with_id(app, MENU_STATUS, "Agent: checking...", false, None::<&str>)?;
    let open = MenuItem::with_id(app, MENU_OPEN, "Open AIPass", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, MENU_HIDE, "Hide Window", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, MENU_REFRESH, "Refresh Status", true, None::<&str>)?;
    let start_agent = MenuItem::with_id(app, MENU_START_AGENT, "Start Agent", false, None::<&str>)?;
    let lock = MenuItem::with_id(app, MENU_LOCK, "Lock Vault", false, None::<&str>)?;
    let proxy_status = MenuItem::with_id(
        app,
        MENU_PROXY_STATUS,
        "Status: checking...",
        false,
        None::<&str>,
    )?;
    let proxy_open = MenuItem::with_id(app, MENU_PROXY_OPEN, "Open Server", true, None::<&str>)?;
    let proxy_start = MenuItem::with_id(app, MENU_PROXY_START, "Start Proxy", false, None::<&str>)?;
    let proxy_stop = MenuItem::with_id(app, MENU_PROXY_STOP, "Stop Proxy", false, None::<&str>)?;
    let proxy_refresh = MenuItem::with_id(
        app,
        MENU_PROXY_REFRESH,
        "Refresh Proxy Status",
        true,
        None::<&str>,
    )?;
    let proxy_menu = Submenu::with_items(
        app,
        "Proxy Server",
        true,
        &[
            &proxy_status,
            &proxy_open,
            &PredefinedMenuItem::separator(app)?,
            &proxy_start,
            &proxy_stop,
            &proxy_refresh,
        ],
    )?;

    let install_login_agent = MenuItem::with_id(
        app,
        MENU_INSTALL_LOGIN_AGENT,
        "Repair Auto-Start",
        true,
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
            &proxy_menu,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    let items = TrayMenuItems {
        status,
        start_agent,
        lock,
        install_login_agent,
        proxy_status,
        proxy_open,
        proxy_start,
        proxy_stop,
        proxy_refresh,
    };

    let menu_feedback = TrayFeedback::Menu(items.clone());
    let tray_feedback = TrayFeedback::Menu(items.clone());
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("AIPass Agent")
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, event| {
            if let Some(action) = menu_action_for(event.id().as_ref()) {
                dispatch_action(app, action, &menu_feedback);
            }
        })
        .on_tray_icon_event(move |tray, event| {
            if let TrayIconEvent::Click {
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                refresh_status_async(tray.app_handle().clone(), tray_feedback.clone());
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

    start_shared(app, TrayFeedback::Menu(items));
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn menu_action_for(id: &str) -> Option<&'static str> {
    match id {
        MENU_OPEN => Some(action::OPEN),
        MENU_HIDE => Some(action::HIDE),
        MENU_REFRESH | MENU_PROXY_REFRESH => Some(action::REFRESH),
        MENU_START_AGENT => Some(action::START_AGENT),
        MENU_LOCK => Some(action::LOCK_VAULT),
        MENU_INSTALL_LOGIN_AGENT => Some(action::REPAIR_AUTOSTART),
        MENU_PROXY_OPEN => Some(action::PROXY_OPEN),
        MENU_PROXY_START => Some(action::PROXY_START),
        MENU_PROXY_STOP => Some(action::PROXY_STOP),
        MENU_QUIT => Some(action::QUIT),
        _ => None,
    }
}

/// Entry point for actions coming from the Swift tray panel / context menu.
#[cfg(target_os = "macos")]
pub(crate) fn dispatch_swift_action(app: &AppHandle, action_id: &str) {
    dispatch_action(app, action_id, &TrayFeedback::Swift);
}

fn dispatch_action(app: &AppHandle, action_id: &str, feedback: &TrayFeedback) {
    match action_id {
        action::OPEN => {
            if let Err(err) = open_main_window(app) {
                eprintln!("failed to open AIPass from tray: {err}");
                feedback.agent_transient("Agent: open failed");
            }
            refresh_status_async(app.clone(), feedback.clone());
        }
        action::HIDE => hide_main_window(app),
        action::REFRESH => refresh_status_async(app.clone(), feedback.clone()),
        #[cfg(target_os = "macos")]
        action::PANEL_OPEN => refresh_status_async(app.clone(), feedback.clone()),
        action::START_AGENT => start_agent_async(app.clone(), feedback.clone()),
        action::LOCK_VAULT => lock_vault_async(app.clone(), feedback.clone()),
        action::REPAIR_AUTOSTART => install_login_agent_async(app.clone(), feedback.clone()),
        action::PROXY_OPEN => {
            if let Err(err) = open_server_window(app) {
                eprintln!("failed to open proxy server workspace from tray: {err}");
                feedback.proxy_transient("Status: open failed");
            }
            refresh_status_async(app.clone(), feedback.clone());
        }
        action::PROXY_START => start_proxy_async(app.clone(), feedback.clone()),
        action::PROXY_STOP => stop_proxy_async(app.clone(), feedback.clone()),
        action::QUIT => quit_aipass_async(app.clone()),
        _ => {}
    }
}

/// Behavior shared by both tray frontends: close-to-tray, the proxy refresh
/// event bridge, an initial status push, and the 30s watchdog.
fn start_shared(app: &App, feedback: TrayFeedback) {
    install_close_to_tray(app);
    let refresh_app = app.handle().clone();
    let refresh_feedback = feedback.clone();
    app.listen(REFRESH_PROXY_TRAY_EVENT, move |_| {
        refresh_status_async(refresh_app.clone(), refresh_feedback.clone());
    });
    refresh_status_async(app.handle().clone(), feedback.clone());
    spawn_status_refresher(app.handle().clone(), feedback);
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

fn open_main_window(app: &AppHandle) -> Result<(), String> {
    if show_existing_main_window(app) {
        return Ok(());
    }

    let client = agent_client(app)?;
    aipass_agent::desktop::open_desktop_window("main", &client.config.vault_dir)
        .map_err(|err| err.to_string())
}

fn open_server_window(app: &AppHandle) -> Result<(), String> {
    if show_existing_main_window(app) {
        app.emit(OPEN_SERVER_WORKSPACE_EVENT, ())
            .map_err(|err| err.to_string())?;
        return Ok(());
    }

    let client = agent_client(app)?;
    aipass_agent::desktop::open_desktop_window("server", &client.config.vault_dir)
        .map_err(|err| err.to_string())
}

fn show_existing_main_window(app: &AppHandle) -> bool {
    if let Some(window) = app.get_webview_window("main") {
        #[cfg(target_os = "macos")]
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
        #[cfg(target_os = "macos")]
        let _ = app.show();

        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        return true;
    }
    false
}

fn hide_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
}

fn refresh_status_async(app: AppHandle, feedback: TrayFeedback) {
    thread::spawn(move || refresh_status(&app, &feedback));
}

fn spawn_status_refresher(app: AppHandle, feedback: TrayFeedback) {
    thread::spawn(move || loop {
        thread::sleep(STATUS_REFRESH_INTERVAL);
        recover_agent_and_refresh_status(&app, &feedback);
    });
}

fn refresh_status(app: &AppHandle, feedback: &TrayFeedback) {
    feedback.apply(app, &current_tray_snapshot(app));
}

fn current_tray_snapshot(app: &AppHandle) -> TraySnapshot {
    let client = match agent_client(app) {
        Ok(client) => client,
        Err(err) => return TraySnapshot::unavailable(err),
    };
    let session = match client
        .request::<SessionStatus>(&AgentRequest::SessionStatus)
        .map_err(agent_error_to_string)
    {
        Ok(status) => status,
        Err(err) => return TraySnapshot::unavailable(err),
    };
    let proxy = client
        .request::<ProxyStatus>(&AgentRequest::ServerStatus)
        .map(ProxyTrayStatus::Available)
        .unwrap_or_else(|err| ProxyTrayStatus::Unavailable(agent_error_to_string(err)));

    TraySnapshot {
        agent: TrayStatus::Running(session),
        proxy,
    }
}

fn recover_agent_and_refresh_status(app: &AppHandle, feedback: &TrayFeedback) {
    let snapshot = current_tray_snapshot(app);
    if !snapshot.agent.can_start() {
        feedback.apply(app, &snapshot);
        return;
    }

    feedback.agent_transient("Agent: starting...");
    feedback.set_agent_start_enabled(false);

    match ensure_agent_running_for_tray(app) {
        Ok(_) => refresh_status(app, feedback),
        Err(err) => {
            eprintln!("failed to auto-start AIPass agent from tray watchdog: {err}");
            feedback.apply(app, &TraySnapshot::unavailable(err));
        }
    }
}

fn ensure_agent_running_for_tray(app: &AppHandle) -> Result<(), String> {
    let _guard = AGENT_START_LOCK
        .lock()
        .map_err(|_| "agent start lock is poisoned".to_string())?;
    agent_client(app).and_then(|client| ensure_agent_running_for_desktop(&client))
}

fn start_agent_async(app: AppHandle, feedback: TrayFeedback) {
    thread::spawn(move || {
        feedback.agent_transient("Agent: starting...");
        feedback.set_agent_start_enabled(false);
        if let Err(err) = ensure_agent_running_for_tray(&app) {
            eprintln!("failed to start AIPass agent from tray: {err}");
            feedback.agent_transient("Agent: start failed");
        }
        refresh_status(&app, &feedback);
    });
}

fn start_proxy_async(app: AppHandle, feedback: TrayFeedback) {
    thread::spawn(move || {
        feedback.proxy_transient("Status: starting...");
        feedback.set_proxy_start_enabled(false);
        let result = agent_request_no_unlock::<ProxyStatus>(&app, AgentRequest::ServerStart);
        match result {
            Ok(_) => {
                let _ = app.emit(PROXY_STATUS_CHANGED_EVENT, ());
            }
            Err(err) => {
                eprintln!("failed to start proxy server from tray: {err}");
                feedback.proxy_transient("Status: start failed");
            }
        }
        refresh_status(&app, &feedback);
    });
}

fn stop_proxy_async(app: AppHandle, feedback: TrayFeedback) {
    thread::spawn(move || {
        feedback.proxy_transient("Status: stopping...");
        feedback.set_proxy_stop_enabled(false);
        let result = agent_request_no_unlock::<ProxyStatus>(&app, AgentRequest::ServerStop);
        match result {
            Ok(_) => {
                let _ = app.emit(PROXY_STATUS_CHANGED_EVENT, ());
            }
            Err(err) => {
                eprintln!("failed to stop proxy server from tray: {err}");
                feedback.proxy_transient("Status: stop failed");
            }
        }
        refresh_status(&app, &feedback);
    });
}

fn lock_vault_async(app: AppHandle, feedback: TrayFeedback) {
    thread::spawn(move || {
        let result = agent_request_no_unlock::<SessionStatus>(
            &app,
            AgentRequest::SessionLock {
                reason: LockReason::Manual,
            },
        );
        if let Err(err) = result {
            eprintln!("failed to lock AIPass vault from tray: {err}");
            feedback.agent_transient("Agent: lock failed");
        }
        refresh_status(&app, &feedback);
    });
}

/// Unlock the vault with the password entered in the Swift tray panel.
/// The panel is notified of the outcome via `report_unlock_result` and a
/// fresh status push.
#[cfg(target_os = "macos")]
pub(crate) fn unlock_vault_with_password(app: AppHandle, password: String) {
    thread::spawn(move || {
        let result = agent_request_no_unlock::<SessionStatus>(
            &app,
            AgentRequest::SessionUnlock {
                mode: SessionUnlockMode::Password {
                    password: SensitiveString::new(password),
                },
            },
        );
        match result {
            Ok(_) => crate::tray_swift::report_unlock_result(None),
            Err(err) => {
                eprintln!("failed to unlock AIPass vault from tray: {err}");
                crate::tray_swift::report_unlock_result(Some(short_error(&err).to_string()));
            }
        }
        refresh_status(&app, &TrayFeedback::Swift);
    });
}

fn install_login_agent_async(app: AppHandle, feedback: TrayFeedback) {
    thread::spawn(move || {
        let result = agent_client(&app).and_then(|client| {
            let agent_binary = aipass_agent::agent_binary_path().map_err(|err| err.to_string())?;
            let desktop_binary = std::env::current_exe().map_err(|err| err.to_string())?;
            aipass_agent::install_agent_autostart(&agent_binary, &client.config.vault_dir)
                .map_err(|err| err.to_string())?;
            aipass_agent::install_tray_autostart(&desktop_binary, &client.config.vault_dir)
                .map_err(|err| err.to_string())?;
            ensure_agent_running_for_desktop(&client)
        });
        match result {
            Ok(_) => {
                feedback.reset_repair_text();
                refresh_status(&app, &feedback);
            }
            Err(err) => {
                eprintln!("failed to install AIPass agent autostart: {err}");
                feedback.agent_transient("Agent: autostart failed");
            }
        }
    });
}

fn quit_aipass_async(app: AppHandle) {
    thread::spawn(move || {
        #[cfg(target_os = "macos")]
        if let Ok(client) = agent_client(&app) {
            if let Err(err) = aipass_agent::stop_tray_autostart(&client.config.vault_dir) {
                eprintln!("failed to stop AIPass tray autostart before quit: {err}");
            }
        }
        #[cfg(target_os = "macos")]
        crate::tray_swift::shutdown();
        app.exit(0);
    });
}

enum TrayStatus {
    Running(SessionStatus),
    Unavailable(String),
}

struct TraySnapshot {
    agent: TrayStatus,
    proxy: ProxyTrayStatus,
}

enum ProxyTrayStatus {
    Available(ProxyStatus),
    Unavailable(String),
}

#[cfg(not(target_os = "macos"))]
struct TrayIconAsset {
    image: tauri::image::Image<'static>,
    is_template: bool,
}

/// Serializable status snapshot consumed by the SwiftUI tray panel.
#[cfg(target_os = "macos")]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrayStatusDto {
    agent_text: String,
    agent_state: &'static str,
    can_start_agent: bool,
    can_lock: bool,
    proxy_text: String,
    proxy_state: &'static str,
    proxy_state_text: String,
    proxy_detail: Option<String>,
    proxy_running: bool,
    can_open_proxy: bool,
    can_start_proxy: bool,
    can_stop_proxy: bool,
    tooltip: String,
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

    #[cfg(target_os = "macos")]
    fn state_id(&self) -> &'static str {
        match self {
            Self::Running(status) if !status.exists => "no-vault",
            Self::Running(status) if status.locked => "locked",
            Self::Running(_) => "unlocked",
            Self::Unavailable(_) => "unreachable",
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

impl TraySnapshot {
    fn unavailable(err: String) -> Self {
        Self {
            agent: TrayStatus::Unavailable(err.clone()),
            proxy: ProxyTrayStatus::Unavailable(err),
        }
    }

    fn can_start_proxy(&self) -> bool {
        matches!(
            (&self.agent, &self.proxy),
            (
                TrayStatus::Running(SessionStatus {
                    exists: true,
                    locked: false,
                    ..
                }),
                ProxyTrayStatus::Available(ProxyStatus { running: false, .. })
            )
        )
    }

    fn can_open_proxy(&self) -> bool {
        true
    }

    fn can_stop_proxy(&self) -> bool {
        matches!(
            &self.proxy,
            ProxyTrayStatus::Available(ProxyStatus { running: true, .. })
        )
    }

    fn tooltip(&self) -> String {
        format!("{}; {}", self.agent.tooltip(), self.proxy.tooltip())
    }

    fn proxy_menu_text(&self) -> String {
        match (&self.agent, &self.proxy) {
            (
                TrayStatus::Running(SessionStatus { exists: false, .. }),
                ProxyTrayStatus::Available(ProxyStatus { running: false, .. }),
            ) => "Status: no vault".to_string(),
            (
                TrayStatus::Running(SessionStatus { locked: true, .. }),
                ProxyTrayStatus::Available(ProxyStatus { running: false, .. }),
            ) => "Status: Vault locked".to_string(),
            _ => self.proxy.menu_text(),
        }
    }

    #[cfg(target_os = "macos")]
    fn dto(&self) -> TrayStatusDto {
        let (proxy_state, proxy_state_text, proxy_detail) = self.proxy_panel_fields();
        TrayStatusDto {
            agent_text: self.agent.menu_text(),
            agent_state: self.agent.state_id(),
            can_start_agent: self.agent.can_start(),
            can_lock: self.agent.can_lock(),
            proxy_text: self.proxy_menu_text(),
            proxy_state,
            proxy_state_text,
            proxy_detail,
            proxy_running: matches!(
                &self.proxy,
                ProxyTrayStatus::Available(ProxyStatus { running: true, .. })
            ),
            can_open_proxy: self.can_open_proxy(),
            can_start_proxy: self.can_start_proxy(),
            can_stop_proxy: self.can_stop_proxy(),
            tooltip: self.tooltip(),
        }
    }

    #[cfg(target_os = "macos")]
    fn proxy_panel_fields(&self) -> (&'static str, String, Option<String>) {
        match (&self.agent, &self.proxy) {
            (
                TrayStatus::Running(SessionStatus { exists: false, .. }),
                ProxyTrayStatus::Available(ProxyStatus { running: false, .. }),
            ) => ("no-vault", "No vault".to_string(), None),
            (
                TrayStatus::Running(SessionStatus { locked: true, .. }),
                ProxyTrayStatus::Available(ProxyStatus { running: false, .. }),
            ) => ("locked", "Vault locked".to_string(), None),
            (
                _,
                ProxyTrayStatus::Available(ProxyStatus {
                    running: true,
                    bind_addr,
                    active_routes,
                    ..
                }),
            ) => (
                "running",
                "Running".to_string(),
                Some(format!("{bind_addr} · {}", route_count(*active_routes))),
            ),
            (_, ProxyTrayStatus::Available(_)) => ("stopped", "Stopped".to_string(), None),
            (_, ProxyTrayStatus::Unavailable(_)) => {
                ("unavailable", "Unavailable".to_string(), None)
            }
        }
    }
}

impl ProxyTrayStatus {
    fn menu_text(&self) -> String {
        match self {
            Self::Available(status) if status.running => format!(
                "Status: Running | {} | {}",
                status.bind_addr,
                route_count(status.active_routes)
            ),
            Self::Available(_) => "Status: Stopped".to_string(),
            Self::Unavailable(_) => "Status: unavailable".to_string(),
        }
    }

    fn tooltip(&self) -> String {
        match self {
            Self::Available(status) if status.running => format!(
                "Proxy Server is running at {} with {}",
                status.bind_addr,
                route_count(status.active_routes)
            ),
            Self::Available(_) => "Proxy Server is stopped".to_string(),
            Self::Unavailable(err) => {
                format!("Proxy Server status is unavailable: {}", short_error(err))
            }
        }
    }
}

fn route_count(count: usize) -> String {
    format!("{count} {}", if count == 1 { "route" } else { "routes" })
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

#[cfg(test)]
mod tests {
    use super::*;
    use aipass_agent_protocol::SessionPolicy;

    #[test]
    fn proxy_status_text_includes_bind_address_and_routes_when_running() {
        let snapshot = snapshot(false, available_proxy(true, 3));

        assert_eq!(
            snapshot.proxy_menu_text(),
            "Status: Running | 127.0.0.1:8787 | 3 routes"
        );
        assert!(snapshot.tooltip().contains("127.0.0.1:8787"));
        assert!(snapshot.tooltip().contains("3 routes"));
    }

    #[test]
    fn proxy_status_text_distinguishes_stopped_locked_and_unavailable() {
        assert_eq!(
            snapshot(false, available_proxy(false, 0)).proxy_menu_text(),
            "Status: Stopped"
        );
        assert_eq!(
            snapshot(true, available_proxy(false, 0)).proxy_menu_text(),
            "Status: Vault locked"
        );
        let mut no_vault = snapshot(true, available_proxy(false, 0));
        if let TrayStatus::Running(status) = &mut no_vault.agent {
            status.exists = false;
        }
        assert_eq!(no_vault.proxy_menu_text(), "Status: no vault");
        assert_eq!(
            snapshot(
                false,
                ProxyTrayStatus::Unavailable("agent error".to_string())
            )
            .proxy_menu_text(),
            "Status: unavailable"
        );
    }

    #[test]
    fn proxy_actions_respect_vault_and_runtime_state() {
        let stopped = snapshot(false, available_proxy(false, 1));
        assert!(stopped.can_open_proxy());
        assert!(stopped.can_start_proxy());
        assert!(!stopped.can_stop_proxy());

        let locked = snapshot(true, available_proxy(false, 1));
        assert!(!locked.can_start_proxy());
        assert!(!locked.can_stop_proxy());

        let locked_running = snapshot(true, available_proxy(true, 1));
        assert!(!locked_running.can_start_proxy());
        assert!(locked_running.can_stop_proxy());

        let unavailable = TraySnapshot::unavailable("offline".to_string());
        assert!(unavailable.can_open_proxy());
        assert!(!unavailable.can_start_proxy());
        assert!(!unavailable.can_stop_proxy());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn dto_mirrors_snapshot_state() {
        let running = snapshot(false, available_proxy(true, 2)).dto();
        assert_eq!(running.agent_state, "unlocked");
        assert_eq!(running.proxy_state, "running");
        assert_eq!(
            running.proxy_detail.as_deref(),
            Some("127.0.0.1:8787 · 2 routes")
        );
        assert!(running.proxy_running);
        assert!(running.can_lock);
        assert!(!running.can_start_agent);
        assert!(running.can_stop_proxy);
        assert!(!running.can_start_proxy);

        let locked = snapshot(true, available_proxy(false, 0)).dto();
        assert_eq!(locked.agent_state, "locked");
        assert_eq!(locked.proxy_state, "locked");
        assert_eq!(locked.proxy_state_text, "Vault locked");
        assert!(!locked.can_lock);
        assert!(!locked.can_start_proxy);

        let mut no_vault = snapshot(true, available_proxy(false, 0));
        if let TrayStatus::Running(status) = &mut no_vault.agent {
            status.exists = false;
        }
        let no_vault = no_vault.dto();
        assert_eq!(no_vault.agent_state, "no-vault");
        assert_eq!(no_vault.proxy_state, "no-vault");

        let offline = TraySnapshot::unavailable("offline".to_string()).dto();
        assert_eq!(offline.agent_state, "unreachable");
        assert_eq!(offline.proxy_state, "unavailable");
        assert!(offline.can_start_agent);
        assert!(!offline.can_stop_proxy);
    }

    fn snapshot(locked: bool, proxy: ProxyTrayStatus) -> TraySnapshot {
        TraySnapshot {
            agent: TrayStatus::Running(SessionStatus {
                exists: true,
                locked,
                policy: SessionPolicy::default(),
                last_lock_reason: None,
                vault_namespace: Some("test".to_string()),
            }),
            proxy,
        }
    }

    fn available_proxy(running: bool, active_routes: usize) -> ProxyTrayStatus {
        ProxyTrayStatus::Available(ProxyStatus {
            running,
            enabled: running,
            bind_addr: "127.0.0.1:8787".to_string(),
            active_routes,
            requests: 0,
            failures: 0,
            last_error: None,
            recent_requests: 0,
            recent_tokens: 0,
        })
    }
}
