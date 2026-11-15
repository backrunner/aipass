//! Verified native Linux replacement. Authorization remains owned by polkit.
use super::{Config, Release};
use anyhow::{ensure, Context, Result};
use std::{
    fs,
    io::Write,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    AppImage,
    Deb,
    Rpm,
}
pub fn kind() -> Result<Kind> {
    if std::env::var_os("APPIMAGE").is_some() {
        return Ok(Kind::AppImage);
    }
    match fs::read_to_string("/usr/lib/AIPass/package-format")
        .as_deref()
        .map(str::trim)
    {
        Ok("deb") => Ok(Kind::Deb),
        Ok("rpm") => Ok(Kind::Rpm),
        _ => anyhow::bail!("update_installation_unsupported"),
    }
}
pub fn target() -> Result<String> {
    Ok(format!(
        "linux-native-{}-{}",
        std::env::consts::ARCH,
        match kind()? {
            Kind::AppImage => "appimage",
            Kind::Deb => "deb",
            Kind::Rpm => "rpm",
        }
    ))
}
pub struct Prepared {
    kind: Kind,
    staging: tempfile::TempDir,
    package: PathBuf,
    destination: PathBuf,
}
impl Prepared {
    pub(super) fn new(config: &Config, _release: &Release, bytes: &[u8]) -> Result<Self> {
        let kind = kind()?;
        validate_format(kind, bytes).map_err(anyhow::Error::msg)?;
        ensure!(config.target == target()?, "update_package_invalid");
        let destination = config.app_path.canonicalize()?;
        let staging = if kind == Kind::AppImage {
            writable_appimage(&destination).map_err(anyhow::Error::msg)?;
            tempfile::Builder::new()
                .prefix(".aipass-update-")
                .tempdir_in(destination.parent().context("missing parent")?)?
        } else {
            ensure!(
                Path::new(manager(kind)).is_file() && Path::new("/usr/bin/pkexec").is_file(),
                "update_authorization_unavailable"
            );
            tempfile::Builder::new()
                .prefix("aipass-verified-update-")
                .tempdir()?
        };
        let package = staging.path().join(match kind {
            Kind::AppImage => "update.AppImage",
            Kind::Deb => "update.deb",
            Kind::Rpm => "update.rpm",
        });
        let mut file = fs::File::create(&package)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        if kind == Kind::AppImage {
            fs::set_permissions(&package, fs::Permissions::from_mode(0o755))?;
        }
        Ok(Self {
            kind,
            staging,
            package,
            destination,
        })
    }
    pub fn restart(
        self,
        args: &[String],
        instance: Option<&crate::runtime::instance::Instance>,
    ) -> Result<()> {
        if self.kind == Kind::AppImage {
            exchange(&self.destination, &self.package)?;
        } else {
            let status = Command::new("/usr/bin/pkexec")
                .args([
                    manager(self.kind),
                    if self.kind == Kind::Deb { "-i" } else { "-U" },
                ])
                .arg(&self.package)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()?;
            authorization_result(status.code()).map_err(anyhow::Error::msg)?;
        }
        let ready = self.staging.path().join("ready");
        let mut command = Command::new(&self.destination);
        command
            .args(args)
            .env("AIPASS_NATIVE_UPDATE_READY", &ready)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(instance) = instance {
            instance.inherit(&mut command)?;
        }
        let start = super::restart::wait(command, &ready);
        if let Err(error) = start {
            if self.kind == Kind::AppImage {
                if let Err(rollback) = exchange(&self.destination, &self.package) {
                    let backup = self.staging.keep();
                    anyhow::bail!(
                        "{error}; rollback failed: {rollback}; backup {}",
                        backup.display()
                    );
                }
            }
            return Err(error);
        }
        Ok(())
    }
}
fn exchange(left: &Path, right: &Path) -> Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let left = CString::new(left.as_os_str().as_bytes())?;
    let right = CString::new(right.as_os_str().as_bytes())?;
    if unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            left.as_ptr(),
            libc::AT_FDCWD,
            right.as_ptr(),
            libc::RENAME_EXCHANGE,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}
