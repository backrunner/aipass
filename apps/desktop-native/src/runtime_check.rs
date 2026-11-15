//! Opt-in verification of a disposable copy of the finished application.
//! The native window, Agent IPC, signature checks and restart run;
//! user data, browser registration and login services stay outside the fixture.
use crate::shell::{Event, Handle};
use aipass_agent_protocol::{AgentRequest, SessionStatus};
use aipass_storage::atomic_write_bytes;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

static CHECK: OnceLock<Check> = OnceLock::new();
static FRAME_READY: AtomicBool = AtomicBool::new(false);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Config {
    mode: Mode,
    endpoint: url::Url,
    expected_version: Option<String>,
    expected_error: Option<String>,
}

#[derive(Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
enum Mode {
    Startup,
    /// Keep the isolated fixture open for native UI review; never an update gate.
    Interactive,
    Install,
    CliInstall,
    Cached,
}

struct Check {
    root: PathBuf,
    config: Config,
    restarted_from: Option<u32>,
}

pub fn active() -> bool {
    CHECK.get().is_some()
}

pub fn isolated_linux_home() -> bool {
    cfg!(target_os = "linux")
        && CHECK.get().is_some_and(|check| {
            std::env::var_os("HOME").is_some_and(|home| Path::new(&home) == check.root.join("home"))
        })
}

pub fn path(name: &str) -> Option<PathBuf> {
    CHECK.get().map(|check| check.root.join(name))
}

pub fn endpoint() -> Option<url::Url> {
    CHECK.get().map(|check| check.config.endpoint.clone())
}

pub fn reinstall_same_version() -> bool {
    CHECK.get().is_some_and(|check| {
        matches!(
            check.config.mode,
            Mode::Install | Mode::CliInstall | Mode::Cached
        ) && check.restarted_from.is_none()
    })
}

pub fn initialize() -> Result<(), String> {
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
    if args.next().is_some() || !cfg!(any(target_os = "macos", target_os = "linux", windows)) {
        return Err(
            "runtime verification requires a supported desktop OS and one fixture directory".into(),
        );
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
    if cfg!(windows) && config.mode != Mode::Startup {
        return Err("Windows runtime verification currently supports startup only".into());
    }
    if cfg!(target_os = "linux")
        && std::env::var_os("HOME").is_none_or(|home| Path::new(&home) != root.join("home"))
    {
        return Err("Linux runtime verification requires an isolated fixture HOME".into());
    }
    let restarted_from = match std::fs::read(root.join("installed.json")) {
        Ok(bytes) => Some(serde_json::from_slice(&bytes).map_err(|err| err.to_string())?),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(err.to_string()),
    };
    if restarted_from.is_some() {
        std::env::set_var(
            "AIPASS_WINDOW_TARGET",
            if config.mode == Mode::CliInstall {
                "tray"
            } else {
                "main"
            },
        );
    }
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
    if cfg!(target_os = "macos") {
        std::env::set_var(
            "AIPASS_AGENT_BINARY",
            root.join("AIPass.app/Contents/Resources/aipass-agent"),
        );
    }
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
    let temporary = std::fs::canonicalize(if cfg!(windows) {
        std::env::temp_dir()
    } else {
        PathBuf::from("/tmp")
    })
    .map_err(|err| err.to_string())?;
    if !root.starts_with(temporary)
        || !root
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("aipass-runtime-"))
        || !fixture_executable(root, exe)
    {
        return Err(
            "runtime verification only accepts a disposable temporary aipass-runtime-* app copy"
                .into(),
        );
    }
    Ok(())
}

