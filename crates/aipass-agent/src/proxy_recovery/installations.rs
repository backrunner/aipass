//! Read-only discovery of vaults registered to this exact installation.
use anyhow::Result;
use std::path::{Path, PathBuf};
pub(super) fn vaults(agent: &Path, home: &Path) -> Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    #[cfg(target_os = "macos")]
    {
        let folder = home.join(".aipass/autostart");
        if folder.exists() {
            let expected = format!(
                "AGENT={}",
                crate::autostart::shell_quote(&agent.display().to_string())
            );
            for entry in std::fs::read_dir(folder)? {
                let path = entry?.path();
                if path.extension().is_none_or(|e| e != "sh") {
                    continue;
                }
                let raw = std::fs::read_to_string(path)?;
                if !raw.lines().any(|l| l == expected) {
                    continue;
                }
                if let Some(value) = raw.lines().find_map(|l| l.strip_prefix("VAULT=")) {
                    let vault = unquote(value)?;
                    result.push(crate::canonical_vault_dir(vault)?);
                }
            }
        }
    }
    #[cfg(windows)]
    {
        result.extend(crate::autostart::owned_windows_vaults(agent)?);
    }
    let _ = (agent, home);
    let default = crate::default_vault_dir()?;
    if default.join("manifest.aipmanifest").is_file() {
        result.push(crate::canonical_vault_dir(default)?);
    }
    result.sort();
    result.dedup();
    Ok(result)
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn unquote(value: &str) -> Result<PathBuf> {
    let raw = value
        .strip_prefix('\'')
        .and_then(|v| v.strip_suffix('\''))
        .ok_or_else(|| anyhow::anyhow!("invalid registered vault path"))?;
    let decoded = raw.replace("'\\''", "'");
    anyhow::ensure!(
        crate::autostart::shell_quote(&decoded) == value,
        "invalid registered vault quoting"
    );
    let path = PathBuf::from(decoded);
    anyhow::ensure!(path.is_absolute(), "registered vault path is not absolute");
    Ok(path)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn registered_paths_are_decoded_literally_without_executing_shell_text() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("中文 ' $(touch secret) `command` & space");
        let quoted = crate::autostart::shell_quote(path.to_str().unwrap());
        assert_eq!(unquote(&quoted).unwrap(), path);
        assert!(unquote("'relative'").is_err());
        assert!(unquote("'safe'; touch /tmp/oops").is_err());
    }
}
