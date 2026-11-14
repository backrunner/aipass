//! Transient pricing editors. Every durable action goes through the Agent.
use crate::{locale::Locale, theme::Theme};
use aipass_agent_protocol::{PricingApplyScope, PricingConfig, PricingGroup};
use aipass_desktop_native::pricing::{draft, Assignment, Completion, Operation, Request, Scope};
use aipass_provider_registry::SecretRef;
use aipass_vault::EntrySummary;
use gpui::{prelude::*, Context, Entity, FocusHandle, Subscription, Window};
use gpui_base::input::{InputAccessibility, InputEvent, InputState};
use uuid::Uuid;

mod assignment;
mod fields;
#[cfg(feature = "parity-render")]
mod fixtures;
mod group;
mod picker;
#[cfg(all(test, feature = "parity-render"))]
mod tests;
mod view;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Panel {
    Assignment,
    Group,
    Confirm,
    DeleteGroup,
    DeleteVersion(i64),
}

struct RuleInputs {
    id: usize,
    inputs: [Entity<InputState>; 13],
    off_peak: bool,
    _subscriptions: Vec<Subscription>,
}

pub struct Pricing {
    entry: Option<EntrySummary>,
    generation: u64,
    pub config: Option<PricingConfig>,
    request: Option<Request>,
    pending: Option<Operation>,
    panel: Option<Panel>,
    closing: bool,
    secret: Option<SecretRef>,
    selected_group: Option<Uuid>,
    original: Option<Assignment>,
    multiplier: Entity<InputState>,
    name: Entity<InputState>,
    group_id: Uuid,
    rules: Vec<RuleInputs>,
    next_rule: usize,
    /// A successful group write is never repeated when the later binding fails.
    bind_after_save: Option<Assignment>,
    retry_binding: Option<Assignment>,
    reference_open: bool,
    reference_disclosure: crate::disclosure::Disclosure,
    history_open: bool,
    history_disclosure: crate::disclosure::Disclosure,
    picker_open: bool,
    picker_focus: crate::menu::MenuFocus,
    picker_bounds: std::rc::Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
    focus: FocusHandle,
    restore: Option<FocusHandle>,
    error: String,
    theme: Theme,
    locale: Locale,
    popup_motion: gpui_base::TransitionState<f32>,
    overlay_motion: gpui_base::TransitionState<f32>,
    motion_initialized: bool,
    _subscriptions: Vec<Subscription>,
}
impl Pricing {
    pub(crate) fn apply_appearance(&mut self, dark: bool, cx: &mut gpui::Context<Self>) {
        self.theme = Theme::new(dark);
        for input in [&self.multiplier, &self.name]
            .into_iter()
            .chain(self.rules.iter().flat_map(|row| row.inputs.iter()))
        {
            input.update(cx, |input, cx| {
                input.set_editor_style(self.theme.input_style());
                cx.notify();
            });
        }
        cx.notify();
    }

