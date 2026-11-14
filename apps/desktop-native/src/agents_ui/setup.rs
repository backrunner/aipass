use super::*;
use crate::{
    parity::Observe,
    providers::controls::{button, icon},
};
use aipass_desktop_platform::clipboard::CopyPermit;
use gpui::{div, px, AnyElement, EventEmitter, FontWeight};
use gpui_base::Dialog;
pub enum Event {
    Close,
    CopyKey,
}
impl EventEmitter<Event> for Setup {}
pub struct Setup {
    pub(super) request: ToolConfigRequest,
    endpoint: String,
    model: String,
    protocol: String,
    pub(super) theme: Theme,
    locale: Locale,
    focus: gpui::FocusHandle,
    pub(super) copied: String,
    pub(super) error: String,
    pub(super) busy: bool,
    scroll: gpui::ScrollHandle,
}
impl Agents {
    pub(super) fn cancel_copy(&mut self) {
        if let Some(cancel) = self.copy_cancel.take() {
            cancel.cancel();
        }
        self.copy_task = None;
    }
    pub(super) fn show_setup(
        &mut self,
        request: ToolConfigRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.entries.iter().find(|e| e.id == request.id) else {
            return;
        };
        let Some(secret) = entry
            .secret_refs
            .iter()
            .find(|s| Some(&s.id) == request.secret_id.as_ref())
        else {
            return;
        };
        let endpoint = secret
            .endpoint
            .clone()
            .or_else(|| {
                entry
                    .endpoints
                    .iter()
                    .find(|e| e.kind == aipass_provider_registry::EndpointKind::Api)
                    .and_then(|e| e.url.clone())
            })
            .unwrap_or_default();
        let model = secret
            .default_model
            .clone()
            .or(entry.default_model.clone())
            .unwrap_or_default();
        let protocol = if secret
            .interface_type
            .as_ref()
            .unwrap_or(&entry.interface_type)
            == &aipass_provider_registry::InterfaceType::AnthropicMessages
        {
            "Anthropic Messages"
        } else {
            "OpenAI Chat Completions"
        }
        .into();
        let view = cx.new(|cx| {
            let focus = cx.focus_handle();
            focus.focus(window, cx);
            Setup {
                request,
                endpoint,
                model,
                protocol,
                theme: self.theme,
                locale: Locale::new(self.chinese),
                focus,
                copied: String::new(),
                error: String::new(),
                busy: false,
                scroll: Default::default(),
            }
        });
        self.setup_subscription =
            Some(
                cx.subscribe_in(&view, window, |this, _, event, window, cx| {
                    match event {
                        Event::Close => {
                            this.cancel_copy();
                            this.setup = None;
                            this.setup_subscription = None;
                            this.switch_focus.focus(window, cx);
                        }
                        Event::CopyKey => this.copy_setup_key(window, cx),
                    }
                    cx.notify();
                }),
            );
        self.setup = Some(view);
        cx.notify();
    }
    fn copy_setup_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending.is_some() {
            return;
        }
        let Some(view) = &self.setup else {
            return;
        };
        if view.read(cx).busy {
            return;
        }
        let request = view.read(cx).request.clone();
        let permit = match CopyPermit::begin() {
            Ok(Some(p)) => p,
            Ok(None) => return,
            Err(error) => {
                view.update(cx, |v, cx| {
                    v.error = error;
                    cx.notify();
                });
                return;
            }
        };
        let policy = cx
            .try_global::<crate::preferences::Preferences>()
            .map(|p| p.0.clone())
            .unwrap_or_default();
        view.update(cx, |v, cx| {
            v.busy = true;
            v.error.clear();
            v.copied.clear();
            cx.notify();
        });
        self.cancel_copy();
        self.copy_cancel = Some(permit.cancellation());
        let operation = Operation::Copy(aipass_desktop_native::credentials::Request {
            scope: aipass_desktop_native::credentials::Scope {
                entry: request.id,
                generation: self.generation,
            },
            operation: aipass_desktop_native::credentials::Operation::Copy {
                secret_id: request.secret_id.clone().unwrap_or_default(),
                permit,
                seconds: policy.clipboard_clear_seconds,
            },
        });
        if !policy.confirm_secret_copy {
            self.send(operation, cx);
            return;
        }
        let generation = self.generation;
        let (title, detail, copy, cancel) =
            aipass_desktop_platform::clipboard::confirmation(self.locale.language());
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            title,
            Some(detail),
            &[copy, cancel],
            cx,
        );
        self.copy_task = Some(cx.spawn(async move |this, cx| {
            let accepted = answer.await == Ok(0);
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation
                    || !this.active
                    || this
                        .setup
                        .as_ref()
                        .is_none_or(|v| v.read(cx).request != request)
                {
                    return;
                }
                if accepted {
                    this.send(operation, cx);
                } else {
                    this.cancel_copy();
                    if let Some(view) = &this.setup {
                        view.update(cx, |v, cx| {
                            v.busy = false;
                            cx.notify();
                        });
                    }
                }
            });
        }));
    }
}
impl Setup {
    fn row(
        &self,
        index: usize,
        label: String,
        value: String,
        kind: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_key = kind == "key";
        let disabled = self.busy || (!is_key && value.is_empty());
        let payload = value.clone();
        let t = self.theme;
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .min_w_0()
            .flex_shrink_0()
            .child(
                div()
                    .w(px(68.))
                    .flex_shrink_0()
                    .text_color(t.secondary)
                    .text_size(px(12.))
                    .line_height(px(15.))
                    .child(label),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(if kind == "protocol" { 12. } else { 11. }))
                    .line_height(px(15.))
                    .when(kind != "protocol", |el| el.font_family("Menlo"))
                    .child(value),
            )
            .when(kind != "protocol", |el| {
                el.child(
                    button(
                        ("setup-copy", index),
                        self.locale.text(if self.copied == kind {
                            "integration.setupCopied"
                        } else if is_key {
                            "integration.setupCopyKey"
                        } else {
                            "common.copy"
                        }),
                        false,
                        t,
                    )
                    .h(px(26.))
                    .px(px(9.))
                    .text_size(px(12.))
                    .line_height(px(12.))
                    .disabled(disabled)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.busy {
                            return;
                        }
                        if is_key {
                            cx.emit(Event::CopyKey);
                        } else {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(payload.clone()));
                            this.copied = kind.into();
                            this.error.clear();
                            cx.notify();
                        }
                    })),
                )
            })
            .into_any_element()
    }
}
impl Render for Setup {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let owner = cx.entity().downgrade();
        let content = div()
            .id("agent-setup-scroll")
            .max_h(window.viewport_size().height - px(48.))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .p(px(20.))
            .rounded(px(14.))
            .border_1()
            .border_color(t.divider)
            .bg(t.surface)
            .text_color(t.text)
            .shadow(vec![gpui::BoxShadow {
                color: gpui::rgba(0x0000004d).into(),
                offset: gpui::point(px(0.), px(20.)),
                blur_radius: px(40.),
                spread_radius: px(0.),
                inset: false,
            }])
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .min_h(px(19.328125))
                    .mb(px(12.))
                    .child(
                        div()
                            .text_size(px(15.))
                            .line_height(px(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(choices::name(&self.request.tool)),
                    )
                    .child(
                        icon(
                            "agent-setup-close",
                            "x",
                            self.locale.text("common.close"),
                            t,
                        )
                        .size(px(16.))
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(Event::Close))),
                    ),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(19.2))
                    .text_color(t.secondary)
                    .mb(px(16.))
                    .child(self.locale.text(&format!(
                        "integration.setup.{}",
                        choices::guide(&self.request.tool).unwrap_or("cline")
                    ))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(self.row(
                        0,
                        self.locale.text("integration.setupProtocol"),
                        self.protocol.clone(),
                        "protocol",
                        cx,
                    ))
                    .child(self.row(
                        1,
                        self.locale.text("integration.setupEndpoint"),
                        self.endpoint.clone(),
                        "endpoint",
                        cx,
                    ))
                    .child(self.row(
                        2,
                        self.locale.text("integration.setupModel"),
                        self.model.clone(),
                        "model",
                        cx,
                    ))
                    .child(self.row(3, "API Key".into(), String::new(), "key", cx)),
            )
            .when(!self.error.is_empty(), |el| {
                el.child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(18.))
                        .text_color(t.danger)
                        .child(self.error.clone()),
                )
            })
            .child(
                div().mt(px(18.)).flex().justify_end().child(
                    button(
                        "agent-setup-done",
                        self.locale.text("common.close"),
                        false,
                        t,
                    )
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(Event::Close))),
                ),
            )
            .observe("agent-setup-dialog");
        Dialog::new(cx)
            .focus_handle(self.focus.clone())
            .accessibility_label(choices::name(&self.request.tool))
            .on_open_change(move |open, _, _, cx| {
                if !open {
                    let _ = owner.update(cx, |_, cx| cx.emit(Event::Close));
                }
            })
            .backdrop(
                div()
                    .absolute()
                    .inset_0()
                    .bg(gpui::rgba(0x00000073))
                    .backdrop_blur(px(4.)),
            )
            .popup(
                div()
                    .w(px(580.).min(window.viewport_size().width - px(48.)))
                    .child(content),
            )
    }
}
