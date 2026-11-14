use super::*;
use crate::parity::Observe;
use gpui::{div, px, svg, AnyElement, FontWeight};
use gpui_base::{Dialog, DialogPopup, Transition};
use std::time::Duration;

impl Pricing {
    fn title(&self) -> String {
        self.locale.text(if self.panel == Some(Panel::Assignment) {
            "pricing.credentialSettings"
        } else if self.current_group().is_some() {
            "pricing.editGroup"
        } else {
            "pricing.newGroup"
        })
    }
    fn footer(&self, cx: &Context<Self>) -> AnyElement {
        let body = if let Some(binding) = self.retry_binding.clone() {
            div()
                .flex()
                .justify_end()
                .gap(px(8.))
                .child(
                    self.button("pricing-binding-close", "common.close", false)
                        .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
                )
                .child(
                    self.button("pricing-binding-retry", "providerRuntime.retry", true)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.submit(Operation::Assign(binding.clone()), cx)
                        })),
                )
                .into_any_element()
        } else if self.panel == Some(Panel::Assignment) {
            div()
                .flex()
                .justify_end()
                .gap(px(8.))
                .child(
                    self.button("pricing-cancel", "common.cancel", false)
                        .bg(gpui::transparent_black())
                        .border_color(gpui::transparent_black())
                        .disabled(self.writing() || self.closing)
                        .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
                )
                .child(
                    self.button("pricing-save", "common.save", true)
                        .disabled(
                            self.busy() || self.config.is_none() || self.assignment(cx).is_none(),
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(a) = this.assignment(cx) {
                                this.submit(Operation::Assign(a), cx);
                            }
                        })),
                )
                .into_any_element()
        } else {
            self.group_footer(cx)
        };
        div()
            .flex_shrink_0()
            .px(px(20.))
            .py(px(if self.panel == Some(Panel::Assignment) {
                12.
            } else {
                14.
            }))
            .border_t_1()
            .border_color(self.theme.divider)
            .child(body)
            .observe("pricing-footer")
            .into_any_element()
    }
}
impl Render for Pricing {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.panel.is_none() {
            return div().into_any_element();
        }
        let t = self.theme;
        let assignment = self.panel == Some(Panel::Assignment);
        if !self.motion_initialized {
            self.popup_motion
                .sample(0., Transition::new(Duration::ZERO), window, cx);
            self.overlay_motion
                .sample(0., Transition::new(Duration::ZERO), window, cx);
            self.motion_initialized = true;
        }
        let closing = self.closing;
        let target = if closing { 0. } else { 1. };
        let popup = self
            .popup_motion
            .sample(
                target,
                Transition::new(Duration::from_millis(if closing { 200 } else { 260 })).ease(
                    move |x| {
                        aipass_desktop_native::motion::cubic_bezier(
                            f64::from(x),
                            if closing { 0.4 } else { 0.22 },
                            if closing { 0. } else { 1. },
                            if closing { 0.85 } else { 0.36 },
                            if closing { 0.4 } else { 1. },
                        ) as f32
                    },
                ),
                window,
                cx,
            )
            .value;
        let overlay = self
            .overlay_motion
            .sample(
                target,
                Transition::new(Duration::from_millis(if closing { 200 } else { 220 })).ease(|x| {
                    aipass_desktop_native::motion::cubic_bezier(f64::from(x), 0.4, 0., 0.2, 1.)
                        as f32
                }),
                window,
                cx,
            )
            .value;
        let max_height = window.viewport_size().height - px(if assignment { 48. } else { 32. });
        let content = DialogPopup::new()
            .w(px(if assignment { 540. } else { 780. }))
            .max_w(window.viewport_size().width - px(32.))
            .max_h(max_height)
            .rounded(px(12.))
            .border_1()
            .border_color(t.border)
            .bg(t.surface)
            .text_color(t.text)
            .font_weight(FontWeight::NORMAL)
            .overflow_hidden()
            .flex()
            .flex_col()
            .shadow(vec![gpui_base::box_shadow(
                0.,
                24.,
                crate::theme::shadow_blur(64.),
                0.,
                if t.bg.r < 0.2 {
                    gpui::rgba(0x00000080).into()
                } else {
                    gpui::rgba(0x0f111038).into()
                },
            )])
            .child(
                div()
                    .max_h(max_height)
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .min_h_0()
                    .child(
                        div()
                            .flex_shrink_0()
                            .px(px(20.))
                            .py(px(16.))
                            .border_b_1()
                            .border_color(t.divider)
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.))
                            .child(
                                div()
                                    .min_w_0()
                                    .text_size(px(15.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(self.title())
                                    .observe("pricing-title"),
                            )
                            .child(
                                crate::providers::controls::icon(
                                    "pricing-close",
                                    "x",
                                    self.locale.text("common.close"),
                                    t,
                                )
                                .size(px(if assignment { 26. } else { 28. }))
                                .disabled(self.writing() || closing)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.close(window, cx)),
                                ),
                            )
                            .observe("pricing-header"),
                    )
                    .child(
                        div()
                            .id("pricing-body")
                            .min_h_0()
                            .overflow_y_scroll()
                            .p(px(20.))
                            .flex()
                            .flex_col()
                            .gap(px(18.))
                            .when(!self.error.is_empty(), |el| {
                                el.child(
                                    div()
                                        .px(px(12.))
                                        .py(px(8.))
                                        .rounded(px(8.))
                                        .bg(t.danger_soft)
                                        .text_color(t.danger)
                                        .text_size(px(12.))
                                        .line_height(px(16.8))
                                        .child(self.error.clone())
                                        .observe("pricing-error"),
                                )
                            })
                            .when(self.config.is_none(), |el| {
                                el.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(12.))
                                        .text_size(px(13.))
                                        .line_height(px(20.))
                                        .when(self.pending.is_some(), |el| {
                                            el.child(
                                                svg()
                                                    .path("spinner")
                                                    .size(px(14.))
                                                    .text_color(t.tertiary),
                                            )
                                            .child(self.locale.text("common.loading"))
                                        })
                                        .when(self.pending.is_none(), |el| {
                                            el.child(
                                                self.button(
                                                    "pricing-load-retry",
                                                    "providerRuntime.retry",
                                                    false,
                                                )
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.submit(Operation::Load, cx)
                                                })),
                                            )
                                        }),
                                )
                            })
                            .when(self.config.is_some(), |el| {
                                el.child(if assignment {
                                    self.assignment_body(window, cx)
                                } else {
                                    self.group_body(window, cx)
                                })
                            })
                            .observe("pricing-body"),
                    )
                    .child(self.footer(cx)),
            )
            .observe("pricing-dialog");
        let owner = cx.entity().downgrade();
        Dialog::new(cx)
            .focus_handle(self.focus.clone())
            .accessibility_label(self.title())
            .close_on_escape(!self.writing() && !closing)
            .close_on_backdrop_press(!self.writing() && !closing)
            .on_ok(|_, _, _| false)
            .on_open_change(move |open, _, window, cx| {
                if !open {
                    let _ = owner.update(cx, |this, cx| {
                        if this.picker_open {
                            this.picker_open = false;
                            this.picker_focus.trigger.focus(window, cx);
                            cx.notify();
                        } else {
                            this.close(window, cx);
                        }
                    });
                }
            })
            .backdrop(
                div()
                    .absolute()
                    .inset_0()
                    .bg(gpui::rgb(0x0f1110).opacity(0.45))
                    .backdrop_blur(px(4.))
                    .opacity(overlay),
            )
            .popup(
                crate::composite::Scale::new(
                    div().opacity(popup).child(content),
                    if closing {
                        0.97 + 0.03 * popup
                    } else {
                        0.96 + 0.04 * popup
                    },
                )
                .translate_relative(gpui::point(
                    0.,
                    (if closing { 0.02 } else { 0.04 }) * (1. - popup),
                )),
            )
            .into_any_element()
    }
}
