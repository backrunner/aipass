//! Native provider quick-integration controller. All files are written by Agent.
use crate::{locale::Locale, theme::Theme};
use aipass_agent_protocol::{
    CodexApiKeyMode, ToolConfigOutcome, ToolConfigRequest, ToolConfigStatus, ToolConfigTool,
};
use aipass_desktop_native::integration::{catalog, Completion, Operation, Outcome, Request, Scope};
use aipass_vault::EntrySummary;
use gpui::{prelude::*, Context, Entity, EventEmitter, Subscription, Task, Window};
use std::{collections::VecDeque, time::Duration};
#[cfg(feature = "parity-render")]
mod fixtures;
mod login;
mod native;
#[cfg(all(test, feature = "parity-render"))]
mod native_tests;
pub(crate) mod preview;
#[cfg(all(test, feature = "parity-render"))]
mod tests;
mod tool_rows;
mod view;

pub struct RequestEvent;
impl EventEmitter<RequestEvent> for Integrations {}
enum Pending {
    Scan,
    Preview(ToolConfigRequest, bool),
    Apply(ToolConfigRequest, bool),
    Reconnect(ToolConfigRequest),
    PrepareLogin(ToolConfigRequest, bool),
    Login,
    Restore,
}

pub struct Integrations {
    entry: Option<EntrySummary>,
    context: String,
    generation: u64,
    secret_id: Option<String>,
    codex_mode: CodexApiKeyMode,
    mode_focus: [gpui::FocusHandle; 2],
    detections: Vec<aipass_config_writers::ToolDetection>,
    statuses: Vec<ToolConfigStatus>,
    scanned: bool,
    expanded: bool,
    disclosure: crate::disclosure::Disclosure,
    picker_open: bool,
    picker_focus: crate::menu::MenuFocus,
    picker_bounds: std::rc::Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
    pending: Option<Pending>,
    requests: VecDeque<Request>,
    in_flight: Option<Scope>,
    login_plan: Option<ToolConfigRequest>,
    login: Option<Entity<login::Login>>,
    login_subscription: Option<Subscription>,
    plan: Option<ToolConfigRequest>,
    preview: Option<Entity<preview::Preview>>,
    preview_subscription: Option<Subscription>,
    blocked: bool,
    error: String,
    message: String,
    applied: Option<aipass_agent_protocol::ToolConfigApplyResponse>,
    message_timer: Option<Task<()>>,
    theme: Theme,
    locale: Locale,
    dark: bool,
    chinese: bool,
}

impl Integrations {
    pub(crate) fn apply_appearance(&mut self, dark: bool, cx: &mut gpui::Context<Self>) {
        self.theme = Theme::new(dark);
        self.dark = dark;
        if let Some(view) = &self.preview {
            view.update(cx, |view, cx| view.apply_appearance(dark, cx));
        }
        if let Some(view) = &self.login {
            view.update(cx, |view, cx| view.apply_appearance(dark, cx));
        }
        cx.notify();
    }

