use crate::models::ToolId;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolDetection {
    pub tool: ToolId,
    pub binary_found: bool,
    pub config_path: Option<PathBuf>,
}

const TOOLS: [ToolId; 7] = [
    ToolId::ClaudeCode,
    ToolId::Codex,
    ToolId::GeminiCli,
    ToolId::OpenCode,
    ToolId::Grok,
    ToolId::Pi,
    ToolId::Cursor,
];

/// Detect supported agent CLI tools by looking for their binaries and their
/// config directories under the user's home directory.
///
/// GUI apps on macOS run with a minimal PATH, so user-level installs such as
/// `~/.opencode/bin` or `~/.local/bin` are invisible there. The binary search
/// therefore resolves the PATH the user's login shell would see (the same
/// strategy tools like T3 Code use), falls back to `launchctl` on macOS, and
/// finally probes well-known install locations under the home directory.
pub fn detect_tools() -> Vec<ToolDetection> {
    let home = home_dir();
    let path_env = std::env::var_os("PATH");
    let mut search_dirs = login_shell_path_dirs();
    search_dirs.extend(binary_search_dirs(path_env.as_deref(), home.as_deref()));
    TOOLS
        .iter()
        .map(|tool| ToolDetection {
            tool: tool.clone(),
            binary_found: binary_names(tool)
                .iter()
                .any(|name| binary_in_dirs(&search_dirs, name)),
            config_path: home.as_deref().and_then(|home| config_dir(home, tool)),
        })
        .collect()
}

fn binary_names(tool: &ToolId) -> &'static [&'static str] {
    match tool {
        ToolId::ClaudeCode => &["claude"],
        ToolId::Codex => &["codex"],
        ToolId::GeminiCli => &["gemini"],
        ToolId::OpenCode => &["opencode"],
        ToolId::Grok => &["grok"],
        ToolId::Pi => &["pi"],
        ToolId::Cursor => &["agent", "cursor-agent", "cursor-agent-local"],
    }
}

fn binary_search_dirs(path_env: Option<&OsStr>, home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = path_env
        .map(|path_env| std::env::split_paths(path_env).collect())
        .unwrap_or_default();
    if let Some(home) = home {
        for rel in [
            ".local/bin",
            "bin",
            ".opencode/bin",
            ".grok/bin",
            ".pi/bin",
            ".bun/bin",
            "Library/pnpm",
            ".local/share/pnpm",
            ".npm-global/bin",
            ".volta/bin",
            ".asdf/shims",
            ".local/share/mise/shims",
            ".proto/shims",
            ".proto/bin",
        ] {
            dirs.push(home.join(rel));
        }
        // nvm keeps one bin directory per installed Node version.
        if let Ok(entries) = std::fs::read_dir(home.join(".nvm/versions/node")) {
            for entry in entries.flatten() {
                dirs.push(entry.path().join("bin"));
            }
        }
    }
    for system in ["/opt/homebrew/bin", "/usr/local/bin", "/snap/bin"] {
        dirs.push(PathBuf::from(system));
    }
    #[cfg(windows)]
    {
        // npm/pnpm/volta/scoop shims live outside the PATH a GUI app inherits.
        let env_dirs: [(&str, &str); 4] = [
            ("APPDATA", "npm"),
            ("LOCALAPPDATA", "Programs\\nodejs"),
            ("LOCALAPPDATA", "Volta\\bin"),
            ("LOCALAPPDATA", "pnpm"),
        ];
        for (var, rel) in env_dirs {
            if let Some(base) = std::env::var_os(var) {
                dirs.push(PathBuf::from(base).join(rel));
            }
        }
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            let profile = PathBuf::from(profile);
            for rel in [".local\\bin", ".bun\\bin", "scoop\\shims"] {
                dirs.push(profile.join(rel));
            }
        }
    }
    dirs
}

/// The PATH directories the user's login shell would see, resolved once per
/// process. Empty when the shell cannot be probed (e.g. Windows).
fn login_shell_path_dirs() -> Vec<PathBuf> {
    static CACHE: OnceLock<Vec<PathBuf>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            login_shell_path()
                .map(|path| std::env::split_paths(&path).collect())
                .unwrap_or_default()
        })
        .clone()
}

