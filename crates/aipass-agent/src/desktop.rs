use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const WINDOW_TARGET_ENV: &str = "AIPASS_WINDOW_TARGET";
pub const VAULT_DIR_ENV: &str = "AIPASS_VAULT_DIR";
pub const SUPPRESS_TRAY_ENV: &str = "AIPASS_AGENT_SUPPRESS_TRAY";
pub const TRAY_WINDOW_TARGET: &str = "tray";
pub const RELEASE_DEEP_LINK_SCHEME: &str = "aipass";
pub const DEVELOPMENT_DEEP_LINK_SCHEME: &str = "aipass-dev";

pub fn open_desktop_window(target: &str, vault_dir: &Path) -> Result<()> {
    if should_open_desktop_url(target, vault_dir) && open_desktop_url(target).is_ok() {
        return Ok(());
    }

    // This path preserves the launch target and custom vault environment.
    Command::new(desktop_binary())
        .env(WINDOW_TARGET_ENV, target)
        .env(VAULT_DIR_ENV, vault_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to open desktop companion")?;
    Ok(())
}

fn should_open_desktop_url(target: &str, vault_dir: &Path) -> bool {
    if target == TRAY_WINDOW_TARGET {
        return false;
    }
    let Ok(default_vault_dir) = crate::paths::default_vault_dir() else {
        return false;
    };
    match (
        crate::paths::canonical_vault_dir(default_vault_dir),
        crate::paths::canonical_vault_dir(vault_dir),
    ) {
        (Ok(default_vault_dir), Ok(vault_dir)) => default_vault_dir == vault_dir,
        _ => false,
    }
}

fn open_desktop_url(target: &str) -> Result<()> {
    let target = match target {
        "main" | "unlock" | "quick-access" | "server" | "tray" => target,
        _ => "main",
    };
    let url = format!("{}://launch/{target}", desktop_deep_link_scheme());
    let mut command = if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg(&url);
        command
    } else if cfg!(target_os = "windows") {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", &url]);
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(&url);
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let child = command.spawn().context("failed to open AIPass URL")?;
    wait_for_url_opener(child)
}

/// How long to let the URL opener prove it failed before assuming it worked.
const URL_OPENER_VERDICT_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1_500);
const URL_OPENER_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(25);

/// A spawned `open`/`xdg-open`/`start` says nothing about whether the deep link
/// resolved: with no handler registered the helper still spawns fine and then
/// exits non-zero. Waiting briefly for that exit is what lets the caller fall
/// back to launching the binary directly instead of silently opening nothing.
///
/// A helper still running after the timeout has handed the URL off, so it is
/// treated as success rather than blocking the caller any longer.
fn wait_for_url_opener(mut child: std::process::Child) -> Result<()> {
    let deadline = std::time::Instant::now() + URL_OPENER_VERDICT_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => anyhow::bail!("URL opener exited with {status}"),
            Ok(None) if std::time::Instant::now() >= deadline => return Ok(()),
            Ok(None) => std::thread::sleep(URL_OPENER_POLL_INTERVAL),
            Err(err) => return Err(err).context("failed to wait for the AIPass URL opener"),
        }
    }
}

fn desktop_deep_link_scheme() -> &'static str {
    if cfg!(debug_assertions) {
        DEVELOPMENT_DEEP_LINK_SCHEME
    } else {
        RELEASE_DEEP_LINK_SCHEME
    }
}

pub fn tray_launch_suppressed() -> bool {
    std::env::var_os(SUPPRESS_TRAY_ENV).is_some_and(|value| value != "0")
}

fn desktop_binary() -> PathBuf {
    desktop_binary_candidates()
        .into_iter()
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| PathBuf::from(desktop_binary_names()[0]))
}

fn desktop_binary_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        for name in desktop_binary_names() {
            push_unique(&mut candidates, exe.with_file_name(name));
        }
        #[cfg(target_os = "macos")]
        if let Some(resources_dir) = exe.parent() {
            if resources_dir
                .file_name()
                .is_some_and(|name| name == "Resources")
            {
                if let Some(contents_dir) = resources_dir.parent() {
                    for name in desktop_binary_names() {
                        push_unique(&mut candidates, contents_dir.join("MacOS").join(name));
                    }
                }
            }
        }
    }
    for name in desktop_binary_names() {
        push_unique(&mut candidates, PathBuf::from(name));
    }
    candidates
}

fn desktop_binary_names() -> &'static [&'static str] {
    if cfg!(target_os = "windows") {
        &["aipass-desktop.exe", "AIPass.exe"]
    } else if cfg!(target_os = "macos") {
        &["aipass-desktop", "AIPass"]
    } else {
        &["aipass-desktop"]
    }
}

fn push_unique(candidates: &mut Vec<PathBuf>, path: PathBuf) {
    if !candidates.iter().any(|candidate| candidate == &path) {
        candidates.push(path);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        desktop_deep_link_scheme, wait_for_url_opener, DEVELOPMENT_DEEP_LINK_SCHEME,
        URL_OPENER_VERDICT_TIMEOUT,
    };
    use std::process::{Command, Stdio};

    #[test]
    fn debug_builds_use_the_development_deep_link_scheme() {
        assert_eq!(desktop_deep_link_scheme(), DEVELOPMENT_DEEP_LINK_SCHEME);
    }

    fn spawn(program: &str, args: &[&str]) -> std::process::Child {
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn helper")
    }

    /// A URL opener that exits non-zero means no handler took the deep link,
    /// so the caller must fall back to launching the binary directly.
    #[cfg(unix)]
    #[test]
    fn failed_url_opener_is_reported_as_an_error() {
        let child = spawn("sh", &["-c", "exit 1"]);
        assert!(wait_for_url_opener(child).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn successful_url_opener_is_reported_as_success() {
        let child = spawn("sh", &["-c", "exit 0"]);
        assert!(wait_for_url_opener(child).is_ok());
    }

    /// A helper that keeps running has already handed the URL off; the caller
    /// must not be blocked waiting for it to exit.
    #[cfg(unix)]
    #[test]
    fn slow_url_opener_is_not_waited_out() {
        let child = spawn("sleep", &["30"]);
        let pid = child.id();
        let started = std::time::Instant::now();
        let result = wait_for_url_opener(child);
        let elapsed = started.elapsed();
        // The helper outlives the wait by design, so reap it here rather than
        // leaving a stray process behind for the rest of the run.
        let _ = Command::new("kill").arg(pid.to_string()).status();

        assert!(result.is_ok());
        assert!(elapsed >= URL_OPENER_VERDICT_TIMEOUT);
        assert!(
            elapsed < URL_OPENER_VERDICT_TIMEOUT * 3,
            "elapsed {elapsed:?}"
        );
    }
}
