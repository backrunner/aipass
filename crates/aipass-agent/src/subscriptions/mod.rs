//! Rust subscription transports and explicitly trusted local provider subprocesses.
//! Wire transformations live in aipass-proxy-conversion; this module owns account
//! authentication, HTTP, cancellation, and durable credential rotation.
use aipass_crypto::SecretString;
use aipass_proxy::UpstreamProxyConfig;
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use futures_util::StreamExt;
use reqwest::{Client, RequestBuilder, Response};
use serde_json::{json, Value};
use sha2::Digest;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::{mpsc, watch};

mod allowance;
mod antigravity;
mod cline;
mod device;
mod gateways;
mod kimi_work;
mod minimax;
mod opencode;
mod plan_quota;
mod plans;
mod plugin;
pub(crate) use plugin::verify_plugin;
mod trae;
use allowance::timestamp;
pub(crate) mod cli_accounts;
mod commandcode;
pub(crate) mod cursor;
pub(crate) mod devin;
mod factory;
pub(crate) mod feedback;
mod grok;
mod kiro;
mod lifecycle;
mod loopback;
use lifecycle::{run_cancellable, transport};
mod mimo;
mod native_cli;
pub(crate) mod native_import;
mod qoder;
mod renewal;
#[cfg(test)]
pub(crate) mod tests;
mod workbuddy;
mod zcode;
mod zed;

type Result<T> = std::result::Result<T, String>;
const LIMIT: usize = 8 * 1024 * 1024;
pub(super) struct Operation {
    input: mpsc::Sender<Value>,
    cancel: watch::Sender<bool>,
    pub canceled: AtomicBool,
    completion: Option<std::sync::Mutex<std::sync::mpsc::Receiver<()>>>,
}
impl Operation {
    pub(crate) fn running(&self) -> bool {
        !self.input.is_closed()
    }
    pub fn send(&self, value: &Value) -> Result<()> {
        // A canceled caller still owes the durable credential ACK. All other
        // messages stop immediately, so no generation starts after cancellation.
        if self.canceled.load(Ordering::Acquire)
            && !matches!(value["type"].as_str(), Some("ack" | "reloaded"))
        {
            return Err("subscription operation cancelled".into());
        }
        self.input
            .try_send(value.clone())
            .map_err(|_| "subscription operation closed or busy".into())
    }
    pub(crate) fn wait_for_plugin_exit(&self, deadline: std::time::Instant) {
        if let Some(rx) = self.completion.as_ref().and_then(|rx| rx.lock().ok()) {
            let _ = rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()));
        }
    }
    pub fn kill(&self) {
        self.canceled.store(true, Ordering::Release);
        let _ = self.cancel.send(true);
    }
}
impl Drop for Operation {
    fn drop(&mut self) {
        self.kill();
    }
}
pub(super) struct Adapter {
    pub process: Arc<Operation>,
    output: mpsc::Receiver<Value>,
}
impl Adapter {
    pub fn start(proxy: Option<&UpstreamProxyConfig>) -> Result<Self> {
        Self::start_with_plugin(proxy, None)
    }
    pub fn start_with_plugin(
        proxy: Option<&UpstreamProxyConfig>,
        plugin: Option<crate::provider_plugins::Launch>,
    ) -> Result<Self> {
        let (finished, completion) = plugin.as_ref().map(|_| std::sync::mpsc::channel()).unzip();
        let client = transport(proxy)?;
        let (input, rx) = mpsc::channel(8);
        let (tx, output) = mpsc::channel(8);
        let (cancel, cancelled) = watch::channel(false);
        let (refreshing, refresh_phase) = watch::channel(false);
        let (generating, generation_phase) = watch::channel(false);
        let outbound = proxy.cloned();
        std::thread::Builder::new()
            .name("subscription-adapter".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                let Ok(runtime) = runtime else {
                    let _ = tx.blocking_send(
                        json!({"type":"error","message":"subscription executor unavailable"}),
                    );
                    return;
                };
                runtime.block_on(async move {
                    let mut context = Context {
                        client,
                        outbound,
                        input: rx,
                        output: tx.clone(),
                        provider: String::new(),
                        auth: Value::Null,
                        native_token: None,
                        native_workspace: String::new(),
                        models: json!({}),
                        session: String::new(),
                        sequence: 0,
                        feedback: Default::default(),
                        policy: Default::default(),
                        history_owner: None,
                        session_binding: String::new(),
                        cancellation: cancelled.clone(),
                        refreshing,
                        generating,
                    };
                    let result = match plugin {
                        Some(plugin) => plugin::run(&mut context, plugin, cancelled).await,
                        None => {
                            run_cancellable(
                                context.run(),
                                cancelled,
                                refresh_phase,
                                generation_phase,
                            )
                            .await
                        }
                    };
                    drop(finished);
                    if let Err(message) = result {
                        let _ = tx.send(json!({"type":"error","message":message})).await;
                    }
                });
            })
            .map_err(|_| "cannot start subscription executor")?;
        Ok(Self {
            process: Arc::new(Operation {
                input,
                completion: completion.map(std::sync::Mutex::new),
                cancel,
                canceled: AtomicBool::new(false),
            }),
            output,
        })
    }
    pub fn next(&mut self) -> Result<Value> {
        let frame = self
            .output
            .blocking_recv()
            .ok_or("subscription operation ended before completion")?;
        if frame["type"] == "error" {
            return Err(s(&frame, "message").to_owned());
        }
        Ok(frame)
    }
    pub fn ack(&self, frame: &Value, ok: bool) -> Result<()> {
        self.process
            .send(&json!({"type":"ack","id":frame["id"],"ok":ok}))
    }
}
impl Drop for Adapter {
    fn drop(&mut self) {
        self.process.kill();
    }
}

