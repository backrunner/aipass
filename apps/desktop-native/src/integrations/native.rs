//! Native CLI subscription switching and target-bound login lifecycle.
use super::*;
use aipass_agent::AgentCommandError;
use aipass_agent_protocol::{ToolConfigLoginStatus, ToolConfigMode};
use uuid::Uuid;

impl Integrations {
    pub fn login_modal(&self) -> Option<Entity<login::Login>> {
        self.login.clone()
    }

    pub(super) fn request_for(&self, tool: ToolConfigTool) -> Option<ToolConfigRequest> {
        let entry = self.entry.as_ref()?;
        let native = catalog::native_tool(entry).as_ref() == Some(&tool);
        Some(ToolConfigRequest {
            id: entry.id,
            secret_id: self.secret_id.clone(),
            mode: if native {
                ToolConfigMode::Official
            } else {
                catalog::default_mode(&tool)
            },
            codex_api_key_mode: (!native && tool == ToolConfigTool::Codex)
                .then(|| self.codex_mode.clone()),
            tool,
            preview_id: None,
        })
    }

    pub(super) fn reconnect_request(&self, tool: &ToolConfigTool) -> Option<ToolConfigRequest> {
        // Reconnect the CLI's current account, even while viewing a different provider.
        if let Some(status) = self
            .statuses
            .iter()
            .find(|s| &s.tool == tool && s.mode == Some(ToolConfigMode::Official))
        {
            return Some(ToolConfigRequest {
                id: status.entry_id?,
                secret_id: status.secret_id.clone(),
                tool: tool.clone(),
                mode: ToolConfigMode::Official,
                codex_api_key_mode: None,
                preview_id: None,
            });
        }
        let request = self.request_for(tool.clone())?;
        (request.mode == ToolConfigMode::Official).then_some(request)
    }

    pub(super) fn reconnect(&mut self, tool: &ToolConfigTool, cx: &mut Context<Self>) {
        if let Some(request) = self.reconnect_request(tool) {
            self.send(
                Operation::Preview(request.clone()),
                Pending::Reconnect(request),
                cx,
            );
        }
    }

    pub(super) fn cancel_ticket(&mut self, ticket: Uuid, cx: &mut Context<Self>) {
        self.requests.push_back(Request {
            // Cleanup never owns the visible entry's generation or pending action.
            scope: Scope {
                entry: Uuid::nil(),
                generation: 0,
            },
            operation: Operation::LoginCancel(ticket),
        });
        if self.has_request() {
            cx.emit(RequestEvent);
        }
    }

    pub(super) fn retire_login(&mut self, cx: &mut Context<Self>) {
        if let Some(login) = self.login.take() {
            if let Some(ticket) = login.read(cx).ticket() {
                self.cancel_ticket(ticket, cx);
            }
        }
        self.login_subscription = None;
        self.login_plan = None;
    }

    pub(super) fn open_login(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy() || self.login.is_some() {
            return;
        }
        let Some(request) = self.login_plan.take() else {
            return;
        };
        let view = cx
            .new(|cx| login::Login::new(request.tool.clone(), self.dark, self.chinese, window, cx));
        self.login_subscription = Some(cx.subscribe(&view, |this, view, event, cx| {
            match event {
                login::Event::Close => {
                    this.retire_login(cx);
                    this.generation = this.generation.wrapping_add(1);
                    this.pending = None;
                    this.requests
                        .retain(|r| matches!(r.operation, Operation::LoginCancel(_)));
                }
                login::Event::Poll => {
                    if !this.busy() {
                        if let Some(ticket) = view.read(cx).ticket() {
                            this.send(Operation::LoginPoll(ticket), Pending::Login, cx);
                        }
                    } else {
                        view.update(cx, |view, cx| view.schedule(cx));
                    }
                }
                login::Event::Code(code) => {
                    if !this.busy() {
                        if let Some(ticket) = view.read(cx).ticket() {
                            this.send(
                                Operation::LoginCode(ticket, code.clone()),
                                Pending::Login,
                                cx,
                            );
                        }
                    }
                }
            }
            cx.notify();
        }));
        self.login = Some(view);
        self.send(Operation::LoginStart(request), Pending::Login, cx);
    }

    pub(super) fn receive_login(
        &mut self,
        status: &ToolConfigLoginStatus,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(login) = self.login.clone() else {
            self.cancel_ticket(status.ticket, cx);
            return;
        };
        if login
            .read(cx)
            .ticket()
            .is_some_and(|ticket| ticket != status.ticket)
        {
            self.cancel_ticket(status.ticket, cx);
            login.update(cx, |view, cx| {
                view.fail(self.locale.text("integration.loginFailed"), cx)
            });
            return;
        }
        login.update(cx, |view, cx| view.receive(status.clone(), window, cx));
        if login::succeeded(status) {
            self.retire_login(cx);
            self.message = self.locale.text("integration.restartCli");
            self.scanned = false;
        }
    }

    pub(super) fn complete_native(
        &mut self,
        pending: &Pending,
        result: &Result<Outcome, AgentCommandError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match (pending, result) {
            (
                Pending::Reconnect(original) | Pending::PrepareLogin(original, _),
                Ok(Outcome::Preview(response)),
            ) => {
                if response.entry_id != original.id
                    || response.tool != original.tool
                    || response.mode != original.mode
                    || response.preview_id.as_deref().is_none_or(str::is_empty)
                {
                    self.error = self.locale.text("integration.externalChange");
                    return true;
                }
                let mut request = original.clone();
                request.preview_id = response.preview_id.clone();
                if matches!(pending, Pending::Reconnect(_)) {
                    self.send(
                        Operation::Apply(request.clone()),
                        Pending::Apply(request, true),
                        cx,
                    );
                } else {
                    self.login_plan = Some(request);
                    if matches!(pending, Pending::PrepareLogin(_, true)) {
                        self.open_login(window, cx);
                    }
                }
                true
            }
            (_, Ok(Outcome::Applied(response)))
                if response.outcome == ToolConfigOutcome::LoginRequired =>
            {
                self.preview = None;
                self.preview_subscription = None;
                self.plan = None;
                let mut request = match pending {
                    Pending::Apply(request, _) => request.clone(),
                    Pending::Restore => ToolConfigRequest {
                        id: response.entry_id,
                        secret_id: None,
                        tool: response.tool.clone(),
                        mode: ToolConfigMode::Official,
                        codex_api_key_mode: None,
                        preview_id: None,
                    },
                    _ => return false,
                };
                request.preview_id = None;
                let open = matches!(pending, Pending::Restore | Pending::Apply(_, true));
                self.send(
                    Operation::Preview(request.clone()),
                    Pending::PrepareLogin(request, open),
                    cx,
                );
                true
            }
            (Pending::Login, Ok(Outcome::Login(status))) => {
                self.receive_login(status, window, cx);
                true
            }
            (Pending::Login, Ok(Outcome::CodeSent)) => {
                if let Some(login) = &self.login {
                    login.update(cx, |view, cx| {
                        view.set_busy(false, cx);
                        view.schedule(cx);
                    });
                }
                true
            }
            (Pending::Login, Err(error)) => {
                if let Some(login) = &self.login {
                    login.update(cx, |view, cx| view.fail(error.message.clone(), cx));
                }
                true
            }
            _ => false,
        }
    }
}
