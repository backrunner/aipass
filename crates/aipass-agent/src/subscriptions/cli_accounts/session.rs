//! Vendor-owned login and renewal through bounded CLI stdio.
use super::*;

pub(super) fn command(c: &Context, provider: &str, root: &Path) -> Result<tokio::process::Command> {
    let exe = executable(provider)?;
    let mut cmd = native_cli::command(&exe, c);
    let mut paths = vec![exe.parent().ok_or("invalid CLI path")?.to_owned()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    cmd.env(
        "PATH",
        std::env::join_paths(paths).map_err(|_| "invalid CLI PATH")?,
    );
    let env = match provider {
        "codex" => "CODEX_HOME",
        "grok" => "GROK_HOME",
        "copilot" => "COPILOT_HOME",
        "gemini-cli" => "GEMINI_CLI_HOME",
        _ => return Err("unknown CLI account".into()),
    };
    cmd.env(env, root).current_dir(root);
    cmd.env(
        "BROWSER",
        std::env::current_exe().map_err(|_| "Agent browser helper unavailable")?,
    )
    .env("AIPASS_NATIVE_OPENED_URL", root.join(".aipass-login-url"));
    if provider == "codex" {
        cmd.args(["-c", "cli_auth_credentials_store=\"file\""]);
    }
    Ok(cmd)
}

pub(crate) async fn login(c: &mut Context, method: usize) -> Result<Value> {
    let provider = c.provider.clone();
    if !status(&provider).available {
        return Err(format!(
            "Install or update the official {provider} CLI, then retry"
        ));
    }
    let path = if method == 1 {
        default_home(&provider)?
    } else {
        new_home(&provider)?
    };
    if method != 1 {
        let mut cmd = command(c, &provider, &path)?;
        match provider.as_str() {
            "codex" => {
                cmd.arg("login");
                native_cli::login(cmd, c).await?;
            }
            "grok" => {
                cmd.args(["login", "--device-auth"]);
                native_cli::login(cmd, c).await?;
            }
            "copilot" => {
                cmd.args(["login", "--web-flow"]);
                native_cli::login(cmd, c).await?;
            }
            "gemini-cli" => {
                cmd.arg("--acp");
                rpc(
                    cmd,
                    c,
                    "authenticate",
                    json!({"methodId":"oauth-personal"}),
                    true,
                    true,
                )
                .await?;
            }
            _ => return Err("unknown CLI account".into()),
        }
    }
    reference(&provider, &path)
}

/// Stdio RPCs ask the official CLI to refresh. No issuer endpoint is called here.
pub(super) async fn rpc(
    mut cmd: tokio::process::Command,
    c: &mut Context,
    method: &str,
    params: Value,
    acp: bool,
    interactive: bool,
) -> Result<Value> {
    let opened = if interactive {
        native_cli::opened_url_path(&cmd)
    } else {
        // Invalid cached credentials must never start a browser login during a request.
        cmd.env("BROWSER", "www-browser")
            .env("NO_BROWSER", "true")
            .env("CI", "true");
        None
    };
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let child = cmd.spawn().map_err(|_| "cannot start official CLI")?;
    let pid = child.id();
    let mut child = native_cli::Running { child, pid };
    let mut input = child.child.stdin.take().ok_or("CLI input unavailable")?;
    let mut out = BufReader::new(
        child
            .child
            .stdout
            .take()
            .ok_or("CLI output unavailable")?
            .take(1024 * 1024 + 1),
    )
    .lines();
    let mut err = BufReader::new(
        child
            .child
            .stderr
            .take()
            .ok_or("CLI output unavailable")?
            .take(1024 * 1024 + 1),
    )
    .lines();
    let initialize = if acp {
        json!({"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"aipass","version":env!("CARGO_PKG_VERSION")}})
    } else {
        json!({"clientInfo":{"name":"aipass","version":env!("CARGO_PKG_VERSION")},"capabilities":{}})
    };
    input
        .write_all(
            format!(
                "{}\n",
                json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":initialize})
            )
            .as_bytes(),
        )
        .await
        .map_err(|_| "CLI input interrupted")?;
    let mut size = 0usize;
    let (mut stdout_done, mut stderr_done, mut shown) = (false, false, false);
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let operation = async {
        loop {
            if !shown {
                if let Some(url) = opened.as_deref().and_then(native_cli::read_opened_url) {
                    c.challenge(
                        &url,
                        "Approve the official CLI sign-in in your browser.",
                        false,
                    )
                    .await?;
                    shown = true;
                }
            }
            if stdout_done && stderr_done {
                return Err("CLI authentication ended before completion".into());
            }
            let (line, stderr) = tokio::select! {l=out.next_line(),if !stdout_done=>(l,false),l=err.next_line(),if !stderr_done=>(l,true),_=tick.tick()=>continue};
            let Some(line) = line.map_err(|_| "CLI output interrupted")? else {
                if stderr {
                    stderr_done = true;
                } else {
                    stdout_done = true;
                }
                continue;
            };
            size += line.len();
            if size > 1024 * 1024 {
                return Err("CLI output exceeds limit".into());
            }
            if stderr || !line.trim_start().starts_with('{') {
                if interactive && !shown {
                    if let Some(url) = native_cli::login_url(&line) {
                        c.challenge(
                            &url,
                            "Approve the official CLI sign-in in your browser.",
                            false,
                        )
                        .await?;
                        shown = true;
                    }
                }
                continue;
            }
            let message: Value =
                serde_json::from_str(&line).map_err(|_| "invalid CLI protocol response")?;
            if message["id"] == 1 {
                if message.get("error").is_some() {
                    return Err("Update the official CLI to use account authentication".into());
                }
                if !acp {
                    input
                        .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"initialized\"}\n")
                        .await
                        .map_err(|_| "CLI input interrupted")?;
                }
                input
                    .write_all(
                        format!(
                            "{}\n",
                            json!({"jsonrpc":"2.0","id":2,"method":method,"params":params})
                        )
                        .as_bytes(),
                    )
                    .await
                    .map_err(|_| "CLI input interrupted")?;
            } else if message["id"] == 2 {
                if message.get("error").is_some() {
                    return Err("Official CLI authentication failed; sign in again".into());
                }
                return Ok(message["result"].clone());
            }
        }
    };
    tokio::time::timeout(
        Duration::from_secs(if interactive { 600 } else { 35 }),
        operation,
    )
    .await
    .map_err(|_| "CLI authentication timed out")?
}

pub(crate) async fn fresh(c: &mut Context) -> Result<()> {
    c.native_token = None;
    check_device(&c.auth)?;
    let provider = c.provider.clone();
    let path = root(&c.auth)?;
    // Fail closed when the CLI's current account changed under a reference.
    let mut value = read(&provider, &path)?;
    let expected = s(&c.auth, "accountId").to_owned();
    if s(&value, "accountId") != expected {
        return Err("CLI account changed; reconnect it explicitly".into());
    }
    if value["expires"]
        .as_i64()
        .is_some_and(|t| t <= (now() / 1000) as i64 + 300)
    {
        let _guard = c.refresh_guard()?;
        let mut cmd = command(c, &provider, &path)?;
        match provider.as_str() {
            "codex" => {
                cmd.arg("app-server");
                rpc(
                    cmd,
                    c,
                    "account/read",
                    json!({"refreshToken":true}),
                    false,
                    false,
                )
                .await?;
            }
            "grok" => {
                cmd.arg("models");
                native_cli::run(cmd, 30).await?;
            }
            "gemini-cli" => {
                cmd.arg("--acp");
                rpc(
                    cmd,
                    c,
                    "authenticate",
                    json!({"methodId":"oauth-personal"}),
                    true,
                    false,
                )
                .await?;
            }
            "copilot" => {}
            _ => return Err("unknown CLI account".into()),
        }
        value = read(&provider, &path)?;
        if s(&value, "accountId") != expected {
            return Err("CLI account changed during renewal".into());
        }
    }
    if value["expires"]
        .as_i64()
        .is_some_and(|t| t <= (now() / 1000) as i64)
    {
        return Err("CLI account expired; sign in again".into());
    }
    c.native_token = Some(aipass_agent_protocol::SensitiveString::new(s(
        &value, "access",
    )));
    c.native_workspace = s(&value, "workspace").into();
    Ok(())
}
