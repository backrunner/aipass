use anyhow::{bail, Result};
use std::{
    fs,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};
pub fn wait(mut command: Command, ready: &Path, appimage: bool) -> Result<()> {
    let mut child = command.spawn()?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if child.try_wait()?.is_some() {
            bail!("updated application exited before readiness");
        }
        // Extract-and-run AppImages keep the runtime alive as a parent of AppRun.
        // Accept only that live child's own PID or its same-user descendants.
        if fs::read_to_string(ready).is_ok_and(|pid| {
            super::restart_identity::matches(&pid, child.id(), appimage, |pid| {
                let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
                super::restart_identity::parent(&status, pid, rustix::process::geteuid().as_raw())
            })
        }) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("updated application readiness timed out");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
