use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Open only provider-owned HTTPS authorization pages in the system browser.
/// A WebView window.open call does not reliably launch an external browser.
#[tauri::command]
pub(crate) async fn oauth_open_verification(uri: String) -> Result<(), String> {
    let url = verification_url(&uri)?;
    open_url(url).await
}

/// The URL comes from an active Agent-owned login, never a frontend argument.
#[tauri::command]
pub(crate) async fn community_open_verification(
    app: tauri::AppHandle,
    ticket: uuid::Uuid,
) -> Result<(), String> {
    let status: aipass_agent_protocol::CommunityLoginStatus =
        crate::commands::agent_request_no_unlock_async(
            app,
            aipass_agent_protocol::AgentRequest::CommunityLoginPoll { ticket },
        )
        .await?;
    if status.status != "pending" {
        return Err("sign-in is no longer pending".into());
    }
    let url = url::Url::parse(status.url.as_deref().ok_or("sign-in has no browser link")?)
        .map_err(|_| "invalid authorization URL")?;
    if !url.username().is_empty()
        || url.password().is_some()
        || !(url.scheme() == "https"
            || (url.scheme() == "http"
                && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))))
    {
        return Err("unsupported authorization URL".into());
    }
    open_url(url).await
}

async fn open_url(url: url::Url) -> Result<(), String> {
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

#[tauri::command]
pub(crate) async fn claude_open_verification(
    app: tauri::AppHandle,
    ticket: uuid::Uuid,
) -> Result<(), String> {
    let status: aipass_agent_protocol::ClaudeLoginStatus =
        crate::commands::agent_request_no_unlock_async(
            app,
            aipass_agent_protocol::AgentRequest::ClaudeLoginPoll { ticket },
        )
        .await?;
    if status.status != "pending" {
        return Err("Claude sign-in is no longer pending".into());
    }
    let uri = status.url.ok_or("Claude sign-in has no browser link")?;
    let url = url::Url::parse(&uri).map_err(|_| "invalid Claude authorization URL")?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
        || !matches!(
            url.host_str(),
            Some("claude.ai" | "console.anthropic.com" | "platform.claude.com")
        )
    {
        return Err("unsupported Claude authorization URL".into());
    }
    open_url(url).await
}

#[tauri::command]
pub(crate) async fn claude_open_install() -> Result<(), String> {
    open_url(
        url::Url::parse("https://code.claude.com/docs/en/setup")
            .map_err(|_| "invalid installation URL")?,
    )
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

pub(crate) fn browser_command() -> Command {
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

#[tauri::command]
pub(crate) async fn subscription_open_install(provider: String) -> Result<(), String> {
    let uri = match provider.as_str() {
        "codex" => "https://developers.openai.com/codex/cli/",
        "grok" => "https://github.com/xai-org/grok-build",
        "copilot" => "https://docs.github.com/en/copilot/how-tos/copilot-cli/install-copilot-cli",
        "gemini-cli" => "https://geminicli.com/docs/get-started/installation/",
        _ => return Err("unsupported subscription CLI".into()),
    };
    open_url(url::Url::parse(uri).map_err(|_| "invalid CLI installation URL")?).await
}
