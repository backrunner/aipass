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

/// Detect supported agent CLI tools by looking for their binaries on `PATH`
/// and their config directories under the user's home directory.
pub fn detect_tools() -> Vec<ToolDetection> {
    let path_env = std::env::var_os("PATH");
    let home = home_dir();
    TOOLS
        .iter()
        .map(|tool| ToolDetection {
            tool: tool.clone(),
            binary_found: binary_on_path(path_env.as_deref(), binary_name(tool)),
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

fn binary_on_path(path_env: Option<&OsStr>, name: &str) -> bool {
    let Some(path_env) = path_env else {
        return false;
    };
    std::env::split_paths(path_env).any(|dir| {
        binary_candidates(name).iter().any(|candidate| {
            let full = dir.join(candidate);
            full.is_file()
        })
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
    let dir = match tool {
        ToolId::ClaudeCode => home.join(".claude"),
        ToolId::Codex => std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex")),
        ToolId::GeminiCli => home.join(".gemini"),
        ToolId::OpenCode => home.join(".config").join("opencode"),
    };
    dir.exists().then_some(dir)
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
        assert!(!binary_on_path(Some(&path_env), "claude"));

        let binary = if cfg!(windows) {
            "claude.exe"
        } else {
            "claude"
        };
        std::fs::write(bin_dir.join(binary), "#!/bin/sh\n").unwrap();
        assert!(binary_on_path(Some(&path_env), "claude"));
        assert!(!binary_on_path(Some(&path_env), "codex"));
        assert!(!binary_on_path(None, "claude"));
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
    fn detect_tools_covers_every_supported_tool() {
        let detections = detect_tools();
        assert_eq!(detections.len(), TOOLS.len());
        for tool in TOOLS {
            assert!(detections.iter().any(|detection| detection.tool == tool));
        }
    }
}
