use super::controls::{button, icon};
use crate::{locale::Locale, theme::Theme};
use aipass_desktop_native::catalog::{draft::Draft, Action};
use aipass_provider_registry::{AuthScheme, InterfaceType};
use aipass_vault::EntrySummary;
use gpui::{
    div, prelude::*, px, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    Subscription, Window,
};
use gpui_base::{
    input::{InputAccessibility, InputEvent, InputState, TextareaState},
    Dialog,
};
use std::collections::BTreeSet;

pub enum FormEvent {
    Save,
    Close,
}
impl EventEmitter<FormEvent> for Form {}

pub struct Form {
    pub(super) theme: Theme,
    pub(super) dark: bool,
    pub(super) scroll: gpui::ScrollHandle,
    scroll_to_end: bool,
    pub(super) locale: Locale,
    pub(super) inputs: Vec<Entity<InputState>>,
    pub(super) draft: Draft,
    pub(super) notes: Entity<TextareaState>,
    pub(super) advanced: bool,
    pub(super) visible: BTreeSet<usize>,
    pub(super) picker: Option<&'static str>,
    pub(super) revealed: bool,
    pub(super) busy: bool,
    pub error: String,
    focus: FocusHandle,
    action: Option<Action>,
    pub(super) credentials: Option<Entity<super::credentials::Credentials>>,
    credentials_subscription: Option<Subscription>,
    save_after_credential: bool,
    _subscriptions: Vec<Subscription>,
}

pub(super) const LABELS: [&str; 10] = [
    "providerForm.title",
    "providerForm.secretLabel",
    "providerForm.apiKey",
    "providerForm.endpointUrl",
    "providerForm.defaultModel",
    "providerForm.tags",
    "providerForm.notes",
    "providerForm.domains",
    "providerForm.consoleUrl",
    "providerForm.maxConcurrentRequests",
];

