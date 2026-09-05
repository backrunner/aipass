use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Open only provider-owned HTTPS authorization pages in the system browser.
/// A WebView window.open call does not reliably launch an external browser.
#[tauri::command]
pub(crate) async fn oauth_open_verification(uri: String) -> Result<(), String> {
    let url = verification_url(&uri)?;
    crate::run_blocking(move || {
        let mut command = browser_command();
        let mut child = command
            .arg(url.as_str())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "could not open authorization page".to_string())?;
        let deadline = Instant::now() + Duration::from_millis(1500);
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(_)) | Err(_) => return Err("could not open authorization page".into()),
                Ok(None) if Instant::now() >= deadline => return Ok(()),
                Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            }
        }
    })
    .await
}

fn verification_url(uri: &str) -> Result<url::Url, String> {
    let url = url::Url::parse(uri).map_err(|_| "invalid authorization URL".to_string())?;
    let host = url.host_str().unwrap_or_default();
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
        || !(host == "auth.openai.com" || host == "x.ai" || host.ends_with(".x.ai"))
    {
        return Err("unsupported authorization URL".into());
    }
    Ok(url)
}

fn browser_command() -> Command {
    #[cfg(target_os = "macos")]
    let command = Command::new("open");
    #[cfg(target_os = "windows")]
    let command = {
        // No cmd.exe shell: the URL remains a single argument, including query strings.
        let mut command = Command::new("rundll32.exe");
        command.arg("url.dll,FileProtocolHandler");
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let command = Command::new("xdg-open");
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorization_links_are_limited_to_provider_https_origins() {
        for uri in [
            "https://auth.openai.com/codex/device",
            "https://accounts.x.ai/activate?user_code=ABCD-EFGH",
            "https://auth.x.ai/device",
        ] {
            assert!(verification_url(uri).is_ok());
        }
        for uri in [
            "http://auth.openai.com/codex/device",
            "https://auth.openai.com.evil.example/device",
            "https://x.ai.evil.example/device",
            "https://example.com/device",
            "https://user:password@auth.x.ai/device",
            "https://auth.x.ai:444/device",
            "file:///tmp/file",
            "javascript:alert(1)",
        ] {
            assert!(verification_url(uri).is_err());
        }
    }
}
