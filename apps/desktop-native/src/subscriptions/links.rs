//! Desktop link policy mirrors the existing Tauri subscription bridge.
use aipass_agent::AgentCommandError;
use aipass_agent_protocol::AgentErrorCode;
use std::{
    path::{Component, Path},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use url::Url;

pub(super) fn error(message: &str) -> AgentCommandError {
    AgentCommandError {
        code: Some(AgentErrorCode::ValidationFailed),
        message: message.into(),
    }
}
pub fn authorization(uri: &str, claude: bool) -> Result<Url, AgentCommandError> {
    let url = Url::parse(uri).map_err(|_| error("Invalid authorization URL"))?;
    let host = url.host_str().unwrap_or_default();
    let allowed = if claude {
        url.scheme() == "https"
            && url.port_or_known_default() == Some(443)
            && matches!(
                host,
                "claude.ai" | "console.anthropic.com" | "platform.claude.com"
            )
    } else {
        url.scheme() == "https"
            || (url.scheme() == "http" && matches!(host, "127.0.0.1" | "localhost" | "[::1]"))
    };
    if !allowed || !url.username().is_empty() || url.password().is_some() {
        return Err(error("Unsupported authorization URL"));
    }
    Ok(url)
}
pub fn public(uri: &str) -> Result<Url, AgentCommandError> {
    let url = authorization(uri, false)?;
    let host = url.host_str().unwrap_or_default();
    if url.scheme() != "https"
        || url.port_or_known_default() != Some(443)
        || !(host == "auth.openai.com"
            || host == "x.ai"
            || host.ends_with(".x.ai")
            || matches!(
                (host, url.path()),
                ("opencode.ai", "/console")
                    | ("app.cline.bot", "/dashboard/account")
                    | ("ollama.com", "/settings/keys" | "/settings/usage")
                    | ("app.kilo.ai", "/")
                    | ("www.kimi.com", "/code/console")
                    | (
                        "platform.minimax.io" | "platform.minimaxi.com",
                        "/subscribe/token-plan"
                    )
                    | ("platform.xiaomimimo.com", "/token-plan")
                    | (
                        "platform.stepfun.com" | "platform.stepfun.ai",
                        "/interface-key"
                    )
                    | ("platform.qianwenai.com", "/home/api-keys")
                    | ("bailian.console.aliyun.com", "/")
                    | (
                        "modelstudio.console.alibabacloud.com",
                        "/ap-southeast-1/subscription/token-plan"
                            | "/ap-southeast-1/subscription/coding-plan"
                    )
                    | ("ark.volcengine.com", "/region:cn-beijing/apikey")
                    | ("console.cloud.tencent.com", "/tokenhub/tokenplan")
            ))
    {
        return Err(error("Unsupported authorization URL"));
    }
    Ok(url)
}
pub fn key_page(provider: &str, region: &str) -> Option<String> {
    Some(
        match provider {
            "opencode-go" | "opencode-zen" => "https://opencode.ai/console",
            "cline" => "https://app.cline.bot/dashboard/account",
            "ollama-cloud" => "https://ollama.com/settings/keys",
            "kilo-gateway" => "https://app.kilo.ai",
            "kimi-code" | "kimi-code-cn" => "https://www.kimi.com/code/console",
            "minimax-m-plan-cn" => "https://platform.minimaxi.com/subscribe/token-plan",
            "minimax-m-plan" => "https://platform.minimax.io/subscribe/token-plan",
            "bailian-token-plan" | "bailian-coding-plan" if region == "intl" => {
                return Some(format!(
                    "https://modelstudio.console.alibabacloud.com/ap-southeast-1/subscription/{}",
                    if provider == "bailian-coding-plan" {
                        "coding-plan"
                    } else {
                        "token-plan"
                    }
                ))
            }
            "bailian-token-plan" | "bailian-coding-plan" => "https://bailian.console.aliyun.com",
            "mimo-plan" => "https://platform.xiaomimimo.com/token-plan",
            "stepfun-plan-cn" => "https://platform.stepfun.com/interface-key",
            "stepfun-plan" => "https://platform.stepfun.ai/interface-key",
            "qwen-token-plan" => "https://platform.qianwenai.com/home/api-keys",
            "volcengine-coding-plan" | "volcengine-agent-plan" => {
                "https://ark.volcengine.com/region:cn-beijing/apikey"
            }
            "tencent-token-plan" | "tencent-token-plan-enterprise" => {
                "https://console.cloud.tencent.com/tokenhub/tokenplan"
            }
            _ => return None,
        }
        .into(),
    )
}
pub fn installation(provider: &str) -> Result<Url, AgentCommandError> {
    Url::parse(match provider {
        "claude" | "anthropic" => "https://code.claude.com/docs/en/setup",
        "codex" => "https://developers.openai.com/codex/cli/",
        "grok" => "https://github.com/xai-org/grok-build",
        "copilot" => "https://docs.github.com/en/copilot/how-tos/copilot-cli/install-copilot-cli",
        "gemini-cli" => "https://geminicli.com/docs/get-started/installation/",
        _ => return Err(error("Unsupported subscription CLI")),
    })
    .map_err(|_| error("Invalid installation URL"))
}
fn opener() -> Command {
    #[cfg(target_os = "macos")]
    let command = Command::new("open");
    #[cfg(target_os = "windows")]
    let command = {
        let mut c = Command::new("rundll32.exe");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let command = Command::new("xdg-open");
    command
}
fn run(mut command: Command) -> Result<(), AgentCommandError> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| error("Could not open location"))?;
    let deadline = Instant::now() + Duration::from_millis(1500);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) | Err(_) => return Err(error("Could not open location")),
            Ok(None) if Instant::now() >= deadline => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                return Ok(());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
        }
    }
}
pub fn open(url: Url) -> Result<(), AgentCommandError> {
    let mut c = opener();
    c.arg(url.as_str());
    run(c)
}
pub fn open_location(path: &Path) -> Result<(), AgentCommandError> {
    if !path.is_absolute()
        || path.components().any(|c| matches!(c, Component::ParentDir))
        || !path.is_dir()
    {
        return Err(error("Account source is not an existing directory"));
    }
    let mut command = opener();
    command.arg(path);
    run(command)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_pages_and_agent_links_have_distinct_boundaries() {
        for uri in [
            "file:///tmp/x",
            "javascript:alert(1)",
            "https://u:p@claude.ai/a",
            "http://example.com/a",
        ] {
            assert!(authorization(uri, false).is_err());
        }
        assert!(authorization("http://127.0.0.1:8192/callback", false).is_ok());
        assert!(authorization("https://claude.ai.evil.invalid/a", true).is_err());
        assert!(public("https://modelstudio.console.alibabacloud.com/redirect").is_err());
        for id in ["bailian-token-plan", "bailian-coding-plan"] {
            for region in ["cn", "intl"] {
                assert!(public(&key_page(id, region).unwrap()).is_ok());
            }
        }
    }
}
