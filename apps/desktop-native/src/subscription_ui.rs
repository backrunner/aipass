//! Native subscription dialogs. Polling and writes use the desktop Agent queue.
use crate::{locale::Locale, theme::Theme};
use aipass_agent_protocol::{ClaudeCliStatus, SubscriptionImportTask};
use aipass_desktop_native::subscriptions::{
    self as model, Completion, Flow, Kind, Login, Operation, Outcome, Provider, Request,
};
use aipass_vault::EntrySummary;
use gpui::{prelude::*, Context, Entity, FocusHandle, ScrollHandle, Subscription, Window};
use gpui_base::input::{InputAccessibility, InputEvent, InputState};
use std::{
    collections::{BTreeMap, VecDeque},
    time::{Duration, Instant},
};
use uuid::Uuid;

mod actions;
mod connect;
mod controls;
#[cfg(feature = "parity-render")]
pub mod fixtures;
mod imports;
mod motion;
mod options;
mod scroll;
#[cfg(all(test, feature = "parity-render"))]
mod tests;
mod usage;
mod view;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Directory,
    Connect,
    Import,
    Options,
}
pub struct Subscriptions {
    runtime_options: options::State,
    theme: Theme,
    locale: Locale,
    dark: bool,
    chinese: bool,
    open: bool,
    modal_motion: motion::ModalMotion,
    screen: Screen,
    focus: FocusHandle,
    dialog_focus: FocusHandle,
    restore: Option<FocusHandle>,
    generation: u64,
    outbox: VecDeque<Request>,
    pending: Option<Kind>,
    providers: Vec<Provider>,
    loaded: bool,
    provider: String,
    method: usize,
    reconnect_entry: Option<Uuid>,
    pub(crate) connected: Option<Uuid>,
    query: Entity<InputState>,
    api_key: Entity<InputState>,
    code: Entity<InputState>,
    inputs: BTreeMap<String, Entity<InputState>>,
    choices: BTreeMap<String, String>,
    picker: Option<String>,
    picker_widths: std::rc::Rc<std::cell::RefCell<BTreeMap<String, gpui::Pixels>>>,
    picker_focus: crate::menu::MenuFocus,
    picker_triggers: BTreeMap<String, FocusHandle>,
    cli: Option<ClaudeCliStatus>,
    login: Option<Login>,
    next_poll: Option<Instant>,
    task: Option<SubscriptionImportTask>,
    import_provider: String,
    cancel_requested: bool,
    error: String,
    feedback: String,
    body_scroll: ScrollHandle,
    provider_scroll: ScrollHandle,
    entry: Option<EntrySummary>,
    usage_generation: u64,
    usage_pending: bool,
    usage_error: String,
    usage_open: bool,
    usage_disclosure: crate::disclosure::Disclosure,
    account_open: bool,
    account_disclosure: crate::disclosure::Disclosure,
    _inputs: Vec<Subscription>,
    #[cfg(feature = "parity-render")]
    preview: bool,
}