    pub fn new(dark: bool, chinese: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        crate::menu::init(cx);
        let theme = Theme::new(dark);
        let locale = Locale::new(chinese);
        let multiplier = fields::input("1", "pricing.multiplier", theme, &locale, window, cx);
        let name = fields::input("", "pricing.group", theme, &locale, window, cx);
        let subscriptions = [&multiplier, &name]
            .into_iter()
            .map(|input| {
                cx.subscribe(input, |_, _, event, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                })
            })
            .collect();
        Self {
            entry: None,
            generation: 0,
            config: None,
            request: None,
            pending: None,
            panel: None,
            closing: false,
            secret: None,
            selected_group: None,
            original: None,
            multiplier,
            name,
            group_id: Uuid::nil(),
            rules: vec![],
            next_rule: 0,
            bind_after_save: None,
            retry_binding: None,
            reference_open: false,
            reference_disclosure: Default::default(),
            history_open: false,
            history_disclosure: Default::default(),
            picker_open: false,
            picker_focus: crate::menu::MenuFocus::new(1, cx),
            picker_bounds: Default::default(),
            focus: cx.focus_handle(),
            restore: None,
            error: String::new(),
            theme,
            locale,
            popup_motion: Default::default(),
            overlay_motion: Default::default(),
            motion_initialized: false,
            _subscriptions: subscriptions,
        }
    }
    pub fn sync(
        &mut self,
        entry: Option<EntrySummary>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let changed = self.entry.as_ref().map(|e| e.id) != entry.as_ref().map(|e| e.id);
        let retired = self.secret.as_ref().is_some_and(|s| {
            entry
                .as_ref()
                .is_none_or(|e| !e.secret_refs.iter().any(|r| r.id == s.id))
        });
        if changed || retired {
            self.clear(window, cx);
        }
        self.entry = entry;
        if changed && self.entry.is_some() {
            self.submit(Operation::Load, cx);
        }
    }
    pub fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.generation += 1;
        self.entry = None;
        self.config = None;
        self.request = None;
        self.pending = None;
        self.panel = None;
        self.closing = false;
        self.secret = None;
        self.original = None;
        self.rules.clear();
        self.bind_after_save = None;
        self.retry_binding = None;
        self.picker_open = false;
        self.error.clear();
        self.restore = None;
        self.motion_initialized = false;
        for input in [&self.multiplier, &self.name] {
            input.update(cx, |s, cx| {
                s.set_disabled(false, cx);
                s.set_value("", window, cx);
            });
        }
        cx.notify();
    }
    pub fn show(&mut self, secret_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.panel.is_some() {
            return;
        }
        let Some(secret) = self
            .entry
            .as_ref()
            .and_then(|e| e.secret_refs.iter().find(|s| s.id == secret_id))
            .cloned()
        else {
            return;
        };
        self.secret = Some(secret);
        self.panel = Some(Panel::Assignment);
        self.closing = false;
        self.restore = window.focused(cx);
        self.focus.focus(window, cx);
        self.reference_open = false;
        self.reference_disclosure = Default::default();
        self.motion_initialized = false;
        self.error.clear();
        if self.config.is_some() {
            self.load_assignment(window, cx);
        }
        // Refresh shared data before enabling writes; a failed read never looks empty.
        if self.pending.is_none() {
            self.submit(Operation::Load, cx);
        }
        cx.notify();
    }
    fn load_assignment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = &self.entry else {
            return;
        };
        let Some(secret) = &self.secret else {
            return;
        };
        let assignment = self.config.as_ref().and_then(|c| {
            c.assignments
                .iter()
                .find(|a| a.entry_id == entry.id && a.secret_id == secret.id)
        });
        self.selected_group = assignment.and_then(|a| a.group_id);
        let value = assignment.map_or(1., |a| a.multiplier);
        self.original = Some(Assignment {
            entry_id: entry.id,
            secret_id: secret.id.clone(),
            group_id: self.selected_group,
            multiplier: value,
        });
        self.multiplier
            .update(cx, |s, cx| s.set_value(value.to_string(), window, cx));
        self.picker_focus =
            crate::menu::MenuFocus::new(self.config.as_ref().map_or(1, |c| c.groups.len() + 1), cx);
    }
    fn busy(&self) -> bool {
        self.pending.is_some() || self.closing
    }
    fn writing(&self) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|p| !matches!(p, Operation::Load))
    }
    pub fn take_request(&mut self) -> Option<Request> {
        self.request.take()
    }
    fn submit(&mut self, operation: Operation, cx: &mut Context<Self>) {
        if self.pending.is_some() || self.closing {
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
            operation: operation.clone(),
        });
        self.pending = Some(operation);
        self.error.clear();
        self.picker_open = false;
        self.set_disabled(true, cx);
        cx.notify();
    }
    fn set_disabled(&self, disabled: bool, cx: &mut Context<Self>) {
        for input in [&self.multiplier, &self.name]
            .into_iter()
            .chain(self.rules.iter().flat_map(|r| r.inputs.iter()))
        {
            input.update(cx, |s, cx| s.set_disabled(disabled, cx));
        }
    }
    pub fn complete(
        &mut self,
        completion: Completion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if completion.scope.generation != self.generation
            || self.pending.is_none()
            || self
                .entry
                .as_ref()
                .is_none_or(|e| e.id != completion.scope.entry)
        {
            return;
        }
        let operation = self.pending.take().unwrap();
        self.set_disabled(false, cx);
        match completion.result {
            Ok(config) => {
                self.config = Some(config);
                match operation {
                    Operation::Load => {
                        if self.panel == Some(Panel::Assignment) {
                            self.load_assignment(window, cx);
                        }
                    }
                    Operation::SaveGroup { .. } => {
                        if let Some(binding) = self.bind_after_save.take() {
                            self.retry_binding = Some(binding.clone());
                            self.submit(Operation::Assign(binding), cx);
                        } else {
                            self.close(window, cx);
                        }
                    }
                    Operation::DeleteVersion { .. } => self.close(window, cx),
                    Operation::Assign(_) | Operation::DeleteGroup(_) => {
                        self.retry_binding = None;
                        self.close(window, cx);
                    }
                }
            }
            Err(error) => {
                if matches!(operation, Operation::Load) {
                    self.config = None;
                }
                self.error = error.message;
            }
        }
        cx.notify();
    }
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.writing() || self.closing || self.panel.is_none() {
            return;
        }
        self.generation += 1;
        let generation = self.generation;
        self.pending = None;
        self.request = None;
        self.picker_open = false;
        self.closing = true;
        self.set_disabled(true, cx);
        let delay = if cx.reduce_motion() { 0 } else { 200 };
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(delay))
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.generation != generation || !this.closing {
                    return;
                }
                this.panel = None;
                this.closing = false;
                this.secret = None;
                this.bind_after_save = None;
                this.retry_binding = None;
                this.rules.clear();
                this.set_disabled(false, cx);
                if let Some(focus) = this.restore.take() {
                    focus.focus(window, cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn assignment(&self, cx: &gpui::App) -> Option<Assignment> {
        let mut assignment = self.original.clone()?;
        assignment.group_id = self.selected_group;
        assignment.multiplier = draft::multiplier(&self.multiplier.read(cx).value())?;
        if assignment.group_id.is_some_and(|id| {
            self.config
                .as_ref()
                .is_none_or(|c| !c.groups.iter().any(|g| g.id == id))
        }) {
            return None;
        }
        Some(assignment)
    }
    fn edit_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy() || self.assignment(cx) != self.original {
            return;
        }
        self.group_id = self.selected_group.unwrap_or_else(Uuid::new_v4);
        self.bind_after_save = self.selected_group.is_none().then(|| {
            let mut a = self.original.clone().unwrap();
            a.group_id = Some(self.group_id);
            a
        });
        self.panel = Some(Panel::Group);
        self.history_open = false;
        self.history_disclosure = Default::default();
        self.error.clear();
        self.load_group(window, cx);
        self.focus.focus(window, cx);
        cx.notify();
    }
    fn current_group(&self) -> Option<&PricingGroup> {
        self.config
            .as_ref()?
            .groups
            .iter()
            .find(|g| g.id == self.group_id)
    }
    fn load_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let group = self.current_group().cloned();
        self.name.update(cx, |s, cx| {
            s.set_value(group.as_ref().map_or("", |g| g.name.as_str()), window, cx)
        });
        let rules = group
            .as_ref()
            .and_then(|g| g.versions.iter().max_by_key(|v| v.effective_from))
            .map(|v| v.rules.iter().map(draft::Rule::from).collect())
            .unwrap_or_else(|| vec![draft::Rule::default()]);
        self.set_rules(rules, window, cx);
    }
    fn set_rules(&mut self, rules: Vec<draft::Rule>, window: &mut Window, cx: &mut Context<Self>) {
        self.rules.clear();
        for rule in rules {
            self.add_rule(rule, window, cx);
        }
    }
    fn add_rule(&mut self, rule: draft::Rule, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.next_rule;
        self.next_rule += 1;
        let inputs = std::array::from_fn(|i| {
            let value = match i {
                0 => &rule.model,
                1..=5 => &rule.prices[i - 1],
                6 => &rule.start,
                7 => &rule.end,
                _ => &rule.off_peak_prices[i - 8],
            };
            let displayed = if matches!(i, 6 | 7) {
                aipass_desktop_native::pricing::presentation::display_time(
                    value,
                    self.locale.language(),
                )
            } else {
                value.clone()
            };
            fields::input(
                &displayed,
                fields::RULE_LABELS[i],
                self.theme,
                &self.locale,
                window,
                cx,
            )
        });
        let subscriptions = inputs
            .iter()
            .map(|input| {
                cx.subscribe(input, |_, _, event, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                })
            })
            .collect();
        self.rules.push(RuleInputs {
            id,
            inputs,
            off_peak: rule.off_peak,
            _subscriptions: subscriptions,
        });
        cx.notify();
    }
    fn draft_group(&self, cx: &gpui::App) -> Option<PricingGroup> {
        let rules: Vec<_> = self
            .rules
            .iter()
            .map(|r| {
                let value = |i: usize| r.inputs[i].read(cx).value().to_string();
                draft::Rule {
                    model: value(0),
                    prices: std::array::from_fn(|i| value(i + 1)),
                    off_peak: r.off_peak,
                    start: aipass_desktop_native::pricing::presentation::canonical_time(&value(6))
                        .unwrap_or_else(|| value(6)),
                    end: aipass_desktop_native::pricing::presentation::canonical_time(&value(7))
                        .unwrap_or_else(|| value(7)),
                    off_peak_prices: std::array::from_fn(|i| value(i + 8)),
                }
            })
            .collect();
        draft::group(self.group_id, &self.name.read(cx).value(), &rules)
    }
    fn save_group(&mut self, scope: PricingApplyScope, cx: &mut Context<Self>) {
        if self.panel != Some(Panel::Confirm) {
            return;
        }
        if let Some(group) = self.draft_group(cx) {
            self.submit(
                Operation::SaveGroup {
                    group,
                    apply_scope: scope,
                },
                cx,
            );
        }
    }
    fn assigned_count(&self) -> usize {
        if self.bind_after_save.is_some() {
            1
        } else {
            self.config.as_ref().map_or(0, |c| {
                c.assignments
                    .iter()
                    .filter(|a| a.group_id == Some(self.group_id))
                    .count()
            })
        }
    }
}
