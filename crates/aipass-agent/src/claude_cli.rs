//! Vendor-owned Claude login and /usage. No Anthropic HTTP auth or usage transport.
use aipass_agent_protocol::{ClaudeCliStatus, ClaudeLoginStatus};
use aipass_provider_registry::SubscriptionWindow;
use chrono::{Datelike, TimeZone, Utc};
use serde_json::Value;
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};
use uuid::Uuid;
use zeroize::Zeroizing;

mod login;
use login::signed_in;
pub(crate) use login::{logins, NativeAccount};
mod usage;
use usage::parse_usage;
#[cfg(test)]
use usage::reset_time;

const OUTPUT_LIMIT: u64 = 256 * 1024;
const LOGIN_TTL: Duration = Duration::from_secs(900);
const OPENED_URL: &str = "AIPASS_CLAUDE_OPENED_URL";
const INSTALL_ERROR: &str = "Install or update Claude Code, then check again in AIPass";

fn candidates() -> Vec<PathBuf> {
    // Ordinary tests must never discover or probe the developer's installed
    // CLI or credential store. Tests pass explicit synthetic executables.
    #[cfg(test)]
    {
        Vec::new()
    }
    #[cfg(not(test))]
    {
        aipass_config_writers::cli_discovery::executable_candidates(&["claude"])
    }
}

/// Shared with the generation bridge; a non-executable file is not a CLI.
pub(crate) fn binary() -> Option<PathBuf> {
    available_from(&candidates()).1
}

pub(crate) fn configure(command: &mut Command, dir: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(crate::windows_process::CREATE_NO_WINDOW);
    }
    if let Some(path) =
        aipass_config_writers::cli_discovery::command_path(Path::new(command.get_program()))
    {
        command.env("PATH", path);
    }
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy();
        if name.starts_with("ANTHROPIC_")
            || name.starts_with("CLAUDE_CODE_")
            || matches!(
                name.as_ref(),
                "CLAUDECODE"
                    | "CLAUDE_CONFIG_DIR"
                    | "CLAUDE_SECURESTORAGE_CONFIG_DIR"
                    | "MAX_THINKING_TOKENS"
                    | "BROWSER"
                    | OPENED_URL
            )
        {
            command.env_remove(key);
        }
    }
    account_environment(command, dir);
    command
        .env("NO_COLOR", "1")
        .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")
        .current_dir(dir)
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
}

/// The default macOS account uses the unsuffixed Keychain service. Setting
/// CLAUDE_CONFIG_DIR alone would make the CLI look in a different, hashed store.
pub(crate) fn account_environment(command: &mut Command, dir: &Path) {
    command.env("CLAUDE_CONFIG_DIR", dir);
    command.env_remove("CLAUDE_SECURESTORAGE_CONFIG_DIR");
    if directories::BaseDirs::new().is_some_and(|base| dir == base.home_dir().join(".claude")) {
        // Explicitly naming the default config directory also moves .claude.json
        // inside it. Keep both profile and secure storage in their native places.
        command.env_remove("CLAUDE_CONFIG_DIR");
        #[cfg(target_os = "macos")]
        command.env("CLAUDE_SECURESTORAGE_CONFIG_DIR", "");
    }
}

fn stop(child: &mut Child) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    #[cfg(windows)]
    crate::windows_process::terminate_tree(child.id());
    #[cfg(unix)]
    if let Some(pid) = rustix::process::Pid::from_raw(child.id() as i32) {
        let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn output(command: &mut Command, timeout: Duration) -> Result<String, String> {
    let file = tempfile::tempfile().map_err(|_| "cannot capture Claude Code output")?;
    command
        .stdout(
            file.try_clone()
                .map_err(|_| "cannot capture Claude Code output")?,
        )
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|_| "Claude Code could not be started")?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        if file.metadata().map_or(true, |m| m.len() > OUTPUT_LIMIT) {
            stop(&mut child);
            return Err("Claude Code output exceeds limit".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                stop(&mut child);
                return Err("Claude Code timed out or could not be run".into());
            }
        }
    };
    if !status.success() {
        return Err("Claude Code command failed; update it or sign in again".into());
    }
    use std::io::{Seek, SeekFrom};
    let mut file = file;
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "cannot read Claude Code output")?;
    let mut bytes = Vec::new();
    file.take(OUTPUT_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read Claude Code output")?;
    if bytes.len() as u64 > OUTPUT_LIMIT {
        return Err("Claude Code output exceeds limit".into());
    }
    String::from_utf8(bytes).map_err(|_| "Claude Code returned invalid output".into())
}

