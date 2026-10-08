//! Only vendor-owned authentication commands use a subprocess. Provider HTTP and
//! conversion always execute in Rust. Native subscription credentials stay in vendor-owned configuration homes.
use super::*;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::{Child, Command},
};
pub(super) struct Running {
    pub(super) child: Child,
    pub(super) pid: Option<u32>,
}
impl Drop for Running {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(Some(_))) {
            return;
        }
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
    let opened = opened_url_path(&command);
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
    let (mut authorization, mut code) = (None::<String>, None::<String>);
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    while !a || !b {
        let line=tokio::select! {v=out.next_line(),if !a=>{if v.as_ref().is_ok_and(Option::is_none){a=true;}v},v=err.next_line(),if !b=>{if v.as_ref().is_ok_and(Option::is_none){b=true;}v},_=tick.tick()=>Ok(None)}.map_err(|_|"provider CLI output interrupted")?;
        if !shown {
            if let Some(url) = opened.as_deref().and_then(read_opened_url) {
                let instructions = code.as_ref().map_or_else(
                    || "Approve the official CLI sign-in in your browser.".to_owned(),
                    |code| format!("Enter code {code} on the official sign-in page."),
                );
                c.challenge(&url, &instructions, false).await?;
                authorization = Some(url);
                shown = true;
            }
        }
        if let Some(line) = line {
            count += line.len();
            if count > 1024 * 1024 {
                return Err("provider CLI output exceeds limit".into());
            }
            if matches!(c.provider.as_str(), "grok" | "copilot") {
                if let Some(candidate) = device_user_code(&line) {
                    if code.as_ref() != Some(&candidate) {
                        code = Some(candidate);
                        if let Some(url) = authorization.as_ref() {
                            c.challenge(
                                url,
                                &format!(
                                    "Enter code {} on the official sign-in page.",
                                    code.as_deref().unwrap()
                                ),
                                false,
                            )
                            .await?;
                        }
                    }
                }
            }
            if !shown {
                if let Some(url) = login_url(&line) {
                    let instructions = code.as_ref().map_or_else(
                        || "Approve the official CLI sign-in in your browser.".to_owned(),
                        |code| format!("Enter code {code} on the official sign-in page."),
                    );
                    c.challenge(&url, &instructions, false).await?;
                    authorization = Some(url);
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

pub(super) fn opened_url_path(command: &Command) -> Option<std::path::PathBuf> {
    command.as_std().get_envs().find_map(|(k, v)| {
        (k == "AIPASS_NATIVE_OPENED_URL")
            .then(|| v.map(std::path::PathBuf::from))
            .flatten()
    })
}
pub(super) fn read_opened_url(path: &std::path::Path) -> Option<String> {
    if std::fs::metadata(path).ok()?.len() > 8192 {
        return None;
    }
    login_url(&std::fs::read_to_string(path).ok()?)
}

// Skip installation/documentation links; only authorization links enter the UI.
pub(super) fn login_url(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let raw = line[start..]
        .split_whitespace()
        .next()?
        .split('\u{1b}')
        .next()?
        .trim_end_matches([')', ']']);
    let u = url::Url::parse(raw).ok()?;
    let host = u.host_str()?;
    if !matches!(
        host,
        "auth.openai.com" | "auth.x.ai" | "grok.com" | "github.com" | "accounts.google.com"
    ) || !u.username().is_empty()
        || u.password().is_some()
    {
        return None;
    }
    Some(raw.to_owned())
}

pub(super) fn device_user_code(line: &str) -> Option<String> {
    if !line.to_ascii_lowercase().contains("code") {
        return None;
    }
    line.split_whitespace()
        .rev()
        .map(|s| s.trim_matches(['\'', '"', '`', ':', ',', '(', ')', '[', ']']))
        .find(|s| {
            (6..=12).contains(&s.len())
                && s.bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-')
                && s.bytes().any(|b| b.is_ascii_alphanumeric())
        })
        .map(str::to_owned)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn device_instructions_accept_only_short_public_codes_and_authorization_hosts() {
        assert_eq!(
            device_user_code("Enter code: ABCD-1234"),
            Some("ABCD-1234".into())
        );
        assert!(device_user_code("access_token: private.secret.token").is_none());
        assert!(device_user_code("device code: abcdefghijklmnopqrstuvwxyz").is_none());
        assert_eq!(
            login_url("visit https://auth.x.ai/activate?user_code=ABCD-1234"),
            Some("https://auth.x.ai/activate?user_code=ABCD-1234".into())
        );
        assert!(login_url("visit https://evil.example/activate").is_none());
    }
}
