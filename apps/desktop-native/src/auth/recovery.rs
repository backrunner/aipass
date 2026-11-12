use super::*;

impl AuthView {
    pub(crate) fn open_reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.reset_busy || self.mode != Mode::Recover {
            return;
        }
        self.reset_open = true;
        self.reset_confirmation
            .update(cx, |input, cx| input.set_value("", window, cx));
        cx.notify();
    }

    fn cancel_reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.reset_busy {
            return;
        }
        self.reset_open = false;
        self.reset_confirmation
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn reset_ready(&self, cx: &Context<Self>) -> bool {
        self.mode == Mode::Recover
            && self.reset_open
            && !self.reset_busy
            && !self.busy
            && self.reset_confirmation.read(cx).value().trim() == "RESET"
    }

    fn submit_reset(&mut self, cx: &mut Context<Self>) {
        if !self.reset_ready(cx) {
            return;
        }
        self.error.clear();
        self.reset_busy = true;
        self.reset_confirmation
            .update(cx, |input, cx| input.set_disabled(true, cx));
        cx.emit(AuthEvent::Reset);
        cx.notify();
    }

    pub(super) fn reset_link(&self, cx: &Context<Self>) -> Button {
        Button::new("reset-request")
            .disabled(self.busy)
            .px(px(8.))
            .py(px(4.))
            .rounded(px(6.))
            .text_size(px(12.))
            .line_height(px(15.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(self.theme.danger)
            .hover(|style| style.bg(self.theme.danger_soft))
            .cursor_pointer()
            .accessibility_label(self.locale.text("auth.reset.trigger"))
            .child(self.locale.text("auth.reset.trigger"))
            .on_click(cx.listener(|this, _, window, cx| this.open_reset(window, cx)))
    }

    pub(super) fn reset_panel(&self, window: &Window, cx: &Context<Self>) -> impl IntoElement {
        let focused = self
            .reset_confirmation
            .read(cx)
            .focus_handle(cx)
            .is_focused(window);
        let ready = self.reset_ready(cx);
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .gap(px(12.))
            .mt(px(12.))
            .child(
                div()
                    .p(px(8.))
                    .px(px(12.))
                    .border_1()
                    .border_color(rgba(0))
                    .rounded(px(8.))
                    .bg(self.theme.danger_soft)
                    .text_color(self.theme.danger)
                    .text_size(px(12.))
                    .line_height(px(16.8))
                    .child(self.locale.text("auth.reset.warning")),
            )
            .child(
                div()
                    .h(px(36.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(if focused {
                        self.theme.accent
                    } else {
                        self.theme.border
                    })
                    .bg(self.theme.surface)
                    .text_size(px(13.))
                    .line_height(px(20.))
                    .when(focused, |el| {
                        el.shadow(vec![BoxShadow::new(
                            px(0.),
                            px(0.),
                            self.theme.focus_ring.into(),
                        )
                        .spread_radius(px(3.))])
                    })
                    .when(self.reset_busy, |el| el.opacity(0.65))
                    .child(Input::new(&self.reset_confirmation)),
            )
            .child(
                Button::new("reset-submit")
                    .relative()
                    .w_full()
                    .h(px(32.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(13.))
                    .line_height(px(13.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(self.theme.danger)
                    .disabled(!ready)
                    .when(!ready, |el| el.opacity(0.45))
                    .hover(|style| style.bg(self.theme.danger_soft))
                    .cursor_pointer()
                    .accessibility_label(self.locale.text(if self.reset_busy {
                        "auth.reset.busy"
                    } else {
                        "auth.reset.submit"
                    }))
                    .child(div().when(self.reset_busy, |el| el.opacity(0.)).child(
                        self.locale.text(if self.reset_busy {
                            "auth.reset.busy"
                        } else {
                            "auth.reset.submit"
                        }),
                    ))
                    .when(self.reset_busy, |el| {
                        el.child(super::feedback::spinner(
                            self.theme.danger,
                            cx.reduce_motion(),
                        ))
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.submit_reset(cx))),
            )
            .child(
                div()
                    .flex()
                    .justify_center()
                    .items_center()
                    .min_h(px(22.))
                    .mt(px(4.))
                    .child(
                        Button::new("reset-cancel")
                            .disabled(self.reset_busy)
                            .px(px(8.))
                            .py(px(4.))
                            .rounded(px(6.))
                            .text_size(px(12.))
                            .line_height(px(15.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(self.theme.accent)
                            .hover(|s| s.bg(self.theme.accent_soft))
                            .accessibility_label(self.locale.text("auth.reset.cancel"))
                            .child(self.locale.text("auth.reset.cancel"))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.cancel_reset(window, cx)),
                            ),
                    ),
            )
    }
}

#[cfg(all(test, feature = "parity-render"))]
mod tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};

    #[gpui::test]
    fn reset_requires_open_panel_exact_confirmation_and_no_pending_request(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(gpui_base::init);
        let (view, cx) =
            cx.add_window_view(|window, cx| AuthView::new(Mode::Recover, false, false, window, cx));
        let calls = Rc::new(Cell::new(0));
        let observed = calls.clone();
        let _subscription = cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event, _| {
                assert!(matches!(event, AuthEvent::Reset));
                observed.set(observed.get() + 1);
            })
        });
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.reset_confirmation
                    .update(cx, |input, cx| input.set_value("RESET", window, cx));
                assert!(!view.reset_ready(cx));
                view.open_reset(window, cx);
                assert!(view.reset_confirmation.read(cx).value().is_empty());
                view.reset_confirmation
                    .update(cx, |input, cx| input.set_value("reset", window, cx));
                view.submit_reset(cx);
                assert!(!view.reset_busy);
                view.reset_confirmation
                    .update(cx, |input, cx| input.set_value(" RESET ", window, cx));
                view.submit_reset(cx);
                assert!(view.reset_busy);
                view.submit_reset(cx);
                view.cancel_reset(window, cx);
                assert!(view.reset_open);
                view.set_busy(false, cx);
                assert!(view.reset_ready(cx));
                view.cancel_reset(window, cx);
                assert!(!view.reset_open);
                assert!(view.reset_confirmation.read(cx).value().is_empty());
            })
        });
        assert_eq!(calls.get(), 1);
    }
}
