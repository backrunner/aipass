use super::{Event, Handle};
use aipass_agent::ipc::framing::{read_frame_with_deadline, write_frame_with_deadline};
#[cfg(windows)]
use aipass_agent::ipc::PrivateListener as Listener;
use anyhow::{Context, Result};
#[cfg(not(target_os = "windows"))]
use interprocess::local_socket::GenericFilePath;
#[cfg(target_os = "windows")]
use interprocess::local_socket::GenericNamespaced;
#[cfg(not(windows))]
use interprocess::local_socket::Listener;
#[cfg(not(windows))]
use interprocess::local_socket::ListenerOptions;
use interprocess::local_socket::{prelude::*, Stream};
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

const REPLACEMENT_BIND_TIMEOUT: Duration = Duration::from_secs(8);
const REPLACEMENT_BIND_INTERVAL: Duration = Duration::from_millis(100);
const REPLACEMENT_EXIT_DELAY: Duration = Duration::from_millis(150);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DesktopInstanceKind {
    Release,
    PackagedDevelopment,
    LiveDevelopment,
}

pub enum SingletonDecision {
    Run(DesktopSingleton),
    Exit,
}

pub struct DesktopSingleton {
    listener: Listener,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SingletonRequest {
    version: String,
    target: String,
    #[serde(default)]
    command: Option<SingletonCommand>,
    #[serde(default)]
    deep_link_urls: Vec<aipass_agent_protocol::SensitiveString>,
    #[serde(default)]
    update: Option<aipass_agent_protocol::AuthenticatedDesktopUpdate>,
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
    Status,
    Update,
}

pub fn acquire(
    current_version: &str,
    target: &str,
    deep_link_urls: Vec<aipass_agent_protocol::SensitiveString>,
) -> Result<SingletonDecision> {
    let request = SingletonRequest {
        version: current_version.to_string(),
        target: target.to_string(),
        command: None,
        deep_link_urls,
        update: None,
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

/// Return deep-link arguments that need forwarding when another desktop instance owns the socket.
pub fn spawn_server(app: Handle, singleton: DesktopSingleton, current_version: String) {
    #[cfg(not(windows))]
    let _ = singleton
        .listener
        .set_nonblocking(interprocess::local_socket::ListenerNonblockingMode::Accept);
    thread::spawn(move || {
        while !app.stopped() {
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
                Err(err)
                    if matches!(err.kind(), ErrorKind::Interrupted | ErrorKind::WouldBlock) =>
                {
                    thread::sleep(Duration::from_millis(50));
                    continue;
                }
                Err(err) => {
                    eprintln!("desktop singleton accept failed: {err}");
                    thread::sleep(REPLACEMENT_BIND_INTERVAL);
                }
            }
        }
    });
}

fn handle_connection(mut stream: Stream, app: Handle, current_version: &str) -> Result<()> {
    stream.set_nonblocking(true)?;
    let request: SingletonRequest = read_frame_with_deadline(&mut stream, Duration::from_secs(5))
        .context("failed to read desktop singleton request")?;

    anyhow::ensure!(
        request.deep_link_urls.len() <= crate::provider_links::QUEUE_LIMIT,
        "too many provider links"
    );
    if request.command == Some(SingletonCommand::Update) {
        return crate::updates::control::handle(stream, app, request.update);
    }

    if matches!(
        request.command,
        Some(SingletonCommand::Quit | SingletonCommand::Status)
    ) {
        write_frame_with_deadline(
            &mut stream,
            &SingletonResponse {
                version: current_version.to_string(),
                action: SingletonAction::UseExisting,
            },
            Duration::from_secs(5),
        )
        .context("failed to send desktop singleton response")?;

        if request.command == Some(SingletonCommand::Quit) {
            thread::spawn(move || {
                thread::sleep(REPLACEMENT_EXIT_DELAY);
                app.stop();
                app.event(Event::Quit);
            });
        }
        return Ok(());
    }

    let action = if incoming_version_replaces_current(&request.version, current_version) {
        SingletonAction::ReplaceExisting
    } else {
        SingletonAction::UseExisting
    };

    write_frame_with_deadline(
        &mut stream,
        &SingletonResponse {
            version: current_version.to_string(),
            action,
        },
        Duration::from_secs(5),
    )
    .context("failed to send desktop singleton response")?;

    match action {
        SingletonAction::UseExisting => {
            // An Agent/login companion launch must not hide a window that the
            // user already opened in the resident desktop.
            if request.target != "tray" {
                app.event(Event::Activate(request.target.clone()));
            }
            let links = request
                .deep_link_urls
                .into_iter()
                .filter_map(|value| crate::provider_links::parse(value.expose().to_owned()))
                .take(crate::provider_links::QUEUE_LIMIT)
                .collect();
            app.event(Event::Links(links));
        }
        SingletonAction::ReplaceExisting => {
            thread::spawn(move || {
                thread::sleep(REPLACEMENT_EXIT_DELAY);
                app.stop();
                app.event(Event::Quit);
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
    stream.set_nonblocking(true)?;
    write_frame_with_deadline(&mut stream, request, Duration::from_secs(5))
        .context("failed to send desktop singleton request")?;
    let response: SingletonResponse = read_frame_with_deadline(&mut stream, Duration::from_secs(5))
        .context("failed to read desktop singleton response")?;

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
        desktop_singleton_name(current_desktop_instance())?.to_ns_name::<GenericNamespaced>()?;
    let stream = Stream::connect(name)?;
    aipass_agent::ipc::verify_pipe_owner(&stream)?;
    Ok(stream)
}

#[cfg(not(target_os = "windows"))]
fn connect_existing_instance() -> Result<Stream> {
    let name = singleton_socket_path()?.to_fs_name::<GenericFilePath>()?;
    Ok(Stream::connect(name)?)
}

#[cfg(target_os = "windows")]
fn listen_for_instances() -> Result<Listener> {
    let claim = desktop_singleton_name(current_desktop_instance())?;
    let name = claim.as_str().to_ns_name::<GenericNamespaced>()?;
    aipass_agent::ipc::private_pipe_listener(name, &claim)
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

#[cfg(not(windows))]
pub fn socket_path() -> Result<PathBuf> {
    singleton_socket_path()
}

fn current_desktop_instance() -> DesktopInstanceKind {
    if !cfg!(debug_assertions) {
        DesktopInstanceKind::Release
    } else if cfg!(feature = "packaged") {
        DesktopInstanceKind::PackagedDevelopment
    } else {
        DesktopInstanceKind::LiveDevelopment
    }
}

#[cfg(any(not(windows), test))]
fn singleton_socket_name(instance: DesktopInstanceKind) -> &'static str {
    match instance {
        DesktopInstanceKind::Release => "desktop-tray.sock",
        DesktopInstanceKind::PackagedDevelopment => "desktop-dev-bundle.sock",
        DesktopInstanceKind::LiveDevelopment => "desktop-dev-server.sock",
    }
}

pub fn should_install_tray_autostart() -> bool {
    should_install_tray_autostart_for(current_desktop_instance())
}

fn should_install_tray_autostart_for(instance: DesktopInstanceKind) -> bool {
    // Packaged builds (release and packaged development) register the tray for
    // auto-start on launch; only the live dev server stays unregistered.
    instance == DesktopInstanceKind::Release
        || (!cfg!(windows) && instance == DesktopInstanceKind::PackagedDevelopment)
}

#[cfg(target_os = "windows")]
fn desktop_singleton_name(instance: DesktopInstanceKind) -> Result<String> {
    let kind = match instance {
        DesktopInstanceKind::Release => "dev.aipass.desktop.tray",
        DesktopInstanceKind::PackagedDevelopment => "dev.aipass.desktop.dev-bundle",
        DesktopInstanceKind::LiveDevelopment => "dev.aipass.desktop.dev-server",
    };
    aipass_agent::desktop::windows_singleton_name(kind)
}

#[cfg(not(target_os = "windows"))]
fn desktop_runtime_dir() -> Result<PathBuf> {
    let kind = match current_desktop_instance() {
        DesktopInstanceKind::Release => "release",
        DesktopInstanceKind::PackagedDevelopment => "packaged-development",
        DesktopInstanceKind::LiveDevelopment => "development",
    };
    let path = aipass_agent::updates::desktop::socket_path(kind)?;
    let dir = path
        .parent()
        .context("desktop runtime path has no parent")?
        .to_path_buf();
    fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    Ok(dir)
}

/// A verified update child shares its parent's lease while the parent checks readiness.
pub fn spawn_replacement(app: Handle, version: String) {
    thread::spawn(move || match wait_for_replacement_listener() {
        Ok(listener) => spawn_server(app, DesktopSingleton { listener }, version),
        Err(_) => {
            app.stop();
            app.event(Event::Quit);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{
        current_desktop_instance, incoming_version_replaces_current,
        should_install_tray_autostart_for, singleton_socket_name, DesktopInstanceKind,
        SingletonCommand, SingletonRequest,
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
        let expected = if cfg!(feature = "packaged") {
            DesktopInstanceKind::PackagedDevelopment
        } else {
            DesktopInstanceKind::LiveDevelopment
        };
        assert_eq!(current_desktop_instance(), expected);
    }

    #[test]
    fn packaged_instances_register_tray_autostart() {
        assert!(should_install_tray_autostart_for(
            DesktopInstanceKind::Release
        ));
        assert_eq!(
            should_install_tray_autostart_for(DesktopInstanceKind::PackagedDevelopment),
            !cfg!(windows)
        );
        assert!(!should_install_tray_autostart_for(
            DesktopInstanceKind::LiveDevelopment
        ));
    }

    #[test]
    fn singleton_request_accepts_optional_quit_command() {
        let legacy: SingletonRequest =
            serde_json::from_str(r#"{"version":"1.0.0","target":"tray"}"#).unwrap();
        assert_eq!(legacy.command, None);

        let status: SingletonRequest =
            serde_json::from_str(r#"{"version":"0.0.0","target":"tray","command":"status"}"#)
                .unwrap();
        assert_eq!(status.command, Some(SingletonCommand::Status));

        let quit: SingletonRequest =
            serde_json::from_str(r#"{"version":"0.0.0","target":"tray","command":"quit"}"#)
                .unwrap();
        assert_eq!(quit.command, Some(SingletonCommand::Quit));
    }
}