#[cfg(not(windows))]
fn login_shell_path() -> Option<OsString> {
    const START: &str = "__AIPASS_PATH_START__";
    const END: &str = "__AIPASS_PATH_END__";
    let script = format!("printf '{START}'; printenv PATH; printf '{END}'");

    let mut shells: Vec<PathBuf> = std::env::var_os("SHELL")
        .map(PathBuf::from)
        .into_iter()
        .collect();
    shells.push(PathBuf::from(if cfg!(target_os = "macos") {
        "/bin/zsh"
    } else {
        "/bin/bash"
    }));

    for shell in shells {
        let mut command = std::process::Command::new(shell);
        let output = run_command_with_timeout(
            command.args(["-ilc", &script]),
            std::time::Duration::from_secs(4),
        );
        if let Some(path) = output
            .filter(|output| output.status.success())
            .and_then(|output| parse_marked_path(&output.stdout, START, END))
        {
            return Some(path);
        }
    }

    #[cfg(target_os = "macos")]
    {
        let mut command = std::process::Command::new("/bin/launchctl");
        if let Some(output) = run_command_with_timeout(
            command.args(["getenv", "PATH"]),
            std::time::Duration::from_secs(2),
        ) {
            if output.status.success() {
                let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !value.is_empty() {
                    return Some(OsString::from(value));
                }
            }
        }
    }
    None
}

#[cfg(windows)]
fn login_shell_path() -> Option<OsString> {
    None
}

/// Extract the PATH value printed between two marker strings. Interactive
/// shells may echo prompts or profile output around the markers.
#[cfg(not(windows))]
fn parse_marked_path(stdout: &[u8], start: &str, end: &str) -> Option<OsString> {
    let text = String::from_utf8_lossy(stdout);
    let after_start = text.rsplit_once(start)?.1;
    let path = after_start.split_once(end)?.0.trim();
    if path.is_empty() {
        None
    } else {
        Some(OsString::from(path))
    }
}

/// Run a command capturing stdout, giving up after `timeout`. Shells sourced
/// by `-ilc` can block on profile scripts, so a hung child is killed.
#[cfg(not(windows))]
fn run_command_with_timeout(
    command: &mut std::process::Command,
    timeout: std::time::Duration,
) -> Option<std::process::Output> {
    use std::io::Read;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    command.stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = command.spawn().ok()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = Vec::new();
                child.stdout.take()?.read_to_end(&mut stdout).ok()?;
                return Some(std::process::Output {
                    status,
                    stdout,
                    stderr: Vec::new(),
                });
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Err(_) => return None,
        }
    }
}

fn binary_in_dirs(dirs: &[PathBuf], name: &str) -> bool {
    dirs.iter().any(|dir| {
        binary_candidates(name)
            .iter()
            .any(|candidate| dir.join(candidate).is_file())
    })
}

fn binary_candidates(name: &str) -> Vec<OsString> {
    if cfg!(windows) {
        // npm-installed CLIs are `.cmd`/`.bat` shims, so honor PATHEXT like
        // the Windows command resolver does.
        let pathext =
            std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
        let mut candidates: Vec<OsString> = pathext
            .split(';')
            .filter(|ext| !ext.is_empty())
            .map(|ext| OsString::from(format!("{name}{ext}")))
            .collect();
        candidates.push(OsString::from(name));
        candidates
    } else {
        vec![OsString::from(name)]
    }
}

