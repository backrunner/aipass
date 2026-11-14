use super::{Config, Release};
use anyhow::{ensure, Context, Result};
use std::{
    fs,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub struct Prepared {
    staging: tempfile::TempDir,
    candidate: PathBuf,
    destination: PathBuf,
    executable: String,
}

impl Prepared {
    pub(super) fn new(config: &Config, release: &Release, bytes: &[u8]) -> Result<Self> {
        ensure!(cfg!(target_os = "macos"), "update_installation_unsupported");
        let destination = fs::canonicalize(&config.app_path)?;
        ensure!(
            destination.extension().is_some_and(|ext| ext == "app"),
            "update_package_invalid"
        );
        let parent = destination.parent().context("missing application parent")?;
        // Same filesystem as the installed app, so replacement can be atomic.
        let staging = tempfile::Builder::new()
            .prefix(".aipass-update-")
            .tempdir_in(parent)?;
        let candidate = extract(bytes, staging.path())?;
        let old = plist::Value::from_file(destination.join("Contents/Info.plist"))?;
        let new = plist::Value::from_file(candidate.join("Contents/Info.plist"))?;
        let string = |value: &plist::Value, key: &str| -> Result<String> {
            value
                .as_dictionary()
                .and_then(|map| map.get(key))
                .and_then(plist::Value::as_string)
                .map(str::to_owned)
                .with_context(|| format!("missing bundle field {key}"))
        };
        ensure!(
            string(&old, "CFBundleIdentifier")? == config.identifier
                && string(&new, "CFBundleIdentifier")? == config.identifier,
            "update_package_invalid: bundle identity"
        );
        ensure!(
            string(&new, "CFBundleShortVersionString")? == release.version.to_string(),
            "update_package_invalid: version"
        );
        ensure!(
            string(&old, "CFBundleShortVersionString")? == config.current_version.to_string(),
            "installed version changed; restart before updating"
        );
        let executable = string(&new, "CFBundleExecutable")?;
        ensure!(
            Path::new(&executable).components().count() == 1 && !executable.starts_with('.'),
            "invalid bundle executable"
        );
        let binary = candidate.join("Contents/MacOS").join(&executable);
        ensure!(binary.is_file(), "missing bundle executable");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            ensure!(
                binary.metadata()?.permissions().mode() & 0o111 != 0,
                "bundle executable is not executable"
            );
        }
        ensure!(
            Command::new("/usr/bin/codesign")
                .args(["--verify", "--deep", "--strict"])
                .arg(&candidate)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()?
                .success(),
            "update_package_invalid: code signature"
        );
        Ok(Self {
            staging,
            candidate,
            destination,
            executable,
        })
    }

    /// Swap atomically; retain the old app until the replacement reports a window.
    /// Arguments come from the running process, never from the release manifest.
    pub fn restart(
        self,
        args: &[String],
        instance: Option<&crate::runtime::instance::Instance>,
    ) -> Result<()> {
        exchange(&self.destination, &self.candidate)?;
        let ready = self.staging.path().join("ready");
        let start = || -> Result<()> {
            let mut command = Command::new(
                self.destination
                    .join("Contents/MacOS")
                    .join(&self.executable),
            );
            command
                .args(args)
                .env("AIPASS_NATIVE_UPDATE_READY", &ready)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            if let Some(instance) = instance {
                instance.inherit(&mut command)?;
            }
            let mut child = command.spawn()?;
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                if child.try_wait()?.is_some() {
                    anyhow::bail!("updated application exited before readiness");
                }
                if fs::read_to_string(&ready).is_ok_and(|pid| pid == child.id().to_string()) {
                    return Ok(());
                }
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    anyhow::bail!("updated application readiness timed out");
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        };
        if let Err(error) = start() {
            if let Err(rollback) = exchange(&self.destination, &self.candidate) {
                // Do not delete the only surviving previous application on rollback failure.
                let backup = self.staging.keep();
                anyhow::bail!(
                    "{error}; rollback failed: {rollback}; backup retained at {}",
                    backup.display()
                );
            }
            return Err(error);
        }
        Ok(())
    }
}

fn extract(bytes: &[u8], destination: &Path) -> Result<PathBuf> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
    let mut root = None;
    let mut total = 0u64;
    let mut count = 0usize;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let components: Vec<_> = path.components().collect();
        ensure!(
            !components.is_empty()
                && components
                    .iter()
                    .all(|part| matches!(part, Component::Normal(_))),
            "unsafe archive path"
        );
        let top = PathBuf::from(components[0].as_os_str());
        ensure!(
            top.extension().is_some_and(|ext| ext == "app"),
            "archive must contain one application"
        );
        ensure!(
            root.as_ref().is_none_or(|root| root == &top),
            "multiple applications in archive"
        );
        root = Some(top);
        // Native bundles currently have no frameworks/symlinks. Fail closed until a
        // specific link-preserving format is implemented and tested.
        let kind = entry.header().entry_type();
        ensure!(kind.is_file() || kind.is_dir(), "unsupported archive entry");
        total = total
            .checked_add(entry.size())
            .context("archive size overflow")?;
        count += 1;
        ensure!(
            total <= 2 * 1024 * 1024 * 1024 && count <= 100_000,
            "archive exceeds extraction limits"
        );
        let target = destination.join(&path);
        ensure!(!target.is_file(), "duplicate archive entry");
        ensure!(
            entry.unpack_in(destination)?,
            "archive path escaped staging"
        );
    }
    Ok(destination.join(root.context("empty update archive")?))
}

#[cfg(target_os = "macos")]
fn exchange(left: &Path, right: &Path) -> Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let left = CString::new(left.as_os_str().as_bytes())?;
    let right = CString::new(right.as_os_str().as_bytes())?;
    // renamex_np(RENAME_SWAP) atomically exchanges directories on the same volume.
    unsafe extern "C" {
        fn renamex_np(
            from: *const std::ffi::c_char,
            to: *const std::ffi::c_char,
            flags: u32,
        ) -> i32;
    }
    let result = unsafe { renamex_np(left.as_ptr(), right.as_ptr(), 2) };
    if result != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn exchange(_: &Path, _: &Path) -> Result<()> {
    anyhow::bail!("update_installation_unsupported")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_links_and_multiple_roots_before_install() {
        for link in [false, true] {
            let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            let mut archive = tar::Builder::new(encoder);
            for path in ["One.app/Contents/a", "Two.app/Contents/b"] {
                let mut header = tar::Header::new_gnu();
                header.set_size(0);
                header.set_mode(0o644);
                if link {
                    header.set_entry_type(tar::EntryType::Symlink);
                    header.set_link_name("/tmp/escape").unwrap();
                }
                header.set_cksum();
                archive.append_data(&mut header, path, &[][..]).unwrap();
            }
            let bytes = archive.into_inner().unwrap().finish().unwrap();
            let dir = tempfile::tempdir().unwrap();
            assert!(extract(&bytes, dir.path()).is_err());
        }
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn atomic_exchange_preserves_previous_application_for_rollback() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        fs::create_dir(&a).unwrap();
        fs::create_dir(&b).unwrap();
        fs::write(a.join("version"), "old").unwrap();
        fs::write(b.join("version"), "new").unwrap();
        exchange(&a, &b).unwrap();
        assert_eq!(fs::read(a.join("version")).unwrap(), b"new");
        exchange(&a, &b).unwrap();
        assert_eq!(fs::read(a.join("version")).unwrap(), b"old");
    }
}
