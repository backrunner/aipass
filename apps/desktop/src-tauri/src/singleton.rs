use aipass_agent_protocol::{read_frame, write_frame};
use anyhow::{Context, Result};
#[cfg(not(target_os = "windows"))]
use directories::ProjectDirs;
#[cfg(not(target_os = "windows"))]
use interprocess::local_socket::GenericFilePath;
#[cfg(target_os = "windows")]
use interprocess::local_socket::GenericNamespaced;
use interprocess::local_socket::{prelude::*, Listener, ListenerOptions, Stream};
#[cfg(any(target_os = "linux", target_os = "android", target_os = "freebsd"))]
use interprocess::os::unix::local_socket::ListenerOptionsExt;
use semver::Version;
use serde::{Deserialize, Serialize};
#[cfg(not(target_os = "windows"))]
use std::fs;
use std::io::{self, ErrorKind};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(not(target_os = "windows"))]
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};
use tauri::AppHandle;

const REPLACEMENT_BIND_TIMEOUT: Duration = Duration::from_secs(8);
const REPLACEMENT_BIND_INTERVAL: Duration = Duration::from_millis(100);
const REPLACEMENT_EXIT_DELAY: Duration = Duration::from_millis(150);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DesktopInstanceKind {
    Release,
    PackagedDevelopment,
    LiveDevelopment,
}

pub(crate) enum SingletonDecision {
    Run(DesktopSingleton),
    Exit,
}

