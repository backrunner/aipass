use super::*;
use aipass_agent_protocol::{
    CommunityLoginInput, SensitiveString, SubscriptionImportInput, SubscriptionImportRetry,
    SubscriptionImportSource,
};

impl Subscriptions {
    pub(super) fn choose(&mut self, provider: String, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open || self.busy() {
            return;
        }
        self.abandon(window, cx);
        self.provider = provider;
        self.screen = Screen::Connect;
        self.method = self
            .provider()
            .and_then(|p| p.methods.first())
            .map(|m| m.index)
            .unwrap_or(0);
        self.code.update(cx, |input, cx| {
            input.set_masked(self.provider == "claude", window, cx);
            input.set_placeholder(
                self.text(if self.provider == "claude" {
                    "claudeConnect.pastePlaceholder"
                } else {
                    "communityConnect.code"
                }),
                window,
                cx,
            );
        });
        self.setup_inputs(window, cx);
        let query = self.query.read(cx).value().trim().to_lowercase();
        if let Some(index) = self
            .providers
            .iter()
            .filter(|p| {
                format!("{} {}", self.name(p), p.id)
                    .to_lowercase()
                    .contains(&query)
            })
            .position(|p| p.id == self.provider)
        {
            // The first layout has no scroll overflow metadata yet, so scroll_to_item
            // would consume the request without moving. Seed a row-aligned offset;
            // GPUI clamps it to the actual content bounds during prepaint.
            self.provider_scroll
                .set_offset(gpui::point(gpui::px(0.), gpui::px(-(index as f32 * 43.))));
        }
        if model::cli_owned(&self.provider) {
            self.queue(Operation::Cli(self.provider.clone()), cx);
        }
        self.body_scroll
            .set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
        cx.notify();
    }
    pub(super) fn setup_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_inputs(window, cx);
        self.inputs.clear();
        self.choices.clear();
        self._inputs.truncate(3);
        let count = self.provider().map(|p| p.methods.len()).unwrap_or_default();
        if self.picker_focus.items.len() < count {
            self.picker_focus
                .items
                .resize_with(count, || cx.focus_handle());
        }
        if let Some(method) = self.method().cloned() {
            for prompt in method.prompts {
                if prompt.kind == "select" {
                    if self.picker_focus.items.len() < prompt.options.len() {
                        self.picker_focus
                            .items
                            .resize_with(prompt.options.len(), || cx.focus_handle());
                    }
                    self.picker_triggers
                        .entry(prompt.key.clone())
                        .or_insert_with(|| cx.focus_handle());
                    self.choices.insert(
                        prompt.key,
                        prompt
                            .options
                            .first()
                            .map(|o| o.value.clone())
                            .unwrap_or_default(),
                    );
                } else {
                    let label = self.translated(prompt.message_key.as_deref(), &prompt.message);
                    let theme = self.theme;
                    let input = cx.new(|cx| {
                        let mut state = InputState::new(window, cx)
                            .placeholder(prompt.placeholder.unwrap_or_default());
                        state.set_editor_style(theme.input_style());
                        state.set_accessibility(
                            InputAccessibility::new(Some(gpui::Role::TextInput))
                                .with_label(Some(label.into())),
                        );
                        state
                    });
                    self._inputs.push(cx.subscribe(&input, |_, _, event, cx| {
                        if matches!(event, InputEvent::Change) {
                            cx.notify();
                        }
                    }));
                    self.inputs.insert(prompt.key, input);
                }
            }
        }
    }
    pub(super) fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.abandon(window, cx);
        self.provider.clear();
        self.reconnect_entry = None;
        self.screen = Screen::Directory;
        self.query
            .update(cx, |input, cx| input.set_value("", window, cx));
        cx.notify();
    }
    pub(super) fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open
            || self.busy()
            || self.pending.is_some()
            || (model::cli_owned(&self.provider) && !self.cli.as_ref().is_some_and(|s| s.available))
        {
            return;
        }
        if self.provider == "claude" {
            self.queue(Operation::ClaudeStart, cx);
            return;
        }
        let Some(method) = self.method().cloned() else {
            return;
        };
        let key = self.api_key.read(cx).value().to_string();
        if method.kind == "api" && key.trim().is_empty() {
            return;
        }
        let mut inputs = BTreeMap::new();
        for prompt in &method.prompts {
            let value = if prompt.kind == "select" {
                self.choices.get(&prompt.key).cloned().unwrap_or_default()
            } else {
                self.inputs
                    .get(&prompt.key)
                    .map(|i| i.read(cx).value().to_string())
                    .unwrap_or_default()
            };
            inputs.insert(prompt.key.clone(), SensitiveString::new(value));
        }
        self.error.clear();
        self.queue(
            Operation::Start(CommunityLoginInput {
                entry_id: self.reconnect_entry,
                provider: self.provider.clone(),
                method: self.method,
                inputs,
                api_key: (!key.is_empty()).then(|| SensitiveString::new(key)),
            }),
            cx,
        );
        // Erase secret fields and their undo state once handed to the typed request.
        self.api_key
            .update(cx, |input, cx| input.set_value("", window, cx));
        for prompt in method
            .prompts
            .iter()
            .filter(|p| p.kind != "select" && !p.retain)
        {
            if let Some(input) = self.inputs.get(&prompt.key) {
                input.update(cx, |input, cx| input.set_value("", window, cx));
            }
        }
        self.set_disabled(cx);
    }
    pub(super) fn submit_code(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending.is_some() {
            return;
        }
        let Some(flow) = self.login.as_ref().filter(|l| l.pending).map(|l| l.flow) else {
            return;
        };
        let value = self.code.read(cx).value().to_string();
        if value.trim().is_empty() {
            return;
        }
        self.code
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.queue(Operation::Code(flow, SensitiveString::new(value)), cx);
        self.set_disabled(cx);
    }
    pub(super) fn retry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending.is_some() {
            return;
        }
        self.error.clear();
        if let Some(flow) = self.login.as_ref().map(|l| l.flow) {
            self.queue(Operation::Poll(flow), cx);
        } else if !self.loaded {
            self.queue(Operation::Catalog, cx);
        } else if model::cli_owned(&self.provider) {
            self.queue(Operation::Cli(self.provider.clone()), cx);
        } else {
            self.start(window, cx);
        }
    }
    pub(super) fn cancel(&mut self, cx: &mut Context<Self>) {
        if self.pending.is_some() {
            return;
        }
        let flow = self
            .login
            .as_ref()
            .map(|l| l.flow)
            .or_else(|| self.task.as_ref().map(|t| Flow::Import(t.ticket)));
        if let Some(flow) = flow {
            self.next_poll = None;
            self.cancel_requested = true;
            self.queue(Operation::Cancel(flow), cx);
        }
    }
    pub fn show_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            self.restore = window.focused(cx);
        }
        self.abandon(window, cx);
        self.open = true;
        self.screen = Screen::Import;
        self.focus.focus(window, cx);
        self.queue(Operation::Import(SubscriptionImportInput::default()), cx);
    }
    pub(super) fn retry_import(&mut self, cx: &mut Context<Self>) {
        let Some(task) = &self.task else {
            return;
        };
        let source_ids = task
            .results
            .iter()
            .filter(|r| {
                matches!(
                    r.status,
                    aipass_agent_protocol::SubscriptionImportStatus::Failed
                        | aipass_agent_protocol::SubscriptionImportStatus::NeedsLogin
                        | aipass_agent_protocol::SubscriptionImportStatus::Cancelled
                )
            })
            .map(|r| r.source_id)
            .collect();
        let input = SubscriptionImportInput {
            retry: Some(SubscriptionImportRetry {
                ticket: task.ticket,
                source_ids,
            }),
            ..Default::default()
        };
        self.task = None;
        self.error.clear();
        self.cancel_requested = false;
        self.queue(Operation::Import(input), cx);
    }
    pub(super) fn choose_directory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending.is_some() {
            return;
        }
        let generation = self.generation;
        let provider = self.import_provider.clone();
        let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(self.text("subscriptionImport.directory").into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = paths.await;
            let _ = this.update_in(cx, |this, _, cx| {
                if this.generation != generation || !this.open {
                    return;
                }
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(root) = paths.into_iter().next() {
                            this.task = None;
                            this.queue(
                                Operation::Import(SubscriptionImportInput {
                                    provider_ids: vec![provider.clone()],
                                    sources: vec![SubscriptionImportSource {
                                        provider,
                                        root,
                                        selector: String::new(),
                                    }],
                                    retry: None,
                                }),
                                cx,
                            );
                        }
                    }
                    Ok(Ok(None)) => {}
                    _ => this.fail("subscriptionImport.error.task_failed".into(), cx),
                }
            });
        })
        .detach();
    }
    pub(super) fn refresh_usage(&mut self, cx: &mut Context<Self>) {
        if self.usage_pending {
            return;
        }
        if let Some(entry) = &self.entry {
            self.usage_pending = true;
            self.usage_error.clear();
            self.outbox.push_back(Request {
                generation: self.usage_generation,
                operation: Operation::Refresh(entry.id),
            });
            cx.notify();
        }
    }
}