impl Subscriptions {
    pub fn new(dark: bool, chinese: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let theme = Theme::new(dark);
        let locale = Locale::new(chinese);
        let input = |label: String, masked, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut state = InputState::new(window, cx).masked(masked);
                state.set_editor_style(theme.input_style());
                state.set_accessibility(
                    InputAccessibility::new(Some(gpui::Role::TextInput))
                        .with_label(Some(label.into())),
                );
                state
            })
        };
        let query = input(locale.text("communityConnect.search"), false, window, cx);
        query.update(cx, |input, cx| {
            input.set_placeholder(locale.text("communityConnect.search"), window, cx)
        });
        let api_key = input("API key".into(), true, window, cx);
        api_key.update(cx, |input, cx| {
            input.set_placeholder(locale.text("communityConnect.apiPlaceholder"), window, cx)
        });
        let code = input(locale.text("communityConnect.code"), true, window, cx);
        let subscriptions = [&query, &api_key, &code]
            .into_iter()
            .map(|input| {
                cx.subscribe_in(input, window, |this, input, event, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. })
                        && *input == this.code
                        && this.provider == "claude"
                    {
                        this.submit_code(window, cx);
                    }
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                })
            })
            .collect();
        Self {
            runtime_options: options::State::default(),
            theme,
            locale,
            dark,
            chinese,
            open: false,
            modal_motion: Default::default(),
            screen: Screen::Directory,
            focus: cx.focus_handle(),
            dialog_focus: cx.focus_handle(),
            restore: None,
            generation: 0,
            outbox: VecDeque::new(),
            pending: None,
            providers: vec![],
            loaded: false,
            provider: String::new(),
            method: 0,
            reconnect_entry: None,
            connected: None,
            query,
            api_key,
            code,
            inputs: BTreeMap::new(),
            choices: BTreeMap::new(),
            picker: None,
            picker_widths: Default::default(),
            picker_focus: crate::menu::MenuFocus::new(64, cx),
            picker_triggers: ["method", "import-provider"]
                .into_iter()
                .map(|key| (key.into(), cx.focus_handle()))
                .collect(),
            cli: None,
            login: None,
            next_poll: None,
            task: None,
            import_provider: "anthropic".into(),
            cancel_requested: false,
            error: String::new(),
            feedback: String::new(),
            body_scroll: ScrollHandle::new(),
            provider_scroll: ScrollHandle::new(),
            entry: None,
            usage_generation: 0,
            usage_pending: false,
            usage_error: String::new(),
            usage_open: true,
            usage_disclosure: Default::default(),
            account_open: false,
            account_disclosure: Default::default(),
            _inputs: subscriptions,
            #[cfg(feature = "parity-render")]
            preview: false,
        }
    }
    pub fn show(
        &mut self,
        provider: Option<String>,
        entry: Option<Uuid>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.abandon(window, cx);
        if !self.open {
            self.restore = window.focused(cx);
        }
        self.open = true;
        self.connected = None;
        self.reconnect_entry = entry;
        self.query
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.provider = provider
            .map(|p| if p == "anthropic" { "claude".into() } else { p })
            .unwrap_or_default();
        self.screen = if self.provider.is_empty() {
            Screen::Directory
        } else {
            Screen::Connect
        };
        self.focus.focus(window, cx);
        if self.loaded {
            if self.screen == Screen::Connect {
                self.choose(self.provider.clone(), window, cx);
            }
        } else {
            self.queue(Operation::Catalog, cx);
        }
        cx.notify();
    }
    fn queue(&mut self, operation: Operation, cx: &mut Context<Self>) {
        self.pending = Some(operation.kind());
        self.outbox.push_back(Request {
            generation: self.generation,
            operation,
        });
        cx.notify();
    }
    #[cfg(feature = "parity-render")]
    pub fn presentation(&self) -> serde_json::Value {
        serde_json::json!({"open":self.open,"provider":self.provider,"method":self.method,"providers":self.providers.len(),"usageOpen":self.usage_open,"accountOpen":self.account_open,"providerScroll":f32::from(self.provider_scroll.offset().y),"providerMaxScroll":f32::from(self.provider_scroll.max_offset().y)})
    }
    pub fn take_request(&mut self) -> Option<Request> {
        self.outbox.pop_front()
    }
    pub fn tick(&mut self, cx: &mut Context<Self>) {
        if self.open
            && self.pending.is_none()
            && self.next_poll.is_some_and(|at| Instant::now() >= at)
        {
            self.next_poll = None;
            if let Some(flow) = self
                .login
                .as_ref()
                .filter(|l| l.pending)
                .map(|l| l.flow)
                .or_else(|| {
                    self.task
                        .as_ref()
                        .filter(|t| matches!(t.phase.as_str(), "discovering" | "importing"))
                        .map(|t| Flow::Import(t.ticket))
                })
            {
                self.queue(Operation::Poll(flow), cx);
            }
        }
    }
    fn clear_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for input in [&self.api_key, &self.code]
            .into_iter()
            .chain(self.inputs.values())
        {
            input.update(cx, |input, cx| {
                input.set_value("", window, cx);
                input.set_disabled(false, cx);
            });
        }
    }
    fn abandon(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let flow = self
            .login
            .take()
            .filter(|l| l.pending)
            .map(|l| l.flow)
            .or_else(|| {
                self.task
                    .take()
                    .filter(|t| matches!(t.phase.as_str(), "discovering" | "importing"))
                    .map(|t| Flow::Import(t.ticket))
            });
        self.generation += 1;
        self.pending = None;
        self.next_poll = None;
        self.picker = None;
        // Keep cleanup requests; discard queued starts and codes from the old panel.
        self.outbox.retain(|r| {
            matches!(
                r.operation,
                Operation::Cancel(_)
                    | Operation::Options(..)
                    | Operation::Refresh(_)
                    | Operation::Console(..)
            )
        });
        if let Some(flow) = flow {
            self.outbox.push_back(Request {
                generation: self.generation - 1,
                operation: Operation::Cancel(flow),
            });
        }
        self.clear_inputs(window, cx);
        self.inputs.clear();
        self.choices.clear();
        self.cli = None;
        self.error.clear();
        self.feedback.clear();
        self.cancel_requested = false;
    }
    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open || (self.screen == Screen::Options && self.runtime_options.busy()) {
            return;
        }
        if self.screen == Screen::Options {
            self.clear_options_secrets(window, cx);
            self.runtime_options.epoch += 1;
            self.runtime_options.pending = false;
            self.runtime_options.loading = false;
            self.runtime_options.discard_after_close = true;
        }
        // Retain token-free presentation during the exit animation, while
        // retiring work and clearing editable secrets immediately.
        self.generation += 1;
        self.pending = None;
        self.next_poll = None;
        self.picker = None;
        self.outbox.retain(|r| {
            matches!(
                r.operation,
                Operation::Cancel(_)
                    | Operation::Options(..)
                    | Operation::Refresh(_)
                    | Operation::Console(..)
            )
        });
        let flow = self
            .login
            .as_ref()
            .filter(|l| l.pending)
            .map(|l| l.flow)
            .or_else(|| {
                self.task
                    .as_ref()
                    .filter(|t| matches!(t.phase.as_str(), "discovering" | "importing"))
                    .map(|t| Flow::Import(t.ticket))
            });
        if let Some(flow) = flow {
            self.outbox.push_back(Request {
                generation: self.generation - 1,
                operation: Operation::Cancel(flow),
            });
        }
        self.clear_inputs(window, cx);
        self.open = false;
        cx.notify();
    }
    pub fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close(window, cx);
        // Locking retires the surface even when a save prevents ordinary dismissal.
        self.open = false;
        self.abandon(window, cx);
        self.reset_options(window, cx);
        self.modal_motion = Default::default();
        self.outbox.clear();
        self.entry = None;
        self.connected = None;
        self.usage_generation += 1;
        self.usage_pending = false;
        self.usage_error.clear();
    }
    pub fn sync_entry(
        &mut self,
        entry: Option<EntrySummary>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.entry.as_ref().map(|e| e.id) != entry.as_ref().map(|e| e.id) {
            self.reset_options(window, cx);
            if self.screen == Screen::Options {
                self.open = false;
            }
            self.usage_generation += 1;
            self.usage_pending = false;
            self.usage_error.clear();
            self.account_open = false;
            self.usage_open = true;
        }
        self.entry = entry;
    }
    fn busy(&self) -> bool {
        self.login.as_ref().is_some_and(|l| l.pending)
            || matches!(self.pending, Some(Kind::Start | Kind::Cancel))
    }
    fn provider(&self) -> Option<&Provider> {
        self.providers.iter().find(|p| p.id == self.provider)
    }
    fn method(&self) -> Option<&model::Method> {
        self.provider()?
            .methods
            .iter()
            .find(|m| m.index == self.method)
    }
    fn text(&self, key: &str) -> String {
        self.locale.text(key)
    }
    fn translated(&self, key: Option<&str>, fallback: &str) -> String {
        key.map(|k| self.text(k)).unwrap_or_else(|| fallback.into())
    }
    fn name(&self, provider: &Provider) -> String {
        if provider.name_key.is_none() && provider.id.starts_with("minimax-code") {
            self.text(&format!("subscriptionName.{}", provider.id))
        } else {
            self.translated(provider.name_key.as_deref(), &provider.name)
        }
    }
    fn fail(&mut self, error: String, cx: &mut Context<Self>) {
        self.error = self.text(&error);
        self.body_scroll
            .set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
        cx.notify();
    }
    pub fn complete(
        &mut self,
        completion: Completion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Kind::Options(id) = completion.kind {
            self.options_complete(id, completion, window, cx);
            return;
        }
        if let Kind::Console(id) = completion.kind {
            if self.usage_generation == completion.generation
                && self.entry.as_ref().is_some_and(|e| e.id == id)
            {
                self.usage_error = completion
                    .result
                    .err()
                    .map(|e| e.message)
                    .unwrap_or_default();
                cx.notify();
            }
            return;
        }
        if let Kind::Refresh(id) = completion.kind {
            if self.usage_generation == completion.generation
                && self.entry.as_ref().is_some_and(|e| e.id == id)
            {
                self.usage_pending = false;
                match completion.result {
                    Ok(Outcome::Refreshed(value)) => {
                        self.entry.as_mut().unwrap().subscription = Some(value);
                        self.usage_error.clear();
                    }
                    Err(error) => self.usage_error = error.message,
                    _ => {}
                }
                cx.notify();
            }
            return;
        }
        if completion.generation != self.generation || !self.open {
            // A start can finish after close/back/reopen. Retire its Agent ticket.
            let flow = match completion.result {
                Ok(Outcome::Login(login)) if login.pending => Some(login.flow),
                Ok(Outcome::Import(task))
                    if matches!(task.phase.as_str(), "discovering" | "importing") =>
                {
                    Some(Flow::Import(task.ticket))
                }
                _ => None,
            };
            if let Some(flow) = flow {
                self.outbox.push_back(Request {
                    generation: completion.generation,
                    operation: Operation::Cancel(flow),
                });
            }
            return;
        }
        if self.pending == Some(completion.kind) {
            self.pending = None;
        }
        match completion.result {
            Err(error) => {
                self.fail(error.message, cx);
                if completion.kind == Kind::Cancel {
                    self.cancel_requested = false;
                    self.next_poll = Some(Instant::now() + Duration::from_secs(1));
                } else if completion.kind == Kind::Poll {
                    if let Some(flow) = self
                        .login
                        .as_ref()
                        .filter(|l| matches!(l.flow, Flow::Community(_)))
                        .map(|l| l.flow)
                        .or_else(|| self.task.as_ref().map(|t| Flow::Import(t.ticket)))
                    {
                        self.queue(Operation::Cancel(flow), cx);
                    }
                }
            }
            Ok(Outcome::Catalog(providers)) => {
                self.providers = providers;
                self.loaded = true;
                if self.screen == Screen::Connect {
                    self.choose(self.provider.clone(), window, cx);
                }
            }
            Ok(Outcome::Cli(status)) => {
                let auto = self.provider == "claude" && status.available;
                self.cli = Some(status);
                if auto {
                    self.queue(Operation::ClaudeStart, cx);
                }
            }
            Ok(Outcome::Login(login)) => {
                if let Some(id) = login.entry_id {
                    self.connected = Some(id);
                    self.close(window, cx);
                    return;
                }
                if login.pending {
                    self.next_poll = Some(Instant::now() + Duration::from_secs(1));
                    self.login = Some(login);
                } else {
                    self.login = None;
                    self.next_poll = None;
                    if completion.kind != Kind::Cancel {
                        self.fail(
                            login
                                .error
                                .unwrap_or_else(|| "communityConnect.cancelled".into()),
                            cx,
                        );
                    }
                }
            }
            Ok(Outcome::Import(task)) => {
                if matches!(task.phase.as_str(), "discovering" | "importing") {
                    self.next_poll = Some(Instant::now() + Duration::from_secs(1));
                }
                self.task = Some(task);
            }
            Ok(Outcome::Done) | Ok(Outcome::Options(_)) => {}
            Ok(Outcome::Refreshed(_)) => {}
        }
        self.set_disabled(cx);
        cx.notify();
    }
    fn set_disabled(&self, cx: &mut Context<Self>) {
        let busy = self.busy();
        for input in [&self.query, &self.api_key]
            .into_iter()
            .chain(self.inputs.values())
        {
            input.update(cx, |s, cx| s.set_disabled(busy, cx));
        }
        self.code.update(cx, |s, cx| {
            s.set_disabled(self.pending == Some(Kind::Code), cx)
        });
    }
}