pub(crate) struct DesktopSingleton {
    listener: Listener,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SingletonRequest {
    version: String,
    target: String,
    #[serde(default)]
    command: Option<SingletonCommand>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SingletonResponse {
    version: String,
    action: SingletonAction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum SingletonAction {
    UseExisting,
    ReplaceExisting,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum SingletonCommand {
    Quit,
}

pub(crate) fn acquire(current_version: &str, target: &str) -> Result<SingletonDecision> {
    let request = SingletonRequest {
        version: current_version.to_string(),
        target: target.to_string(),
        command: None,
    };

    if let Ok(decision) = request_existing_instance(&request, current_version) {
        return Ok(decision);
    }

    match listen_for_instances() {
        Ok(listener) => Ok(SingletonDecision::Run(DesktopSingleton { listener })),
        Err(err) if is_addr_in_use(&err) => request_existing_instance(&request, current_version),
        Err(err) => Err(err),
    }
}

pub(crate) fn spawn_server(app: AppHandle, singleton: DesktopSingleton, current_version: String) {
    thread::spawn(move || loop {
        match singleton.listener.accept() {
            Ok(stream) => {
                let app = app.clone();
                let current_version = current_version.clone();
                thread::spawn(move || {
                    if let Err(err) = handle_connection(stream, app, &current_version) {
                        eprintln!("desktop singleton request failed: {err}");
                    }
                });
            }
            Err(err) if err.kind() == ErrorKind::Interrupted => continue,
            Err(err) => {
                eprintln!("desktop singleton accept failed: {err}");
                thread::sleep(REPLACEMENT_BIND_INTERVAL);
            }
        }
    });
}

fn handle_connection(mut stream: Stream, app: AppHandle, current_version: &str) -> Result<()> {
    let request: SingletonRequest =
        read_frame(&mut stream).context("failed to read desktop singleton request")?;

    if request.command == Some(SingletonCommand::Quit) {
        write_frame(
            &mut stream,
            &SingletonResponse {
                version: current_version.to_string(),
                action: SingletonAction::UseExisting,
            },
        )
        .context("failed to send desktop singleton response")?;

        thread::spawn(move || {
            thread::sleep(REPLACEMENT_EXIT_DELAY);
            app.exit(0);
        });
        return Ok(());
    }

    let action = if incoming_version_replaces_current(&request.version, current_version) {
        SingletonAction::ReplaceExisting
    } else {
        SingletonAction::UseExisting
    };

    write_frame(
        &mut stream,
        &SingletonResponse {
            version: current_version.to_string(),
            action,
        },
    )
    .context("failed to send desktop singleton response")?;

    match action {
        SingletonAction::UseExisting => crate::activate_window_target(&app, &request.target),
        SingletonAction::ReplaceExisting => {
            thread::spawn(move || {
                thread::sleep(REPLACEMENT_EXIT_DELAY);
                app.exit(0);
            });
        }
    }

    Ok(())
}

fn request_existing_instance(
    request: &SingletonRequest,
    current_version: &str,
) -> Result<SingletonDecision> {
    let mut stream = connect_existing_instance()?;
    write_frame(&mut stream, request).context("failed to send desktop singleton request")?;
    let response: SingletonResponse =
        read_frame(&mut stream).context("failed to read desktop singleton response")?;

    match response.action {
        SingletonAction::UseExisting => {
            eprintln!(
                "AIPass desktop {} is already running; exiting this instance",
                response.version
            );
            Ok(SingletonDecision::Exit)
        }
        SingletonAction::ReplaceExisting => {
            eprintln!(
                "replacing older AIPass desktop {} with {}",
                response.version, current_version
            );
            Ok(SingletonDecision::Run(DesktopSingleton {
                listener: wait_for_replacement_listener()?,
            }))
        }
    }
}

fn incoming_version_replaces_current(incoming: &str, current: &str) -> bool {
    match (parse_version(incoming), parse_version(current)) {
        (Some(incoming), Some(current)) => incoming > current,
        _ => false,
    }
}

fn parse_version(value: &str) -> Option<Version> {
    Version::parse(value.trim().trim_start_matches('v')).ok()
}

fn wait_for_replacement_listener() -> Result<Listener> {
    let deadline = Instant::now() + REPLACEMENT_BIND_TIMEOUT;
    loop {
        match listen_for_instances() {
            Ok(listener) => return Ok(listener),
            Err(err) if Instant::now() < deadline => {
                let _ = err;
                thread::sleep(REPLACEMENT_BIND_INTERVAL);
            }
            Err(err) => return Err(err).context("older desktop instance did not exit in time"),
        }
    }
}

#[cfg(target_os = "windows")]
fn connect_existing_instance() -> Result<Stream> {
    let name =
        desktop_singleton_name(current_desktop_instance()).to_ns_name::<GenericNamespaced>()?;
    Ok(Stream::connect(name)?)
}

#[cfg(not(target_os = "windows"))]
fn connect_existing_instance() -> Result<Stream> {
    let name = singleton_socket_path()?.to_fs_name::<GenericFilePath>()?;
    Ok(Stream::connect(name)?)
}

#[cfg(target_os = "windows")]
fn listen_for_instances() -> Result<Listener> {
    let name =
        desktop_singleton_name(current_desktop_instance()).to_ns_name::<GenericNamespaced>()?;
    Ok(ListenerOptions::new().name(name).create_sync()?)
}

#[cfg(not(target_os = "windows"))]
fn listen_for_instances() -> Result<Listener> {
    let path = singleton_socket_path()?;
    listen_at_path(&path)
}

#[cfg(not(target_os = "windows"))]
fn listen_at_path(path: &PathBuf) -> Result<Listener> {
    match create_listener_at_path(path) {
        Ok(listener) => Ok(listener),
        Err(err) if err.kind() == ErrorKind::AddrInUse && stale_socket_path(path) => {
            let _ = fs::remove_file(path);
            Ok(create_listener_at_path(path)?)
        }
        Err(err) => Err(err.into()),
    }
}

#[cfg(not(target_os = "windows"))]
fn create_listener_at_path(path: &Path) -> io::Result<Listener> {
    let name = path.to_path_buf().to_fs_name::<GenericFilePath>()?;
    #[allow(unused_mut)]
    let mut options = ListenerOptions::new().name(name);
    #[cfg(any(target_os = "linux", target_os = "android", target_os = "freebsd"))]
    {
        options = options.mode(0o600);
    }
    let listener = options.create_sync()?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

#[cfg(not(target_os = "windows"))]
fn stale_socket_path(path: &Path) -> bool {
    let Ok(name) = path.to_path_buf().to_fs_name::<GenericFilePath>() else {
        return false;
    };
    Stream::connect(name).is_err()
}

fn is_addr_in_use(err: &anyhow::Error) -> bool {
    err.downcast_ref::<io::Error>()
        .is_some_and(|err| err.kind() == ErrorKind::AddrInUse)
}

#[cfg(not(target_os = "windows"))]
fn singleton_socket_path() -> Result<PathBuf> {
    Ok(desktop_runtime_dir()?.join(singleton_socket_name(current_desktop_instance())))
}

fn current_desktop_instance() -> DesktopInstanceKind {
    if !cfg!(debug_assertions) {
        DesktopInstanceKind::Release
    } else if cfg!(feature = "custom-protocol") {
        DesktopInstanceKind::PackagedDevelopment
    } else {
        DesktopInstanceKind::LiveDevelopment
    }
}

fn singleton_socket_name(instance: DesktopInstanceKind) -> &'static str {
    match instance {
        DesktopInstanceKind::Release => "desktop-tray.sock",
        DesktopInstanceKind::PackagedDevelopment => "desktop-dev-bundle.sock",
        DesktopInstanceKind::LiveDevelopment => "desktop-dev-server.sock",
    }
}

#[cfg(target_os = "windows")]
fn desktop_singleton_name(instance: DesktopInstanceKind) -> &'static str {
    match instance {
        DesktopInstanceKind::Release => "dev.aipass.desktop.tray",
        DesktopInstanceKind::PackagedDevelopment => "dev.aipass.desktop.dev-bundle",
        DesktopInstanceKind::LiveDevelopment => "dev.aipass.desktop.dev-server",
    }
}

#[cfg(not(target_os = "windows"))]
fn desktop_runtime_dir() -> Result<PathBuf> {
    let dirs =
        ProjectDirs::from("dev", "aipass", "desktop").context("cannot determine project dir")?;
    let dir = if let Some(explicit) = std::env::var_os("AIPASS_DESKTOP_RUNTIME_DIR") {
        PathBuf::from(explicit)
    } else {
        let root = if cfg!(target_os = "macos") {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .context("HOME is not set")?
                .join(".aipass")
                .join("run")
        } else {
            std::env::var_os("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| dirs.data_local_dir().join("runtime"))
        };
        root.join("aipass-desktop")
    };
    fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::{
        current_desktop_instance, incoming_version_replaces_current, singleton_socket_name,
        DesktopInstanceKind, SingletonCommand, SingletonRequest,
    };

    #[test]
    fn newer_semver_replaces_current() {
        assert!(incoming_version_replaces_current("1.2.4", "1.2.3"));
        assert!(incoming_version_replaces_current("2.0.0", "1.9.9"));
        assert!(incoming_version_replaces_current("1.0.0", "1.0.0-beta.1"));
    }

    #[test]
    fn same_or_older_semver_uses_existing() {
        assert!(!incoming_version_replaces_current("1.2.3", "1.2.3"));
        assert!(!incoming_version_replaces_current("1.2.3", "1.2.4"));
        assert!(!incoming_version_replaces_current("1.0.0-beta.1", "1.0.0"));
    }

    #[test]
    fn invalid_versions_do_not_replace_existing() {
        assert!(!incoming_version_replaces_current("nightly", "1.0.0"));
        assert!(!incoming_version_replaces_current("1.0.1", "dev"));
    }

    #[test]
    fn release_and_development_instances_use_distinct_sockets() {
        assert_eq!(
            singleton_socket_name(DesktopInstanceKind::Release),
            "desktop-tray.sock"
        );
        assert_eq!(
            singleton_socket_name(DesktopInstanceKind::PackagedDevelopment),
            "desktop-dev-bundle.sock"
        );
        assert_eq!(
            singleton_socket_name(DesktopInstanceKind::LiveDevelopment),
            "desktop-dev-server.sock"
        );
    }

    #[test]
    fn debug_build_uses_the_expected_development_instance() {
        let expected = if cfg!(feature = "custom-protocol") {
            DesktopInstanceKind::PackagedDevelopment
        } else {
            DesktopInstanceKind::LiveDevelopment
        };
        assert_eq!(current_desktop_instance(), expected);
    }

    #[test]
    fn singleton_request_accepts_optional_quit_command() {
        let legacy: SingletonRequest =
            serde_json::from_str(r#"{"version":"1.0.0","target":"tray"}"#).unwrap();
        assert_eq!(legacy.command, None);

        let quit: SingletonRequest =
            serde_json::from_str(r#"{"version":"0.0.0","target":"tray","command":"quit"}"#)
                .unwrap();
        assert_eq!(quit.command, Some(SingletonCommand::Quit));
    }
}
