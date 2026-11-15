//! Provider runtime editor state and account-scoped completions.
use super::*;
use aipass_agent_protocol::{ProviderRuntimeOptions, SensitiveString};
use model::options::{Action, ResultData};
mod actions;
mod fields;
mod overview;
#[cfg(all(test, feature = "parity-render"))]
mod tests;
mod view;
const SECTIONS: [(&str, &str); 4] = [
    ("providerRuntime.quotaTab", "clock-3"),
    ("providerRuntime.proxyTab", "globe"),
    ("providerRuntime.balanceTab", "circle-dollar-sign"),
    ("providerRuntime.webhooksTab", "bell"),
];
struct InputValue {
    value: String,
    secret: bool,
    placeholder: String,
}

#[derive(Default)]
pub(super) struct State {
    pub(super) value: Option<ProviderRuntimeOptions>,
    // Agent responses contain secret-presence flags, never editable secret values.
    saved_value: Option<ProviderRuntimeOptions>,
    pub(super) routes: Vec<(Uuid, String)>,
    pub(super) expanded: bool,
    pub(super) disclosure: crate::disclosure::Disclosure,
    pub(super) tab: usize,
    pub(super) tab_focus: Vec<gpui::FocusHandle>,
    pub(super) dirty: bool,
    pub(super) discard_after_close: bool,
    pub(super) pending: bool,
    pub(super) loading: bool,
    pub(super) epoch: u64,
    pub(super) error: String,
    pub(super) notice: String,
    pub(super) fields: BTreeMap<String, Entity<InputState>>,
    pub(super) body: Option<Entity<gpui_base::input::TextareaState>>,
    pub(super) subscriptions: Vec<Subscription>,
}
impl State {
    pub(super) fn busy(&self) -> bool {
        self.pending && !self.loading
    }
}
impl Subscriptions {
    fn discard_options_fields(
        &mut self,
        prefixes: &[&str],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keys: Vec<_> = self
            .runtime_options
            .fields
            .keys()
            .filter(|key| prefixes.iter().any(|prefix| key.starts_with(prefix)))
            .cloned()
            .collect();
        for key in keys {
            if let Some(field) = self.runtime_options.fields.remove(&key) {
                field.update(cx, |f, cx| f.set_value("", window, cx));
            }
        }
    }
    pub(super) fn reset_options(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.erase_options_inputs(window, cx);
        let epoch = self.runtime_options.epoch + 1;
        // Focus identities outlive a draft, including the final exit-animation frame.
        let tab_focus = std::mem::take(&mut self.runtime_options.tab_focus);
        self.runtime_options = State {
            epoch,
            tab_focus,
            ..Default::default()
        };
    }
    pub(super) fn discard_options_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.runtime_options.saved_value.take();
        let expanded = self.runtime_options.expanded;
        let disclosure = std::mem::take(&mut self.runtime_options.disclosure);
        let routes = std::mem::take(&mut self.runtime_options.routes);
        self.reset_options(window, cx);
        self.runtime_options.value = value;
        self.runtime_options.expanded = expanded;
        self.runtime_options.disclosure = disclosure;
        self.runtime_options.routes = routes;
    }
    pub(super) fn erase_options_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.runtime_options.subscriptions.clear();
        for input in self.runtime_options.fields.values() {
            input.update(cx, |i, cx| i.set_value("", window, cx));
        }
        self.runtime_options.fields.clear();
        if let Some(body) = self.runtime_options.body.take() {
            body.update(cx, |f, cx| f.set_value("", window, cx));
        }
    }
    fn options_request(&mut self, action: Action, cx: &mut Context<Self>) {
        if self.runtime_options.pending {
            return;
        }
        let Some(id) = self.entry.as_ref().map(|e| e.id) else {
            return;
        };
        self.runtime_options.epoch += 1;
        self.runtime_options.pending = true;
        self.runtime_options.loading = matches!(action, Action::Load);
        self.runtime_options.error.clear();
        self.runtime_options.notice.clear();
        self.outbox.push_back(Request {
            generation: self.runtime_options.epoch,
            operation: Operation::Options(id, action),
        });
        for field in self.runtime_options.fields.values() {
            field.update(cx, |i, cx| i.set_disabled(true, cx));
        }
        if let Some(body) = &self.runtime_options.body {
            body.update(cx, |i, cx| i.set_disabled(true, cx));
        }
        cx.notify();
    }
    pub(super) fn options_complete(
        &mut self,
        id: Uuid,
        completion: Completion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if completion.generation != self.runtime_options.epoch
            || self.entry.as_ref().map(|e| e.id) != Some(id)
        {
            return;
        }
        self.runtime_options.pending = false;
        self.runtime_options.loading = false;
        for field in self.runtime_options.fields.values() {
            field.update(cx, |i, cx| i.set_disabled(false, cx));
        }
        if let Some(body) = &self.runtime_options.body {
            body.update(cx, |i, cx| i.set_disabled(false, cx));
        }
        match completion.result {
            Ok(Outcome::Options(ResultData::Loaded(value, routes, error))) => {
                self.runtime_options.saved_value = Some(value.as_ref().clone());
                self.runtime_options.value = Some(*value);
                self.runtime_options.routes = routes;
                self.runtime_options.error = error.unwrap_or_default();
                if self.open && self.screen == Screen::Options {
                    self.options_inputs(window, cx);
                }
            }
            Ok(Outcome::Options(ResultData::Saved(value))) => {
                self.runtime_options.saved_value = Some(value.as_ref().clone());
                self.runtime_options.value = Some(*value);
                self.runtime_options.dirty = false;
                self.runtime_options.notice = self.text("providerRuntime.saved");
                self.options_inputs(window, cx);
            }
            Ok(Outcome::Options(ResultData::Balance(value))) => self.runtime_options.notice = value,
            Ok(Outcome::Options(ResultData::Reset(value))) => {
                self.runtime_options.notice =
                    self.text(&format!("subscriptionPolicy.resetOutcome.{value}"));
                self.refresh_usage(cx);
            }
            Ok(Outcome::Options(ResultData::Sent)) => {
                self.runtime_options.notice = self.text("providerRuntime.sent")
            }
            Err(error) => self.runtime_options.error = error.message,
            _ => {}
        }
        cx.notify();
    }
    pub(super) fn show_options(&mut self, tab: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.runtime_options.discard_after_close {
            self.discard_options_draft(window, cx);
        }
        self.abandon(window, cx);
        if !self.open {
            self.restore = window.focused(cx);
        }
        self.open = true;
        self.screen = Screen::Options;
        self.runtime_options.tab = tab;
        if self.runtime_options.tab_focus.is_empty() {
            self.runtime_options.tab_focus = (0..4).map(|_| cx.focus_handle()).collect();
        }
        self.runtime_options.dirty = false;
        self.runtime_options.saved_value = self.runtime_options.value.clone();
        self.runtime_options.notice.clear();
        self.focus.focus(window, cx);
        if self.runtime_options.value.is_some() {
            self.options_inputs(window, cx);
        } else {
            self.options_request(Action::Load, cx);
        }
        cx.notify();
    }
    fn options_input(
        &mut self,
        key: String,
        label: &str,
        content: InputValue,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let InputValue {
            value,
            secret,
            placeholder,
        } = content;
        if self.runtime_options.fields.contains_key(&key) {
            return;
        }
        let theme = self.theme;
        let label = if label.contains('.') {
            self.text(label)
        } else {
            label.into()
        };
        let field = cx.new(|cx| {
            let mut state = InputState::new(window, cx)
                .masked(secret)
                .placeholder(placeholder);
            state.set_editor_style(theme.input_style());
            state.set_accessibility(
                InputAccessibility::new(Some(gpui::Role::TextInput)).with_label(Some(label.into())),
            );
            state.set_value(value, window, cx);
            state
        });
        self.runtime_options
            .subscriptions
            .push(cx.subscribe(&field, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    this.runtime_options.dirty = true;
                    this.runtime_options.notice.clear();
                    cx.notify();
                }
            }));
        self.runtime_options.fields.insert(key, field);
    }
    fn options_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.erase_options_inputs(window, cx);
        self.ensure_options_inputs(window, cx);
    }
    fn ensure_options_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(value) = self.runtime_options.value.clone() else {
            return;
        };
        let keep = self.text("providerRuntime.keepSecret");
        if let Some(proxy) = value.proxy {
            for (key, label, value, secret, placeholder) in [
                (
                    "proxy-url",
                    "proxy.proxyUrl",
                    proxy.url.unwrap_or_default(),
                    false,
                    "http://127.0.0.1:7890".into(),
                ),
                (
                    "proxy-user",
                    "proxy.username",
                    String::new(),
                    false,
                    if proxy.has_credentials {
                        keep.clone()
                    } else {
                        String::new()
                    },
                ),
                (
                    "proxy-password",
                    "proxy.password",
                    String::new(),
                    true,
                    if proxy.has_credentials {
                        keep.clone()
                    } else {
                        String::new()
                    },
                ),
            ] {
                self.options_input(
                    key.into(),
                    label,
                    InputValue {
                        value,
                        secret,
                        placeholder,
                    },
                    window,
                    cx,
                );
            }
        }
        if let Some(balance) = value.balance {
            if self.runtime_options.body.is_none() {
                let theme = self.theme;
                let label = self.text("providerRuntime.body");
                let body = cx.new(|cx| {
                    let mut f = gpui_base::input::TextareaState::new(window, cx)
                        .rows(3)
                        .placeholder(keep.clone());
                    f.set_editor_style(theme.input_style());
                    f.set_accessibility(
                        InputAccessibility::new(Some(gpui::Role::MultilineTextInput))
                            .with_label(Some(label.into())),
                    );
                    f
                });
                self.runtime_options.subscriptions.push(cx.subscribe(
                    &body,
                    |this, _, event, cx| {
                        if matches!(event, InputEvent::Change) {
                            this.runtime_options.dirty = true;
                            this.runtime_options.notice.clear();
                            cx.notify();
                        }
                    },
                ));
                self.runtime_options.body = Some(body);
            }
            for (key, label, value, secret, placeholder) in [
                (
                    "balance-url",
                    "URL",
                    balance.url,
                    false,
                    "https://example.com/account/balance".into(),
                ),
                (
                    "balance-path",
                    "providerRuntime.path",
                    balance.json_path,
                    false,
                    "$.data.balance".into(),
                ),
                (
                    "balance-unit",
                    "providerRuntime.unit",
                    balance.unit,
                    false,
                    String::new(),
                ),
            ] {
                self.options_input(
                    key.into(),
                    label,
                    InputValue {
                        value,
                        secret,
                        placeholder,
                    },
                    window,
                    cx,
                );
            }
            for (i, (name, _)) in balance.headers.iter().enumerate() {
                self.options_input(
                    format!("header-{i}-name"),
                    "providerRuntime.headerName",
                    InputValue {
                        value: name.clone(),
                        secret: false,
                        placeholder: "Authorization".into(),
                    },
                    window,
                    cx,
                );
                self.options_input(
                    format!("header-{i}-value"),
                    "providerRuntime.headerValue",
                    InputValue {
                        value: String::new(),
                        secret: true,
                        placeholder: keep.clone(),
                    },
                    window,
                    cx,
                );
            }
        }
        for hook in value.webhooks {
            self.options_input(
                format!("hook-{}-url", hook.id),
                "URL",
                InputValue {
                    value: hook.url,
                    secret: false,
                    placeholder: "https://example.com/webhook".into(),
                },
                window,
                cx,
            );
            self.options_input(
                format!("hook-{}-secret", hook.id),
                "webhook.secret",
                InputValue {
                    value: String::new(),
                    secret: true,
                    placeholder: if hook.has_secret {
                        keep.clone()
                    } else {
                        "Bearer token".into()
                    },
                },
                window,
                cx,
            );
        }
        for key in ["interval", "cap", "proxy", "method", "route"] {
            self.picker_triggers
                .entry(format!("runtime-{key}"))
                .or_insert_with(|| cx.focus_handle());
        }
    }
    fn options_value(&self, key: &str, cx: &Context<Self>) -> String {
        self.runtime_options
            .fields
            .get(key)
            .map(|f| f.read(cx).value().to_string())
            .unwrap_or_default()
    }
    fn options_draft(&self, cx: &Context<Self>) -> Option<ProviderRuntimeOptions> {
        let mut value = self.runtime_options.value.clone()?;
        let secret = |key: &str| {
            let value = self.options_value(key, cx);
            (!value.is_empty()).then(|| SensitiveString::new(value))
        };
        if let Some(proxy) = &mut value.proxy {
            proxy.url = Some(self.options_value("proxy-url", cx)).filter(|s| !s.is_empty());
            proxy.username = secret("proxy-user");
            proxy.password = secret("proxy-password");
        }
        if let Some(balance) = &mut value.balance {
            balance.url = self.options_value("balance-url", cx);
            balance.json_path = self.options_value("balance-path", cx);
            balance.unit = self.options_value("balance-unit", cx);
            balance.body = self
                .runtime_options
                .body
                .as_ref()
                .map(|f| f.read(cx).value().to_string())
                .filter(|s| !s.is_empty())
                .map(SensitiveString::new);
            for (i, pair) in balance.headers.iter_mut().enumerate() {
                pair.0 = self.options_value(&format!("header-{i}-name"), cx);
                pair.1 = secret(&format!("header-{i}-value"));
            }
        }
        for hook in &mut value.webhooks {
            hook.url = self.options_value(&format!("hook-{}-url", hook.id), cx);
            hook.secret = secret(&format!("hook-{}-secret", hook.id));
        }
        Some(value)
    }
    fn options_save(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.runtime_options.dirty || self.runtime_options.pending {
            return;
        }
        let Some(value) = self.options_draft(cx) else {
            return;
        };
        // Keep the editable draft on failure, just like ProviderRuntimeDialog.
        // Success replaces these entities; close, lock and selection changes erase them.
        self.options_request(Action::Save(Box::new(value)), cx);
    }
    pub(super) fn clear_options_secrets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Retire secrets and their undo history when the editing lifetime ends.
        if let Some(body) = &self.runtime_options.body {
            body.update(cx, |f, cx| f.set_value("", window, cx));
        }
        for (key, field) in &self.runtime_options.fields {
            if key.ends_with("password")
                || key.ends_with("secret")
                || key.ends_with("value")
                || key == "proxy-user"
                || key == "balance-body"
            {
                field.update(cx, |f, cx| f.set_value("", window, cx));
            }
        }
    }
}
