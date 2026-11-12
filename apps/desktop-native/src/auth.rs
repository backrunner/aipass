use crate::{backdrop::Backdrop, locale::Locale, theme::Theme};
use aipass_agent_protocol::SensitiveString;
use gpui::{
    div, prelude::*, px, rgb, rgba, svg, BoxShadow, Context, Entity, EventEmitter, Focusable,
    FontWeight, Subscription, Window,
};
use gpui_base::{
    input::{Input, InputAccessibility, InputEvent, InputState},
    Button,
};
mod feedback;
mod recovery;
mod scroll_fade;
mod strength;
mod surface;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Choose,
    Create,
    Unlock,
    Recover,
}

pub enum AuthEvent {
    Unlock(SensitiveString),
    Create(SensitiveString),
    Recover {
        key: SensitiveString,
        password: SensitiveString,
    },
    Import,
    Reset,
}

pub struct AuthView {
    pub mode: Mode,
    theme: Theme,
    locale: Locale,
    password: Entity<InputState>,
    confirmation: Entity<InputState>,
    recovery: Entity<InputState>,
    reset_confirmation: Entity<InputState>,
    reset_open: bool,
    reset_busy: bool,
    pub busy: bool,
    pub error: String,
    revealed: bool,
    backdrop: Backdrop,
    shadow: surface::CardShadow,
    scroll: gpui::ScrollHandle,
    scroll_fade: scroll_fade::ScrollFade,
    scroll_to_end: bool,
    focus: gpui::FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<AuthEvent> for AuthView {}

impl AuthView {
    pub fn sync_exists(&mut self, exists: bool, window: &mut Window, cx: &mut Context<Self>) {
        let next = match (exists, self.mode) {
            (false, Mode::Unlock | Mode::Recover) => Some(Mode::Choose),
            (true, Mode::Choose | Mode::Create) => Some(Mode::Unlock),
            _ => None,
        };
        if let Some(mode) = next {
            self.clear(window, cx);
            self.mode = mode;
            cx.notify();
        }
    }
    #[cfg(feature = "parity-render")]
    pub fn capture_ready(&self) -> Result<bool, String> {
        self.backdrop
            .ready()
            .map(|ready| ready && (self.shadow.ready() || self.scroll.max_offset().y > px(1.)))
    }
    pub fn new(
        mode: Mode,
        dark: bool,
        chinese: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let locale = Locale::new(chinese);
        let mut field = |key: &str, masked| {
            cx.new(|cx| {
                let mut input = InputState::new(window, cx).masked(masked);
                input.set_editor_style(Theme::new(dark).input_style());
                input.set_accessibility(
                    InputAccessibility::new(Some(gpui::Role::TextInput))
                        .with_label(Some(locale.text(key).into()))
                        .with_value_exposed(false),
                );
                input
            })
        };
        let password = field("auth.masterPassword", true);
        let confirmation = field("auth.confirmPassword", true);
        let recovery = field("auth.recoveryKey", false);
        let reset_confirmation = field("auth.reset.placeholder", false);
        reset_confirmation.update(cx, |input, cx| {
            let mut style = Theme::new(dark).input_style();
            // This plain input uses WebKit's default placeholder color; the
            // recovery-key field explicitly uses the theme's muted color.
            style.muted_foreground = rgb(0xa9a9a9).into();
            input.set_editor_style(style);
            input.set_placeholder(locale.text("auth.reset.placeholder"), window, cx)
        });
        recovery.update(cx, |input, cx| {
            input.set_placeholder("AIPASS-...", window, cx)
        });
        let mut subscriptions: Vec<_> = [&password, &confirmation, &recovery]
            .into_iter()
            .map(|field| {
                cx.subscribe_in(field, window, |this, _, event, window, cx| match event {
                    InputEvent::PressEnter { .. } => this.submit(window, cx),
                    InputEvent::Change => cx.notify(),
                    _ => {}
                })
            })
            .collect();
        subscriptions
            .push(cx.subscribe(&reset_confirmation, |_, _, _: &InputEvent, cx| cx.notify()));
        if mode == Mode::Unlock {
            password.update(cx, |input, cx| input.focus(window, cx));
        }
        Self {
            mode,
            theme: Theme::new(dark),
            locale,
            password,
            confirmation,
            recovery,
            reset_confirmation,
            reset_open: false,
            reset_busy: false,
            busy: false,
            error: String::new(),
            revealed: false,
            backdrop: Backdrop::new(dark, true),
            shadow: surface::CardShadow::default(),
            scroll: gpui::ScrollHandle::new(),
            scroll_fade: scroll_fade::ScrollFade::default(),
            scroll_to_end: false,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    pub fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        if self.reset_busy {
            self.reset_busy = busy;
            self.reset_confirmation
                .update(cx, |input, cx| input.set_disabled(busy, cx));
            cx.notify();
            return;
        }
        self.busy = busy;
        for input in [&self.password, &self.confirmation, &self.recovery] {
            input.update(cx, |input, cx| input.set_disabled(busy, cx));
        }
        cx.notify();
    }

    fn ready(&self, cx: &Context<Self>) -> bool {
        if self.busy || self.reset_busy {
            return false;
        }
        let password = self.password.read(cx).value();
        if password.is_empty() {
            return false;
        }
        match self.mode {
            Mode::Unlock => true,
            Mode::Create => password == self.confirmation.read(cx).value(),
            Mode::Recover => {
                password == self.confirmation.read(cx).value()
                    && !self.recovery.read(cx).value().trim().is_empty()
            }
            Mode::Choose => false,
        }
    }

    pub fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for input in [&self.password, &self.confirmation, &self.recovery] {
            // set_value also clears the editor's undo and IME history.
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.error.clear();
        self.reset_open = false;
        self.reset_busy = false;
        self.reset_confirmation.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.set_disabled(false, cx);
        });
        self.revealed = false;
        for input in [&self.password, &self.confirmation] {
            input.update(cx, |input, cx| input.set_masked(true, window, cx));
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode == Mode::Unlock {
            self.password.read(cx).focus_handle(cx).focus(window, cx);
        } else {
            self.focus.focus(window, cx);
        }
    }

