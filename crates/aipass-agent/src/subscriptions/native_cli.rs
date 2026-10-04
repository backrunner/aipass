//! Only vendor-owned authentication commands use a subprocess. Provider HTTP and
//! conversion always execute in Rust. Credentials are staged in private temp dirs.
use super::*;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::{Child, Command},
};
pub(super) struct Running {
    child: Child,
    pid: Option<u32>,
}
impl Drop for Running {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self
            .pid
            .and_then(|p| rustix::process::Pid::from_raw(p as i32))
        {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
        let _ = self.child.start_kill();
    }
}
pub(super) fn command(exe: &std::path::Path, c: &Context) -> Command {
    let mut command = Command::new(exe);
    command
        .env_clear()
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null());
    for key in [
        "PATH",
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "TMPDIR",
        "TEMP",
        "SystemRoot",
        "LANG",
        "SSL_CERT_FILE",
    ] {
        if let Some(v) = std::env::var_os(key) {
            command.env(key, v);
        }
    }
    if let Some(env) = c
        .outbound
        .as_ref()
        .and_then(aipass_proxy::cli_proxy_environment)
    {
        command.envs(env);
    } else {
        for key in [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "NO_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
            "no_proxy",
        ] {
            if let Some(v) = std::env::var_os(key) {
                command.env(key, v);
            }
        }
    }
    #[cfg(unix)]
    command.process_group(0);
    command
}
pub(super) async fn run(mut command: Command, seconds: u64) -> Result<String> {
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let child = command
        .spawn()
        .map_err(|_| "cannot start provider authentication CLI")?;
    let pid = child.id();
    let mut child = Running { child, pid };
    let mut stdout = child
        .child
        .stdout
        .take()
        .ok_or("provider output unavailable")?;
    let operation = async {
        let mut bytes = zeroize::Zeroizing::new(Vec::new());
        (&mut stdout)
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| "provider CLI output interrupted")?;
        if bytes.len() > 1024 * 1024 {
            return Err("provider CLI output exceeds limit".into());
        }
        let status = child
            .child
            .wait()
            .await
            .map_err(|_| "provider CLI stopped")?;
        if !status.success() {
            return Err("provider CLI authentication failed".into());
        }
        String::from_utf8(bytes.to_vec()).map_err(|_| "invalid provider CLI output".into())
    };
    tokio::time::timeout(Duration::from_secs(seconds), operation)
        .await
        .map_err(|_| "provider CLI timed out")?
}
pub(super) async fn login(mut command: Command, c: &mut Context) -> Result<()> {
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let child = command
        .spawn()
        .map_err(|_| "cannot start provider authentication CLI")?;
    let pid = child.id();
    let mut child = Running { child, pid };
    let mut out = BufReader::new(
        child
            .child
            .stdout
            .take()
            .ok_or("provider output unavailable")?
            .take(1024 * 1024 + 1),
    )
    .lines();
    let mut err = BufReader::new(
        child
            .child
            .stderr
            .take()
            .ok_or("provider output unavailable")?
            .take(1024 * 1024 + 1),
    )
    .lines();
    let (mut count, mut shown, mut a, mut b) = (0, false, false, false);
    while !a || !b {
        let line=tokio::select! {v=out.next_line(),if !a=>{if v.as_ref().is_ok_and(Option::is_none){a=true;}v},v=err.next_line(),if !b=>{if v.as_ref().is_ok_and(Option::is_none){b=true;}v}}.map_err(|_|"provider CLI output interrupted")?;
        if let Some(line) = line {
            count += line.len();
            if count > 1024 * 1024 {
                return Err("provider CLI output exceeds limit".into());
            }
            if !shown {
                if let Some(i) = line.find("https://") {
                    let url = line[i..]
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .split('\u{1b}')
                        .next()
                        .unwrap_or("")
                        .trim_end_matches([')', ']']);
                    c.challenge(url, "Approve the sign-in in your browser.", false)
                        .await?;
                    shown = true;
                }
            }
        }
    }
    let status = child
        .child
        .wait()
        .await
        .map_err(|_| "provider CLI stopped")?;
    if !status.success() {
        return Err("provider CLI sign-in failed".into());
    }
    Ok(())
}
