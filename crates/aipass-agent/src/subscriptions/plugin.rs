//! Translate the public, account-scoped plugin contract into internal subscription events.
use super::*;
use crate::provider_plugins::Launch;
use aipass_agent_protocol::{
    subscription_plugin::{PluginCall as Call, PluginError, PluginEvent as Event, PluginModel},
    SensitiveString,
};
use std::process::Stdio;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{ChildStdin, ChildStdout, Command},
    time::timeout,
};

struct Channel {
    input: ChildStdin,
    output: ChildStdout,
}
impl Channel {
    async fn send(&mut self, call: &Call<'_>) -> Result<()> {
        let bytes =
            zeroize::Zeroizing::new(serde_json::to_vec(call).map_err(|_| "Invalid plugin call")?);
        if bytes.len() > LIMIT {
            return Err("Plugin request exceeds limit".into());
        }
        self.input
            .write_u32(bytes.len() as u32)
            .await
            .map_err(|_| "Plugin disconnected")?;
        self.input
            .write_all(&bytes)
            .await
            .map_err(|_| "Plugin disconnected")?;
        self.input
            .flush()
            .await
            .map_err(|_| "Plugin disconnected".into())
    }
    async fn next(&mut self, limit: usize) -> Result<Event> {
        let n = self
            .output
            .read_u32()
            .await
            .map_err(|_| "Plugin disconnected")? as usize;
        if !(1..=limit).contains(&n) {
            return Err("Invalid plugin frame size".into());
        }
        let mut raw = zeroize::Zeroizing::new(vec![0; n]);
        self.output
            .read_exact(&mut raw)
            .await
            .map_err(|_| "Incomplete plugin frame")?;
        let event = serde_json::from_slice(&raw).map_err(|_| "Invalid plugin response")?;
        if let Event::Error { code } = event {
            return Err(match code {
                PluginError::Reconnect => "Reconnect this subscription account",
                PluginError::Unavailable => "Plugin service unavailable",
                PluginError::RateLimited => "Subscription rate limit reached",
                PluginError::Unsupported => "Plugin operation unsupported",
            }
            .into());
        }
        Ok(event)
    }
}

fn spawn(launch: &Launch) -> Result<(tokio::process::Child, Channel)> {
    let mut cmd = Command::new(&launch.command);
    cmd.args(&launch.arguments)
        .env_clear()
        .current_dir(launch.dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        if let Some(root) = std::env::var_os("SystemRoot") {
            cmd.env("SystemRoot", root);
        }
        if launch.manifest.runtime == Some(crate::provider_plugins::Runtime::NativeSandbox) {
            cmd.env("AIPASS_PLUGIN_AGENT_PID", std::process::id().to_string());
        }
        cmd.creation_flags(0x08000000);
    }
    #[cfg(unix)]
    if launch.manifest.runtime.is_some() {
        crate::provider_plugins::sandbox::limits(&mut cmd);
    }
    let mut child = cmd
        .spawn()
        .map_err(|_| "Cannot start subscription plugin")?;
    let channel = Channel {
        input: child.stdin.take().ok_or("Missing plugin stdin")?,
        output: child.stdout.take().ok_or("Missing plugin stdout")?,
    };
    Ok((child, channel))
}

async fn initialize(launch: &Launch, channel: &mut Channel, mode: &str) -> Result<()> {
    let nonce = uuid::Uuid::new_v4().to_string();
    timeout(Duration::from_secs(5), async {
        channel
            .send(&Call::Initialize {
                protocol: 1,
                nonce: &nonce,
                provider: &launch.manifest.id,
                proxy_mode: if launch.manifest.runtime.is_some() {
                    "direct"
                } else {
                    mode
                },
            })
            .await?;
        match channel.next(4096).await? {
            Event::Ready {
                protocol: 1,
                nonce: reply,
            } if reply == nonce => Ok(()),
            _ => Err("Incompatible plugin handshake".to_string()),
        }
    })
    .await
    .map_err(|_| "Plugin handshake timed out")?
    .map_err(|error| {
        if launch.manifest.runtime.is_some() && error == "Plugin disconnected" {
            "Sandboxed plugin failed to start".to_owned()
        } else {
            error
        }
    })
}

async fn close(mut child: tokio::process::Child, mut channel: Channel) {
    let _ = timeout(Duration::from_millis(100), channel.send(&Call::Shutdown)).await;
    drop(channel);
    if timeout(Duration::from_millis(250), child.wait())
        .await
        .is_err()
    {
        let _ = child.kill().await;
    }
}