    pub fn new(dark: bool, chinese: bool, cx: &mut Context<Self>) -> Self {
        crate::menu::init(cx);
        Self {
            entry: None,
            context: String::new(),
            generation: 0,
            secret_id: None,
            codex_mode: CodexApiKeyMode::AuthJson,
            mode_focus: std::array::from_fn(|_| cx.focus_handle()),
            detections: vec![],
            statuses: vec![],
            scanned: false,
            expanded: true,
            disclosure: Default::default(),
            picker_open: false,
            picker_focus: crate::menu::MenuFocus::new(0, cx),
            picker_bounds: Default::default(),
            pending: None,
            requests: VecDeque::new(),
            in_flight: None,
            login_plan: None,
            login: None,
            login_subscription: None,
            plan: None,
            preview: None,
            preview_subscription: None,
            blocked: false,
            error: String::new(),
            message: String::new(),
            applied: None,
            message_timer: None,
            theme: Theme::new(dark),
            locale: Locale::new(chinese),
            dark,
            chinese,
        }
    }
    pub fn has_request(&self) -> bool {
        self.in_flight.is_none() && !self.requests.is_empty()
    }
    pub fn take_request(&mut self) -> Option<Request> {
        if self.in_flight.is_some() {
            return None;
        }
        let request = self.requests.pop_front()?;
        self.in_flight = Some(request.scope);
        Some(request)
    }
    pub fn modal(&self) -> Option<Entity<preview::Preview>> {
        self.preview.clone()
    }
    pub fn is_visible(&self) -> bool {
        self.entry.is_some()
    }
    fn invalidate(&mut self, cx: &mut Context<Self>) {
        self.retire_login(cx);
        self.generation = self.generation.wrapping_add(1);
        self.pending = None;
        self.requests
            .retain(|r| matches!(r.operation, Operation::LoginCancel(_)));
        self.plan = None;
        self.preview = None;
        self.preview_subscription = None;
        self.error.clear();
        self.message.clear();
        self.message_timer = None;
        self.applied = None;
        self.picker_open = false;
    }
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.invalidate(cx);
        // Session lock retires its generation and may discard the old reply.
        self.in_flight = None;
        self.requests.clear();
        self.entry = None;
        self.context.clear();
        self.secret_id = None;
        self.detections.clear();
        self.statuses.clear();
        self.scanned = false;
        cx.notify();
    }
    pub fn sync(&mut self, entry: Option<EntrySummary>, blocked: bool, cx: &mut Context<Self>) {
        let entry = entry.filter(|e| {
            e.credential_kind == aipass_provider_registry::CredentialKind::Api
                || catalog::native_tool(e).is_some()
        });
        let context = entry
            .as_ref()
            .map(|e| {
                serde_json::json!([
                    e.id,
                    e.title,
                    e.provider_id,
                    e.credential_kind,
                    e.provider_kind,
                    e.account_identity,
                    e.interface_type,
                    e.auth_scheme,
                    e.endpoints,
                    e.default_model,
                    e.supports_websockets,
                    e.secret_refs
                ])
                .to_string()
            })
            .unwrap_or_default();
        if self.context != context {
            let switched = self.entry.as_ref().map(|e| e.id) != entry.as_ref().map(|e| e.id);
            self.invalidate(cx);
            if switched {
                self.codex_mode = CodexApiKeyMode::AuthJson;
                self.expanded = true;
                self.secret_id = None;
            }
            self.secret_id = entry.as_ref().and_then(|e| {
                self.secret_id
                    .clone()
                    .filter(|id| e.secret_refs.iter().any(|s| &s.id == id))
                    .or_else(|| (e.secret_refs.len() == 1).then(|| e.secret_refs[0].id.clone()))
            });
            self.picker_focus =
                crate::menu::MenuFocus::new(entry.as_ref().map_or(0, |e| e.secret_refs.len()), cx);
            self.context = context;
            self.entry = entry;
        }
        self.blocked = blocked;
        if self.entry.is_some() && !self.scanned && !self.busy() {
            self.scan(cx);
        }
    }
    fn busy(&self) -> bool {
        self.blocked || self.pending.is_some()
    }
    fn send(&mut self, operation: Operation, pending: Pending, cx: &mut Context<Self>) {
        if self.pending.is_some() || (self.blocked && !matches!(operation, Operation::Scan)) {
            return;
        }
        let Some(entry) = &self.entry else {
            return;
        };
        self.generation = self.generation.wrapping_add(1);
        self.requests.push_back(Request {
            scope: Scope {
                entry: entry.id,
                generation: self.generation,
            },
            operation,
        });
        if matches!(pending, Pending::Login) {
            if let Some(login) = &self.login {
                login.update(cx, |view, cx| view.set_busy(true, cx));
            }
        }
        self.pending = Some(pending);
        self.error.clear();
        cx.emit(RequestEvent);
        cx.notify();
    }
    fn scan(&mut self, cx: &mut Context<Self>) {
        if self.pending.is_some() {
            return;
        }
        self.plan = None;
        self.preview = None;
        self.preview_subscription = None;
        self.send(Operation::Scan, Pending::Scan, cx);
    }
    fn select_secret(&mut self, id: String, cx: &mut Context<Self>) {
        if self.busy()
            || !self
                .entry
                .as_ref()
                .is_some_and(|e| e.secret_refs.iter().any(|s| s.id == id))
        {
            return;
        }
        self.invalidate(cx);
        self.secret_id = Some(id);
        cx.notify();
    }
    fn set_mode(&mut self, mode: CodexApiKeyMode, cx: &mut Context<Self>) {
        if self.pending.is_some() || self.codex_mode == mode {
            return;
        }
        self.invalidate(cx);
        self.codex_mode = mode;
        cx.notify();
    }
    fn tools(&self) -> Vec<ToolConfigTool> {
        let Some(entry) = &self.entry else {
            return vec![];
        };
        if let Some(tool) = catalog::native_tool(entry) {
            return catalog::installed(&tool, &self.detections)
                .then_some(tool)
                .into_iter()
                .collect();
        }
        catalog::AUTOMATIC
            .iter()
            .filter(|(tool, _)| {
                catalog::installed(tool, &self.detections)
                    && entry
                        .secret_refs
                        .iter()
                        .filter(|s| self.secret_id.as_ref().is_none_or(|id| &s.id == id))
                        .any(|s| catalog::compatible(tool, entry, s))
            })
            .map(|(tool, _)| tool.clone())
            .collect()
    }
    fn can_configure(&self, tool: &ToolConfigTool) -> bool {
        !self.busy()
            && catalog::installed(tool, &self.detections)
            && self.entry.as_ref().is_some_and(|e| {
                e.secret_refs
                    .iter()
                    .find(|s| Some(&s.id) == self.secret_id.as_ref())
                    .is_some_and(|s| {
                        catalog::native_tool(e).as_ref() == Some(tool) || catalog::ready(tool, e, s)
                    })
            })
    }
    fn show_preview(&mut self, tool: ToolConfigTool, readonly: bool, cx: &mut Context<Self>) {
        if !self.can_configure(&tool) {
            return;
        }
        self.plan = None;
        self.preview = None;
        self.preview_subscription = None;
        let request = self.request_for(tool).unwrap();
        self.send(
            Operation::Preview(request.clone()),
            Pending::Preview(request, readonly),
            cx,
        );
    }
    fn confirm(&mut self, cx: &mut Context<Self>) {
        if self.busy()
            || self
                .preview
                .as_ref()
                .is_none_or(|p| !p.read(cx).can_confirm())
        {
            return;
        }
        let Some(request) = self.plan.take() else {
            return;
        };
        if request.preview_id.as_deref().is_none_or(str::is_empty) {
            self.error = self.locale.text("integration.externalChange");
            return;
        }
        if let Some(preview) = &self.preview {
            preview.update(cx, |p, cx| {
                p.busy = true;
                cx.notify();
            });
        }
        self.send(
            Operation::Apply(request.clone()),
            Pending::Apply(request, false),
            cx,
        );
    }
    fn restore(&mut self, tool: &ToolConfigTool, cx: &mut Context<Self>) {
        let Some(status) = self
            .statuses
            .iter()
            .find(|s| &s.tool == tool && s.state != "conflict")
        else {
            return;
        };
        if let Some(id) = status.operation_id {
            self.send(Operation::Rollback(id), Pending::Restore, cx);
        }
    }
    pub fn complete(
        &mut self,
        completion: Completion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.in_flight == Some(completion.scope) {
            self.in_flight = None;
        }
        if matches!(completion.result, Ok(Outcome::Cancelled)) {
            if self.has_request() {
                cx.emit(RequestEvent);
            }
            return;
        }
        if self.entry.as_ref().map(|e| e.id) != Some(completion.scope.entry)
            || completion.scope.generation != self.generation
        {
            if let Ok(Outcome::Login(status)) = completion.result {
                self.cancel_ticket(status.ticket, cx);
            }
            if self.has_request() {
                cx.emit(RequestEvent);
            }
            return;
        }
        let Some(pending) = self.pending.take() else {
            return;
        };
        self.blocked = false;
        if self.complete_native(&pending, &completion.result, window, cx) {
            if self.has_request() {
                cx.emit(RequestEvent);
            }
            cx.notify();
            return;
        }
        match completion.result {
            Ok(Outcome::Scanned {
                detections,
                statuses,
            }) => {
                self.scanned = true;
                self.detections = detections;
                self.statuses = statuses;
            }
            Ok(Outcome::Preview(response)) => {
                if let Pending::Preview(mut request, readonly) = pending {
                    if response.entry_id != request.id
                        || response.tool != request.tool
                        || response.mode != request.mode
                    {
                        self.error = self.locale.text("integration.externalChange");
                    } else {
                        request.preview_id = response.preview_id.clone();
                        let can_apply = !readonly
                            && request
                                .preview_id
                                .as_deref()
                                .is_some_and(|id| !id.is_empty());
                        let view = cx.new(|cx| {
                            preview::Preview::new(
                                response,
                                can_apply,
                                self.dark,
                                self.chinese,
                                window,
                                cx,
                            )
                        });
                        self.preview_subscription =
                            Some(cx.subscribe(&view, |this, _, event, cx| match event {
                                preview::Event::Confirm => this.confirm(cx),
                                preview::Event::Close => {
                                    if !this.busy() {
                                        this.plan = None;
                                        this.preview = None;
                                        this.preview_subscription = None;
                                        cx.notify();
                                    }
                                }
                            }));
                        self.plan = can_apply.then_some(request);
                        self.preview = Some(view);
                    }
                }
            }
            Ok(Outcome::Applied(result)) => {
                self.preview = None;
                self.plan = None;
                self.preview_subscription = None;
                if result.outcome == ToolConfigOutcome::Applied {
                    self.message = self.locale.text(if matches!(pending, Pending::Restore) {
                        "integration.restored"
                    } else {
                        "integration.restartCli"
                    });
                    if matches!(pending, Pending::Apply(_, _)) {
                        self.applied = Some(result);
                        self.message_timer = Some(cx.spawn(async move |this, cx| {
                            cx.background_executor().timer(Duration::from_secs(4)).await;
                            let _ = this.update(cx, |this, cx| {
                                this.applied = None;
                                cx.notify();
                            });
                        }));
                    }
                    self.scanned = false;
                } else {
                    self.error = self.locale.text(match result.outcome {
                        ToolConfigOutcome::LoginRequired => "integration.loginRequired",
                        ToolConfigOutcome::Conflict => "integration.externalChange",
                        _ => "integration.storageUnavailable",
                    });
                }
            }
            Ok(Outcome::Login(_) | Outcome::CodeSent | Outcome::Cancelled) => {}
            Err(error) => {
                self.scanned = true;
                self.preview = None;
                self.plan = None;
                self.preview_subscription = None;
                self.error = error.message;
            }
        }
        if self.has_request() {
            cx.emit(RequestEvent);
        }
        cx.notify();
    }
}