pub(crate) struct ConfigDir {
    path: PathBuf,
    temporary: Option<tempfile::TempDir>,
}
impl ConfigDir {
    pub(crate) fn new() -> Result<Self, String> {
        tempfile::Builder::new()
            .prefix("aipass-claude-")
            .tempdir()
            .map(|temporary| Self {
                path: temporary.path().to_owned(),
                temporary: Some(temporary),
            })
            .map_err(|_| "cannot create private Claude configuration".into())
    }
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
    fn login() -> Result<Self, String> {
        #[cfg(test)]
        return Self::new();
        #[cfg(not(test))]
        {
            let base = directories::BaseDirs::new()
                .ok_or("home directory unavailable")?
                .home_dir()
                .join(".claude/aipass-accounts");
            std::fs::create_dir_all(&base)
                .map_err(|_| "cannot create Claude CLI account directory")?;
            let temporary = tempfile::Builder::new()
                .prefix("login-")
                .tempdir_in(&base)
                .map_err(|_| "cannot create Claude CLI account directory")?;
            Ok(Self {
                path: temporary.path().to_owned(),
                temporary: Some(temporary),
            })
        }
    }
    fn keep(&mut self) {
        if let Some(directory) = self.temporary.take() {
            self.path = directory.keep();
        }
        for name in [".login-output", ".opened-url"] {
            let _ = std::fs::remove_file(self.path.join(name));
        }
    }
}
impl Drop for ConfigDir {
    fn drop(&mut self) {
        if self.temporary.is_none() {
            return;
        }
        #[cfg(target_os = "macos")]
        {
            let mut command = Command::new("security");
            command.args([
                "delete-generic-password",
                "-s",
                &crate::claude_bridge::native_service(self.path()),
            ]);
            let _ = output(&mut command, Duration::from_secs(3));
        }
        let path = self.path().join(".credentials.json");
        if let Ok(meta) = path.metadata() {
            if meta.len() <= 1024 * 1024 {
                let _ = std::fs::write(path, vec![0; meta.len() as usize]);
            }
        }
    }
}

fn probe(path: &Path) -> Result<String, String> {
    let dir = ConfigDir::new()?;
    let mut command = Command::new(path);
    configure(&mut command, dir.path());
    command.arg("--version");
    let version = output(&mut command, Duration::from_secs(5))?;
    if !version.contains("(Claude Code)") || version.trim().len() > 80 {
        return Err("unsupported".into());
    }
    let mut command = Command::new(path);
    configure(&mut command, dir.path());
    command.args(["auth", "login", "--help"]);
    if !output(&mut command, Duration::from_secs(5))?.contains("--claudeai") {
        return Err("unsupported".into());
    }
    let mut command = Command::new(path);
    configure(&mut command, dir.path());
    command.arg("--help");
    let help = output(&mut command, Duration::from_secs(5))?;
    if ![
        "--no-session-persistence",
        "--setting-sources",
        "--strict-mcp-config",
    ]
    .iter()
    .all(|flag| help.contains(flag))
    {
        return Err("unsupported".into());
    }
    Ok(version.trim().to_owned())
}

fn available_from(paths: &[PathBuf]) -> (ClaudeCliStatus, Option<PathBuf>) {
    let mut reason = "missing";
    for path in paths {
        match probe(path) {
            Ok(version) => {
                return (
                    ClaudeCliStatus {
                        available: true,
                        reason: None,
                        version: Some(version),
                    },
                    Some(path.clone()),
                )
            }
            Err(error) => {
                reason = if error == "unsupported" {
                    "unsupported"
                } else {
                    "unusable"
                }
            }
        }
    }
    (
        ClaudeCliStatus {
            available: false,
            reason: Some(reason.into()),
            version: None,
        },
        None,
    )
}
pub(crate) fn status() -> ClaudeCliStatus {
    available_from(&candidates()).0
}

fn authorization_url(raw: &str) -> Option<url::Url> {
    let url = url::Url::parse(raw).ok()?;
    (url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && matches!(
            url.host_str(),
            Some("claude.ai" | "console.anthropic.com" | "platform.claude.com")
        ))
    .then_some(url)
}

/// Invoked as Claude Code's BROWSER before argument parsing or Agent startup.
pub fn capture_opened_url() -> bool {
    if let Some(path) = std::env::var_os("AIPASS_NATIVE_OPENED_URL") {
        let args = std::env::args().skip(1).collect::<Vec<_>>();
        if let [uri] = args.as_slice() {
            if let Ok(url) = url::Url::parse(uri) {
                if url.scheme() == "https"
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.port_or_known_default() == Some(443)
                    && matches!(
                        url.host_str(),
                        Some(
                            "auth.openai.com"
                                | "auth.x.ai"
                                | "grok.com"
                                | "github.com"
                                | "accounts.google.com"
                        )
                    )
                {
                    let _ = crate::claude_bridge::write_private(Path::new(&path), uri.as_bytes());
                }
            }
        }
        return true;
    }
    let Some(path) = std::env::var_os(OPENED_URL) else {
        return false;
    };
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if let [uri] = args.as_slice() {
        if authorization_url(uri).is_some() {
            let _ = crate::claude_bridge::write_private(Path::new(&path), uri.as_bytes());
        }
    }
    true
}

pub(crate) fn usage_native(
    path: &Path,
    outbound: &aipass_proxy::UpstreamProxyConfig,
) -> Result<Vec<SubscriptionWindow>, String> {
    let binary = binary().ok_or(INSTALL_ERROR)?;
    let mut command = Command::new(binary);
    configure(&mut command, path);
    if let Some(environment) = aipass_proxy::cli_proxy_environment(outbound) {
        for key in [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "NO_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
            "no_proxy",
        ] {
            command.env_remove(key);
        }
        command.envs(environment);
    }
    command.args([
        "-p",
        "/usage",
        "--tools",
        "",
        "--strict-mcp-config",
        "--mcp-config",
        "{\"mcpServers\":{}}",
        "--setting-sources",
        "",
        "--no-session-persistence",
    ]);
    parse_usage(&output(&mut command, Duration::from_secs(35))?, Utc::now())
}

pub(crate) fn local_account(path: &Path) -> Result<NativeAccount, String> {
    let binary = binary().ok_or(INSTALL_ERROR)?;
    signed_in(
        &binary,
        &ConfigDir {
            path: path.to_owned(),
            temporary: None,
        },
    )
}

#[cfg(test)]
mod tests;

#[cfg(all(test, unix))]
use login::LoginManager;
