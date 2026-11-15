//! Local proxy workspace, using the same typed Agent operations as the shell.
use crate::{
    admin_ui::{self, Fields},
    form_ui::select::{Choice, Selected},
    locale::Locale,
    theme::Theme,
};
use aipass_agent_protocol::{AgentRequest, ServerUsageSummary};
use aipass_desktop_native::administration::{
    Completion, Operation, Outcome, Request, Scope, Surface,
};
use aipass_proxy::{
    ProxyConfig, ProxyLogEntry, ProxyRouteConfig, ProxyStatus, UsageTimeseriesPoint,
};
use gpui::{prelude::*, Context, Entity, FocusHandle, ScrollHandle, Subscription, Window};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
use uuid::Uuid;
mod bind;
mod channel;
mod chart;
mod context_menu;
mod copying;
mod credential_picker;
mod dialogs;
#[cfg(feature = "parity-render")]
mod fixtures;
mod header;
mod integration;
mod integration_card;
mod log_format;
mod logs;
mod members;
mod range;
mod route;
mod search;
#[cfg(all(test, feature = "parity-render"))]
mod tests;
mod usage;
mod view;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Modal {
    Route,
    Logs,
    Search,
    ClearUsage,
    DeleteRoute,
}
pub struct Proxy {
    detections: Vec<aipass_config_writers::ToolDetection>,
    scanned: bool,
    plan: Option<aipass_agent_protocol::ToolConfigProxyRequest>,
    preview: Option<Entity<crate::integrations::preview::Preview>>,
    preview_readonly: bool,
    preview_subscription: Option<Subscription>,
    setup: Option<Entity<crate::agents_ui::setup::Setup>>,
    setup_subscription: Option<Subscription>,
    copy_cancel: Option<aipass_desktop_platform::clipboard::Cancellation>,
    copying: bool,
    dark: bool,
    revision: String,
    theme: Theme,
    locale: Locale,
    active: bool,
    loaded: bool,
    pending: bool,
    refreshing: bool,
    deferred_operation: Option<(Operation, bool)>,
    generation: u64,
    outbox: VecDeque<Request>,
    reload: bool,
    last_refresh: Instant,
    pub config: Option<ProxyConfig>,
    status: Option<ProxyStatus>,
    usage: Option<ServerUsageSummary>,
    series: Vec<UsageTimeseriesPoint>,
    logs: Vec<ProxyLogEntry>,
    log_list: gpui::ListState,
    log_focus: FocusHandle,
    route_width: gpui_base::TransitionState<f32>,
    entries: Vec<aipass_vault::EntrySummary>,
    selected: Option<Uuid>,
    context_menu: Option<(Uuid, gpui::Point<gpui::Pixels>)>,
    context_focus: crate::menu::MenuFocus,
    error: String,
    notice: String,
    editing_bind: bool,
    saving_bind: bool,
    bind_focus: FocusHandle,
    modal: Option<Modal>,
    closing_modal: Option<Modal>,
    modal_motion: crate::subscription_ui::motion::ModalMotion,
    advanced_disclosure: crate::disclosure::Disclosure,
    fields: Fields,
    route: Option<ProxyRouteConfig>,
    search_config: aipass_proxy::SearchConfig,
    bind_models: bool,
    advanced: bool,
    picker: Entity<credential_picker::Picker>,
    _picker_subscription: Subscription,
    protocol: Entity<Choice>,
    _protocol_subscription: Subscription,
    strategy: Entity<Choice>,
    _strategy_subscription: Subscription,
    choices: std::collections::BTreeMap<String, Entity<Choice>>,
    choice_subscriptions: Vec<Subscription>,
    usage_range: Entity<range::Range>,
    _range_subscription: Subscription,
    integration_open: bool,
    integration_disclosure: crate::disclosure::Disclosure,
    integration_endpoint_scroll: ScrollHandle,
    integration_token_scroll: ScrollHandle,
    usage_open: bool,
    usage_disclosure: crate::disclosure::Disclosure,
    hovered_bucket: Option<String>,
    chart_focus: chart::ChartFocus,
    fixture_now: Option<time::OffsetDateTime>,
    days: u32,
    hourly: bool,

