//! Native provider quick-integration controller. All files are written by Agent.
use crate::{locale::Locale, theme::Theme};
use aipass_agent_protocol::{
    CodexApiKeyMode, ToolConfigOutcome, ToolConfigRequest, ToolConfigStatus, ToolConfigTool,
};
use aipass_desktop_native::integration::{catalog, Completion, Operation, Outcome, Request, Scope};
use aipass_vault::EntrySummary;
use gpui::{prelude::*, Context, Entity, EventEmitter, Subscription, Task, Window};
use std::time::Duration;
#[cfg(feature = "parity-render")]
mod fixtures;
mod preview;
#[cfg(all(test, feature = "parity-render"))]
mod tests;
mod view;

pub struct RequestEvent;
impl EventEmitter<RequestEvent> for Integrations {}
enum Pending {
    Scan,
    Preview(ToolConfigRequest, bool),
    Apply,
    Restore,
}

pub struct Integrations {
    entry: Option<EntrySummary>,
    context: String,
    generation: u64,
    secret_id: Option<String>,
    codex_mode: CodexApiKeyMode,
    detections: Vec<aipass_config_writers::ToolDetection>,
    statuses: Vec<ToolConfigStatus>,
    scanned: bool,
    expanded: bool,
    picker_open: bool,
    picker_focus: crate::menu::MenuFocus,
    pending: Option<Pending>,
    request: Option<Request>,
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
    pub fn new(dark: bool, chinese: bool, cx: &mut Context<Self>) -> Self {
        crate::menu::init(cx);
        Self {
            entry: None,
            context: String::new(),
            generation: 0,
            secret_id: None,
            codex_mode: CodexApiKeyMode::AuthJson,
            detections: vec![],
            statuses: vec![],
            scanned: false,
            expanded: true,
            picker_open: false,
            picker_focus: crate::menu::MenuFocus::new(0, cx),
            pending: None,
            request: None,
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
        self.request.is_some()
    }
    pub fn take_request(&mut self) -> Option<Request> {
        self.request.take()
    }
    pub fn modal(&self) -> Option<Entity<preview::Preview>> {
        self.preview.clone()
    }
    fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.pending = None;
        self.request = None;
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
        self.invalidate();
        self.entry = None;
        self.context.clear();
        self.secret_id = None;
        self.detections.clear();
        self.statuses.clear();
        self.scanned = false;
        cx.notify();
    }
    pub fn sync(&mut self, entry: Option<EntrySummary>, blocked: bool, cx: &mut Context<Self>) {
        let entry =
            entry.filter(|e| e.credential_kind == aipass_provider_registry::CredentialKind::Api);
        let context = entry
            .as_ref()
            .map(|e| {
                serde_json::json!([
                    e.id,
                    e.title,
                    e.provider_id,
                    e.credential_kind,
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
            self.invalidate();
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
        if self.busy() {
            return;
        }
        let Some(entry) = &self.entry else {
            return;
        };
        self.generation = self.generation.wrapping_add(1);
        self.request = Some(Request {
            scope: Scope {
                entry: entry.id,
                generation: self.generation,
            },
            operation,
        });
        self.pending = Some(pending);
        self.error.clear();
        cx.emit(RequestEvent);
        cx.notify();
    }
    fn scan(&mut self, cx: &mut Context<Self>) {
        if self.busy() {
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
        self.invalidate();
        self.secret_id = Some(id);
        cx.notify();
    }
    fn set_mode(&mut self, mode: CodexApiKeyMode, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        self.invalidate();
        self.codex_mode = mode;
        cx.notify();
    }
    fn tools(&self) -> Vec<ToolConfigTool> {
        let Some(entry) = &self.entry else {
            return vec![];
        };
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
                    .is_some_and(|s| catalog::ready(tool, e, s))
            })
    }
    fn show_preview(&mut self, tool: ToolConfigTool, readonly: bool, cx: &mut Context<Self>) {
        if !self.can_configure(&tool) {
            return;
        }
        self.plan = None;
        self.preview = None;
        self.preview_subscription = None;
        let request = ToolConfigRequest {
            id: self.entry.as_ref().unwrap().id,
            secret_id: self.secret_id.clone(),
            mode: catalog::default_mode(&tool),
            codex_api_key_mode: (tool == ToolConfigTool::Codex).then(|| self.codex_mode.clone()),
            tool,
            preview_id: None,
        };
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
        self.send(Operation::Apply(request), Pending::Apply, cx);
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
        if self.entry.as_ref().map(|e| e.id) != Some(completion.scope.entry)
            || completion.scope.generation != self.generation
        {
            return;
        }
        let Some(pending) = self.pending.take() else {
            return;
        };
        self.blocked = false;
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
                    if matches!(pending, Pending::Apply) {
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
            Err(error) => {
                self.scanned = true;
                self.preview = None;
                self.plan = None;
                self.preview_subscription = None;
                self.error = error.message;
            }
        }
        cx.notify();
    }
}