    fn change_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.reset_busy {
            return;
        }
        self.clear(window, cx);
        self.mode = mode;
        self.focus(window, cx);
        cx.notify();
    }

    fn submit(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.ready(cx) {
            return;
        }
        let password = SensitiveString::new(self.password.read(cx).value().to_string());
        let event = match self.mode {
            Mode::Unlock => AuthEvent::Unlock(password),
            Mode::Create => AuthEvent::Create(password),
            Mode::Recover => AuthEvent::Recover {
                password,
                key: SensitiveString::new(self.recovery.read(cx).value().to_string()),
            },
            Mode::Choose => return,
        };
        // Keep fields available for correction after failure, as in the reference.
        // Success, lock and mode changes clear the editor and its undo history.
        self.error.clear();
        self.set_busy(true, cx);
        cx.emit(event);
    }

    fn field(
        &self,
        input: &Entity<InputState>,
        label: &str,
        toggle: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let focused = input.read(cx).focus_handle(cx).is_focused(window);
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(15.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(self.theme.secondary)
                    .child(self.locale.text(label)),
            )
            .child(
                div()
                    .h(px(if input == &self.recovery { 36. } else { 38. }))
                    .w_full()
                    .flex()
                    .items_center()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(if focused {
                        self.theme.accent
                    } else {
                        self.theme.border
                    })
                    .when(focused, |el| {
                        el.shadow(vec![BoxShadow::new(
                            px(0.),
                            px(0.),
                            self.theme.focus_ring.into(),
                        )
                        .spread_radius(px(3.))])
                    })
                    .bg(self.theme.surface)
                    .text_size(px(13.))
                    .line_height(px(20.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px(px(12.))
                            .relative()
                            .when(input == &self.recovery, |el| el.font_family("Menlo"))
                            .when(
                                input != &self.recovery
                                    && !self.revealed
                                    && !input.read(cx).value().is_empty(),
                                |el| el.font_family("AIPass Password Dots"),
                            )
                            .child(
                                div()
                                    .when(self.busy, |el| el.opacity(0.))
                                    .child(Input::new(input)),
                            )
                            .when(self.busy, |el| {
                                // The editor remains mounted and disabled for native
                                // semantics. Its built-in 50% dim differs from the
                                // reference's 65%; paint the inert value once.
                                let value = input.read(cx).value();
                                let text = if input != &self.recovery && !self.revealed {
                                    "•".repeat(value.chars().count())
                                } else {
                                    value.to_string()
                                };
                                el.child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .px(px(12.))
                                        .text_color(self.theme.text.opacity(0.65))
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .child(text),
                                )
                            }),
                    )
                    .when(toggle, |el| {
                        el.child(
                            Button::new("password-visibility")
                                .disabled(self.busy)
                                .accessibility_label(self.locale.text(if self.revealed {
                                    "password.hide"
                                } else {
                                    "password.show"
                                }))
                                .w(px(36.))
                                .h_full()
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .border_l_1()
                                .border_color(self.theme.divider)
                                .cursor_pointer()
                                .text_color(self.theme.tertiary)
                                .hover(|style| style.text_color(self.theme.text))
                                .when(self.busy, |el| el.opacity(0.55))
                                .child(
                                    svg()
                                        .path(if self.revealed { "eye-off" } else { "eye" })
                                        .size(px(15.))
                                        .text_color(self.theme.tertiary),
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    if this.busy {
                                        return;
                                    }
                                    this.revealed = !this.revealed;
                                    for input in [&this.password, &this.confirmation] {
                                        input.update(cx, |input, cx| {
                                            input.set_masked(!this.revealed, window, cx)
                                        });
                                    }
                                    cx.notify();
                                })),
                        )
                    }),
            )
    }

    fn button(&self, id: &'static str, label: &str, primary: bool, cx: &Context<Self>) -> Button {
        let loading = id == "submit" && self.busy;
        Button::new(id)
            .relative()
            .accessibility_label(self.locale.text(label))
            .disabled(self.busy)
            .w_full()
            .h(px(32.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(8.))
            .border_1()
            .border_color(if primary { rgba(0) } else { self.theme.border })
            .bg(if primary {
                rgb(0x2563eb)
            } else {
                self.theme.surface
            })
            .text_color(if primary {
                rgb(0xffffff)
            } else {
                self.theme.text
            })
            .text_size(px(13.))
            .line_height(px(13.))
            .font_weight(FontWeight::MEDIUM)
            .cursor_pointer()
            .hover(|style| {
                style.bg(if primary {
                    rgb(0x1f4fd0)
                } else {
                    self.theme.surface2
                })
            })
            .child(
                div()
                    .when(loading, |el| el.opacity(0.))
                    .child(self.locale.text(label)),
            )
            .when(loading, |el| {
                el.child(feedback::spinner(
                    self.theme.surface.blend(rgb(0xffffff).opacity(0.45)),
                    cx.reduce_motion(),
                ))
            })
    }
}