fn fixture_executable(root: &Path, exe: &Path) -> bool {
    if cfg!(windows) {
        return exe == root.join("package/aipass-desktop.exe");
    }
    if !cfg!(target_os = "linux") {
        return exe == root.join("AIPass.app/Contents/MacOS/aipass-desktop");
    }
    if exe == root.join("package/usr/bin/aipass-desktop") {
        return true;
    }
    // AppImage extract-and-run creates a fresh temporary AppDir after each restart.
    let Some(image) = std::env::var_os("APPIMAGE").and_then(|p| std::fs::canonicalize(p).ok())
    else {
        return false;
    };
    let Some(dir) = std::env::var_os("APPDIR").and_then(|p| std::fs::canonicalize(p).ok()) else {
        return false;
    };
    image == root.join("install/AIPass.AppImage")
        && dir.starts_with(std::fs::canonicalize("/tmp").unwrap_or_default())
        && exe == dir.join("usr/bin/aipass-desktop")
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

/// Only called by the native Render implementation after building its view.
pub fn rendered() {
    FRAME_READY.store(true, Ordering::Release);
}
pub fn probe(width: f32, height: f32, tray: bool) -> Result<(), String> {
    if !FRAME_READY.load(Ordering::Acquire) {
        return Err("native view has not rendered".into());
    }
    if (width - 960.).abs() > 1. || (height - 640.).abs() > 1. {
        return Err(format!("unexpected viewport {width}x{height}"));
    }
    if !tray {
        return Err("native tray is unavailable".into());
    }
    Ok(())
}
pub fn installed() -> anyhow::Result<()> {
    if let Some(path) = path("installed.json") {
        atomic_write_bytes(path, &serde_json::to_vec(&std::process::id())?)?;
    }
    let _ = crate::shell::logging::log_event("desktop.update.installed_restarting", &[]);
    Ok(())
}
pub fn restart_args(args: &[String]) -> Vec<String> {
    path("")
        .map(|root| {
            vec![
                "--verify-runtime".into(),
                root.to_string_lossy().into_owned(),
            ]
        })
        .unwrap_or_else(|| args.to_vec())
}
pub fn worker_enabled() -> bool {
    CHECK.get().is_none_or(|c| {
        !matches!(c.config.mode, Mode::Install | Mode::CliInstall) || c.restarted_from.is_some()
    })
}
pub fn start(shell: Handle) {
    if !active() {
        return;
    }
    std::thread::spawn(move || {
        let check = CHECK.get().unwrap();
        let initial = if check.config.mode == Mode::Cached && check.restarted_from.is_none() {
            Ok(true)
        } else {
            observe(&shell)
        };
        let mut result = initial.and_then(|_| {
            if check.restarted_from.is_none() && check.config.mode == Mode::Install {
                let install = (|| -> anyhow::Result<()> {
                    let context = shell
                        .update
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("fixture updater missing"))?;
                    let updater = context.updater()?;
                    let release = updater
                        .check()?
                        .ok_or_else(|| anyhow::anyhow!("fixture release missing"))?;
                    let prepared = updater.prepare_install(&release)?;
                    context.runtime.install_when(
                        true,
                        || true,
                        || updater.clear(),
                        || {
                            installed()?;
                            prepared.restart(&restart_args(&[]), Some(&context.instance))
                        },
                    )?;
                    shell.stop();
                    shell.event(Event::Quit);
                    Ok(())
                })();
                if let Some(expected) = &check.config.expected_error {
                    if !install
                        .as_ref()
                        .is_err_and(|e| format!("{e:#}").contains(expected))
                    {
                        return Err(format!("expected {expected}, got {install:?}"));
                    }
                    return observe(&shell);
                }
                install.map_err(|e| e.to_string())?;
                return Ok(false);
            }
            if check.config.mode == Mode::CliInstall {
                let marker = if check.restarted_from.is_some() {
                    "cli-confirmed.json"
                } else {
                    "installed.json"
                };
                let deadline = Instant::now() + Duration::from_secs(90);
                while !check.root.join(marker).exists() {
                    if Instant::now() >= deadline {
                        return Err(format!("CLI did not complete {marker}"));
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                if check.restarted_from.is_none() {
                    return Ok(false);
                }
            }
            if !matches!(check.config.mode, Mode::Startup | Mode::Interactive) {
                if check.restarted_from.is_none() && check.config.mode == Mode::Cached {
                    let deadline = Instant::now() + Duration::from_secs(90);
                    while !shell.stopped() && Instant::now() < deadline {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    if shell.stopped() {
                        return Ok(false);
                    }
                }
                let previous = check
                    .restarted_from
                    .ok_or("cached update did not restart")?;
                if previous == std::process::id() {
                    return Err("update reused old process".into());
                }
                for name in ["cache/updates/package", "cache/updates/metadata.json"] {
                    if check.root.join(name).exists() {
                        return Err("installed update left pending cache".into());
                    }
                }
            }
            if check
                .config
                .expected_version
                .as_deref()
                .is_some_and(|v| v != env!("CARGO_PKG_VERSION"))
            {
                return Err("unexpected running version".into());
            }
            Ok(true)
        });
        if matches!(result, Ok(false)) {
            return;
        }
        if check.config.mode != Mode::Interactive {
            shell.stop();
            // Drain existing requests and permanently reject new status polls
            // before shutting down the fixture Agent. Otherwise a late poll
            // can restart it between the shutdown and the window's Quit event.
            let cleanup = shell
                .gate
                .retire_with(|| Ok(()))
                .and_then(|()| shell.client().shutdown());
            if let Err(error) = cleanup {
                let cleanup_error = format!("fixture Agent cleanup failed: {error:#}");
                result = Err(match result {
                    Err(original) => format!("{original}; {cleanup_error}"),
                    _ => cleanup_error,
                });
            }
        }
        let report = serde_json::json!({"ok":result.is_ok(),"error":result.err(),"pid":std::process::id(),"restartedFrom":check.restarted_from,"version":env!("CARGO_PKG_VERSION")});
        let _ = atomic_write_bytes(
            check.root.join("result.json"),
            &serde_json::to_vec_pretty(&report).unwrap(),
        );
        if check.config.mode != Mode::Interactive {
            shell.event(Event::Quit);
        }
    });
}
fn observe(shell: &Handle) -> Result<bool, String> {
    let deadline = Instant::now() + Duration::from_secs(45);
    while !FRAME_READY.load(Ordering::Acquire) {
        if Instant::now() >= deadline {
            return Err("native view did not render within 45 seconds".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    for _ in 0..20 {
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        shell.event(Event::Probe(send));
        receive
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| "native event loop stopped responding")??;
        shell
            .request::<SessionStatus>(AgentRequest::SessionStatus)
            .map_err(|e| e.to_string())?;
        std::thread::sleep(Duration::from_millis(500));
    }
    crate::shell::logging::log_event("desktop.runtime_check.ready", &[])?;
    Ok(true)
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
