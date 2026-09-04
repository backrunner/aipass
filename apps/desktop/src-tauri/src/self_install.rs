//! macOS self-installation: when the app is launched straight from the
//! installer DMG (including via Gatekeeper App Translocation), copy the
//! bundle into /Applications and relaunch from there, so double-clicking
//! the app icon in the installer window is enough to install.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::logging;

const APPLICATIONS_DIR: &str = "/Applications";

/// Returns true when a moved copy was relaunched from /Applications and the
/// current process must exit instead of continuing startup.
pub(crate) fn install_from_dmg_if_needed() -> bool {
    if cfg!(debug_assertions) {
        return false;
    }
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let Some(bundle) = app_bundle_for_exe(&exe) else {
        return false;
    };
    if bundle.parent() == Some(Path::new(APPLICATIONS_DIR))
        || !is_transient_install_location(&bundle)
    {
        return false;
    }
    let Some(name) = bundle.file_name() else {
        return false;
    };
    let target = Path::new(APPLICATIONS_DIR).join(name);
    let from = bundle.to_string_lossy().into_owned();
    let to = target.to_string_lossy().into_owned();
    let _ = logging::log_event(
        "desktop.self_install.begin",
        &[("from", &from), ("to", &to)],
    );
    match copy_bundle(&bundle, &target) {
        Ok(()) => {
            strip_quarantine(&target);
            if relaunch(&target) {
                let _ = logging::log_event("desktop.self_install.relaunch", &[("to", &to)]);
                true
            } else {
                let err = format!("failed to relaunch from {}", target.display());
                let _ = logging::log_event("desktop.self_install.relaunch_failed", &[("to", &to)]);
                show_install_failure_alert(&err);
                false
            }
        }
        Err(err) => {
            let _ = logging::log_event("desktop.self_install.failed", &[("error", &err)]);
            show_install_failure_alert(&err);
            false
        }
    }
}

fn app_bundle_for_exe(exe: &Path) -> Option<PathBuf> {
    exe.ancestors()
        .find(|path| path.extension() == Some(OsStr::new("app")))
        .map(Path::to_path_buf)
}

fn is_transient_install_location(bundle: &Path) -> bool {
    let path = bundle.to_string_lossy();
    path.starts_with("/Volumes/") || path.contains("/AppTranslocation/")
}

fn copy_bundle(source: &Path, target: &Path) -> Result<(), String> {
    if let Ok(metadata) = fs::symlink_metadata(target) {
        let removed = if metadata.file_type().is_symlink() {
            fs::remove_file(target)
        } else {
            fs::remove_dir_all(target)
        };
        removed.map_err(|err| format!("failed to replace {}: {err}", target.display()))?;
    }
    // ditto preserves code signatures, permissions, and extended attributes.
    let status = Command::new("ditto")
        .arg(source)
        .arg(target)
        .status()
        .map_err(|err| format!("failed to run ditto: {err}"))?;
    if !status.success() {
        return Err(format!("ditto exited with {status}"));
    }
    Ok(())
}

fn strip_quarantine(target: &Path) {
    let _ = Command::new("xattr")
        .args(["-dr", "com.apple.quarantine"])
        .arg(target)
        .status();
}

fn relaunch(target: &Path) -> bool {
    Command::new("open")
        .arg("-n")
        .arg(target)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn show_install_failure_alert(reason: &str) {
    let reason = reason.replace(['\\', '"'], " ");
    let message = if system_locale_is_chinese() {
        format!(
            "无法将 AIPass 移动到「应用程序」文件夹。\\n\\n{reason}\\n\\n你可以在安装窗口中将 AIPass 拖移到「应用程序」文件夹来完成安装。"
        )
    } else {
        format!(
            "AIPass could not move itself to the Applications folder.\\n\\n{reason}\\n\\nYou can finish installing by dragging AIPass onto the Applications folder in the installer window."
        )
    };
    let script = format!(
        "display dialog \"{message}\" buttons {{\"OK\"}} default button \"OK\" with icon caution with title \"AIPass\""
    );
    let _ = Command::new("osascript").args(["-e", &script]).status();
}

fn system_locale_is_chinese() -> bool {
    Command::new("osascript")
        .args(["-e", "user locale of (system info)"])
        .output()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .trim()
                .starts_with("zh")
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_app_bundle_ancestor() {
        let exe = Path::new("/Volumes/AIPass/AIPass.app/Contents/MacOS/aipass-desktop");
        assert_eq!(
            app_bundle_for_exe(exe).as_deref(),
            Some(Path::new("/Volumes/AIPass/AIPass.app"))
        );
        assert!(app_bundle_for_exe(Path::new("/usr/local/bin/aipass")).is_none());
    }

    #[test]
    fn detects_transient_install_locations() {
        assert!(is_transient_install_location(Path::new(
            "/Volumes/AIPass/AIPass.app"
        )));
        assert!(is_transient_install_location(Path::new(
            "/private/var/folders/ab/cd/AppTranslocation/EF012345/d/AIPass.app"
        )));
        assert!(!is_transient_install_location(Path::new(
            "/Applications/AIPass.app"
        )));
        assert!(!is_transient_install_location(Path::new(
            "/Users/alice/Downloads/AIPass.app"
        )));
    }
}
