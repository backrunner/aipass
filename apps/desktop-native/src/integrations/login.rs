//! The login dialog owns only transient display state; Agent owns the ticket.
use crate::{locale::Locale, providers::controls::button, theme::Theme};
use aipass_agent_protocol::{
    SensitiveString, ToolConfigLoginStatus, ToolConfigOutcome, ToolConfigTool,
};
use gpui::{
    div, prelude::*, px, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    Subscription, Task, Window,
};
use gpui_base::{
    input::{Input, InputAccessibility, InputEvent, InputState},
    Dialog,
};
use std::time::Duration;
use uuid::Uuid;

pub enum Event {
    Close,
    Poll,
    Code(SensitiveString),
}
impl EventEmitter<Event> for Login {}
pub struct Login {
    tool: ToolConfigTool,
    status: Option<ToolConfigLoginStatus>,
    code: Entity<InputState>,
    _input: Subscription,
    focus: FocusHandle,
    previous_focus: Option<FocusHandle>,
    timer: Option<Task<()>>,
    busy: bool,
    error: String,
    theme: Theme,
    locale: Locale,
    dark: bool,
}
pub(super) fn succeeded(status: &ToolConfigLoginStatus) -> bool {
    status.status == "complete"
        && status
            .result
            .as_ref()
            .is_some_and(|r| r.outcome == ToolConfigOutcome::Applied)
}
fn terminal(status: &str) -> bool {
    ["complete", "failed", "error", "expired", "cancelled"].contains(&status)
}

impl Login {
    pub(crate) fn apply_appearance(&mut self, dark: bool, cx: &mut gpui::Context<Self>) {
        self.theme = Theme::new(dark);
        self.dark = dark;
        self.code.update(cx, |input, cx| {
            input.set_editor_style(self.theme.input_style());
            cx.notify();
        });
        cx.notify();
    }

