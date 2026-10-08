//! Cancel-safe streaming and model-specific subscription dispatch.
use super::*;

pub(super) fn operation(op: &str, account: &Account) -> Value {
    json!({"op":op,"provider":account.provider,"auth":serde_json::from_str::<Value>(account.auth.expose()).unwrap_or(Value::Null),"models":account.models,"nativeMethod":account.native_method})
}
type BodyItem = Result<Bytes, Box<dyn std::error::Error + Send + Sync>>;
pub(super) struct GenerationOutput {
    pub(super) head: oneshot::Sender<Result<(SubscriptionResponse, String), String>>,
    pub(super) body_tx: mpsc::Sender<BodyItem>,
    pub(super) body_rx: mpsc::Receiver<BodyItem>,
    pub(super) cancel: Arc<Cancel>,
}
#[derive(Default)]
pub(super) struct Cancel {
    pub(super) canceled: AtomicBool,
    pub(super) process: Mutex<Weak<Operation>>,
}
impl Cancel {
    pub(super) fn attach(&self, p: &Arc<Operation>) {
        if let Ok(mut current) = self.process.lock() {
            *current = Arc::downgrade(p);
            if self.canceled.load(Ordering::Acquire) {
                p.kill();
            }
        }
    }
    pub(super) fn cancel(&self) {
        self.canceled.store(true, Ordering::Release);
        if let Ok(current) = self.process.lock() {
            if let Some(p) = current.upgrade() {
                p.kill();
            }
        }
    }
}
struct CancelOnDrop(Option<Arc<Cancel>>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(c) = &self.0 {
            c.cancel();
        }
    }
}
pub(super) struct Body {
    pub(super) receiver: mpsc::Receiver<Result<Bytes, Box<dyn std::error::Error + Send + Sync>>>,
    pub(super) _cancel: Arc<Cancel>,
}
impl Stream for Body {
    type Item = Result<Bytes, Box<dyn std::error::Error + Send + Sync>>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(cx)
    }
}
impl Drop for Body {
    fn drop(&mut self) {
        self._cancel.cancel();
    }
}
impl SubscriptionBackend for CommunityBridge {
    fn request(
        &self,
        target: ResolvedTarget,
        payload: Value,
        session: Option<String>,
    ) -> SubscriptionFuture {
        self.request_protocol(
            target,
            payload,
            aipass_proxy::Protocol::OpenAiChatCompletions,
            session,
        )
    }
    fn request_protocol(
        &self,
        target: ResolvedTarget,
        payload: Value,
        protocol: aipass_proxy::Protocol,
        session: Option<String>,
    ) -> SubscriptionFuture {
        let bridge = self.clone();
        Box::pin(async move {
            let streaming = payload["stream"] == true;
            let owner = target.config.id;
            let (head_tx, head_rx) = oneshot::channel();
            let (body_tx, body_rx) = mpsc::channel(8);
            let cancel = Arc::new(Cancel::default());
            let mut guard = CancelOnDrop(Some(cancel.clone()));
            let worker = bridge.clone();
            std::thread::spawn(move || {
                worker.generate(
                    target,
                    payload,
                    session,
                    protocol,
                    GenerationOutput {
                        head: head_tx,
                        body_tx,
                        body_rx,
                        cancel,
                    },
                )
            });
            let (response, wire) = head_rx.await.map_err(|_| "community worker stopped")??;
            let result = bridge
                .inner
                .codec
                .normalize_protocol(response, &wire, owner, streaming, protocol)
                .await;
            guard.0 = None;
            result
        })
    }
    fn models(&self, target: ResolvedTarget) -> SubscriptionFuture {
        let bridge = self.clone();
        Box::pin(async move {
            let account = tokio::task::spawn_blocking(move || {
                bridge.load(target.config.provider_entry_id, &target.api_key)
            })
            .await
            .map_err(|_| "model discovery stopped")??;
            let data:Vec<Value>=account.models.as_object().ok_or("model catalog unavailable")?.iter().map(|(id,m)|json!({"id":id,"object":"model","owned_by":account.provider,"name":m["name"],"context_window":m["limit"]["context"],"max_output_tokens":m["limit"]["output"],"reasoning":m["reasoning"],"tool_call":m["tool_call"]})).collect();
            let body = Bytes::from(json!({"object":"list","data":data}).to_string());
            let mut headers = hyper::HeaderMap::new();
            headers.insert(
                hyper::header::CONTENT_TYPE,
                hyper::header::HeaderValue::from_static("application/json"),
            );
            Ok(SubscriptionResponse {
                status: hyper::StatusCode::OK,
                headers,
                body: Box::pin(stream::once(async move { Ok(body) })),
            })
        })
    }
    fn history_owner(&self, payload: &Value) -> Result<Option<Uuid>, String> {
        self.inner.codec.history_owner(payload)
    }
    fn revoke(&self) {
        if let Ok(mut recovery) = self.inner.recovery.lock() {
            recovery.epoch = recovery.epoch.wrapping_add(1);
            recovery.pending.clear();
        }
        if let Ok(mut runs) = self.inner.runs.lock() {
            for run in runs.drain(..) {
                if let Some(p) = run.process.upgrade() {
                    p.kill();
                }
            }
        }
        self.inner.codec.retain_targets(&HashSet::new());
        if let Ok(mut owners) = self.inner.owners.lock() {
            owners.clear();
        }
    }
    fn retain_targets(&self, targets: &[&ResolvedTarget]) -> Vec<Uuid> {
        let mut ids: HashSet<Uuid> = targets
            .iter()
            .filter(|t| t.upstream_kind == UpstreamKind::CommunitySubscription)
            .map(|t| t.config.id)
            .collect();
        if let Ok(mut owners) = self.inner.owners.lock() {
            owners.retain(|id, generation| {
                targets.iter().any(|t| {
                    t.config.id == *id && t.api_key == format!("aipass:community:{generation}")
                })
            });
            ids.retain(|id| owners.contains_key(id));
        }
        self.inner.codec.retain_targets(&ids);
        if let Ok(mut runs) = self.inner.runs.lock() {
            runs.retain(|run| {
                let Some(p) = run.process.upgrade() else {
                    return false;
                };
                if let Some(old) = &run.target {
                    if !targets.iter().any(|t| {
                        t.config.id == old.config.id
                            && t.api_key == old.api_key
                            && t.upstream_proxy == old.upstream_proxy
                            && t.upstream_kind == old.upstream_kind
                    }) {
                        p.kill();
                        return false;
                    }
                }
                true
            });
        }
        Vec::new()
    }
}
pub(crate) struct Dispatch {
    pub claude: Arc<crate::claude_bridge::ClaudeBridge>,
    pub community: Arc<CommunityBridge>,
}
impl SubscriptionBackend for Dispatch {
    fn request_protocol(
        &self,
        t: ResolvedTarget,
        p: Value,
        protocol: aipass_proxy::Protocol,
        s: Option<String>,
    ) -> SubscriptionFuture {
        if t.upstream_kind == UpstreamKind::CommunitySubscription {
            self.community.request_protocol(t, p, protocol, s)
        } else {
            self.claude.request(t, p, s)
        }
    }
    fn request(&self, t: ResolvedTarget, p: Value, s: Option<String>) -> SubscriptionFuture {
        if t.upstream_kind == UpstreamKind::CommunitySubscription {
            self.community.request(t, p, s)
        } else {
            self.claude.request(t, p, s)
        }
    }
    fn models(&self, t: ResolvedTarget) -> SubscriptionFuture {
        if t.upstream_kind == UpstreamKind::CommunitySubscription {
            self.community.models(t)
        } else {
            self.claude.models(t)
        }
    }
    fn revoke(&self) {
        self.claude.revoke();
        self.community.revoke();
    }
    fn retain_targets(&self, t: &[&ResolvedTarget]) -> Vec<Uuid> {
        let mut ids = self.claude.retain_targets(t);
        ids.extend(self.community.retain_targets(t));
        ids
    }
    fn history_owner(&self, p: &Value) -> Result<Option<Uuid>, String> {
        let a = self.claude.history_owner(p)?;
        let b = self.community.history_owner(p)?;
        if a.is_some() && b.is_some() && a != b {
            Err("conversation mixes subscription accounts".into())
        } else {
            Ok(a.or(b))
        }
    }
}
