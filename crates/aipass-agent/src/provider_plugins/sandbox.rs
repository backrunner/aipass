#[cfg(target_os = "linux")]
#[path = "linux_filter.rs"]
mod linux_filter;
use super::{Manifest, Result, Runtime};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

pub(super) fn safe_entry(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    !stem.is_empty()
        && !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
        && !((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit())
        && !name.ends_with('.')
        && !name.eq_ignore_ascii_case("manifest.json")
        && !name.to_ascii_lowercase().ends_with(".json")
        && name.len() <= 120
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'))
}

pub(super) fn validate(m: &Manifest, program: &[u8]) -> Result<()> {
    match m.runtime {
        Some(Runtime::Wasi) if m.target.is_none() => {
            crate::plugin_wasi::validate(program).map_err(|_| "Invalid WASI module".into())
        }
        Some(Runtime::NativeSandbox) => {
            let target = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
            let format = super::binary::compatible(
                program,
                &m.entry,
                std::env::consts::OS,
                std::env::consts::ARCH,
            );
            if m.target.as_deref() != Some(&target) || !format {
                return Err("Incompatible native plugin target".into());
            }
            native_launcher().map(|_| ())
        }
        Some(Runtime::Wasi) => Err("Invalid WASI module".into()),
        None if !program.is_empty() => Ok(()),
        None => Err("Invalid plugin entry".into()),
    }
}

fn native_launcher() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    let path = PathBuf::from("/usr/bin/sandbox-exec");
    #[cfg(target_os = "linux")]
    let path = PathBuf::from("/usr/bin/bwrap");
    #[cfg(windows)]
    let path = std::env::current_exe().map_err(|_| "System sandbox unavailable")?;
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    let path = PathBuf::new();
    if !path.is_file() {
        return Err("System sandbox unavailable".into());
    }
    Ok(path)
}

fn worker() -> Result<PathBuf> {
    // Integration fixtures also construct a proxy bridge inside the Rust test harness.
    #[cfg(test)]
    if let Some(program) = std::env::var_os("AIPASS_EXAMPLE_TEST_AGENT") {
        return Ok(program.into());
    }
    std::env::current_exe().map_err(|_| "Sandbox worker unavailable".into())
}
pub(super) fn command(m: &Manifest, entry: &Path) -> Result<(PathBuf, Vec<OsString>)> {
    let entry = entry.canonicalize().map_err(|_| "Cannot stage plugin")?;
    let entry_arg = entry.as_os_str().to_owned();
    match m.runtime {
        Some(Runtime::Wasi) => Ok((worker()?, vec!["--plugin-wasi".into(), entry_arg])),
        Some(Runtime::NativeSandbox) => Ok((worker()?, vec!["--plugin-native".into(), entry_arg])),
        None => {
            let (path, args) = m
                .interpreter
                .as_ref()
                .map_or((entry.clone(), vec![]), |p| (p.clone(), vec![entry_arg]));
            #[cfg(windows)]
            if !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
            {
                return Err("Plugin command must be an executable".into());
            }
            Ok((path, args))
        }
    }
}

#[cfg(target_os = "macos")]
fn native_args(entry: &Path) -> Result<Vec<OsString>> {
    let mut param = OsString::from("PLUGIN=");
    param.push(entry);
    Ok(vec![
        "-D".into(),
        param,
        "-p".into(),
        include_str!("plugin.sb").into(),
        entry.into(),
    ])
}
#[cfg(target_os = "linux")]
fn native_args(entry: &Path) -> Result<Vec<OsString>> {
    let mut args: Vec<OsString> = [
        "--unshare-all",
        "--unshare-user",
        "--disable-userns",
        "--die-with-parent",
        "--new-session",
        "--cap-drop",
        "ALL",
        "--clearenv",
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    for path in ["/usr/lib", "/usr/lib64", "/lib", "/lib64"] {
        if Path::new(path).is_dir() {
            args.extend(["--ro-bind".into(), path.into(), path.into()]);
        }
    }
    args.extend([
        "--ro-bind".into(),
        entry.into(),
        "/plugin".into(),
        "--remount-ro".into(),
        "/".into(),
        "--chdir".into(),
        "/".into(),
        "--".into(),
        "/plugin".into(),
    ]);
    Ok(args)
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub fn run_native_plugin(entry: &Path) -> anyhow::Result<()> {
    use std::os::unix::process::CommandExt;
    let entry = entry.canonicalize()?;
    let launcher = native_launcher().map_err(anyhow::Error::msg)?;
    let args = native_args(&entry).map_err(anyhow::Error::msg)?;
    let mut command = std::process::Command::new(launcher);
    #[cfg(target_os = "linux")]
    let filter = linux_filter::descriptor()?;
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        command.arg("--seccomp").arg(filter.as_raw_fd().to_string());
    }
    command.env_clear().args(args);
    Err(command.exec().into())
}

#[cfg(unix)]
pub(crate) fn limits(cmd: &mut tokio::process::Command) {
    use rustix::process::{getrlimit, setrlimit, Resource, Rlimit};
    // SAFETY: only async-signal-safe getrlimit/setrlimit syscalls in the forked child.
    unsafe {
        cmd.pre_exec(|| {
            let limit = |r, max: u64| {
                let cap = getrlimit(r).maximum.unwrap_or(max).min(max);
                setrlimit(
                    r,
                    Rlimit {
                        current: Some(cap),
                        maximum: Some(cap),
                    },
                )
                .map_err(std::io::Error::from)
            };
            limit(Resource::Cpu, 60)?;
            limit(Resource::Core, 0)?;
            limit(Resource::Nofile, 64)?;
            #[cfg(target_os = "linux")]
            {
                limit(Resource::As, 2 * 1024 * 1024 * 1024)?;
                limit(Resource::Nproc, 128)?;
            }
            Ok(())
        });
    }
}

// Darwin's rlimits do not impose a hard address-space limit on mmap. Supervise
// the worker PID (retained across sandbox-exec) as well as its CPU rlimit.
pub(crate) async fn watch_memory(pid: Option<u32>, runtime: Option<Runtime>) {
    #[cfg(target_os = "macos")]
    if let (Some(pid), Some(_)) = (pid, runtime) {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let mut usage = std::mem::MaybeUninit::<libc::rusage_info_v2>::uninit();
            // SAFETY: the selected flavor writes a complete rusage_info_v2 on success.
            let result = unsafe {
                libc::proc_pid_rusage(pid as i32, libc::RUSAGE_INFO_V2, usage.as_mut_ptr().cast())
            };
            if result == 0 && unsafe { usage.assume_init() }.ri_phys_footprint > 384 * 1024 * 1024 {
                return;
            }
        }
    }
    let _ = (pid, runtime);
    std::future::pending::<()>().await;
}