impl Form {
    #[cfg(feature = "parity-render")]
    pub fn preview_advanced(&mut self) {
        self.advanced = true;
        self.scroll_to_end = true;
    }
    pub fn is_editing(&self) -> bool {
        self.draft.original.is_some()
    }
    pub fn new(
        entry: Option<EntrySummary>,
        dark: bool,
        chinese: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let draft = Draft::new(entry);
        let theme = Theme::new(dark);
        let locale = Locale::new(chinese);
        let values = [
            &draft.title,
            &draft.secret_label,
            &String::new(),
            &draft.endpoint,
            &draft.model,
            &draft.tags,
            &draft.notes,
            &draft.domains,
            &draft.console,
            &draft.concurrency,
        ];
        let inputs: Vec<_> = values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                cx.new(|cx| {
                    let placeholder = match index {
                        0 => locale.text("providerForm.titlePlaceholder"),
                        1 => locale.text("providerForm.secretLabelPlaceholder"),
                        2 => locale.text("providerForm.pasteApiKey"),
                        3 => "https://api.example.com".into(),
                        4 => "gpt-4o".into(),
                        5 => "prod, team".into(),
                        7 => locale.text("providerForm.domainFromApi"),
                        8 => "https://console.example.com".into(),
                        9 => locale.text("providerForm.unlimitedConcurrency"),
                        _ => String::new(),
                    };
                    let mut input = InputState::new(window, cx)
                        .masked(index == 2)
                        .placeholder(placeholder);
                    input.set_editor_style(theme.input_style());
                    input.set_accessibility(
                        InputAccessibility::new(Some(gpui::Role::TextInput))
                            .with_label(Some(locale.text(LABELS[index]).into())),
                    );
                    input.set_value((*value).clone(), window, cx);
                    input
                })
            })
            .collect();
        let notes = cx.new(|cx| {
            let mut input = TextareaState::new(window, cx).rows(3);
            input.set_editor_style(theme.input_style());
            input.set_value(draft.notes.clone(), window, cx);
            input.set_accessibility(
                InputAccessibility::new(Some(gpui::Role::MultilineTextInput))
                    .with_label(Some(locale.text(LABELS[6]).into())),
            );
            input
        });
        let mut subscriptions: Vec<_> = inputs
            .iter()
            .enumerate()
            .map(|(index, input)| {
                cx.subscribe(input, move |this, _, event, cx| {
                    if matches!(event, InputEvent::Change) {
                        if index == 9 {
                            this.draft.concurrency_touched = true;
                        }
                        cx.notify();
                    }
                })
            })
            .collect();
        subscriptions.push(cx.subscribe(&notes, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        }));
        let mut visible = BTreeSet::new();
        if draft.original.is_none() || !draft.endpoint.is_empty() {
            visible.insert(3);
        }
        for (index, value) in values.iter().enumerate().skip(4) {
            if !value.is_empty() {
                visible.insert(index);
            }
        }
        let focus = cx.focus_handle();
        inputs[0].read(cx).focus_handle(cx).focus(window, cx);
        Self {
            theme,
            dark,
            scroll: gpui::ScrollHandle::new(),
            scroll_to_end: false,
            locale,
            inputs,
            draft,
            notes,
            advanced: false,
            visible,
            picker: None,
            revealed: false,
            busy: false,
            error: String::new(),
            focus,
            action: None,
            credentials: None,
            credentials_subscription: None,
            save_after_credential: false,
            _subscriptions: subscriptions,
        }
    }

    pub fn take_action(&mut self) -> Option<Action> {
        self.action.take()
    }
    pub fn attach_credentials(
        &mut self,
        credentials: Entity<super::credentials::Credentials>,
        cx: &mut Context<Self>,
    ) {
        self.credentials_subscription = Some(cx.subscribe(&credentials, |this, _, event, cx| {
            if let super::credentials::Event::Settled(saved) = event {
                let resume = this.save_after_credential && *saved;
                this.save_after_credential = false;
                if resume {
                    this.set_busy(false, cx);
                    this.save(cx);
                }
            }
            cx.notify();
        }));
        self.credentials = Some(credentials);
    }
    /// Refresh fields owned by Agent background work without replacing edits.
    pub fn refresh_original(
        &mut self,
        entry: &EntrySummary,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .draft
            .original
            .as_ref()
            .is_none_or(|original| original.id != entry.id)
        {
            return;
        }
        if !self.draft.concurrency_touched {
            let value = entry
                .max_concurrent_requests
                .map(|n| n.to_string())
                .unwrap_or_default();
            if self.inputs[9].read(cx).value().as_ref() != value {
                self.inputs[9].update(cx, |input, cx| input.set_value(value, window, cx));
            }
        }
        if !self.draft.websocket_touched {
            self.draft.websockets = entry.supports_websockets.unwrap_or(true);
        }
        self.draft.original = Some(entry.clone());
    }
    pub fn set_error(&mut self, error: String) {
        self.error = match error.split_once("websocket_probe_unconfirmed:") {
            Some((_, message)) => self
                .locale
                .text("providerForm.websocketProbeFailed")
                .replace("{message}", message.trim()),
            None => error,
        };
    }
    pub fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        self.busy = busy;
        self.notes
            .update(cx, |input, cx| input.set_disabled(busy, cx));
        for input in &self.inputs {
            input.update(cx, |input, cx| input.set_disabled(busy, cx));
        }
        cx.notify();
    }
    pub fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_after_credential = false;
        if let Some(credentials) = self.credentials.take() {
            credentials.update(cx, |view, cx| view.cancel(window, cx));
        }
        self.credentials_subscription = None;
        for input in &self.inputs {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.notes
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.action = None;
        self.draft.api_key = Default::default();
    }
    pub(super) fn save(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.save_after_credential {
            return;
        }
        if self.is_editing() {
            if let Some(credentials) = &self.credentials {
                if credentials.read(cx).has_editor() {
                    self.save_after_credential = credentials.update(cx, |view, cx| view.save(cx));
                    cx.notify();
                    return;
                }
            }
        }
        let value = |index: usize| self.inputs[index].read(cx).value().to_string();
        let mut draft = Draft::new(self.draft.original.clone());
        draft.provider_id = self.draft.provider_id.clone();
        draft.interface = self.draft.interface.clone();
        draft.auth = self.draft.auth.clone();
        draft.websockets = self.draft.websockets;
        draft.websocket_touched = self.draft.websocket_touched;
        draft.title = value(0);
        draft.secret_label = value(1);
        draft.api_key = value(2).into();
        draft.endpoint = value(3);
        draft.model = value(4);
        draft.tags = value(5);
        draft.notes = self.notes.read(cx).value().to_string();
        draft.domains = value(7);
        draft.console = value(8);
        draft.concurrency = value(9);
        draft.concurrency_touched = self.draft.concurrency_touched;
        match draft.into_action() {
            Ok(action) => {
                self.action = Some(action);
                self.error.clear();
                self.set_busy(true, cx);
                cx.emit(FormEvent::Save);
            }
            Err(key) => {
                self.error = self.locale.text(key);
                cx.notify();
            }
        }
    }
    pub(super) fn choose(
        &mut self,
        kind: &'static str,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        match kind {
            "provider" => {
                let definitions = aipass_provider_registry::default_provider_definitions();
                if let Some(definition) = definitions.get(index) {
                    self.draft.choose_provider(definition.id.to_owned());
                    if self.inputs[3].read(cx).value().is_empty() {
                        if let Some((_, _, endpoint)) =
                            definition.endpoints.iter().find(|(_, kind, _)| {
                                *kind == aipass_provider_registry::EndpointKind::Api
                            })
                        {
                            self.inputs[3]
                                .update(cx, |input, cx| input.set_value(*endpoint, window, cx));
                            self.visible.insert(3);
                        }
                    }
                    if self.inputs[0].read(cx).value().is_empty() {
                        self.inputs[0].update(cx, |input, cx| {
                            input.set_value(definition.display_name.to_owned(), window, cx)
                        });
                    }
                }
            }
            "interface" => self.draft.choose_interface(
                [
                    InterfaceType::OpenAiCompatible,
                    InterfaceType::AnthropicMessages,
                    InterfaceType::Gemini,
                    InterfaceType::AzureOpenAi,
                    InterfaceType::Bedrock,
                    InterfaceType::CustomHttp,
                ][index]
                    .clone(),
            ),
            "auth" => {
                self.draft.auth_touched = true;
                self.draft.auth = [
                    AuthScheme::Bearer,
                    AuthScheme::XApiKey,
                    AuthScheme::GoogleApiKey,
                    AuthScheme::AzureApiKey,
                    AuthScheme::AwsProfile,
                    AuthScheme::CustomHeader,
                ][index]
                    .clone()
            }
            "add-field" => {
                self.visible.insert(index);
                if index >= 7 {
                    self.advanced = true;
                }
                if index == 6 {
                    self.notes.read(cx).focus_handle(cx).focus(window, cx);
                } else {
                    self.inputs[index]
                        .read(cx)
                        .focus_handle(cx)
                        .focus(window, cx);
                }
            }
            _ => {}
        }
        self.picker = None;
        cx.notify();
    }
}

