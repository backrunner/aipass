//! Scoped, transient UI for endpoint tests, usage previews and provider reset.
use crate::{locale::Locale, theme::Theme};
use aipass_agent_protocol::{ProbeResult, SensitiveString, UsageProbeMode, UsageProbeResult};
use aipass_desktop_native::provider_tools::{Completion, Operation, Outcome, Request, Scope};
use aipass_provider_registry::EndpointKind;
use aipass_vault::EntrySummary;
use gpui::{prelude::*, Context, Entity, EventEmitter, FocusHandle, Subscription, Window};
use gpui_base::input::{InputAccessibility, InputEvent, InputState};

mod fields;
#[cfg(feature = "parity-render")]
mod fixtures;
mod results;
#[cfg(all(test, feature = "parity-render"))]
mod tests;
mod view;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Panel {
    Reset,
    Usage,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pending {
    Probe,
    Usage,
    Apply,
    Reset,
}
pub enum Event {
    Request,
}
impl EventEmitter<Event> for Tools {}
pub struct Tools {
    entry: Option<EntrySummary>,
    generation: u64,
    panel: Option<Panel>,
    pending: Option<Pending>,
    request: Option<Request>,
    pub probe: Option<ProbeResult>,
    usage: Option<UsageProbeResult>,
    reset_snapshot: Vec<aipass_provider_registry::SecretRef>,
    reset_ready: Option<EntrySummary>,
    mode: UsageProbeMode,
    secret_id: Option<String>,
    inputs: [Entity<InputState>; 3],
    key_open: bool,
    key_focus: crate::menu::MenuFocus,
    mode_focus: [FocusHandle; 4],
    key_bounds: std::rc::Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
    focus: FocusHandle,
    restore: Option<FocusHandle>,
    error: String,
    notice: Option<(&'static str, std::time::Instant)>,
    theme: Theme,
    locale: Locale,
    popup_motion: gpui_base::TransitionState<f32>,
    overlay_motion: gpui_base::TransitionState<f32>,
    motion_initialized: bool,
    _inputs: Vec<Subscription>,
}
const LABELS: [&str; 3] = [
    "providerDetail.baseUrlOverride",
    "providerDetail.accessToken",
    "providerDetail.userId",
];
impl Tools {
    pub fn new(dark: bool, chinese: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        crate::menu::init(cx);
        let theme = Theme::new(dark);
        let locale = Locale::new(chinese);
        let inputs = std::array::from_fn(|i| {
            cx.new(|cx| {
                let mut state = InputState::new(window, cx).masked(i == 1);
                state.set_editor_style(theme.input_style());
                state.set_accessibility(
                    InputAccessibility::new(Some(gpui::Role::TextInput))
                        .with_label(Some(locale.text(LABELS[i]).into())),
                );
                state
            })
        });
        let observers = inputs
            .iter()
            .map(|input| {
                cx.subscribe(input, |this, _, event, cx| {
                    if matches!(event, InputEvent::Change) && this.pending.is_none() {
                        this.usage = None;
                        this.error.clear();
                        cx.notify();
                    }
                })
            })
            .collect();
        Self {
            entry: None,
            generation: 0,
            panel: None,
            pending: None,
            request: None,
            probe: None,
            usage: None,
            reset_snapshot: vec![],
            reset_ready: None,
            mode: UsageProbeMode::Auto,
            secret_id: None,
            inputs,
            key_open: false,
            key_focus: crate::menu::MenuFocus::new(0, cx),
            mode_focus: std::array::from_fn(|_| cx.focus_handle()),
            key_bounds: Default::default(),
            focus: cx.focus_handle(),
            restore: None,
            error: String::new(),
            notice: None,
            theme,
            locale,
            popup_motion: Default::default(),
            overlay_motion: Default::default(),
            motion_initialized: false,
            _inputs: observers,
        }
    }
    pub fn sync(
        &mut self,
        entry: Option<EntrySummary>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let same = match (&self.entry, &entry) {
            (Some(a), Some(b)) => {
                a.id == b.id
                    && a.provider_id == b.provider_id
                    && a.endpoints == b.endpoints
                    && a.secret_refs == b.secret_refs
            }
            (None, None) => true,
            _ => false,
        };
        let resetting_same_entry = self.panel == Some(Panel::Reset)
            && self
                .entry
                .as_ref()
                .zip(entry.as_ref())
                .is_some_and(|(a, b)| a.id == b.id);
        if !same && !resetting_same_entry {
            self.clear(window, cx);
            self.key_focus =
                crate::menu::MenuFocus::new(entry.as_ref().map_or(0, |e| e.secret_refs.len()), cx);
        }
        self.entry = entry;
    }
    pub fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.generation += 1;
        self.panel = None;
        self.pending = None;
        self.request = None;
        self.entry = None;
        self.probe = None;
        self.usage = None;
        self.reset_snapshot.clear();
        self.reset_ready = None;
        self.secret_id = None;
        self.key_open = false;
        self.error.clear();
        self.notice = None;
        self.restore = None;
        self.motion_initialized = false;
        for input in &self.inputs {
            input.update(cx, |s, cx| {
                s.set_disabled(false, cx);
                s.set_value("", window, cx);
            });
        }
        cx.notify();
    }
    fn mutating(&self) -> bool {
        matches!(self.pending, Some(Pending::Apply | Pending::Reset))
    }
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn has_request(&self) -> bool {
        self.request.is_some()
    }
    pub fn take_request(&mut self) -> Option<Request> {
        self.request.take()
    }
    pub fn take_reset(&mut self) -> Option<EntrySummary> {
        self.reset_ready.take()
    }
    pub fn show_reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy() || self.entry.is_none() {
            return;
        }
        self.reset_snapshot = self.entry.as_ref().unwrap().secret_refs.clone();
        self.show(Panel::Reset, window, cx);
    }
    pub fn show_usage(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy() || !self.has_keys() {
            return;
        }
        let entry = self.entry.as_ref().unwrap();
        let base = entry
            .endpoints
            .iter()
            .find(|e| e.kind == EndpointKind::Api)
            .or_else(|| entry.endpoints.first())
            .and_then(|e| e.url.clone())
            .unwrap_or_default();
        self.secret_id = entry.secret_refs.first().map(|s| s.id.clone());
        self.mode = UsageProbeMode::Auto;
        self.usage = None;
        self.inputs[0].update(cx, |s, cx| s.set_value(base, window, cx));
        self.inputs[1].update(cx, |s, cx| s.set_value("", window, cx));
        self.inputs[2].update(cx, |s, cx| s.set_value("", window, cx));
        self.show(Panel::Usage, window, cx);
    }
    fn show(&mut self, panel: Panel, window: &mut Window, cx: &mut Context<Self>) {
        self.generation += 1;
        self.restore = window.focused(cx);
        self.panel = Some(panel);
        self.error.clear();
        self.key_open = false;
        self.motion_initialized = false;
        self.focus.focus(window, cx);
        cx.notify();
    }
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mutating() {
            return;
        }
        self.generation += 1;
        self.pending = None;
        self.request = None;
        for input in &self.inputs {
            input.update(cx, |s, cx| s.set_disabled(false, cx));
        }
        self.panel = None;
        self.key_open = false;
        self.reset_snapshot.clear();
        self.inputs[1].update(cx, |s, cx| s.set_value("", window, cx));
        self.motion_initialized = false;
        if let Some(focus) = self.restore.take() {
            focus.focus(window, cx);
        }
        cx.notify();
    }
    fn has_keys(&self) -> bool {
        self.entry
            .as_ref()
            .is_some_and(|e| !e.secret_refs.is_empty())
    }
    fn submit(
        &mut self,
        operation: Operation,
        kind: Pending,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy() {
            return;
        }
        let Some(entry) = &self.entry else {
            return;
        };
        self.generation += 1;
        self.request = Some(Request {
            scope: Scope {
                entry: entry.id,
                generation: self.generation,
            },
            operation,
        });
        self.pending = Some(kind);
        self.error.clear();
        self.key_open = false;
        for input in &self.inputs {
            input.update(cx, |s, cx| s.set_disabled(true, cx));
        }
        cx.emit(Event::Request);
        cx.notify();
    }
    pub fn probe(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.has_keys() || self.busy() {
            return;
        }
        self.probe = None;
        self.submit(Operation::Probe, Pending::Probe, window, cx);
    }
    fn test_usage(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.panel != Some(Panel::Usage) || self.busy() {
            return;
        }
        let value = |i: usize| {
            let value = self.inputs[i].read(cx).value().trim().to_owned();
            (!value.is_empty()).then_some(value)
        };
        let operation = Operation::Usage {
            mode: self.mode,
            secret_id: self.secret_id.clone(),
            base_url: value(0),
            access_token: if self.mode == UsageProbeMode::NewApiAdvanced {
                value(1).map(SensitiveString::new)
            } else {
                None
            },
            user_id: if self.mode == UsageProbeMode::NewApiAdvanced {
                value(2)
            } else {
                None
            },
        };
        self.usage = None;
        self.submit(operation, Pending::Usage, window, cx);
        // Ownership moves to the Agent job; remove editor text and undo history.
        self.inputs[1].update(cx, |s, cx| s.set_value("", window, cx));
    }
    fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.panel != Some(Panel::Usage) || self.busy() {
            return;
        }
        if let (Some(entry), Some(result)) = (&self.entry, &self.usage) {
            if aipass_desktop_native::provider_tools::can_apply(result) {
                self.submit(
                    Operation::Apply {
                        entry: Box::new(entry.clone()),
                        result: Box::new(result.clone()),
                    },
                    Pending::Apply,
                    window,
                    cx,
                );
            }
        }
    }
    fn confirm_reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.panel != Some(Panel::Reset) || self.busy() {
            return;
        }
        if let Some(entry) = &self.entry {
            self.submit(
                Operation::Reset {
                    secrets: self.reset_snapshot.clone(),
                    archived: entry.archived_at.is_some(),
                },
                Pending::Reset,
                window,
                cx,
            );
        }
    }
    pub fn complete(
        &mut self,
        completion: Completion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .entry
            .as_ref()
            .is_none_or(|e| e.id != completion.scope.entry)
            || completion.scope.generation != self.generation
            || self.pending.is_none()
        {
            return;
        }
        let pending = self.pending.take();
        for input in &self.inputs {
            input.update(cx, |s, cx| s.set_disabled(false, cx));
        }
        match completion.result {
            Ok(Outcome::Probe(result)) => self.probe = Some(result),
            Ok(Outcome::Usage(result)) => self.usage = Some(*result),
            Ok(Outcome::Applied) => {
                self.close(window, cx);
                self.notice = Some(("notice.usageProbeApplied", std::time::Instant::now()));
            }
            Ok(Outcome::Reset(entry)) => {
                self.entry = Some(*entry.clone());
                self.reset_ready = Some(*entry);
                self.close(window, cx);
                self.notice = Some(("notice.providerReset", std::time::Instant::now()));
            }
            Err(error) => {
                if pending == Some(Pending::Probe) {
                    self.probe = self.entry.as_ref().map(|e| ProbeResult {
                        ok: false,
                        provider_id: e.provider_id.clone(),
                        interface_type: e.interface_type.clone(),
                        status: None,
                        endpoint: None,
                        model_count: None,
                        websocket: None,
                        error: Some(error.message.clone()),
                    });
                }
                self.error = if error.code == Some(aipass_agent_protocol::AgentErrorCode::Conflict)
                {
                    self.locale.text("providerDetail.changedDuringAction")
                } else {
                    error.message
                };
            }
        }
        if let Some((_, instant)) = self.notice {
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(1800))
                    .await;
                let _ = this.update_in(cx, |this, _, cx| {
                    if this.notice.is_some_and(|(_, at)| at == instant) {
                        this.notice = None;
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        cx.notify();
    }
}