struct Context {
    client: Client,
    outbound: Option<UpstreamProxyConfig>,
    input: mpsc::Receiver<Value>,
    output: mpsc::Sender<Value>,
    provider: String,
    auth: Value,
    native_token: Option<aipass_agent_protocol::SensitiveString>,
    native_workspace: String,
    models: Value,
    session: String,
    sequence: u64,
    feedback: feedback::Feedback,
    policy: aipass_agent_protocol::SubscriptionPolicy,
    history_owner: Option<uuid::Uuid>,
    session_binding: String,
    cancellation: watch::Receiver<bool>,
    refreshing: watch::Sender<bool>,
    generating: watch::Sender<bool>,
}

struct RefreshGuard(watch::Sender<bool>);
impl Drop for RefreshGuard {
    fn drop(&mut self) {
        self.0.send_replace(false);
    }
}
impl Context {
    fn refresh_guard(&self) -> Result<RefreshGuard> {
        if *self.cancellation.borrow() {
            return Err("subscription operation cancelled".into());
        }
        self.refreshing.send_replace(true);
        Ok(RefreshGuard(self.refreshing.clone()))
    }
    async fn emit(&self, value: Value) -> Result<()> {
        self.output
            .send(value)
            .await
            .map_err(|_| "subscription operation cancelled".into())
    }
    async fn read(&mut self) -> Result<Value> {
        self.input
            .recv()
            .await
            .ok_or_else(|| "subscription operation cancelled".into())
    }
    async fn acknowledged(&mut self, mut value: Value) -> Result<()> {
        self.sequence += 1;
        value["id"] = json!(self.sequence);
        self.emit(value).await?;
        let reply = tokio::time::timeout(Duration::from_secs(30), self.read())
            .await
            .map_err(|_| "account update timed out")??;
        if reply["type"] != "ack" || reply["id"] != self.sequence || reply["ok"] != true {
            return Err("Agent refused account update".into());
        }
        Ok(())
    }
    async fn save(&mut self, mut auth: Value) -> Result<()> {
        renewal::retain_rejections(&self.auth, &mut auth);
        if auth["nativeSource"].is_null() && !self.auth["nativeSource"].is_null() {
            auth["nativeSource"] = self.auth["nativeSource"].clone();
            auth["nativeDevice"] = self.auth["nativeDevice"].clone();
        }
        super::community::ensure_owner(&self.auth, &auth)?;
        self.acknowledged(json!({"type":"auth","value":auth}))
            .await?;
        native_import::erase(&mut self.auth);
        self.auth = auth;
        if *self.cancellation.borrow() {
            Err("subscription operation cancelled".into())
        } else {
            Ok(())
        }
    }
    async fn challenge(
        &mut self,
        url: &str,
        instructions: &str,
        manual: bool,
    ) -> Result<Option<String>> {
        if !url.is_empty() {
            let u = url::Url::parse(url).map_err(|_| "invalid sign-in URL")?;
            if u.scheme() != "https" || !u.username().is_empty() || u.password().is_some() {
                return Err("invalid sign-in URL".into());
            }
        }
        let user_code = if matches!(self.provider.as_str(), "grok" | "copilot") && !manual {
            native_cli::device_user_code(instructions)
        } else {
            None
        };
        self.emit(json!({"type":"challenge","value":{"url":url,"instructions":instructions,"userCode":user_code,"method":if manual {"code"} else {"auto"}}})).await?;
        if !manual {
            return Ok(None);
        }
        let frame = self.read().await?;
        if frame["type"] != "code" {
            return Err("sign-in code missing".into());
        }
        Ok(Some(s(&frame, "code").to_owned()))
    }
    fn token(&self) -> Result<&str> {
        if let Some(token) = &self.native_token {
            return Ok(token.expose());
        }
        self.auth["access"]
            .as_str()
            .or(self.auth["key"].as_str())
            .filter(|x| !x.is_empty())
            .ok_or_else(|| "subscription credentials missing".into())
    }
    async fn catalog(&mut self) -> Result<()> {
        let live = match self.provider.as_str() {
            "workbuddy" | "workbuddy-ai" => workbuddy::models(self).await,
            "opencode-go" | "opencode-zen" => opencode::models(self).await,
            "antigravity" => antigravity::models(self).await,
            "kimi-work" => kimi_work::models(self).await,
            "minimax-code" | "minimax-code-global" => minimax::models(self).await,
            "cline" => cline::models(self).await,
            "trae-cn" | "trae-global" => trae::models(self).await,
            id if plans::PROVIDERS.contains(&id) => plans::models(self).await,
            id if gateways::PROVIDERS.contains(&id) => gateways::models(self).await,
            "zcode" => zcode::models(self).await,
            "grok" => grok::models(self).await,
            "codex" | "copilot" | "gemini-cli" => cli_accounts::models(self).await,
            "commandcode-plan" => commandcode::models(self).await,
            "zed" => zed::models(self).await,
            "kiro" => kiro::models(self).await,
            "qoder" | "qoder-cn" => qoder::models(self).await,
            "devin" => devin::models(self).await,
            "cursor" => cursor::models(self).await,
            _ => Ok(self.models.clone()),
        };
        // A failed discovery must never erase the last known catalog. Static
        // metadata is only a fallback until the provider answers successfully.
        if let Ok(models) = live {
            if models
                .as_object()
                .is_some_and(|m| !m.is_empty() && m.len() <= 4096)
            {
                self.models = models;
            }
        }
        if self.models.as_object().is_none_or(|m| m.is_empty()) {
            return Err("No models are available; check the account and reconnect".into());
        }
        self.acknowledged(json!({"type":"models","value":self.models}))
            .await
    }
    async fn fresh(&mut self) -> Result<()> {
        match self.provider.as_str() {
            "factory" => factory::fresh(self).await,
            "workbuddy" | "workbuddy-ai" => workbuddy::fresh(self).await,
            "opencode-go" | "opencode-zen" => opencode::fresh(self).await,
            "antigravity" => antigravity::fresh(self).await,
            "kimi-work" => kimi_work::fresh(self).await,
            "minimax-code" | "minimax-code-global" => minimax::fresh(self).await,
            "cline" => cline::fresh(self).await,
            "trae-cn" | "trae-global" => trae::fresh(self).await,
            id if plans::PROVIDERS.contains(&id) => plans::fresh(self).await,
            id if gateways::PROVIDERS.contains(&id) => gateways::fresh(self).await,
            "mimo-app" => mimo::fresh(self).await,
            "zcode" => zcode::fresh(self).await,
            "grok" => grok::fresh(self).await,
            "codex" | "copilot" | "gemini-cli" => cli_accounts::fresh(self).await,
            "commandcode-plan" => commandcode::fresh(self).await,
            "zed" => zed::fresh(self).await,
            "kiro" => kiro::fresh(self).await,
            "qoder" | "qoder-cn" => qoder::fresh(self).await,
            "devin" => devin::fresh(self).await,
            "cursor" => cursor::fresh(self).await,
            _ => Err("unknown subscription provider".into()),
        }
    }
    async fn run(&mut self) -> Result<()> {
        let initial = self.read().await?;
        let catalog: Value = serde_json::from_str(include_str!("catalog.json"))
            .map_err(|_| "invalid provider catalog")?;
        if initial["op"] == "catalog" {
            let list: Vec<Value> = catalog
                .as_array()
                .ok_or("invalid provider catalog")?
                .iter()
                .cloned()
                .map(|mut p| {
                    p["localAuthorization"] =
                        json!(crate::community::supports_local_authorization(s(&p, "id")));
                    p.as_object_mut().unwrap().remove("models");
                    p
                })
                .collect();
            return self.emit(json!({"type":"result","value":list})).await;
        }
        self.provider = s(&initial, "provider").into();
        let config = catalog
            .as_array()
            .and_then(|a| a.iter().find(|p| p["id"] == self.provider))
            .ok_or("unknown subscription provider")?;
        self.auth = initial["auth"].clone();
        self.session_binding = initial["binding"].as_str().unwrap_or("").into();
        self.feedback = serde_json::from_value(initial["feedback"].clone()).unwrap_or_default();
        self.policy = serde_json::from_value(initial["policy"].clone()).unwrap_or_default();
        self.history_owner = initial["owner"]
            .as_str()
            .and_then(|s| uuid::Uuid::parse_str(s).ok());
        if !self.auth["nativeSource"].is_null()
            && self.auth["nativeDevice"] != cli_accounts::device()?
        {
            return Err("Native account belongs to another device; reconnect locally".into());
        }
        self.models = if initial["models"].as_object().is_some_and(|m| !m.is_empty()) {
            initial["models"].clone()
        } else {
            config["models"].clone()
        };
        // Migrate old, encrypted catalogs to explicit native wire metadata.
        for (_, model) in self.models.as_object_mut().ok_or("invalid model catalog")? {
            if model["wire"].is_null() {
                model["wire"] = json!(match model
                    .pointer("/api/npm")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                {
                    "@ai-sdk/anthropic" => "anthropic",
                    "@ai-sdk/openai" => "responses",
                    "@ai-sdk/google" => "gemini",
                    _ => "chat",
                });
            }
        }
        if self.provider == "factory" {
            factory::normalize_models(&mut self.models);
        }
        self.session = initial["session"]
            .as_str()
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                if matches!(self.provider.as_str(), "qoder" | "qoder-cn") {
                    // Qoder derives a stable fallback from the request's opening.
                    String::new()
                } else {
                    uuid::Uuid::new_v4().to_string()
                }
            });
        if initial["op"] == "login" {
            let method = initial["method"].as_u64().ok_or("invalid sign-in method")? as usize;
            let descriptor = config["methods"]
                .get(method)
                .ok_or("unknown sign-in method")?;
            for prompt in descriptor["prompts"].as_array().into_iter().flatten() {
                if prompt["type"] == "select"
                    && !prompt["options"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|o| o["value"] == initial["inputs"][s(prompt, "key")])
                {
                    return Err("choose a valid sign-in option".into());
                }
            }
            let auth = if descriptor["type"] == "api" {
                let key = s(&initial, "key");
                if key.trim().is_empty() {
                    return Err("API key is required".into());
                }
                json!({"type":"api","key":key,"metadata":initial["inputs"]})
            } else {
                match self.provider.as_str() {
                    "factory" => factory::login(self, method).await?,
                    "workbuddy" | "workbuddy-ai" => workbuddy::login(self, method).await?,
                    "opencode-go" | "opencode-zen" => opencode::login(self, method).await?,
                    "antigravity" => antigravity::login(self, method).await?,
                    "kimi-work" => kimi_work::login(self, method).await?,
                    "minimax-code" | "minimax-code-global" => minimax::login(self, method).await?,
                    "cline" => cline::login(self, method).await?,
                    "trae-cn" | "trae-global" => trae::login(self, method).await?,
                    "mimo-app" => mimo::login(self, method).await?,
                    "zcode" => zcode::login(self, method).await?,
                    "grok" => grok::login(self, method).await?,
                    "codex" | "copilot" | "gemini-cli" => cli_accounts::login(self, method).await?,
                    "commandcode-plan" => commandcode::login(self, method).await?,
                    "zed" => zed::login(self, method).await?,
                    "kiro" => kiro::login(self, method).await?,
                    "qoder" | "qoder-cn" => qoder::login(self, method).await?,
                    "devin" => devin::login(self, method).await?,
                    "cursor" => cursor::login(self, method).await?,
                    _ => return Err("unknown subscription provider".into()),
                }
            };
            self.save(auth).await?;
            if initial["nativeVerifyOnly"] == true && self.provider == "codex" {
                cli_accounts::verify(self).await?;
                return self.emit(json!({"type":"result","value":{"identity":super::community::identity(&self.auth).unwrap_or_default(),"nativeMethod":method}})).await;
            }
            self.fresh().await?;
            self.catalog().await?;
            return self.emit(json!({"type":"result","value":{"identity":super::community::identity(&self.auth).unwrap_or_default(),"nativeMethod":if descriptor["native"] == true { json!(method) } else { Value::Null }}})).await;
        }
        if initial["op"] == "native_verify" {
            if self.provider != "codex" {
                return Err("native verification requires Codex".into());
            }
            cli_accounts::verify(self).await?;
            return self
                .emit(json!({"type":"result","value":{"verified":true}}))
                .await;
        }
        self.fresh().await?;
        self.check_revoked()?;
        if initial["op"] == "reset" {
            if self.provider != "codex" {
                return Err("Reset credits require Codex".into());
            }
            let request = serde_json::from_value(initial["reset"].clone())
                .map_err(|_| "Invalid reset request")?;
            let value = cli_accounts::consume_reset(self, request).await?;
            return self.emit(json!({"type":"result","value":value})).await;
        }
        if initial["op"] == "refresh" {
            self.catalog().await?;
            let usage = match self.provider.as_str() {
                "factory" => factory::usage(self).await?,
                "workbuddy" | "workbuddy-ai" => workbuddy::usage(self).await?,
                "opencode-go" | "opencode-zen" => opencode::usage(self).await?,
                "antigravity" => antigravity::usage(self).await?,
                "kimi-work" => kimi_work::usage(self).await?,
                "minimax-code" | "minimax-code-global" => minimax::usage(self).await?,
                "cline" => cline::usage(self).await?,
                "trae-cn" | "trae-global" => trae::usage(self).await?,
                id if plans::PROVIDERS.contains(&id) => plans::usage(self).await?,
                id if gateways::PROVIDERS.contains(&id) => gateways::usage(self).await?,
                "mimo-app" => mimo::usage(self).await?,
                "zcode" => zcode::usage(self).await?,
                "grok" => grok::usage(self).await?,
                "codex" | "copilot" | "gemini-cli" => cli_accounts::usage(self).await?,
                "commandcode-plan" => commandcode::usage(self).await?,
                "zed" => zed::usage(self).await?,
                "kiro" => kiro::usage(self).await?,
                "qoder" | "qoder-cn" => qoder::usage(self).await?,
                "devin" => devin::usage(self).await?,
                "cursor" => cursor::usage(self).await?,
                _ => return Err("unknown subscription provider".into()),
            };
            return self.emit(json!({"type":"result","value":usage})).await;
        }
        if initial["op"] != "request" {
            return Err("unknown subscription operation".into());
        }
        let id = s(&initial, "model");
        if self.models[id].is_null() {
            self.catalog().await?;
        }
        let model = self.models[id].clone();
        if model.is_null() {
            return Err("model is not available for this subscription".into());
        }
        self.emit(json!({"type":"prepared","value":{"wire":model["wire"],"model":model.pointer("/api/id").and_then(Value::as_str).unwrap_or(id)}})).await?;
        let request = self.read().await?;
        if request["type"] != "request" {
            return Err("request preparation cancelled".into());
        }
        let body = request["body"].clone();
        aipass_proxy_conversion::providers::validate_request(&body)?;
        self.generating.send_replace(true);
        match self.provider.as_str() {
            "factory" => factory::generate(self, &model, body).await,
            "workbuddy" | "workbuddy-ai" => workbuddy::generate(self, &model, body).await,
            "opencode-go" | "opencode-zen" => opencode::generate(self, &model, body).await,
            "antigravity" => antigravity::generate(self, &model, body).await,
            "kimi-work" => kimi_work::generate(self, &model, body).await,
            "minimax-code" | "minimax-code-global" => minimax::generate(self, &model, body).await,
            "cline" => cline::generate(self, &model, body).await,
            "trae-cn" | "trae-global" => trae::generate(self, &model, body).await,
            id if plans::PROVIDERS.contains(&id) => plans::generate(self, &model, body).await,
            id if gateways::PROVIDERS.contains(&id) => gateways::generate(self, &model, body).await,
            "mimo-app" => mimo::generate(self, &model, body).await,
            "zcode" => zcode::generate(self, &model, body).await,
            "grok" => grok::generate(self, &model, body).await,
            "codex" | "copilot" | "gemini-cli" => cli_accounts::generate(self, &model, body).await,
            "commandcode-plan" => commandcode::generate(self, &model, body).await,
            "zed" => zed::generate(self, &model, body).await,
            "kiro" => kiro::generate(self, &model, body).await,
            "qoder" | "qoder-cn" => qoder::generate(self, &model, body).await,
            "devin" => devin::generate(self, &model, body).await,
            "cursor" => cursor::generate(self, &model, body).await,
            _ => Err("unknown subscription provider".into()),
        }
    }
    async fn headers(&self, status: u16, headers: Value) -> Result<()> {
        self.emit(json!({"type":"headers","value":{"status":status,"headers":headers}}))
            .await
    }
    async fn data(&mut self, data: &[u8]) -> Result<()> {
        for chunk in data.chunks(48 * 1024) {
            self.acknowledged(json!({"type":"data","value":STANDARD.encode(chunk)}))
                .await?;
        }
        Ok(())
    }
    async fn pipe(&mut self, response: Response) -> Result<()> {
        if response.status().is_success() {
            let _ = self.served().await;
        }
        let _ = self.observe_headers(response.headers()).await;
        let status = response.status().as_u16();
        let headers: Vec<_> = response
            .headers()
            .iter()
            .filter_map(|(k, v)| v.to_str().ok().map(|v| json!([k.as_str(), v])))
            .collect();
        if status == 401 {
            let mut stream = response.bytes_stream();
            let mut body = Vec::new();
            while let Some(bytes) = stream.next().await {
                let bytes = bytes.map_err(|_| "subscription response interrupted")?;
                if body.len() + bytes.len() > 64 * 1024 {
                    return Err("subscription error exceeds limit".into());
                }
                body.extend_from_slice(&bytes);
            }
            let _ = self.refused(&body).await;
            self.headers(status, json!(headers)).await?;
            self.data(&body).await?;
            return self.emit(json!({"type":"end"})).await;
        }
        self.headers(status, json!(headers)).await?;
        let mut stream = response.bytes_stream();
        let mut count = 0;
        let mut events = feedback::Events::default();
        while let Some(bytes) = stream.next().await {
            let bytes = bytes.map_err(|_| "subscription stream interrupted")?;
            count += bytes.len();
            if count > 128 * 1024 * 1024 {
                return Err("subscription reply exceeds limit".into());
            }
            if self.provider == "codex" {
                for event in events.push(&bytes) {
                    let _ = self.observe_event(&event).await;
                }
            }
            self.data(&bytes).await?;
        }
        self.emit(json!({"type":"end"})).await
    }
    async fn converted(
        &mut self,
        response: Response,
        converter: &mut impl aipass_proxy_conversion::providers::ProviderStream,
    ) -> Result<()> {
        if !response.status().is_success() {
            return self.pipe(response).await;
        }
        let _ = self.served().await;
        let mut stream = response.bytes_stream();
        let mut sent = false;
        let mut count = 0usize;
        loop {
            let next = stream.next().await;
            let last = next.is_none();
            let result = if let Some(bytes) = next {
                let bytes = bytes.map_err(|_| "subscription stream interrupted")?;
                count += bytes.len();
                if count > 128 * 1024 * 1024 {
                    return Err("subscription reply exceeds limit".into());
                }
                converter.push(&bytes)
            } else {
                converter.finish()
            };
            let chunks = match result {
                Ok(chunks) => chunks,
                Err(error) => {
                    if sent || converter.has_output() {
                        return Err(error);
                    }
                    let Some(status) = converter.failure_status() else {
                        return Err(error);
                    };
                    self.headers(status, json!([["content-type", "application/json"]]))
                        .await?;
                    self.data(
                        json!({"error":{"message":error,"type":"subscription_error"}})
                            .to_string()
                            .as_bytes(),
                    )
                    .await?;
                    return self.emit(json!({"type":"end"})).await;
                }
            };
            if !chunks.is_empty() && !sent {
                self.headers(200, json!([["content-type", "text/event-stream"]]))
                    .await?;
                sent = true;
            }
            for chunk in chunks {
                self.data(&chunk).await?;
            }
            if last {
                break;
            }
        }
        if !sent {
            return Err("subscription returned no events".into());
        }
        self.emit(json!({"type":"end"})).await
    }
}
fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn expires_after(value: &Value, default: u64, unit_ms: u64) -> u64 {
    let lifetime = value.as_u64().filter(|n| *n > 0).unwrap_or(default);
    now().saturating_add(lifetime.saturating_mul(unit_ms).min(30 * 86400 * 1000))
}
pub(super) fn claims(token: &str) -> Value {
    token
        .split('.')
        .nth(1)
        .and_then(|s| URL_SAFE_NO_PAD.decode(s).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(Value::Null)
}
fn expires(token: &str) -> u64 {
    claims(token)["exp"]
        .as_u64()
        .unwrap_or(0)
        .saturating_mul(1000)
}
fn hex_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
async fn send(request: RequestBuilder) -> Result<Response> {
    request.send().await.map_err(|e| {
        if e.is_timeout() {
            "subscription request timed out".into()
        } else {
            "subscription transport failed".into()
        }
    })
}
async fn raw_json(request: RequestBuilder) -> Result<(u16, Value)> {
    let response = send(request.timeout(Duration::from_secs(30))).await?;
    let status = response.status().as_u16();
    Ok((
        status,
        serde_json::from_slice(&read_bytes(response, LIMIT).await?)
            .map_err(|_| "provider returned invalid JSON")?,
    ))
}
async fn json_request(request: RequestBuilder) -> Result<Value> {
    let (status, value) = raw_json(request).await?;
    if !(200..300).contains(&status) {
        return Err(format!("subscription API returned HTTP {status}"));
    }
    Ok(value)
}
async fn read_bytes(response: Response, limit: usize) -> Result<Vec<u8>> {
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "subscription response interrupted")?;
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err("subscription response exceeds limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
fn home() -> Result<std::path::PathBuf> {
    directories::BaseDirs::new()
        .map(|d| d.home_dir().to_owned())
        .ok_or_else(|| "home directory unavailable".into())
}
fn read_json(path: &std::path::Path) -> Result<Value> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            "native sign-in permission denied"
        } else {
            "native sign-in not found; sign in to the provider application first"
        }
    })?;
    let mut bytes = zeroize::Zeroizing::new(Vec::new());
    file.take(LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read native sign-in")?;
    if bytes.len() > LIMIT {
        return Err("native sign-in exceeds limit".into());
    }
    serde_json::from_slice(&bytes)
        .or_else(|_| {
            let text = std::str::from_utf8(&bytes).map_err(|_| ())?;
            json5::from_str(text).map_err(|_| ())
        })
        .map_err(|_| "invalid native sign-in".into())
}
fn model(id: &str, name: &str, wire: &str, base: &str, context: u64, output: u64) -> Value {
    json!({"id":id,"name":name,"wire":wire,"api":{"id":id,"url":base},"limit":{"context":context,"output":output},"tool_call":true})
}
fn path(wire: &str) -> &'static str {
    match wire {
        "anthropic" => "/messages",
        "responses" => "/responses",
        _ => "/chat/completions",
    }
}

fn vendor_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(windows) {
        "win32"
    } else {
        std::env::consts::OS
    }
}

fn identifier(v: &Value) -> String {
    v.as_str()
        .map(str::to_owned)
        .or_else(|| v.as_u64().map(|n| n.to_string()))
        .unwrap_or_default()
}