#[cfg(all(test, feature = "parity-render"))]
mod tests;

impl Render for AuthView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.scroll_to_end && self.scroll.max_offset().y > px(0.) {
            self.scroll
                .set_offset(gpui::point(px(0.), -self.scroll.max_offset().y));
            self.scroll_to_end = false;
        }
        let (title, description, submit) = match self.mode {
            Mode::Choose => ("auth.setup.title", "auth.setup.desc", ""),
            Mode::Create => (
                "auth.create.title",
                "auth.create.desc",
                if self.busy {
                    "auth.create.busy"
                } else {
                    "auth.create.submit"
                },
            ),
            Mode::Unlock => (
                "auth.unlock.title",
                "auth.unlock.desc",
                if self.busy {
                    "auth.unlock.busy"
                } else {
                    "auth.unlock.submit"
                },
            ),
            Mode::Recover => (
                "auth.recover.title",
                "auth.recover.desc",
                if self.busy {
                    "auth.recover.busy"
                } else {
                    "auth.recover.submit"
                },
            ),
        };
        let form = div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .mb(px(4.))
                    .child(
                        div()
                            .text_size(px(22.))
                            .line_height(px(26.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.locale.text(title)),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(19.5))
                            .text_color(self.theme.secondary)
                            .child(self.locale.text(description)),
                    ),
            )
            .when(self.mode == Mode::Choose, |el| {
                el.child(
                    self.button("import", "auth.import.title", true, cx)
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(AuthEvent::Import))),
                )
                .child(
                    self.button("create", "auth.create.title", false, cx)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.change_mode(Mode::Create, window, cx)
                        })),
                )
            })
            .when(self.mode != Mode::Choose, |el| {
                el.when(self.mode == Mode::Recover, |el| {
                    el.child(self.field(&self.recovery, "auth.recoveryKey", false, window, cx))
                })
                .child(self.field(
                    &self.password,
                    if self.mode == Mode::Recover {
                        "auth.newPassword"
                    } else {
                        "auth.masterPassword"
                    },
                    true,
                    window,
                    cx,
                ))
                .when(matches!(self.mode, Mode::Create | Mode::Recover), |el| {
                    el.child(self.strength_meter(cx))
                        .child(self.field(
                            &self.confirmation,
                            if self.mode == Mode::Recover {
                                "auth.confirmNewPassword"
                            } else {
                                "auth.confirmPassword"
                            },
                            false,
                            window,
                            cx,
                        ))
                        .when(!self.confirmation.read(cx).value().is_empty(), |el| {
                            el.child(self.confirmation_feedback(cx))
                        })
                })
                .child(
                    self.button("submit", submit, true, cx)
                        .disabled(!self.ready(cx))
                        .when(!self.ready(cx), |el| {
                            // Composite the button colors once; GPUI element opacity
                            // otherwise blends the label over an already faded fill.
                            el.bg(self.theme.surface.blend(rgb(0x2563eb).opacity(0.45)))
                                .text_color(self.theme.surface.blend(rgb(0xffffff).opacity(0.45)))
                        })
                        .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                )
                .child(self.auth_navigation(cx))
            });
        div()
            .track_focus(&self.focus)
            .size_full()
            .on_children_prepainted(crate::parity::record(&["auth-background", "auth-card"]))
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .p(px(32.))
            .bg(self.theme.bg)
            .text_color(self.theme.text)
            .font_family(".AppleSystemUIFont")
            .child(self.backdrop.render(window, cx))
            .child(
                div()
                    .relative()
                    .w(px(420.))
                    .max_h_full()
                    .flex()
                    .flex_col()
                    .when(f32::from(self.scroll.max_offset().y) <= 1., |el| {
                        el.child(self.shadow.render())
                    })
                    .child(
                        div()
                            .id("auth-card")
                            .relative()
                            .w(px(420.))
                            .max_h_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .p(px(28.))
                            .rounded(px(14.))
                            .border_1()
                            .border_color(self.theme.border.opacity(0.7))
                            .bg(self.theme.surface.opacity(0.88))
                            .flex()
                            .flex_col()
                            .gap(px(20.))
                            .shadow(vec![BoxShadow::new(
                                px(0.),
                                px(1.),
                                rgba(0xffffff0f).into(),
                            )
                            .inset()])
                            .child(
                                div()
                                    .h(px(28.))
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .text_size(px(15.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(crate::logo::logo(28., window))
                                    .child("AIPass"),
                            )
                            .child(form.flex_shrink_0())
                            .when(self.reset_open && self.mode == Mode::Recover, |el| {
                                el.child(self.reset_panel(window, cx))
                            })
                            .when(!self.error.is_empty(), |el| el.child(self.error_notice())),
                    )
                    .child(
                        self.scroll_fade
                            .render(self.backdrop.source(), self.scroll.clone()),
                    ),
            )
    }
}