    focus: FocusHandle,
    close_focus: FocusHandle,
    previous_focus: Option<FocusHandle>,
    list_scroll: ScrollHandle,
    detail_scroll: ScrollHandle,
    modal_scroll: ScrollHandle,
}
impl Proxy {
    pub fn new(dark: bool, chinese: bool, cx: &mut Context<Self>) -> Self {
        let t = Theme::new(dark);
        let locale = Locale::new(chinese);
        let picker = cx.new(|cx| credential_picker::Picker::new(t, &locale, cx));
        let picker_subscription = cx.subscribe(&picker, |s: &mut Self, _, e: &Selected, cx| {
            s.add_member(&e.0, cx);
        });
        let protocol = cx.new(|cx| {
            Choice::new(
                locale.text("routeGroup.inboundProtocol"),
                route::protocols(&locale),
                "open_ai_responses",
                t,
                cx,
            )
        });
        let protocol_subscription = cx.subscribe(&protocol, |s: &mut Self, _, e: &Selected, cx| {
            if let Some(r) = &mut s.route {
                if let Ok(v) = serde_json::from_value(serde_json::Value::String(e.0.clone())) {
                    r.inbound_protocol = v;
                    r.upstream_protocol = v;
                }
            }
            cx.notify();
        });
        let strategy = cx.new(|cx| {
            Choice::new(
                locale.text("server.strategy"),
                route::strategies(&locale),
                "fallback",
                t,
                cx,
            )
        });
        let strategy_subscription = cx.subscribe(&strategy, |s: &mut Self, _, e: &Selected, cx| {
            if let Some(r) = &mut s.route {
                if let Ok(v) = serde_json::from_value(serde_json::Value::String(e.0.clone())) {
                    r.strategy = v;
                }
            }
            cx.notify();
        });
        let usage_range = cx.new(|cx| range::Range::new(t, &locale, cx));
        let range_subscription = cx.subscribe(&usage_range, |s: &mut Self, _, e: &Selected, cx| {
            s.hourly = e.0 == "24h";
            s.days = if s.hourly {
                1
            } else {
                e.0.parse().unwrap_or(7)
            };
            s.hovered_bucket = None;
            s.refresh(cx);
        });
        Self {
            detections: vec![],
            scanned: false,
            plan: None,
            preview: None,
            preview_readonly: false,
            preview_subscription: None,
            setup: None,
            setup_subscription: None,
            copy_cancel: None,
            copying: false,
            dark,
            revision: String::new(),
            theme: t,
            locale,
            active: false,
            loaded: false,
            pending: false,
            refreshing: false,
            deferred_operation: None,
            generation: 0,
            outbox: VecDeque::new(),
            reload: false,
            last_refresh: Instant::now(),
            config: None,
            status: None,
            usage: None,
            series: vec![],
            logs: vec![],
            log_list: gpui::ListState::new(0, gpui::ListAlignment::Top, gpui::px(116.)),
            log_focus: cx.focus_handle(),
            route_width: Default::default(),
            entries: vec![],
            selected: None,
            context_menu: None,
            context_focus: crate::menu::MenuFocus::new(2, cx),
            error: String::new(),
            notice: String::new(),
            editing_bind: false,
            saving_bind: false,
            bind_focus: cx.focus_handle(),
            modal: None,
            closing_modal: None,
            modal_motion: Default::default(),
            advanced_disclosure: Default::default(),
            fields: Fields::default(),
            route: None,
            search_config: Default::default(),
            bind_models: false,
            advanced: false,
            picker,
            _picker_subscription: picker_subscription,
            protocol,
            _protocol_subscription: protocol_subscription,
            strategy,
            _strategy_subscription: strategy_subscription,
            choices: Default::default(),
            choice_subscriptions: vec![],
            usage_range,
            _range_subscription: range_subscription,
            integration_open: true,
            integration_disclosure: Default::default(),
            integration_endpoint_scroll: ScrollHandle::new(),
            integration_token_scroll: ScrollHandle::new(),
            usage_open: true,
            usage_disclosure: Default::default(),
            hovered_bucket: None,
            chart_focus: Default::default(),
            fixture_now: None,
            days: 7,
            hourly: false,

            focus: cx.focus_handle(),
            close_focus: cx.focus_handle(),
            previous_focus: None,
            list_scroll: ScrollHandle::new(),
            detail_scroll: ScrollHandle::new(),
            modal_scroll: ScrollHandle::new(),
        }
    }
    pub fn activate(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.active == active {
            return;
        }
        self.active = active;
        if active {
            self.refresh(cx);
        } else {
            self.retire(cx);
        }
        cx.notify();
    }
    fn retire(&mut self, cx: &mut Context<Self>) {
        self.preview = None;
        self.preview_subscription = None;
        self.setup = None;
        self.setup_subscription = None;
        self.plan = None;
        self.context_menu = None;
        self.cancel_copy();
        self.generation = self.generation.wrapping_add(1);
        self.pending = false;
        self.refreshing = false;
        self.deferred_operation = None;
        self.outbox.clear();
        self.modal = None;
        self.closing_modal = None;
        self.modal_motion = Default::default();
        self.fields = Fields::default();
        self.editing_bind = false;
        self.saving_bind = false;
        self.choices.clear();
        self.choice_subscriptions.clear();
        self.route = None;
        self.logs.clear();
        self.log_list.reset(0);
        self.chart_focus = Default::default();
        self.hovered_bucket = None;
        self.usage_range.update(cx, |v, cx| v.close(cx));
        self.picker.update(cx, |v, cx| v.close(cx));
        self.protocol.update(cx, |v, cx| v.close(cx));
        self.strategy.update(cx, |v, cx| v.close(cx));
    }
    pub fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.retire(cx);
        self.active = false;
        self.loaded = false;
        self.config = None;
        self.status = None;
        self.usage = None;
        self.series.clear();
        self.entries.clear();
        self.fields.clear(window, cx);
        cx.notify();
    }
    pub fn sync(&mut self, entries: Vec<aipass_vault::EntrySummary>, cx: &mut Context<Self>) {
        let revision = serde_json::to_string(&entries).expect("entry metadata");
        if self.revision == revision {
            return;
        }
        self.revision = revision;
        self.entries = entries;
        self.reconcile_picker(cx);
    }
    pub fn appearance(&mut self, dark: bool, cx: &mut Context<Self>) {
        self.dark = dark;
        self.theme = Theme::new(dark);
        if let Some(v) = &self.preview {
            v.update(cx, |v, cx| v.apply_appearance(dark, cx));
        }
        if let Some(v) = &self.setup {
            v.update(cx, |v, cx| v.appearance(dark, cx));
        }
        self.fields.appearance(self.theme, cx);
        self.reconcile_picker(cx);
        for picker in [&self.protocol, &self.strategy] {
            picker.update(cx, |v, cx| {
                v.theme = self.theme;
                cx.notify();
            });
        }
        cx.notify();
    }
    pub fn take_request(&mut self) -> Option<Request> {
        self.outbox.pop_front()
    }
    pub fn tick(&mut self, cx: &mut Context<Self>) {
        if self.active
            && self.modal == Some(Modal::Logs)
            && !self.pending
            && self.last_refresh.elapsed() >= Duration::from_secs(2)
        {
            self.last_refresh = Instant::now();
            self.request(Operation::Logs, false, cx);
            return;
        }
        if self.active
            && self.loaded
            && !self.pending
            && self.modal.is_none()
            && self.preview.is_none()
            && self.setup.is_none()
            && !self.copying
            && self.last_refresh.elapsed() >= Duration::from_secs(3)
        {
            self.refresh(cx);
        }
    }
    fn request(&mut self, operation: Operation, reload: bool, cx: &mut Context<Self>) {
        if self.pending {
            if self.refreshing && self.deferred_operation.is_none() {
                self.deferred_operation = Some((operation, reload));
                cx.notify();
            }
            return;
        }
        self.refreshing =
            self.loaded && matches!(&operation, Operation::LoadProxy { .. } | Operation::Logs);
        self.pending = true;
        self.reload = reload;
        if !self.refreshing {
            self.error.clear();
            self.notice.clear();
        }
        self.outbox.push_back(Request {
            scope: Scope {
                surface: Surface::Proxy,
                generation: self.generation,
            },
            operation,
        });
        cx.notify();
    }
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.last_refresh = Instant::now();
        let offset = time::UtcOffset::current_local_offset()
            .map(|v| v.whole_minutes().into())
            .unwrap_or(0);
        self.request(
            Operation::LoadProxy {
                days: self.days,
                hourly: self.hourly,
                offset,
            },
            false,
            cx,
        );
    }
    fn apply(&mut self, request: AgentRequest, cx: &mut Context<Self>) {
        self.request(Operation::Apply(Box::new(request)), true, cx);
    }
    pub fn complete(
        &mut self,
        completion: Completion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if completion.scope
            != (Scope {
                surface: Surface::Proxy,
                generation: self.generation,
            })
            || !self.active
        {
            return;
        }
        let was_refreshing = self.refreshing;
        self.pending = false;
        self.refreshing = false;
        match completion.result {
            Err(e) => {
                self.error = e.message;
                if !was_refreshing {
                    self.saving_bind = false;
                }
                if let Some(v) = &self.preview {
                    v.update(cx, |v, cx| {
                        v.busy = false;
                        v.error = self.error.clone();
                        cx.notify();
                    });
                }
                self.reload = false;
            }
            Ok(Outcome::Proxy {
                config,
                status,
                usage,
                series,
            }) => {
                self.loaded = true;
                self.config = Some(*config);
                self.status = Some(status);
                self.usage = Some(usage);
                self.series = series;
                if self.selected.is_none_or(|id| {
                    !self
                        .config
                        .as_ref()
                        .unwrap()
                        .routes
                        .iter()
                        .any(|r| r.id == id)
                }) {
                    self.selected = self.config.as_ref().unwrap().routes.first().map(|r| r.id);
                }
            }
            Ok(Outcome::Tools(detections)) => {
                self.scanned = true;
                self.detections = detections;
            }
            Ok(Outcome::ToolPreview(preview)) => self.show_tool_preview(preview, window, cx),
            Ok(Outcome::ToolApplied(result)) => {
                if result.outcome == aipass_agent_protocol::ToolConfigOutcome::Applied {
                    self.preview = None;
                    self.preview_subscription = None;
                    self.plan = None;
                    if let Some(focus) = self.previous_focus.take() {
                        focus.focus(window, cx);
                    }
                    self.request(Operation::Tools, false, cx);
                    self.notice = result.summary;
                } else {
                    self.error = result.message.unwrap_or(result.summary);
                    if let Some(v) = &self.preview {
                        v.update(cx, |v, cx| {
                            v.busy = false;
                            v.error = self.error.clone();
                            cx.notify();
                        });
                    }
                }
            }
            Ok(Outcome::Logs(logs)) => {
                self.update_logs(logs);
                self.last_refresh = Instant::now();
            }
            Ok(Outcome::Applied) => {
                if self.saving_bind {
                    self.saving_bind = false;
                    self.cancel_bind(window, cx);
                }
                self.close_modal(window, cx);
            }
            Ok(_) => {}
        }
        if let Some((operation, reload)) = self.deferred_operation.take() {
            self.request(operation, reload, cx);
            return;
        }
        if self.loaded && !self.scanned && !self.pending {
            self.request(Operation::Tools, false, cx);
        }
        if self.reload {
            self.reload = false;
            self.refresh(cx);
        }
        cx.notify();
    }
    fn button(&self, id: &str, label: &str, primary: bool) -> gpui_base::Button {
        crate::providers::controls::button(
            id.to_owned(),
            self.locale.text(label),
            primary,
            self.theme,
        )
        .disabled(self.busy())
    }
    fn busy(&self) -> bool {
        (self.pending && !self.refreshing) || self.deferred_operation.is_some()
    }
    fn field(
        &mut self,
        key: &str,
        label: &str,
        value: &str,
        secret: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        self.fields.field(
            (key, label, value, secret),
            self.theme,
            &self.locale,
            window,
            cx,
        )
    }
    fn open_modal(&mut self, modal: Modal, window: &mut Window, cx: &mut Context<Self>) {
        self.usage_range.update(cx, |v, cx| v.close(cx));
        self.fields.clear(window, cx);
        self.route_width = Default::default();
        self.choices.clear();
        self.choice_subscriptions.clear();
        self.closing_modal = None;
        self.modal = Some(modal);
        self.error.clear();
        self.previous_focus = window.focused(cx);
        self.close_focus.focus(window, cx);
        self.modal_scroll
            .set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
        cx.notify();
    }
    fn close_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        if self.pending && self.modal == Some(Modal::Logs) {
            self.generation = self.generation.wrapping_add(1);
            self.pending = false;
            self.refreshing = false;
            self.outbox.clear();
        }
        self.closing_modal = self.modal.take();
        if let Some(f) = self.previous_focus.take() {
            f.focus(window, cx);
        }
        cx.notify();
    }
}
