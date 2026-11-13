//! Native instance ownership survives replacement through an inherited lease.
//! No commands or credentials are accepted through the lease file.
use anyhow::{ensure, Context, Result};
use std::{
    fs::{File, OpenOptions},
    path::Path,
    process::Command,
    sync::Arc,
};

const LEASE_FD: &str = "AIPASS_NATIVE_INSTANCE_FD";

#[derive(Clone)]
pub struct Instance(Arc<File>);

impl Instance {
    pub fn acquire(path: &Path) -> Result<Self> {
        std::fs::create_dir_all(path.parent().context("instance lease has no parent")?)?;
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let file = options.open(path)?;
        #[cfg(unix)]
        if let Some(raw) = std::env::var_os(LEASE_FD) {
            use std::os::fd::{BorrowedFd, RawFd};
            use std::os::unix::fs::MetadataExt;
            std::env::remove_var(LEASE_FD);
            let fd: RawFd = raw
                .to_str()
                .context("invalid instance descriptor")?
                .parse()?;
            ensure!(
                fd >= 3 && unsafe { libc::fcntl(fd, libc::F_GETFD) } >= 0,
                "invalid inherited instance descriptor"
            );
            // Duplicate before owning it; only the inherited descriptor belongs to us.
            let inherited = File::from(unsafe { BorrowedFd::borrow_raw(fd) }.try_clone_to_owned()?);
            let wanted = file.metadata()?;
            let actual = inherited.metadata()?;
            ensure!(
                actual.is_file() && actual.dev() == wanted.dev() && actual.ino() == wanted.ino(),
                "instance lease belongs to another application"
            );
            inherited
                .try_lock()
                .context("inherited instance lease is not exclusive")?;
            unsafe {
                libc::close(fd);
            }
            return Ok(Self(Arc::new(inherited)));
        }
        file.try_lock()
            .context("AIPass native instance is already running")?;
        Ok(Self(Arc::new(file)))
    }

    pub fn inherit(&self, command: &mut Command) -> Result<()> {
        #[cfg(unix)]
        {
            use std::os::{fd::AsRawFd, unix::process::CommandExt};
            let fd = self.0.as_raw_fd();
            command.env(LEASE_FD, fd.to_string());
            // Change descriptor flags only in the forked replacement. Other
            // concurrent spawns keep close-on-exec and cannot retain this lease.
            unsafe {
                command.pre_exec(move || {
                    let flags = libc::fcntl(fd, libc::F_GETFD);
                    if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            Ok(())
        }
        #[cfg(not(unix))]
        {
            let _ = (&self.0, command);
            anyhow::bail!("native instance handoff is not implemented on this platform")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lease_excludes_competitors_and_releases_on_last_owner() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("instance");
        let first = Instance::acquire(&path).unwrap();
        let retained = first.clone();
        assert!(Instance::acquire(&path).is_err());
        drop(first);
        assert!(Instance::acquire(&path).is_err());
        drop(retained);
        assert!(Instance::acquire(&path).is_ok());
    }
}