pub(crate) fn verify_plugin(launch: &Launch, mut cancel: watch::Receiver<bool>) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "Cannot start subscription plugin")?;
    runtime.block_on(async {
        if *cancel.borrow() {
            return Err("Plugin operation revoked".into());
        }
        let (mut child, mut channel) = spawn(launch)?;
        let budget =
            crate::provider_plugins::sandbox::watch_memory(child.id(), launch.manifest.runtime);
        let verify = async {
            initialize(launch, &mut channel, "direct").await?;
            // A fresh worker must accept shutdown as its very first operation.
            // EOF and a successful exit complete the exchange. Any other output is invalid.
            channel.send(&Call::Shutdown).await?;
            drop(channel.input);
            let mut extra = [0u8; 1];
            let eof = channel
                .output
                .read(&mut extra)
                .await
                .map_err(|_| "Invalid plugin response")?
                == 0;
            if !eof || !child.wait().await.is_ok_and(|status| status.success()) {
                return Err("Plugin shutdown verification failed".into());
            }
            Ok::<_, String>(())
        };
        let result = tokio::select! { biased;
            _ = cancel.changed() => Err("Plugin operation revoked"),
            _ = budget => Err("Plugin worker exceeded its memory budget"),
            result = timeout(Duration::from_secs(8), verify) => match result {
                Ok(Ok(())) => Ok(()),
                _ => Err("Plugin protocol verification failed"),
            },
        };
        if let Err(error) = result {
            let _ = child.kill().await;
            return Err(error.into());
        }
        Ok(())
    })
}

pub(super) async fn run(
    ctx: &mut Context,
    launch: Launch,
    mut cancel: watch::Receiver<bool>,
) -> Result<()> {
    // Wait until the caller registers the operation before executing user code.
    let initial = tokio::select! {
        _ = cancel.changed() => return Err("Plugin operation cancelled".into()),
        initial = ctx.read() => initial?,
    };
    if *cancel.borrow() {
        return Err("Plugin operation cancelled".into());
    }
    let mode = match ctx.outbound.as_ref().map(|p| &p.mode) {
        None | Some(aipass_proxy::UpstreamProxyMode::System) => "system",
        Some(aipass_proxy::UpstreamProxyMode::Direct) => "direct",
        _ => return Err("Subscription plugins require system or direct proxy mode".into()),
    };
    if initial["provider"] != launch.manifest.id {
        return Err("Plugin provider binding changed".into());
    }
    if initial["op"] != "login" && initial["auth"]["pluginRevision"] != launch.digest {
        return Err("Reconnect this account to authorize the new plugin version".into());
    }
    let (child, mut channel) = spawn(&launch)?;
    let budget =
        crate::provider_plugins::sandbox::watch_memory(child.id(), launch.manifest.runtime);
    let session = async {
        initialize(&launch, &mut channel, mode).await?;
        exchange(ctx, &launch, &mut channel, initial).await
    };
    let result = tokio::select! { biased;
        _ = cancel.changed() => Err("Plugin operation cancelled".into()),
        _ = budget => Err("Plugin worker exceeded its memory budget".into()),
        result = timeout(Duration::from_secs(1200),session) => result.unwrap_or_else(|_|Err("Plugin operation timed out".into())),
    };
    close(child, channel).await;
    result
}

fn valid_label(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}
async fn models(ctx: &mut Context, items: Vec<PluginModel>) -> Result<()> {
    if items.len() > 4096 {
        return Err("Plugin model catalog exceeds limit".into());
    }
    let mut map = serde_json::Map::new();
    for m in items {
        if !valid_label(&m.id)
            || !valid_label(&m.name)
            || !matches!(m.wire.as_str(), "chat" | "anthropic" | "responses")
            || map.contains_key(&m.id)
        {
            return Err("Invalid plugin model".into());
        }
        map.insert(m.id.clone(), json!({"id":m.id,"name":m.name,"wire":m.wire}));
    }
    ctx.acknowledged(json!({"type":"models","value":map})).await
}
async fn save(ctx: &mut Context, launch: &Launch, id: &str, state: SensitiveString) -> Result<()> {
    if !valid_label(id) || state.expose().len() > 256 * 1024 {
        return Err("Invalid plugin account".into());
    }
    ctx.save(
        json!({"type":"oauth","accountId":id,"pluginRevision":launch.digest,"pluginState":state,"pluginName":launch.manifest.name}),
    )
    .await
}

