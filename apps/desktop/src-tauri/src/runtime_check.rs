//! Opt-in verification of a disposable copy of the finished application.
//! The normal WebView, agent IPC, updater signature checks and restart run;
//! user data, browser registration and login services stay outside the fixture.
use aipass_agent_protocol::{AgentRequest, SessionStatus};
use aipass_storage::atomic_write_bytes;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

static CHECK: OnceLock<Check> = OnceLock::new();
static FRONTEND_READY: AtomicBool = AtomicBool::new(false);
static FRONTEND_FAILED: AtomicBool = AtomicBool::new(false);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Config {
    mode: Mode,
    endpoint: url::Url,
}

#[derive(Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
enum Mode {
    Startup,
    Install,
    Cached,
}

struct Check {
    root: PathBuf,
    config: Config,
    restarted_from: Option<u32>,
}

pub(crate) fn active() -> bool {
    CHECK.get().is_some()
}

pub(crate) fn path(name: &str) -> Option<PathBuf> {
    CHECK.get().map(|check| check.root.join(name))
}

pub(crate) fn endpoint() -> Option<url::Url> {
    CHECK.get().map(|check| check.config.endpoint.clone())
}

pub(crate) fn reinstall_same_version() -> bool {
    CHECK
        .get()
        .is_some_and(|check| check.config.mode != Mode::Startup && check.restarted_from.is_none())
}

pub(crate) fn initialize() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let Some(argument) = args.next() else {
        return Ok(());
    };
    if argument != "--verify-runtime" {
        return Ok(());
    }
    let root = args
        .next()
        .ok_or("missing runtime verification directory")?;
    if args.next().is_some() || !cfg!(target_os = "macos") {
        return Err("runtime verification requires macOS and one fixture directory".into());
    }
    let root = std::fs::canonicalize(root).map_err(|err| err.to_string())?;
    let exe = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_err(|err| err.to_string())?;
    validate_fixture_executable(&root, &exe)?;
    let config: Config = serde_json::from_slice(
        &std::fs::read(root.join("runtime-check.json")).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;
    validate_endpoint(&config.endpoint)?;
    let restarted_from = match std::fs::read(root.join("installed.json")) {
        Ok(bytes) => Some(serde_json::from_slice(&bytes).map_err(|err| err.to_string())?),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(err.to_string()),
    };
    for (key, relative) in [
        ("AIPASS_VAULT_DIR", "data/vault"),
        ("AIPASS_AGENT_RUNTIME_DIR", "agent-run"),
        ("AIPASS_DESKTOP_RUNTIME_DIR", "desktop-run"),
        ("AIPASS_LOG_DIR", "logs"),
    ] {
        let value = root.join(relative);
        std::fs::create_dir_all(&value).map_err(|err| err.to_string())?;
        std::env::set_var(key, value);
    }
    std::env::set_var(
        "AIPASS_AGENT_BINARY",
        root.join("AIPass.app/Contents/Resources/aipass-agent"),
    );
    // A local-only fixture must never discover or migrate the user's iCloud vault.
    let settings = root.join("data/agent/sync-settings.json");
    atomic_write_bytes(
        settings,
        br#"{"mode":"local","cloudkitMigrationComplete":true}"#,
    )
    .map_err(|err| err.to_string())?;
    atomic_write_bytes(
        root.join("window-size.json"),
        br#"{"width":960,"height":640}"#,
    )
    .map_err(|err| err.to_string())?;
    CHECK
        .set(Check {
            root,
            config,
            restarted_from,
        })
        .map_err(|_| "runtime verification already initialized".into())
}

fn validate_fixture_executable(root: &Path, exe: &Path) -> Result<(), String> {
    let temporary = std::fs::canonicalize("/tmp").map_err(|err| err.to_string())?;
    if !root.starts_with(temporary)
        || !root
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("aipass-runtime-"))
        || exe != root.join("AIPass.app/Contents/MacOS/aipass-desktop")
    {
        return Err(
            "runtime verification only accepts a disposable /tmp/aipass-runtime-* app copy".into(),
        );
    }
    Ok(())
}

fn validate_endpoint(endpoint: &url::Url) -> Result<(), String> {
    if endpoint.scheme() != "http"
        || endpoint.host_str() != Some("127.0.0.1")
        || endpoint.port().is_none()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
    {
        return Err("runtime verification feed must be a loopback HTTP fixture".into());
    }
    Ok(())
}

pub(crate) fn configure(context: &mut tauri::Context<tauri::Wry>) {
    if !active() {
        return;
    }
    for window in &mut context.config_mut().app.windows {
        window.width = 960.0;
        window.height = 640.0;
        window.incognito = true;
    }
    // Only this validated, loopback fixture uses HTTP. The production public
    // key remains unchanged and every package still requires its signature.
    context.config_mut().plugins.0.get_mut("updater").unwrap()
        ["dangerousInsecureTransportProtocol"] = true.into();
}

pub(crate) fn frontend_stage(stage: &str) {
    if stage == "complete" {
        FRONTEND_READY.store(true, Ordering::SeqCst);
    } else if stage == "error" {
        FRONTEND_FAILED.store(true, Ordering::SeqCst);
    }
}