fn validate_format(kind: Kind, bytes: &[u8]) -> Result<(), String> {
    let valid = match kind {
        Kind::AppImage => compatible_appimage(bytes),
        Kind::Deb => bytes.starts_with(b"!<arch>\n"),
        Kind::Rpm => bytes.starts_with(&[0xed, 0xab, 0xee, 0xdb]),
    };
    if valid {
        Ok(())
    } else {
        Err("update_package_invalid".into())
    }
}

fn compatible_appimage(bytes: &[u8]) -> bool {
    let machine = match std::env::consts::ARCH {
        "aarch64" => [183, 0],
        "x86_64" => [62, 0],
        _ => return false,
    };
    bytes.len() >= 64
        && bytes.starts_with(b"\x7fELF\x02\x01\x01")
        && bytes.get(8..11) == Some(b"AI\x02")
        && matches!(bytes.get(16..18), Some([2 | 3, 0]))
        && bytes.get(18..20) == Some(machine.as_slice())
}

fn writable_appimage(image: &Path) -> Result<(), String> {
    let image = image
        .canonicalize()
        .map_err(|_| "update_install_permission_denied")?;
    let metadata = fs::metadata(&image).map_err(|_| "update_install_permission_denied")?;
    if !metadata.is_file() || metadata.uid() != rustix::process::geteuid().as_raw() {
        return Err("update_install_permission_denied".into());
    }
    let parent = image.parent().ok_or("update_install_permission_denied")?;
    // Test real directory access (ACLs and read-only mounts), not only mode bits.
    tempfile::Builder::new()
        .prefix(".aipass-update-probe-")
        .tempfile_in(parent)
        .map_err(|_| "update_install_permission_denied")?;
    Ok(())
}

fn manager(kind: Kind) -> &'static str {
    if kind == Kind::Deb {
        "/usr/bin/dpkg"
    } else {
        "/usr/bin/rpm"
    }
}

fn authorization_result(code: Option<i32>) -> Result<(), String> {
    match code {
        Some(0) => Ok(()),
        Some(126) => Err("update_install_cancelled".into()),
        Some(127) => Err("update_install_permission_denied".into()),
        _ => Err("update_install_failed".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn package_formats_and_authorization_outcomes_fail_closed() {
        let mut image = [0; 64];
        image[..11].copy_from_slice(b"\x7fELF\x02\x01\x01\x00AI\x02");
        image[16] = 2;
        image[18] = if cfg!(target_arch = "aarch64") {
            183
        } else {
            62
        };
        assert!(compatible_appimage(&image));
        let mut other_arch = image;
        other_arch[18] = if image[18] == 183 { 62 } else { 183 };
        assert!(!compatible_appimage(&other_arch));
        assert!(!compatible_appimage(&image[..20]));
        for (kind, data) in [
            (Kind::Deb, b"!<arch>\n".as_slice()),
            (Kind::Rpm, &[0xed, 0xab, 0xee, 0xdb]),
            (Kind::AppImage, image.as_slice()),
        ] {
            assert!(validate_format(kind, data).is_ok());
            for other in [Kind::Deb, Kind::Rpm, Kind::AppImage]
                .into_iter()
                .filter(|other| *other != kind)
            {
                assert!(validate_format(other, data).is_err());
            }
        }
        assert_eq!(
            authorization_result(Some(126)),
            Err("update_install_cancelled".into())
        );
        assert_eq!(
            authorization_result(Some(127)),
            Err("update_install_permission_denied".into())
        );
        assert!(authorization_result(Some(1)).is_err());
        assert!(authorization_result(None).is_err());
        assert!(authorization_result(Some(0)).is_ok());
    }
    #[test]
    fn appimage_permissions_are_checked_before_stopping_the_runtime() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("AIPass.AppImage");
        fs::write(&image, b"fixture").unwrap();
        assert!(writable_appimage(&image).is_ok());
        if !rustix::process::geteuid().is_root() {
            fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o555)).unwrap();
            assert_eq!(
                writable_appimage(&image),
                Err("update_install_permission_denied".into())
            );
            fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        }
        assert!(writable_appimage(&dir.path().join("absent")).is_err());
    }
}
