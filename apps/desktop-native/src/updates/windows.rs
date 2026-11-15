//! NSIS installation runs from a disposable helper after the old process exits.
use super::{Config, Release};
use anyhow::{ensure, Context, Result};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    parent: u32,
    config: Config,
    release: Release,
    args: Vec<String>,
}
pub struct Prepared {
    stage: tempfile::TempDir,
    config: Config,
    release: Release,
}
impl Prepared {
    pub(super) fn new(config: &Config, release: &Release, bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.starts_with(b"MZ"), "update_package_invalid");
        let installed = config
            .app_path
            .parent()
            .context("missing installed directory")?;
        ensure!(
            installed.join("aipass-update-helper.exe").is_file(),
            "missing native updater helper"
        );
        fs::create_dir_all(&config.cache_dir)?;
        let stage = tempfile::Builder::new()
            .prefix("install-")
            .tempdir_in(&config.cache_dir)?;
        aipass_storage::atomic_write_bytes(stage.path().join("setup.exe"), bytes)?;
        fs::copy(
            installed.join("aipass-update-helper.exe"),
            stage.path().join("helper.exe"),
        )?;
        Ok(Self {
            stage,
            config: config.clone(),
            release: release.clone(),
        })
    }
    pub fn restart(
        self,
        args: &[String],
        _instance: Option<&crate::runtime::instance::Instance>,
    ) -> Result<()> {
        let plan = Plan {
            parent: std::process::id(),
            config: self.config,
            release: self.release,
            args: args.to_vec(),
        };
        let file = self.stage.path().join("plan.json");
        aipass_storage::atomic_write_bytes(&file, &serde_json::to_vec(&plan)?)?;
        let mut helper = Command::new(self.stage.path().join("helper.exe"))
            .arg(&file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let ready = self.stage.path().join("helper-ready");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready.is_file() {
            ensure!(
                helper.try_wait()?.is_none(),
                "update helper exited before handoff"
            );
            if Instant::now() >= deadline {
                let _ = helper.kill();
                let _ = helper.wait();
                anyhow::bail!("update helper handshake timed out");
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        // The independent helper owns cleanup once the desktop releases its files.
        self.stage.keep();
        Ok(())
    }
}
pub fn run() -> Result<()> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
    };
    let mut args = std::env::args_os().skip(1);
    let file = PathBuf::from(args.next().context("missing update plan")?);
    ensure!(args.next().is_none(), "unexpected helper argument");
    let stage = file
        .parent()
        .context("missing update stage")?
        .canonicalize()?;
    ensure!(
        std::env::current_exe()?
            .parent()
            .context("missing helper directory")?
            .canonicalize()?
            == stage,
        "helper and plan must be colocated"
    );
    let plan: Plan = serde_json::from_slice(&fs::read(&file)?)?;
    ensure!(plan.parent != std::process::id(), "invalid update parent");
    let mut config = plan.config;
    // A writable plan never controls the signature trust root.
    config.public_key = super::production::public_key()?;
    let updater = super::Updater::new(config.clone())?;
    let bytes = super::bounded_read(
        fs::File::open(stage.join("setup.exe"))?,
        super::MAX_PACKAGE,
        |_| {},
    )?;
    updater.verify(&plan.release, &bytes)?;
    let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, plan.parent) };
    ensure!(!process.is_null(), "old desktop process is unavailable");
    aipass_storage::atomic_write_bytes(stage.join("helper-ready"), b"ready")?;
    let result = unsafe { WaitForSingleObject(process, 30_000) };
    unsafe { CloseHandle(process) };
    ensure!(result == WAIT_OBJECT_0, "old desktop did not exit");
    let install = || -> Result<()> {
        let directory = config
            .app_path
            .parent()
            .context("missing installation directory")?;
        // /D is deliberately last, as required by NSIS. No command shell is used.
        let status = Command::new(stage.join("setup.exe"))
            .args(["/S", "/UPDATE"])
            .arg(format!("/D={}", directory.display()))
            .stdin(Stdio::null())
            .status()?;
        ensure!(status.success(), "update_install_failed");
        let metadata: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.join("desktop.json"))?)?;
        ensure!(
            metadata["version"].as_str() == Some(plan.release.version.to_string().as_str()),
            "installed version mismatch"
        );
        Ok(())
    };
    let result = install();
    if let Err(error) = &result {
        aipass_storage::atomic_write_bytes(
            config.cache_dir.join("install-error"),
            error.to_string().as_bytes(),
        )?;
    }
    // Also restart the installed app after a cancelled/failed installer.
    Command::new(&config.app_path)
        .args(&plan.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    for name in ["setup.exe", "plan.json", "helper-ready"] {
        let _ = fs::remove_file(stage.join(name));
    }
    result
}