pub(crate) fn installed() -> Result<(), String> {
    if let Some(path) = path("installed.json") {
        atomic_write_bytes(path, &serde_json::to_vec(&std::process::id()).unwrap())
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}

pub(crate) fn start(app: AppHandle) {
    let Some(check) = CHECK.get() else {
        return;
    };
    std::thread::spawn(move || {
        let result = observe(&app).and_then(|()| {
            if check.config.mode == Mode::Install && check.restarted_from.is_none() {
                tauri::async_runtime::block_on(crate::updates::install_update(
                    app.clone(),
                    "official".into(),
                ))?;
                return Err("installer returned without restarting the application".into());
            }
            if check.config.mode != Mode::Startup {
                let previous = check
                    .restarted_from
                    .ok_or("cached update did not restart")?;
                if previous == std::process::id() {
                    return Err("update reused the installing process".into());
                }
                for name in ["cache/updates/package", "cache/updates/metadata.json"] {
                    if check.root.join(name).exists() {
                        return Err("installed update left a pending package".into());
                    }
                }
            }
            Ok(())
        });
        let report = serde_json::json!({
            "ok": result.is_ok(),
            "error": result.err(),
            "pid": std::process::id(),
            "restartedFrom": check.restarted_from,
            "version": app.package_info().version.to_string(),
        });
        let report_written = atomic_write_bytes(
            check.root.join("result.json"),
            &serde_json::to_vec_pretty(&report).unwrap(),
        )
        .is_ok();
        if let Ok(client) = crate::agent_client(&app) {
            let _ = client.shutdown();
        }
        crate::ALLOW_PROCESS_EXIT.store(true, Ordering::SeqCst);
        app.exit(if report["ok"] == true && report_written {
            0
        } else {
            1
        });
    });
}

fn observe(app: &AppHandle) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(45);
    while !FRONTEND_READY.load(Ordering::SeqCst) {
        if FRONTEND_FAILED.load(Ordering::SeqCst) || Instant::now() >= deadline {
            capture_frontend(app);
            return Err("frontend failed or did not finish startup within 45 seconds".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    crate::logging::log_event("desktop.runtime_check.ready", &[])?;
    // Catch delayed exits and ensure native event-loop and typed agent IPC
    // remain responsive after the frontend reports ready.
    for _ in 0..20 {
        let window = app
            .get_webview_window("main")
            .ok_or("main window missing")?;
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        window
            .eval_with_callback(
                "!!window.__TAURI_INTERNALS__ && document.readyState === 'complete' && document.querySelector('#app')?.childElementCount > 0",
                move |result| {
                    let _ = send.send(result);
                },
            )
            .map_err(|err| err.to_string())?;
        if receive
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| "frontend stopped responding")?
            != "true"
        {
            return Err("frontend lost its rendered application".into());
        }
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        app.run_on_main_thread(move || {
            let _ = send.send(());
        })
        .map_err(|err| err.to_string())?;
        receive
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| "desktop event loop stopped responding")?;
        crate::agent_client(app)?
            .request::<SessionStatus>(&AgentRequest::SessionStatus)
            .map_err(|err| err.to_string())?;
        std::thread::sleep(Duration::from_millis(500));
    }
    let window = app
        .get_webview_window("main")
        .ok_or("main window missing")?;
    let size = window.inner_size().map_err(|err| err.to_string())?;
    let scale = window.scale_factor().map_err(|err| err.to_string())?;
    let size = size.to_logical::<f64>(scale);
    if (size.width - 960.0).abs() > 1.0 || (size.height - 640.0).abs() > 1.0 {
        return Err(format!(
            "unexpected validation viewport {}x{}",
            size.width, size.height
        ));
    }
    if std::env::var("AIPASS_WINDOW_TARGET").as_deref() != Ok("tray")
        && !window.is_visible().map_err(|err| err.to_string())?
    {
        return Err("main window was not revealed".into());
    }
    Ok(())
}

fn capture_frontend(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let Some(path) = path("logs/frontend.json") else {
        return;
    };
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    // This only runs in the empty disposable fixture; never record user pages.
    let _ = window.eval_with_callback(
        "JSON.stringify({url:location.href,readyState:document.readyState,tauri:!!window.__TAURI_INTERNALS__,body:document.body?.innerText,scripts:[...document.scripts].map(s=>s.src),resources:performance.getEntriesByType('resource').map(r=>({name:r.name,duration:r.duration}))})",
        move |result| {
            let _ = atomic_write_bytes(&path, result.as_bytes());
            let _ = send.send(());
        },
    );
    let _ = receive.recv_timeout(Duration::from_secs(2));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_verification_refuses_installed_apps_and_remote_feeds() {
        assert!(validate_fixture_executable(
            Path::new("/Applications"),
            Path::new("/Applications/AIPass.app/Contents/MacOS/aipass-desktop")
        )
        .is_err());
        for endpoint in [
            "https://example.com/update.json",
            "http://localhost:1234/update.json",
            "http://127.0.0.1/update.json",
            "http://user@127.0.0.1:1234/update.json",
        ] {
            assert!(validate_endpoint(&endpoint.parse().unwrap()).is_err());
        }
        assert!(validate_endpoint(&"http://127.0.0.1:1234/update.json".parse().unwrap()).is_ok());
    }
}