async fn exchange(
    ctx: &mut Context,
    launch: &Launch,
    io: &mut Channel,
    mut first: Value,
) -> Result<()> {
    let op = s(&first, "op").to_owned();
    let id = s(&first["auth"], "accountId").to_owned();
    let state = SensitiveString::new(s(&first["auth"], "pluginState"));
    match op.as_str() {
        "login" => {
            let index = first["method"]
                .as_u64()
                .and_then(|i| usize::try_from(i).ok())
                .ok_or("Invalid plugin method")?;
            let method = launch
                .manifest
                .methods
                .get(index)
                .ok_or("Invalid plugin method")?;
            io.send(&Call::Login {
                method: index,
                inputs: method.inputs(
                    serde_json::from_value(first["inputs"].take())
                        .map_err(|_| "Invalid plugin inputs")?,
                )?,
                key: serde_json::from_value(first["key"].take())
                    .map_err(|_| "Invalid plugin key")?,
            })
            .await?;
        }
        "refresh" => {
            io.send(&Call::Refresh {
                account_id: &id,
                state: &state,
            })
            .await?
        }
        "request" => {
            let model = s(&first, "model");
            let wire = s(&first["models"][model], "wire");
            if !matches!(wire, "chat" | "responses" | "anthropic") {
                return Err("Unknown plugin model".into());
            }
            ctx.emit(json!({"type":"prepared","value":{"model":model,"wire":wire}}))
                .await?;
            let mut next = ctx.read().await?;
            if next["type"] != "request" {
                return Err("Plugin request cancelled".into());
            }
            let body = next["body"].take();
            aipass_proxy_conversion::providers::validate_request(&body)?;
            io.send(&Call::Generate {
                account_id: &id,
                state: &state,
                model,
                wire,
                body,
            })
            .await?;
        }
        _ => return Err("Unsupported plugin operation".into()),
    }
    let mut headers = false;
    let mut requests = 0;
    loop {
        match io.next(LIMIT).await? {
            Event::Http(request) if launch.manifest.runtime.is_some() && requests < 16 => {
                requests += 1;
                let (status, body) =
                    crate::provider_plugins::network::request(&launch.manifest.origins, request)
                        .await?;
                io.send(&Call::HttpResult { status, body }).await?;
            }
            Event::Challenge {
                url,
                instructions,
                manual,
            } if op == "login" => {
                if launch.manifest.runtime.is_some()
                    && !url.is_empty()
                    && !crate::provider_plugins::network::approved(&launch.manifest.origins, &url)
                {
                    return Err("Plugin sign-in origin not authorized".into());
                }
                if let Some(code) = ctx.challenge(&url, &instructions, manual).await? {
                    io.send(&Call::Code {
                        code: SensitiveString::new(code),
                    })
                    .await?;
                }
            }
            Event::Connected {
                account_id,
                state,
                models: items,
            } if op == "login" => {
                models(ctx, items).await?;
                save(ctx, launch, &account_id, state).await?;
                return ctx
                    .emit(json!({"type":"result","value":{"identity":account_id}}))
                    .await;
            }
            Event::Rotated { state } if op != "login" && !headers => {
                save(ctx, launch, &id, state).await?;
                io.send(&Call::Ack).await?;
            }
            Event::Usage {
                plan,
                balance,
                until,
                windows,
                sections,
                models: items,
            } if op == "refresh" => {
                if sections.len() > 8
                    || !sections.iter().all(|s| s.valid())
                    || serde_json::to_vec(&sections)
                        .map_err(|_| "Invalid plugin panels")?
                        .len()
                        > 64 * 1024
                {
                    return Err("Invalid plugin panels".into());
                }
                if windows.len() > 128
                    || windows
                        .iter()
                        .any(|w| !valid_label(&w.name) || !(0.0..=100.0).contains(&w.used))
                {
                    return Err("Invalid plugin quota window".into());
                }
                if let Some(items) = items {
                    models(ctx, items).await?;
                }
                return ctx.emit(json!({"type":"result","value":{"plan":plan,"balance":balance,"until":until,"windows":windows,"sections":sections}})).await;
            }
            Event::Headers {
                status,
                content_type,
            } if op == "request"
                && !headers
                && (200..=599).contains(&status)
                && matches!(
                    content_type.as_str(),
                    "application/json" | "text/event-stream"
                ) =>
            {
                ctx.headers(status, json!([["content-type", content_type]]))
                    .await?;
                headers = true;
            }
            Event::Data { base64 } if headers && base64.len() <= 65536 => {
                let data = STANDARD
                    .decode(base64)
                    .map_err(|_| "Invalid plugin chunk")?;
                ctx.data(&data).await?;
                io.send(&Call::Ack).await?;
            }
            Event::End if headers => return ctx.emit(json!({"type":"end"})).await,
            _ => return Err("Unexpected plugin event".into()),
        }
    }
}