fn config_dir(home: &Path, tool: &ToolId) -> Option<PathBuf> {
    match tool {
        ToolId::ClaudeCode => first_existing([
            home.join(".claude"),
            // Claude Code writes this file on first run, before any directory.
            home.join(".claude.json"),
        ]),
        ToolId::Codex => {
            let dir = std::env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".codex"));
            dir.exists().then_some(dir)
        }
        ToolId::GeminiCli => first_existing([home.join(".gemini")]),
        ToolId::OpenCode => {
            let mut candidates = Vec::new();
            // opencode honors XDG_CONFIG_HOME when it is set.
            if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
                candidates.push(PathBuf::from(xdg).join("opencode"));
            }
            candidates.push(home.join(".config").join("opencode"));
            // The official installer always creates ~/.opencode for the binary.
            candidates.push(home.join(".opencode"));
            first_existing(candidates)
        }
        ToolId::Grok => first_existing([home.join(".grok")]),
        ToolId::Pi => first_existing([home.join(".pi").join("agent"), home.join(".pi")]),
        ToolId::Cursor => first_existing([
            home.join(".cursor"),
            home.join(".config").join("cursor"),
            home.join(".local").join("share").join("cursor-agent"),
        ]),
    }
}

fn first_existing(paths: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    paths.into_iter().find(|path| path.exists())
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};
    use tempfile::tempdir;

    fn config_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn binary_detection_scans_path_entries() {
        let dir = tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let path_env = std::env::join_paths([bin_dir.as_path()]).unwrap();
        let dirs = binary_search_dirs(Some(&path_env), None);
        assert!(!binary_in_dirs(&dirs, "claude"));

        let binary = binary_candidates("claude").remove(0);
        std::fs::write(bin_dir.join(&binary), "#!/bin/sh\n").unwrap();
        assert!(binary_in_dirs(&dirs, "claude"));
        assert!(!binary_in_dirs(&dirs, "codex"));
        assert!(!binary_in_dirs(&binary_search_dirs(None, None), "claude"));
    }

    #[test]
    fn cursor_detection_accepts_primary_legacy_and_local_binary_names() {
        let dir = tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let dirs = vec![bin_dir.clone()];

        std::fs::write(
            bin_dir.join(binary_candidates("agent").remove(0)),
            "#!/bin/sh\n",
        )
        .unwrap();
        assert!(binary_names(&ToolId::Cursor)
            .iter()
            .any(|name| binary_in_dirs(&dirs, name)));

        std::fs::remove_file(bin_dir.join(binary_candidates("agent").remove(0))).unwrap();
        std::fs::write(
            bin_dir.join(binary_candidates("cursor-agent").remove(0)),
            "#!/bin/sh\n",
        )
        .unwrap();
        assert!(binary_names(&ToolId::Cursor)
            .iter()
            .any(|name| binary_in_dirs(&dirs, name)));

        std::fs::remove_file(bin_dir.join(binary_candidates("cursor-agent").remove(0))).unwrap();
        std::fs::write(
            bin_dir.join(binary_candidates("cursor-agent-local").remove(0)),
            "#!/bin/sh\n",
        )
        .unwrap();
        assert!(binary_names(&ToolId::Cursor)
            .iter()
            .any(|name| binary_in_dirs(&dirs, name)));
    }

    #[test]
    fn binary_detection_finds_well_known_user_install_dirs() {
        let dir = tempdir().unwrap();
        let home = dir.path();
        // GUI processes run with a PATH that lacks user-level install dirs.
        let gui_path = std::env::join_paths([Path::new("/usr/bin"), Path::new("/bin")]).unwrap();

        let opencode_bin = home.join(".opencode/bin");
        std::fs::create_dir_all(&opencode_bin).unwrap();
        std::fs::write(
            opencode_bin.join(binary_candidates("opencode").remove(0)),
            "#!/bin/sh\n",
        )
        .unwrap();

        let dirs = binary_search_dirs(Some(&gui_path), Some(home));
        assert!(binary_in_dirs(&dirs, "opencode"));
        assert!(!binary_in_dirs(&dirs, "gemini"));

        // nvm-managed Node versions expose their own bin directory.
        let nvm_bin = home.join(".nvm/versions/node/v22.0.0/bin");
        std::fs::create_dir_all(&nvm_bin).unwrap();
        std::fs::write(
            nvm_bin.join(binary_candidates("gemini").remove(0)),
            "#!/bin/sh\n",
        )
        .unwrap();

        let dirs = binary_search_dirs(Some(&gui_path), Some(home));
        assert!(binary_in_dirs(&dirs, "gemini"));
    }

    #[cfg(not(windows))]
    #[test]
    fn login_shell_path_parses_path_between_markers() {
        let output = b"shell profile noise\n__AIPASS_PATH_START__/opt/homebrew/bin:/usr/local/bin:/usr/bin\n__AIPASS_PATH_END__";
        assert_eq!(
            parse_marked_path(output, "__AIPASS_PATH_START__", "__AIPASS_PATH_END__"),
            Some(OsString::from("/opt/homebrew/bin:/usr/local/bin:/usr/bin"))
        );
        assert_eq!(
            parse_marked_path(
                b"no markers here",
                "__AIPASS_PATH_START__",
                "__AIPASS_PATH_END__"
            ),
            None
        );
        assert_eq!(
            parse_marked_path(
                b"__AIPASS_PATH_START____AIPASS_PATH_END__",
                "__AIPASS_PATH_START__",
                "__AIPASS_PATH_END__"
            ),
            None
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn login_shell_path_resolves_real_shell_path() {
        // The developer shell always has a PATH; this exercises the probe and
        // the marker parsing end to end.
        let path = login_shell_path().expect("login shell should yield a PATH");
        let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
        assert!(dirs.iter().any(|dir| dir == Path::new("/usr/bin")));
    }

    #[test]
    fn config_dir_reports_existing_tool_directories() {
        let _guard = config_env_lock().lock().unwrap();
        let dir = tempdir().unwrap();
        assert_eq!(config_dir(dir.path(), &ToolId::ClaudeCode), None);

        let claude = dir.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        assert_eq!(config_dir(dir.path(), &ToolId::ClaudeCode), Some(claude));

        let opencode = dir.path().join(".config").join("opencode");
        std::fs::create_dir_all(&opencode).unwrap();
        assert_eq!(config_dir(dir.path(), &ToolId::OpenCode), Some(opencode));
        assert_eq!(config_dir(dir.path(), &ToolId::GeminiCli), None);

        let grok = dir.path().join(".grok");
        std::fs::create_dir_all(&grok).unwrap();
        assert_eq!(config_dir(dir.path(), &ToolId::Grok), Some(grok));

        let pi = dir.path().join(".pi").join("agent");
        std::fs::create_dir_all(&pi).unwrap();
        assert_eq!(config_dir(dir.path(), &ToolId::Pi), Some(pi));
    }

    #[test]
    fn config_dir_falls_back_to_install_marks() {
        let _guard = config_env_lock().lock().unwrap();
        let dir = tempdir().unwrap();
        let home = dir.path();

        // Only the opencode install dir exists (fresh install, never launched).
        let install_dir = home.join(".opencode");
        std::fs::create_dir_all(install_dir.join("bin")).unwrap();
        assert_eq!(
            config_dir(home, &ToolId::OpenCode),
            Some(install_dir.clone())
        );

        // The XDG config dir wins once it exists.
        let xdg = home.join("xdg");
        let xdg_opencode = xdg.join("opencode");
        std::fs::create_dir_all(&xdg_opencode).unwrap();
        std::env::set_var("XDG_CONFIG_HOME", &xdg);
        let found = config_dir(home, &ToolId::OpenCode);
        std::env::remove_var("XDG_CONFIG_HOME");
        assert_eq!(found, Some(xdg_opencode));

        // Claude Code may only have written ~/.claude.json so far.
        let claude_json = home.join(".claude.json");
        std::fs::write(&claude_json, "{}").unwrap();
        assert_eq!(config_dir(home, &ToolId::ClaudeCode), Some(claude_json));
    }

    #[test]
    fn detect_tools_covers_every_supported_tool() {
        let _guard = config_env_lock().lock().unwrap();
        let detections = detect_tools();
        assert_eq!(detections.len(), TOOLS.len());
        for tool in TOOLS {
            assert!(detections.iter().any(|detection| detection.tool == tool));
        }
    }
}
