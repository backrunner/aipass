//! Installed Agent management, retaining selections across refreshes.
use crate::{
    form_ui::select::{Choice, Selected},
    locale::Locale,
    theme::Theme,
};
use aipass_agent_protocol::{
    ToolConfigOutcome, ToolConfigRequest, ToolConfigStatus, ToolConfigTool as Tool,
};
use aipass_desktop_native::agent_management::{
    choices, Completion, Kind, Operation, Outcome, Request,
};
use aipass_vault::EntrySummary;
use gpui::{prelude::*, Context, Entity, ScrollHandle, Subscription, Window};
use std::collections::{BTreeMap, VecDeque};
#[cfg(feature = "parity-render")]
pub(crate) mod fixtures;
mod setup;
#[cfg(all(test, feature = "parity-render"))]
mod tests;
mod view;

pub struct Agents {
    entries: Vec<EntrySummary>,
    revision: String,
    detections: Vec<aipass_config_writers::ToolDetection>,
    statuses: Vec<ToolConfigStatus>,
    selected: Option<Tool>,
    selections: BTreeMap<String, String>,
    picker: Entity<Choice>,
    subscriptions: Entity<crate::subscription_ui::Subscriptions>,
    _picker_subscription: Subscription,
    active: bool,
    loaded: bool,
    pending: Option<Kind>,
    generation: u64,
    outbox: VecDeque<Request>,
    summary: Option<aipass_agent_protocol::ServerUsageSummary>,
    usage_failed: bool,
    usage_needed: bool,
    error: String,
    message: String,
    plan: Option<ToolConfigRequest>,
    preview: Option<Entity<crate::integrations::preview::Preview>>,
    preview_subscription: Option<Subscription>,
    setup: Option<Entity<setup::Setup>>,
    setup_subscription: Option<Subscription>,
    copy_cancel: Option<aipass_desktop_platform::clipboard::Cancellation>,
    copy_task: Option<gpui::Task<()>>,
    theme: Theme,
    locale: Locale,
    dark: bool,
    chinese: bool,
    list_scroll: ScrollHandle,
    detail_scroll: ScrollHandle,
    switch_focus: gpui::FocusHandle,
    row_focus: Vec<gpui::FocusHandle>,
}
fn key(tool: &Tool) -> String {
    serde_json::to_string(tool).expect("tool enum")
}
impl Agents {
    pub fn new(
        subscriptions: Entity<crate::subscription_ui::Subscriptions>,
        dark: bool,
        chinese: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let locale = Locale::new(chinese);
        let theme = Theme::new(dark);
        let picker =
            cx.new(|cx| Choice::new(locale.text("agents.credential"), vec![], "", theme, cx));
        let picker_subscription = cx.subscribe(&picker, |this, _, event: &Selected, cx| {
            if let Some(tool) = &this.selected {
                this.selections.insert(key(tool), event.0.clone());
            }
            cx.notify();
        });
        let switch_focus = cx.focus_handle();
        picker.update(cx, |picker, _| picker.next = Some(switch_focus.clone()));
        Self {
            entries: vec![],
            revision: String::new(),
            detections: vec![],
            statuses: vec![],
            selected: None,
            selections: BTreeMap::new(),
            picker,
            subscriptions,
            _picker_subscription: picker_subscription,
            active: false,
            loaded: false,
            pending: None,
            generation: 0,
            outbox: VecDeque::new(),
            summary: None,
            usage_failed: false,
            usage_needed: true,
            error: String::new(),
            message: String::new(),
            plan: None,
            preview: None,
            preview_subscription: None,
            setup: None,
            setup_subscription: None,
            copy_cancel: None,
            copy_task: None,
            theme,
            locale,
            dark,
            chinese,
            list_scroll: ScrollHandle::new(),
            detail_scroll: ScrollHandle::new(),
            switch_focus,
            row_focus: (0..22).map(|_| cx.focus_handle()).collect(),
        }
    }
    pub fn appearance(&mut self, dark: bool, cx: &mut Context<Self>) {
        self.dark = dark;
        self.theme = Theme::new(dark);
        self.picker.update(cx, |view, cx| {
            view.theme = self.theme;
            cx.notify();
        });
        if let Some(view) = &self.preview {
            view.update(cx, |view, cx| view.apply_appearance(dark, cx));
        }
        if let Some(view) = &self.setup {
            view.update(cx, |view, cx| {
                view.theme = self.theme;
                cx.notify();
            });
        }
        cx.notify();
    }
    pub fn activate(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.active == active {
            return;
        }
        self.active = active;
        if active {
            self.scan(cx);
        } else {
            self.retire(cx);
        }
        cx.notify();
    }
    fn retire(&mut self, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1);
        self.pending = None;
        self.outbox.clear();
        self.preview = None;
        self.preview_subscription = None;
        self.plan = None;
        self.setup = None;
        self.setup_subscription = None;
        self.cancel_copy();
        self.picker.update(cx, |view, cx| view.close(cx));
    }
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.retire(cx);
        self.active = false;
        self.entries.clear();
        self.revision.clear();
        self.detections.clear();
        self.statuses.clear();
        self.selected = None;
        self.selections.clear();
        self.loaded = false;
        self.summary = None;
        self.usage_failed = false;
        self.usage_needed = true;
        self.error.clear();
        self.message.clear();
        self.reconcile(cx);
        cx.notify();
    }
    pub fn sync(&mut self, entries: &[EntrySummary], cx: &mut Context<Self>) {
        let revision = serde_json::to_string(entries).expect("entry metadata");
        if self.revision == revision {
            return;
        }
        self.entries = entries.to_vec();
        self.revision = revision;
        // Catalog changes retire pending reads and copies as well as visible plans.
        if matches!(self.pending, Some(Kind::Preview | Kind::Copy)) {
            self.generation = self.generation.wrapping_add(1);
            self.pending = None;
            self.outbox.clear();
            self.cancel_copy();
            if let Some(view) = &self.setup {
                view.update(cx, |v, cx| {
                    v.busy = false;
                    cx.notify();
                });
            }
        }
        // A preview is a capability for one exact catalog revision.
        if self.pending != Some(Kind::Apply) {
            self.preview = None;
            self.plan = None;
            self.preview_subscription = None;
        }
        if self.setup.as_ref().is_some_and(|v| {
            !self.entries.iter().any(|e| {
                e.id == v.read(cx).request.id
                    && e.secret_refs
                        .iter()
                        .any(|s| Some(&s.id) == v.read(cx).request.secret_id.as_ref())
            })
        }) {
            self.setup = None;
            self.setup_subscription = None;
            self.cancel_copy();
        }
        self.reconcile(cx);
        cx.notify();
    }
    fn status(&self) -> Option<&ToolConfigStatus> {
        self.statuses
            .iter()
            .find(|s| Some(&s.tool) == self.selected.as_ref())
    }
    pub fn entry(&self) -> Option<EntrySummary> {
        let status = self.status()?;
        if status.state != "ready"
            && !(status.state == "unmanaged"
                && status.mode == Some(aipass_agent_protocol::ToolConfigMode::Official))
        {
            return None;
        }
        self.entries
            .iter()
            .find(|e| Some(e.id) == status.entry_id)
            .cloned()
    }
    fn choose(&mut self, tool: Tool, cx: &mut Context<Self>) {
        if self.pending.is_some() || self.preview.is_some() {
            return;
        }
        self.selected = Some(tool);
        self.detail_scroll = ScrollHandle::new();
        self.reconcile(cx);
        cx.notify();
    }
    fn reconcile(&mut self, cx: &mut Context<Self>) {
        let tools = choices::tools(&self.detections);
        if !tools.iter().any(|t| Some(t) == self.selected.as_ref()) {
            self.selected = tools.first().cloned();
        }
        for tool in &tools {
            let available = choices::choices(&self.entries, tool);
            let selected = choices::reconcile(
                &available,
                self.selections
                    .get(&key(tool))
                    .map(String::as_str)
                    .unwrap_or_default(),
                self.statuses.iter().find(|s| &s.tool == tool),
            );
            self.selections.insert(key(tool), selected);
        }
        let available = self
            .selected
            .as_ref()
            .map(|t| choices::choices(&self.entries, t))
            .unwrap_or_default();
        let selected = self
            .selected
            .as_ref()
            .and_then(|t| self.selections.get(&key(t)))
            .cloned()
            .unwrap_or_default();
        let disabled =
            self.pending.is_some() || !self.loaded || available.is_empty() || self.needs_runtime();
        self.picker.update(cx, |picker, cx| {
            picker.aria_label = self.selected.as_ref().map(|tool| {
                format!(
                    "{}: {}",
                    choices::name(tool),
                    self.locale.text("agents.credential")
                )
            });
            picker.placeholder = self.locale.text(if available.is_empty() {
                "agents.noCompatible"
            } else {
                "agents.choose"
            });
            picker.set_options(
                available.into_iter().map(|c| (c.value, c.label)).collect(),
                cx,
            );
            picker.selected = selected;
            picker.disabled = disabled;
            cx.notify();
        });
    }
    fn needs_runtime(&self) -> bool {
        self.selected == Some(Tool::Cursor)
            && !self
                .detections
                .iter()
                .any(|d| d.tool == aipass_config_writers::ToolId::Cursor && d.local_runtime_found)
    }
    fn can_switch(&self, cx: &gpui::App) -> bool {
        self.loaded
            && self.pending.is_none()
            && self.preview.is_none()
            && !self.picker.read(cx).selected.is_empty()
            && !self.needs_runtime()
            && self.status().is_none_or(|s| {
                !matches!(
                    s.state.as_str(),
                    "conflict" | "storage_unavailable" | "unavailable"
                )
            })
    }
    fn send(&mut self, operation: Operation, cx: &mut Context<Self>) {
        if self.pending.is_some() {
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        self.pending = Some(operation.kind());
        self.outbox.push_back(Request {
            generation: self.generation,
            operation,
        });
        self.reconcile(cx);
        cx.notify();
    }
    pub fn focused(&mut self, cx: &mut Context<Self>) {
        if self.active && self.setup.is_none() && self.copy_task.is_none() {
            self.scan(cx);
        }
    }
    pub fn take_request(&mut self) -> Option<Request> {
        self.outbox.pop_front()
    }
    fn scan(&mut self, cx: &mut Context<Self>) {
        if self.preview.is_none() && self.pending.is_none() {
            self.error.clear();
            self.send(Operation::Scan, cx);
        }
    }
    fn load_usage(&mut self, cx: &mut Context<Self>) {
        self.usage_failed = false;
        self.usage_needed = false;
        let offset = i32::from(
            time::UtcOffset::current_local_offset()
                .unwrap_or(time::UtcOffset::UTC)
                .whole_minutes(),
        );
        self.send(
            Operation::Usage {
                timezone_offset_minutes: offset,
            },
            cx,
        );
    }
    fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_switch(cx) {
            return;
        }
        let Some(tool) = &self.selected else {
            return;
        };
        let Some(choice) = choices::choices(&self.entries, tool)
            .into_iter()
            .find(|c| c.value == self.picker.read(cx).selected)
        else {
            return;
        };
        self.error.clear();
        self.message.clear();
        if choices::guide(tool).is_some() {
            self.show_setup(choice.request, window, cx);
            return;
        }
        self.plan = Some(choice.request.clone());
        self.send(Operation::Preview(choice.request), cx);
    }
    fn restore(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.status().and_then(|s| s.operation_id) {
            self.error.clear();
            self.message.clear();
            self.send(Operation::Restore(id), cx);
        }
    }
    pub fn complete(
        &mut self,
        completion: Completion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active
            || completion.generation != self.generation
            || self.pending != Some(completion.kind)
        {
            return;
        }
        self.pending = None;
        match completion.result {
            Ok(Outcome::Scanned {
                detections,
                statuses,
            }) => {
                self.detections = detections;
                self.statuses = statuses;
                self.loaded = true;
                self.reconcile(cx);
                if self.usage_needed {
                    self.load_usage(cx);
                }
            }
            Ok(Outcome::Usage(summary)) => {
                self.summary = Some(summary);
                self.usage_failed = false;
            }
            Ok(Outcome::Preview(response)) => {
                if let Some(token) = response.preview_id.clone().filter(|s| !s.is_empty()) {
                    if let Some(plan) = &mut self.plan {
                        plan.preview_id = Some(token);
                    }
                    let view = cx.new(|cx| {
                        crate::integrations::preview::Preview::new(
                            response,
                            true,
                            self.dark,
                            self.chinese,
                            window,
                            cx,
                        )
                    });
                    view.update(cx, |v, _| v.managed = true);
                    self.preview_subscription =
                        Some(
                            cx.subscribe_in(&view, window, |this, view, event, window, cx| {
                                if this.pending.is_some() {
                                    return;
                                }
                                match event {
                                    crate::integrations::preview::Event::Close => {
                                        this.preview = None;
                                        this.plan = None;
                                        this.switch_focus.focus(window, cx);
                                    }
                                    crate::integrations::preview::Event::Confirm => {
                                        if let Some(plan) = this.plan.clone() {
                                            view.update(cx, |v, cx| {
                                                v.busy = true;
                                                cx.notify();
                                            });
                                            this.send(Operation::Apply(plan), cx);
                                        }
                                    }
                                }
                                cx.notify();
                            }),
                        );
                    self.preview = Some(view);
                } else {
                    self.plan = None;
                    self.error = self.locale.text("agents.previewUnavailable");
                }
            }
            Ok(Outcome::Applied(result)) => {
                self.preview = None;
                self.preview_subscription = None;
                self.plan = None;
                self.switch_focus.focus(window, cx);
                if result.outcome == ToolConfigOutcome::Applied {
                    self.message = self.locale.text(if completion.kind == Kind::Restore {
                        "integration.restored"
                    } else {
                        "agents.switched"
                    });
                } else {
                    self.error = result.message.unwrap_or_else(|| {
                        self.locale
                            .text(if result.outcome == ToolConfigOutcome::LoginRequired {
                                "integration.loginRequired"
                            } else {
                                "integration.externalChange"
                            })
                    });
                }
                self.send(Operation::Scan, cx);
            }
            Ok(Outcome::Copied(copied)) => {
                self.copy_cancel = None;
                if let Some(setup) = &self.setup {
                    setup.update(cx, |v, cx| {
                        v.busy = false;
                        if copied {
                            v.copied = "key".into();
                        }
                        cx.notify();
                    });
                }
            }
            Err(error) => {
                if completion.kind == Kind::Usage {
                    self.usage_failed = true;
                } else if completion.kind == Kind::Copy {
                    if let Some(setup) = &self.setup {
                        setup.update(cx, |v, cx| {
                            v.busy = false;
                            v.error = error.message;
                            cx.notify();
                        });
                    }
                    self.cancel_copy();
                } else {
                    self.error = error.message;
                    if completion.kind == Kind::Scan {
                        self.loaded = true;
                    }
                    if completion.kind == Kind::Apply {
                        self.preview = None;
                        self.preview_subscription = None;
                        self.plan = None;
                        self.send(Operation::Scan, cx);
                    }
                }
            }
        }
        self.reconcile(cx);
        cx.notify();
    }
    pub fn modal(&self) -> Option<Entity<crate::integrations::preview::Preview>> {
        self.preview.clone()
    }
    pub fn setup_modal(&self) -> Option<Entity<setup::Setup>> {
        self.setup.clone()
    }
}
impl Drop for Agents {
    fn drop(&mut self) {
        self.cancel_copy();
    }
}