#[cfg(all(test, feature = "parity-render"))]
mod tests;

impl Render for Form {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.scroll_to_end && self.scroll.max_offset().y > px(0.) {
            self.scroll
                .set_offset(gpui::point(px(0.), -self.scroll.max_offset().y));
            self.scroll_to_end = false;
        }
        let t = self.theme;
        let editing = self.draft.original.is_some();
        if editing {
            return self.edit(window, cx);
        }
        let title = self.locale.text(if editing {
            "providerModal.editProvider"
        } else {
            "providerList.addProvider"
        });
        let content = div()
            .on_children_prepainted(crate::parity::record(if self.error.is_empty() {
                &[
                    "provider-form-header",
                    "provider-form-body",
                    "provider-form-footer",
                ]
            } else {
                &[
                    "provider-form-header",
                    "provider-form-body",
                    "provider-form-error",
                    "provider-form-footer",
                ]
            }))
            .flex()
            .flex_col()
            .min_h_0()
            .overflow_hidden()
            .rounded(px(14.))
            .border_1()
            .border_color(t.border)
            .bg(t.surface)
            .text_color(t.text)
            .w_full()
            .max_h(window.viewport_size().height - px(32.))
            .child(
                div()
                    .flex_shrink_0()
                    .px(px(20.))
                    .py(px(16.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .border_b_1()
                    .border_color(t.divider)
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title.clone()),
                    )
                    .child(
                        icon(
                            "provider-form-close",
                            "x",
                            self.locale.text("common.close"),
                            t,
                        )
                        .disabled(self.busy)
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(FormEvent::Close))),
                    ),
            )
            .child(self.fields(window, cx))
            .when(!self.error.is_empty(), |el| {
                el.child(
                    div()
                        .mx(px(20.))
                        .mb(px(12.))
                        .p(px(12.))
                        .rounded(px(8.))
                        .bg(t.danger_soft)
                        .text_color(t.danger)
                        .text_size(px(12.))
                        .child(self.error.clone()),
                )
            })
            .child(
                div()
                    .flex_shrink_0()
                    .px(px(20.))
                    .py(px(14.))
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .border_t_1()
                    .border_color(t.divider)
                    .child(
                        button(
                            "provider-cancel",
                            self.locale.text("common.cancel"),
                            false,
                            t,
                        )
                        .border_color(gpui::rgba(0))
                        .bg(gpui::rgba(0))
                        .disabled(self.busy)
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(FormEvent::Close))),
                    )
                    .child(
                        button(
                            "provider-save",
                            self.locale.text(if self.busy {
                                "common.loading"
                            } else if editing {
                                "providerModal.saveChanges"
                            } else {
                                "providerList.addProvider"
                            }),
                            true,
                            t,
                        )
                        .disabled(self.busy)
                        .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                    ),
            );
        if editing {
            return content.into_any_element();
        }
        let owner = cx.entity().downgrade();
        Dialog::new(cx)
            .focus_handle(self.focus.clone())
            .accessibility_label(title)
            .close_on_escape(!self.busy)
            .close_on_backdrop_press(!self.busy)
            .on_open_change(move |open, _, _, cx| {
                if !open {
                    let _ = owner.update(cx, |_, cx| cx.emit(FormEvent::Close));
                }
            })
            .backdrop(div().absolute().inset_0().bg(gpui::rgba(0x0f111073)))
            .popup(div().w(px(540.)).child(content))
            .into_any_element()
    }
}