    pub fn new(
        tool: ToolConfigTool,
        dark: bool,
        chinese: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let theme = Theme::new(dark);
        let locale = Locale::new(chinese);
        let code = cx.new(|cx| {
            let mut input = InputState::new(window, cx)
                .placeholder(locale.text("claudeConnect.pastePlaceholder"));
            input.set_editor_style(theme.input_style());
            input.set_accessibility(
                InputAccessibility::new(Some(gpui::Role::TextInput))
                    .with_label(Some(locale.text("claudeConnect.pasteLabel").into())),
            );
            input
        });
        let input = cx.subscribe_in(&code, window, |this, _, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.submit(window, cx);
            }
            cx.notify();
        });
        let previous_focus = window.focused(cx);
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            tool,
            status: None,
            code,
            _input: input,
            focus,
            previous_focus,
            timer: None,
            busy: true,
            error: String::new(),
            theme,
            locale,
            dark,
        }
    }
    pub fn ticket(&self) -> Option<Uuid> {
        self.status.as_ref().map(|s| s.ticket)
    }
    pub fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        self.busy = busy;
        self.code
            .update(cx, |input, cx| input.set_disabled(busy, cx));
        cx.notify();
    }
    pub fn schedule(&mut self, cx: &mut Context<Self>) {
        if !self.error.is_empty() || self.status.as_ref().is_none_or(|s| terminal(&s.status)) {
            return;
        }
        self.timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let _ = this.update(cx, |_, cx| cx.emit(Event::Poll));
        }));
    }
    pub fn fail(&mut self, error: String, cx: &mut Context<Self>) {
        self.timer = None;
        self.error = error;
        self.set_busy(false, cx);
    }
    pub fn receive(
        &mut self,
        status: ToolConfigLoginStatus,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let failed = terminal(&status.status) && !succeeded(&status);
        let focus_code =
            status.requires_code && self.status.as_ref().is_none_or(|s| !s.requires_code);
        let error = status
            .message
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.locale.text("integration.loginFailed"));
        self.status = Some(status);
        self.set_busy(false, cx);
        if failed {
            self.fail(error, cx);
        } else {
            self.schedule(cx);
        }
        if focus_code {
            self.code.read(cx).focus_handle(cx).focus(window, cx);
        }
    }
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.timer = None;
        self.code
            .update(cx, |input, cx| input.set_value("", window, cx));
        if let Some(focus) = &self.previous_focus {
            focus.focus(window, cx);
        }
        cx.emit(Event::Close);
    }
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy
            || !self.error.is_empty()
            || self.status.as_ref().is_none_or(|s| !s.requires_code)
        {
            return;
        }
        let value = self.code.read(cx).value().to_string();
        if value.trim().is_empty() {
            return;
        }
        self.timer = None;
        self.code
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.set_busy(true, cx);
        cx.emit(Event::Code(SensitiveString::new(value)));
    }
    fn open_browser(&mut self, cx: &mut Context<Self>) {
        let Some(url) = self.status.as_ref().and_then(|s| s.url.as_deref()) else {
            return;
        };
        match aipass_desktop_native::subscriptions::links::authorization(
            url,
            self.tool == ToolConfigTool::ClaudeCode,
        ) {
            Ok(url) => cx.open_url(url.as_str()),
            Err(error) => self.fail(error.message, cx),
        }
    }
}
impl Render for Login {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let status = self.status.as_ref();
        let requires_code = status.is_some_and(|s| s.requires_code);
        let url = status.and_then(|s| s.url.clone());
        let user_code = status.and_then(|s| s.user_code.clone());
        let content = div()
            .w(px(460.))
            .max_h(window.viewport_size().height - px(64.))
            .rounded(px(14.))
            .border_1()
            .border_color(t.border)
            .bg(t.surface)
            .shadow(vec![gpui::BoxShadow {
                color: if self.dark {
                    gpui::rgba(0x00000080)
                } else {
                    gpui::rgba(0x0f111038)
                }
                .into(),
                offset: gpui::point(px(0.), px(24.)),
                blur_radius: crate::theme::shadow_blur(64.),
                spread_radius: px(0.),
                inset: false,
            }])
            .text_color(t.text)
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                div()
                    .px(px(20.))
                    .pt(px(20.))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(22.4))
                            .font_weight(FontWeight(650.))
                            .child(self.locale.text("integration.relogin")),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(18.))
                            .text_color(t.secondary)
                            .child(self.locale.text("integration.loginKeepsCurrent")),
                    ),
            )
            .child(
                div()
                    .id("tool-login-body")
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(20.))
                    .pt(px(18.))
                    .pb(px(20.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .when(!self.error.is_empty(), |el| {
                        el.child(
                            div()
                                .p(px(10.))
                                .rounded(px(8.))
                                .text_size(px(12.))
                                .text_color(t.danger)
                                .bg(t.danger.opacity(0.08))
                                .child(self.error.clone()),
                        )
                    })
                    .when(self.error.is_empty(), |el| {
                        el.when_some(url.clone(), |el, url| {
                            el.child(
                                div()
                                    .min_w_0()
                                    .px(px(12.))
                                    .py(px(10.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(t.border)
                                    .bg(t.surface2)
                                    .text_color(t.secondary)
                                    .font_family(super::preview::MONO)
                                    .text_size(px(11.))
                                    .text_ellipsis()
                                    .child(url),
                            )
                        })
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(18.))
                                .text_color(t.secondary)
                                .child(self.locale.text(if url.is_some() {
                                    "oauthConnect.waiting"
                                } else {
                                    "common.loading"
                                })),
                        )
                        .when_some(user_code, |el, code| {
                            el.child(
                                div()
                                    .self_start()
                                    .px(px(12.))
                                    .py(px(8.))
                                    .rounded(px(8.))
                                    .bg(t.surface2)
                                    .font_family(super::preview::MONO)
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(code),
                            )
                        })
                        .when(requires_code, |el| {
                            el.child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .child(self.locale.text("claudeConnect.pasteLabel")),
                                    )
                                    .child(
                                        div()
                                            .h(px(36.))
                                            .px(px(10.))
                                            .py(px(8.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(t.border)
                                            .text_size(px(13.))
                                            .child(Input::new(&self.code)),
                                    ),
                            )
                        })
                    }),
            )
            .child(
                div()
                    .px(px(20.))
                    .py(px(14.))
                    .border_t_1()
                    .border_color(t.divider)
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        button(
                            "tool-login-cancel",
                            self.locale.text("common.cancel"),
                            false,
                            t,
                        )
                        .border_color(gpui::rgba(0))
                        .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
                    )
                    .when(url.is_some() && self.error.is_empty(), |el| {
                        el.child(
                            button(
                                "tool-login-browser",
                                self.locale.text("oauthConnect.openBrowser"),
                                !requires_code,
                                t,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.open_browser(cx))),
                        )
                    })
                    .when(requires_code && self.error.is_empty(), |el| {
                        el.child(
                            button(
                                "tool-login-submit",
                                self.locale.text("claudeConnect.submit"),
                                true,
                                t,
                            )
                            .disabled(self.busy || self.code.read(cx).value().trim().is_empty())
                            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                        )
                    }),
            );
        let owner = cx.entity().downgrade();
        Dialog::new(cx)
            .focus_handle(self.focus.clone())
            .accessibility_label(self.locale.text("integration.relogin"))
            .on_open_change(move |open, _, window, cx| {
                if !open {
                    let _ = owner.update(cx, |this, cx| this.close(window, cx));
                }
            })
            .backdrop(
                div()
                    .absolute()
                    .inset_0()
                    .bg(gpui::rgba(0x00000066))
                    .backdrop_blur(px(4.)),
            )
            .popup(content)
    }
}

#[cfg(all(test, feature = "parity-render"))]
mod tests;
