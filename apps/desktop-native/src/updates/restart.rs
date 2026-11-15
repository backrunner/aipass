use anyhow::{bail, Result};
use std::{
    fs,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};
pub fn wait(mut command: Command, ready: &Path) -> Result<()> {
    let mut child = command.spawn()?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if child.try_wait()?.is_some() {
            bail!("updated application exited before readiness");
        }
        if fs::read_to_string(ready).is_ok_and(|pid| pid == child.id().to_string()) {
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
