use super::*;
use aipass_agent_protocol::{
    BalanceEndpoint, NativeSwitchPolicy, ProviderProxyOptions, ProviderWebhook,
    ProviderWebhookEvent,
};
use aipass_proxy::UpstreamProxyMode;

pub(super) const EVENTS: &[(ProviderWebhookEvent, &str)] = &[
    (ProviderWebhookEvent::QuotaLow, "eventQuotaLow"),
    (ProviderWebhookEvent::QuotaExhausted, "eventQuotaExhausted"),
    (ProviderWebhookEvent::RateLimitDetected, "eventRateLimit"),
    (ProviderWebhookEvent::ProviderError, "eventProviderError"),
    (
        ProviderWebhookEvent::ProviderDegraded,
        "eventProviderDegraded",
    ),
    (
        ProviderWebhookEvent::CredentialFailed,
        "eventCredentialFailed",
    ),
    (
        ProviderWebhookEvent::SubscriptionExpiring,
        "eventSubExpiring",
    ),
    (ProviderWebhookEvent::SubscriptionExpired, "eventSubExpired"),
];
impl Subscriptions {
    pub(in crate::subscription_ui) fn options_choice(
        &mut self,
        key: &str,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.runtime_options.pending {
            return;
        }
        let Some(options) = &mut self.runtime_options.value else {
            return;
        };
        match key {
            "runtime-interval" => {
                if let Ok(n) = value.parse() {
                    options.quota_refresh_seconds = n;
                }
            }
            "runtime-cap" => {
                if let Ok(n) = value.parse::<u8>() {
                    options.subscription.quota_use_limit_percent = (n < 100).then_some(n);
                }
            }
            "runtime-method" => {
                if let Some(b) = &mut options.balance {
                    b.post = value == "POST";
                }
            }
            "runtime-route" => {
                if let (Some(policy), Ok(id)) = (
                    &mut options.subscription.native_switch,
                    value.parse::<Uuid>(),
                ) {
                    policy.route_id = id;
                }
            }
            "runtime-proxy" => {
                if value == "inherit" {
                    options.proxy = None;
                } else {
                    let mode = match value {
                        "system" => UpstreamProxyMode::System,
                        "direct" => UpstreamProxyMode::Direct,
                        "environment" => UpstreamProxyMode::Environment,
                        "custom" => UpstreamProxyMode::Custom,
                        _ => return,
                    };
                    options
                        .proxy
                        .get_or_insert(ProviderProxyOptions {
                            mode,
                            url: None,
                            username: None,
                            password: None,
                            has_credentials: false,
                            clear_credentials: false,
                        })
                        .mode = mode;
                }
            }
            _ => return,
        }
        self.runtime_options.dirty = true;
        self.runtime_options.notice.clear();
        if key == "runtime-proxy" && value == "inherit" {
            self.discard_options_fields(&["proxy-"], window, cx);
        }
        self.ensure_options_inputs(window, cx);
        cx.notify();
    }
    pub(super) fn options_toggle(
        &mut self,
        key: &str,
        on: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.runtime_options.pending {
            return;
        }
        let Some(options) = &mut self.runtime_options.value else {
            return;
        };
        match key {
            "tracking" => options.quota_tracking = on,
            "paid" => options.subscription.allow_paid_credits = on,
            "reset" => options.subscription.auto_reset_credits = on,
            "preferred" => {
                if let Some(policy) = &mut options.subscription.native_switch {
                    policy.return_preferred = on;
                }
            }
            "native" => {
                options.subscription.native_switch = if on {
                    self.runtime_options
                        .routes
                        .first()
                        .map(|(id, _)| NativeSwitchPolicy {
                            route_id: *id,
                            return_preferred: true,
                        })
                } else {
                    None
                }
            }
            "clear-proxy" => {
                if let Some(p) = &mut options.proxy {
                    p.clear_credentials = on;
                }
            }
            "balance" => {
                options.balance = on.then(|| BalanceEndpoint {
                    url: String::new(),
                    post: false,
                    body: None,
                    headers: vec![],
                    json_path: "$.data.balance".into(),
                    unit: "USD".into(),
                })
            }
            _ => {
                let Some((id, attribute)) =
                    key.strip_prefix("hook-").and_then(|s| s.rsplit_once('/'))
                else {
                    return;
                };
                let Some(hook) = options.webhooks.iter_mut().find(|h| h.id.to_string() == id)
                else {
                    return;
                };
                if attribute == "enabled" {
                    hook.enabled = on;
                } else if attribute == "clear" {
                    hook.clear_secret = on;
                } else if let Some((event, _)) =
                    EVENTS.iter().find(|(_, label)| *label == attribute)
                {
                    hook.events.retain(|e| e != event);
                    if on {
                        hook.events.push(*event);
                    }
                }
            }
        }
        self.runtime_options.dirty = true;
        self.runtime_options.notice.clear();
        if key == "balance" {
            self.discard_options_fields(&["balance-", "header-"], window, cx);
            if let Some(body) = self.runtime_options.body.take() {
                body.update(cx, |f, cx| f.set_value("", window, cx));
            }
        }
        self.ensure_options_inputs(window, cx);
        cx.notify();
    }
    pub(super) fn options_add_header(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.runtime_options.pending {
            return;
        }
        if let Some(b) = self
            .runtime_options
            .value
            .as_mut()
            .and_then(|v| v.balance.as_mut())
        {
            b.headers.push((String::new(), None));
            self.runtime_options.dirty = true;
            self.ensure_options_inputs(window, cx);
            cx.notify();
        }
    }
    pub(super) fn options_remove_header(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.runtime_options.pending {
            return;
        }
        let Some(b) = self
            .runtime_options
            .value
            .as_mut()
            .and_then(|v| v.balance.as_mut())
        else {
            return;
        };
        if index >= b.headers.len() {
            return;
        }
        let count = b.headers.len();
        b.headers.remove(index);
        for suffix in ["name", "value"] {
            if let Some(field) = self
                .runtime_options
                .fields
                .remove(&format!("header-{index}-{suffix}"))
            {
                field.update(cx, |f, cx| f.set_value("", window, cx));
            }
            for i in index + 1..count {
                if let Some(field) = self
                    .runtime_options
                    .fields
                    .remove(&format!("header-{i}-{suffix}"))
                {
                    self.runtime_options
                        .fields
                        .insert(format!("header-{}-{suffix}", i - 1), field);
                }
            }
        }
        self.runtime_options.dirty = true;
        cx.notify();
    }
    pub(super) fn options_add_hook(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.runtime_options.pending {
            return;
        }
        let Some(options) = &mut self.runtime_options.value else {
            return;
        };
        if options.webhooks.len() >= 8 {
            return;
        }
        options.webhooks.push(ProviderWebhook {
            id: Uuid::new_v4(),
            url: String::new(),
            enabled: false,
            events: vec![
                ProviderWebhookEvent::QuotaLow,
                ProviderWebhookEvent::QuotaExhausted,
            ],
            secret: None,
            has_secret: false,
            clear_secret: false,
        });
        self.runtime_options.dirty = true;
        self.ensure_options_inputs(window, cx);
        cx.notify();
    }
    pub(super) fn options_remove_hook(
        &mut self,
        id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.runtime_options.pending {
            return;
        }
        if let Some(options) = &mut self.runtime_options.value {
            options.webhooks.retain(|h| h.id != id);
        }
        for suffix in ["url", "secret"] {
            if let Some(field) = self
                .runtime_options
                .fields
                .remove(&format!("hook-{id}-{suffix}"))
            {
                field.update(cx, |f, cx| f.set_value("", window, cx));
            }
        }
        self.runtime_options.dirty = true;
        cx.notify();
    }
}
