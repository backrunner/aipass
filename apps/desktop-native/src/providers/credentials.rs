//! Transient credential editor. Vault writes and audited reads stay in Agent.
use crate::{locale::Locale, theme::Theme};
use aipass_agent_protocol::SensitiveString;
use aipass_desktop_native::credentials::{Completion, Operation, Outcome, Request, Scope};
use aipass_provider_registry::{BillingRule, InterfaceType, SecretRef};
use aipass_vault::{EntrySummary, SecretMetadataInput};
use gpui::{prelude::*, Context, Entity, EventEmitter, Focusable, Subscription, Task, Window};
use gpui_base::input::{InputAccessibility, InputEvent, InputState};
use std::{collections::BTreeMap, time::Duration};

mod copying;
mod pricing;
#[cfg(all(test, feature = "parity-render"))]
mod tests;
mod view;

pub enum Event {
    Pricing(String),
    Request,
    Settled(bool),
    Changed,
}
impl EventEmitter<Event> for Credentials {}

enum Pending {
    Reveal(String),
    Copy(String),
    Edit(String),
    Save,
    Remove(String),
}

struct Revealed {
    value: SensitiveString,
    scroll: gpui::ScrollHandle,
}

pub struct Credentials {
    pub pricing_config: Option<aipass_agent_protocol::PricingConfig>,
    entry: Option<EntrySummary>,
    theme: Theme,
    locale: Locale,
    editing_provider: bool,
    blocked: bool,
    generation: u64,
    pending: Option<Pending>,
    request: Option<Request>,
    revealed: BTreeMap<String, Revealed>,
    reveal_timer: Option<Task<()>>,
    editor: Option<Option<String>>,
    inputs: Vec<Entity<InputState>>,
    interface: InterfaceType,
    visible: bool,
    billing_open: bool,
    disclosure: crate::disclosure::Disclosure,
    disclosure_frame: crate::disclosure::Frame,
    billing_note: Option<String>,
    picker_open: bool,
    picker_focus: crate::menu::MenuFocus,
    picker_bounds: std::rc::Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
    error: String,
    notice: String,
    copied: Option<String>,
    hovered_key: Option<String>,
    notice_timer: Option<Task<()>>,
    copy_task: Option<Task<()>>,
    copy_cancellation: Option<aipass_desktop_platform::clipboard::Cancellation>,
    _subscriptions: Vec<Subscription>,
}

const LABELS: [&str; 8] = [
    "providerDetail.secretLabel",
    "providerDetail.secretValue",
    "providerDetail.keyGroup",
    "credential.endpointOverride",
    "credential.modelOverride",
    "providerDetail.gatewayRate",
    "providerForm.billingCurrency",
    "providerForm.billingUnitPrice",
];

impl Credentials {
    pub(crate) fn apply_appearance(&mut self, dark: bool, cx: &mut gpui::Context<Self>) {
        self.theme = Theme::new(dark);
        for (index, input) in self.inputs.iter().enumerate() {
            let mut style = self.theme.input_style();
            if index < 3 {
                style.muted_foreground = gpui::rgb(0xa9a9a9).into();
            }
            input.update(cx, |input, cx| {
                input.set_editor_style(style);
                cx.notify();
            });
        }
        cx.notify();
    }

