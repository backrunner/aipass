use crate::models::ToolId;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolDetection {
    pub tool: ToolId,
    pub binary_found: bool,
    pub config_path: Option<PathBuf>,
}

const TOOLS: [ToolId; 4] = [
    ToolId::ClaudeCode,
    ToolId::Codex,
    ToolId::GeminiCli,
    ToolId::OpenCode,
];

/// Detect supported agent CLI tools by looking for their binaries and their
/// config directories under the user's home directory.
///
/// GUI apps on macOS run with a minimal PATH, so user-level installs such as
/// `~/.opencode/bin` or `~/.local/bin` are invisible there. The binary search
/// therefore also probes well-known install locations under the home
/// directory in addition to every PATH entry.
pub fn detect_tools() -> Vec<ToolDetection> {
    let path_env = std::env::var_os("PATH");
    let home = home_dir();
    let search_dirs = binary_search_dirs(path_env.as_deref(), home.as_deref());
    TOOLS
        .iter()
        .map(|tool| ToolDetection {
            tool: tool.clone(),
            binary_found: binary_in_dirs(&search_dirs, binary_name(tool)),
            config_path: home.as_deref().and_then(|home| config_dir(home, tool)),
        })
        .collect()
}

fn binary_name(tool: &ToolId) -> &'static str {
    match tool {
        ToolId::ClaudeCode => "claude",
        ToolId::Codex => "codex",
        ToolId::GeminiCli => "gemini",
        ToolId::OpenCode => "opencode",
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
            ".bun/bin",
            "Library/pnpm",
            ".npm-global/bin",
            ".volta/bin",
            ".asdf/shims",
            ".local/share/mise/shims",
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
    dirs
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
        vec![OsString::from(format!("{name}.exe"))]
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
    use tempfile::tempdir;

    #[test]
    fn binary_detection_scans_path_entries() {
        let dir = tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let path_env = std::env::join_paths([bin_dir.as_path()]).unwrap();
        let dirs = binary_search_dirs(Some(&path_env), None);
        assert!(!binary_in_dirs(&dirs, "claude"));

        let binary = if cfg!(windows) {
            "claude.exe"
        } else {
            "claude"
        };
        std::fs::write(bin_dir.join(binary), "#!/bin/sh\n").unwrap();
        assert!(binary_in_dirs(&dirs, "claude"));
        assert!(!binary_in_dirs(&dirs, "codex"));
        assert!(!binary_in_dirs(&binary_search_dirs(None, None), "claude"));
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

    #[test]
    fn config_dir_reports_existing_tool_directories() {
        let dir = tempdir().unwrap();
        assert_eq!(config_dir(dir.path(), &ToolId::ClaudeCode), None);

        let claude = dir.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        assert_eq!(config_dir(dir.path(), &ToolId::ClaudeCode), Some(claude));

        let opencode = dir.path().join(".config").join("opencode");
        std::fs::create_dir_all(&opencode).unwrap();
        assert_eq!(config_dir(dir.path(), &ToolId::OpenCode), Some(opencode));
        assert_eq!(config_dir(dir.path(), &ToolId::GeminiCli), None);
    }

    #[test]
    fn config_dir_falls_back_to_install_marks() {
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
        let detections = detect_tools();
        assert_eq!(detections.len(), TOOLS.len());
        for tool in TOOLS {
            assert!(detections.iter().any(|detection| detection.tool == tool));
        }
    }
}