impl AuthView {
    fn auth_navigation(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .when(self.mode != Mode::Create, |el| el.mt(px(4.)).min_h(px(22.)))
            .child(
                Button::new("auth-back")
                    .disabled(self.busy)
                    .when(self.busy, |el| {
                        el.opacity(if self.mode == Mode::Create { 0.45 } else { 0.5 })
                    })
                    .accessibility_label(self.locale.text(match self.mode {
                        Mode::Unlock => "auth.forgotPassword",
                        Mode::Recover => "auth.backToUnlock",
                        _ => "auth.setup.back",
                    }))
                    .justify_center()
                    .text_center()
                    .text_size(px(12.))
                    .line_height(px(15.))
                    .when(self.mode == Mode::Create, |el| {
                        el.w_full().h(px(32.)).text_size(px(13.)).rounded(px(8.))
                    })
                    .when(self.mode != Mode::Create, |el| {
                        el.px(px(8.)).py(px(4.)).rounded(px(6.))
                    })
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if self.mode == Mode::Create {
                        self.theme.text
                    } else {
                        self.theme.accent
                    })
                    .hover(|style| style.bg(self.theme.accent_soft))
                    .cursor_pointer()
                    .child(self.locale.text(match self.mode {
                        Mode::Unlock => "auth.forgotPassword",
                        Mode::Recover => "auth.backToUnlock",
                        _ => "auth.setup.back",
                    }))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.change_mode(
                            match this.mode {
                                Mode::Unlock => Mode::Recover,
                                Mode::Recover => Mode::Unlock,
                                _ => Mode::Choose,
                            },
                            window,
                            cx,
                        )
                    })),
            )
            .when(self.mode == Mode::Recover && !self.reset_open, |el| {
                el.child(self.reset_link(cx))
            })
    }
}