    pub fn new(dark: bool, chinese: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let theme = Theme::new(dark);
        let locale = Locale::new(chinese);
        crate::menu::init(cx);
        let inputs: Vec<_> = LABELS
            .iter()
            .enumerate()
            .map(|(index, key)| {
                cx.new(|cx| {
                    let mut input =
                        InputState::new(window, cx)
                            .masked(index == 1)
                            .placeholder(match index {
                                0 => locale.text("providerDetail.secretLabelPlaceholder"),
                                1 => locale.text("providerDetail.apiKey"),
                                2 => locale.text("providerDetail.keyGroupPlaceholder"),
                                5 => "1.0".into(),
                                6 => "USD".into(),
                                _ => String::new(),
                            });
                    let mut style = theme.input_style();
                    if index < 3 {
                        style.muted_foreground = gpui::rgb(0xa9a9a9).into();
                    }
                    input.set_editor_style(style);
                    input.set_accessibility(
                        InputAccessibility::new(Some(gpui::Role::TextInput))
                            .with_label(Some(locale.text(key).into())),
                    );
                    input
                })
            })
            .collect();
        let subscriptions = inputs
            .iter()
            .map(|input| {
                cx.subscribe(input, |_, _, event, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.emit(Event::Changed);
                        cx.notify();
                    }
                })
            })
            .collect();
        Self {
            pricing_config: None,
            entry: None,
            theme,
            locale,
            editing_provider: false,
            blocked: false,
            generation: 0,
            pending: None,
            request: None,
            revealed: BTreeMap::new(),
            reveal_timer: None,
            editor: None,
            inputs,
            interface: InterfaceType::OpenAiCompatible,
            visible: false,
            billing_open: false,
            disclosure: Default::default(),
            disclosure_frame: Default::default(),
            billing_note: None,
            error: String::new(),
            notice: String::new(),
            copied: None,
            hovered_key: None,
            picker_open: false,
            picker_focus: crate::menu::MenuFocus::new(6, cx),
            picker_bounds: Default::default(),
            notice_timer: None,
            copy_task: None,
            copy_cancellation: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn has_editor(&self) -> bool {
        self.editor.is_some()
    }
    pub fn has_request(&self) -> bool {
        self.request.is_some()
    }
    pub fn take_request(&mut self) -> Option<Request> {
        self.request.take()
    }
    #[cfg(feature = "parity-render")]
    pub fn presentation_state(&self) -> serde_json::Value {
        serde_json::json!({"editor": self.editor.as_ref().map(|id| if id.is_some() { "edit" } else { "add" }),
            "revealed": self.revealed.len(), "billing": self.billing_open, "pending": self.pending.is_some()})
    }
    #[cfg(feature = "parity-render")]
    pub fn preview(&mut self, scenario: &str, window: &mut Window, cx: &mut Context<Self>) {
        match scenario {
            "providers-key-add" => self.begin(None, window, cx),
            "providers-key-edit" | "providers-key-billing" | "providers-key-reveal" => {
                let secret = self.entry.as_ref().unwrap().secret_refs[0].clone();
                if scenario == "providers-key-reveal" {
                    self.reveal(secret.id, cx);
                } else {
                    self.begin(Some(secret), window, cx);
                }
                let request = self.take_request().unwrap();
                self.complete(
                    Completion {
                        scope: request.scope,
                        result: Ok(Outcome::Revealed(
                            "synthetic-parity-key-not-a-credential".into(),
                        )),
                    },
                    window,
                    cx,
                );
                self.billing_open = scenario == "providers-key-billing";
            }
            _ => {}
        }
    }

    /// Also called on hidden panes, so filtering/removal cannot retain plaintext.
    pub fn sync(
        &mut self,
        entry: Option<EntrySummary>,
        editing: bool,
        blocked: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entry = entry
            .filter(|entry| entry.credential_kind == aipass_provider_registry::CredentialKind::Api);
        let changed_entry = self.entry.as_ref().map(|e| e.id) != entry.as_ref().map(|e| e.id);
        let changed_keys =
            self.entry.as_ref().map(|e| &e.secret_refs) != entry.as_ref().map(|e| &e.secret_refs);
        if changed_entry {
            self.clear(window, cx);
        } else if changed_keys {
            self.revealed.clear();
            self.reveal_timer = None;
            // Invalidate a read of a key that was replaced/removed during refresh.
            let changed = |id: &str| {
                self.entry
                    .as_ref()
                    .and_then(|e| e.secret_refs.iter().find(|s| s.id == id))
                    != entry
                        .as_ref()
                        .and_then(|e| e.secret_refs.iter().find(|s| s.id == id))
            };
            if self
                .editor
                .as_ref()
                .and_then(Option::as_deref)
                .is_some_and(changed)
                || matches!(&self.pending, Some(Pending::Reveal(id) | Pending::Edit(id) | Pending::Copy(id)) if changed(id))
            {
                self.cancel(window, cx);
            }
        }
        self.entry = entry;
        self.editing_provider = editing;
        self.blocked = blocked;
        self.update_disabled(cx);
    }

    pub fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel(window, cx);
        self.entry = None;
        self.hovered_key = None;
        self.revealed.clear();
        self.reveal_timer = None;
        self.notice.clear();
        self.notice_timer = None;
        self.blocked = false;
    }

    pub fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_copy();
        self.copied = None;
        self.generation = self.generation.wrapping_add(1);
        self.request = None;
        self.pending = None;
        self.editor = None;
        self.visible = false;
        self.billing_open = false;
        self.picker_open = false;
        self.billing_note = None;
        self.error.clear();
        for input in &self.inputs {
            // set_value replaces the editor and its undo history.
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.inputs[1].update(cx, |input, cx| input.set_masked(true, window, cx));
        self.update_disabled(cx);
        cx.emit(Event::Changed);
        cx.notify();
    }

    fn busy(&self) -> bool {
        self.blocked || self.pending.is_some()
    }
    fn update_disabled(&self, cx: &mut Context<Self>) {
        let busy = self.busy();
        for input in &self.inputs {
            input.update(cx, |input, cx| input.set_disabled(busy, cx));
        }
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
        self.notice.clear();
        self.update_disabled(cx);
        cx.emit(Event::Request);
        cx.notify();
    }

    fn reveal(&mut self, id: String, cx: &mut Context<Self>) {
        if self.revealed.remove(&id).is_some() {
            cx.notify();
            return;
        }
        if self
            .entry
            .as_ref()
            .is_some_and(|e| e.secret_refs.iter().any(|s| s.id == id))
        {
            self.send(Operation::Reveal(id.clone()), Pending::Reveal(id), cx);
        }
    }

    pub fn begin(
        &mut self,
        secret: Option<SecretRef>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy() || self.entry.is_none() {
            return;
        }
        self.cancel(window, cx);
        let entry = self.entry.as_ref().unwrap();
        self.interface = secret
            .as_ref()
            .and_then(|s| s.interface_type.clone())
            .unwrap_or_else(|| entry.interface_type.clone());
        let group = secret
            .as_ref()
            .and_then(|s| s.group.as_deref())
            .or_else(|| {
                secret
                    .as_ref()
                    .and(entry.gateway.as_ref())
                    .and_then(|g| g.group.as_deref())
            });
        let billing = secret.as_ref().and_then(|s| s.billing.as_ref());
        self.billing_note = billing.and_then(|b| b.note.clone());
        let values = [
            secret.as_ref().map(|s| s.label.as_str()),
            None,
            group,
            secret.as_ref().and_then(|s| s.endpoint.as_deref()),
            secret.as_ref().and_then(|s| s.default_model.as_deref()),
            billing.and_then(|b| b.rate.as_deref()).or_else(|| {
                secret
                    .as_ref()
                    .and(entry.gateway.as_ref())
                    .and_then(|g| g.rate.as_deref())
            }),
            billing.and_then(|b| b.currency.as_deref()),
            billing.and_then(|b| b.unit_price.as_deref()),
        ];
        for (input, value) in self.inputs.iter().zip(values) {
            input.update(cx, |input, cx| {
                input.set_value(value.unwrap_or_default().to_owned(), window, cx)
            });
        }
        let endpoint = entry
            .endpoints
            .iter()
            .find(|e| e.kind == aipass_provider_registry::EndpointKind::Api)
            .and_then(|e| e.url.clone())
            .unwrap_or_default();
        let model = entry
            .default_model
            .clone()
            .unwrap_or_else(|| self.locale.text("credential.inheritSite"));
        self.inputs[3].update(cx, |input, cx| input.set_placeholder(endpoint, window, cx));
        self.inputs[4].update(cx, |input, cx| input.set_placeholder(model, window, cx));
        self.editor = Some(secret.as_ref().map(|s| s.id.clone()));
        if let Some(secret) = secret {
            self.revealed.remove(&secret.id);
            self.send(
                Operation::Reveal(secret.id.clone()),
                Pending::Edit(secret.id),
                cx,
            );
        } else {
            self.inputs[0].read(cx).focus_handle(cx).focus(window, cx);
        }
        cx.emit(Event::Changed);
        cx.notify();
    }

    pub fn save(&mut self, cx: &mut Context<Self>) -> bool {
        if self.busy() {
            return false;
        }
        let Some(id) = self.editor.clone() else {
            return false;
        };
        let value = |i: usize| self.inputs[i].read(cx).value().to_string();
        let raw = SensitiveString::from(value(1));
        if let Err(key) =
            aipass_desktop_native::credentials::validate(&value(0), raw.expose(), &value(3))
        {
            self.error = self.locale.text(key);
            cx.notify();
            return false;
        }
        let metadata = SecretMetadataInput {
            interface_type: Some(self.interface.clone()),
            group: Some(value(2).trim().into()),
            endpoint: Some(value(3).trim().into()),
            default_model: Some(value(4).trim().into()),
            billing: id.as_ref().map(|_| BillingRule {
                rate: Some(value(5).trim().into()),
                currency: Some(value(6).trim().into()),
                unit_price: Some(value(7).trim().into()),
                note: self.billing_note.clone(),
            }),
        };
        let label = value(0).trim().to_owned();
        let operation = match id {
            Some(secret_id) => Operation::Update {
                secret_id,
                label,
                value: raw.expose().trim().into(),
                metadata,
            },
            None => Operation::Add {
                label,
                value: raw,
                metadata,
            },
        };
        self.send(operation, Pending::Save, cx);
        true
    }

    fn remove(&mut self, id: String, cx: &mut Context<Self>) {
        if self.editing_provider
            && self
                .entry
                .as_ref()
                .is_some_and(|e| e.secret_refs.iter().any(|s| s.id == id))
        {
            self.send(Operation::Remove(id.clone()), Pending::Remove(id), cx);
        }
    }

    pub fn complete(
        &mut self,
        completion: Completion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.entry.as_ref().map(|entry| entry.id) != Some(completion.scope.entry)
            || self.generation != completion.scope.generation
        {
            return;
        }
        let Some(pending) = self.pending.take() else {
            return;
        };
        self.blocked = false;
        self.copy_task = None;
        self.copy_cancellation = None;
        match completion.result {
            Ok(Outcome::Copied(copied)) => {
                if copied {
                    if let Pending::Copy(id) = pending {
                        self.copied = Some(id);
                    }
                }
                self.notice_timer = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(1800))
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        this.copied = None;
                        cx.notify();
                    });
                }));
                cx.emit(Event::Settled(copied));
            }
            Ok(Outcome::Revealed(value)) => match pending {
                Pending::Reveal(id) => {
                    self.revealed.insert(
                        id,
                        Revealed {
                            value,
                            scroll: gpui::ScrollHandle::new(),
                        },
                    );
                    self.reveal_timer = Some(cx.spawn(async move |this, cx| {
                        cx.background_executor()
                            .timer(Duration::from_secs(30))
                            .await;
                        let _ = this.update(cx, |this, cx| {
                            this.revealed.clear();
                            cx.notify();
                        });
                    }));
                }
                Pending::Edit(id)
                    if self.editor.as_ref().and_then(Option::as_deref) == Some(&id) =>
                {
                    self.inputs[1].update(cx, |input, cx| {
                        input.set_value(value.expose().to_owned(), window, cx)
                    });
                    self.inputs[0].read(cx).focus_handle(cx).focus(window, cx);
                }
                _ => {}
            },
            Ok(Outcome::Applied) => {
                let key = match pending {
                    Pending::Remove(ref id) => {
                        self.revealed.remove(id);
                        "notice.secretRemoved"
                    }
                    _ if self.editor.as_ref().is_some_and(Option::is_some) => {
                        "notice.secretUpdated"
                    }
                    _ => "notice.secretAdded",
                };
                self.cancel(window, cx);
                self.notice = self.locale.text(key);
                self.notice_timer = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(1800))
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        this.notice.clear();
                        cx.notify();
                    });
                }));
                cx.emit(Event::Settled(true));
            }
            Err(error) => {
                if matches!(pending, Pending::Edit(_)) {
                    self.cancel(window, cx);
                }
                self.error = error.message;
                cx.emit(Event::Settled(false));
            }
        }
        self.update_disabled(cx);
        cx.emit(Event::Changed);
        cx.notify();
    }
}
