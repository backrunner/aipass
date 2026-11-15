//! Installed helper discovery shared by desktop, CLI and native messaging.
use std::path::{Path, PathBuf};

pub fn helper_candidates(executable: &Path, name: &str) -> Vec<PathBuf> {
    let mut candidates = vec![executable.with_file_name(name)];
    #[cfg(target_os = "linux")]
    if std::env::var_os("APPIMAGE").is_some() {
        if let Some(home) = std::env::var_os("HOME") {
            // AppImage mounts are transient. Desktop installation refreshes
            // these owner-installed helpers before starting Agent/Native Host.
            candidates.insert(0, PathBuf::from(home).join(".local/lib/AIPass").join(name));
        }
    }
    if let Some(directory) = executable.parent() {
        candidates.push(directory.join("resources").join(name));
        candidates.push(directory.join("Resources").join(name));
        if let Some(parent) = directory.parent() {
            candidates.push(parent.join("Resources").join(name));
            candidates.push(parent.join("resources").join(name));
            // Native deb/rpm and AppImage resources live beside usr/bin in
            // usr/lib/<product name>, not beside the desktop executable.
            candidates.push(parent.join("lib/AIPass").join(name));
        }
    }
    #[cfg(target_os = "linux")]
    candidates.push(Path::new("/usr/lib/AIPass").join(name));
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packaged_helpers_cover_macos_linux_and_appimage_layouts() {
        for (executable, resource) in [
            (
                "/Applications/AIPass.app/Contents/MacOS/aipass-desktop",
                "/Applications/AIPass.app/Contents/Resources/aipass-agent",
            ),
            ("/usr/bin/aipass-desktop", "/usr/lib/AIPass/aipass-agent"),
            (
                "/tmp/.mount_AIPass/usr/bin/aipass-desktop",
                "/tmp/.mount_AIPass/usr/lib/AIPass/aipass-agent",
            ),
            ("/usr/lib/AIPass/aipass", "/usr/lib/AIPass/aipass-agent"),
        ] {
            assert!(helper_candidates(Path::new(executable), "aipass-agent")
                .contains(&PathBuf::from(resource)));
        }
    }
}
