use super::*;
use crate::{parity::Observe, providers::controls};
use gpui::{
    div, percentage, px, relative, svg, Animation, AnimationExt, AnyElement, FontWeight,
    Transformation,
};
use gpui_base::{Dialog, DialogPopup, Transition};
use std::time::Duration;

impl Tools {
    fn title(&self) -> String {
        self.locale.text(if self.panel == Some(Panel::Reset) {
            "titlebar.resetProvider"
        } else {
            "providerDetail.usageProbeTitle"
        })
    }
    fn header(&self, cx: &Context<Self>) -> AnyElement {
        let t = self.theme;
        div()
            .flex_shrink_0()
            .flex()
            .justify_between()
            .gap(px(18.))
            .pt(px(18.))
            .px(px(20.))
            .pb(px(14.))
            .border_b_1()
            .border_color(t.divider)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(15.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(650.))
                            .text_color(t.text)
                            .child(self.title())
                            .observe("usage-title"),
                    )
                    .when(self.panel == Some(Panel::Usage), |el| {
                        el.child(
                            div()
                                .mt(px(5.))
                                .text_size(px(12.))
                                .line_height(px(17.4))
                                .font_weight(FontWeight::NORMAL)
                                .text_color(t.tertiary)
                                .child(self.locale.text("providerDetail.usageProbeDescription"))
                                .observe("usage-subtitle"),
                        )
                    }),
            )
            .child(
                gpui_base::Button::new("provider-tools-close")
                    .size(px(30.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.))
                    .text_color(t.tertiary)
                    .cursor_pointer()
                    .accessibility_label(self.locale.text("common.close"))
                    .hover(move |s| s.bg(t.surface2).text_color(t.text))
                    .focus_visible(move |s| s.shadow(t.outline(t.focus_ring, 2., 1.)))
                    .child(svg().path("x").size(px(16.)).inherit_text_color())
                    .disabled(self.mutating())
                    .on_click(cx.listener(|this, _, window, cx| this.close(window, cx)))
                    .observe("usage-close"),
            )
            .observe("usage-header")
            .into_any_element()
    }
    fn action_button(
        &self,
        id: &'static str,
        key: &str,
        icon: &'static str,
        primary: bool,
        pending: Pending,
        cx: &Context<Self>,
    ) -> gpui_base::Button {
        let t = self.theme;
        let loading = self.pending == Some(pending);
        let color = if primary { gpui::rgb(0xffffff) } else { t.text };
        controls::base_button(id, primary, t)
            .relative()
            .accessibility_label(self.locale.text(key))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .when(loading, |el| el.opacity(0.))
                    .child(svg().path(icon).size(px(13.)).text_color(color))
                    .child(self.locale.text(key)),
            )
            .when(loading, |el| {
                let spinner = svg()
                    .path("spinner")
                    .size(px(12.))
                    .absolute()
                    .left(relative(0.5))
                    .top(relative(0.5))
                    .ml(px(-6.))
                    .mt(px(-6.))
                    .text_color(color);
                el.child(if cx.reduce_motion() {
                    spinner.into_any_element()
                } else {
                    spinner
                        .with_animation(
                            id,
                            Animation::new(Duration::from_millis(700)).repeat(),
                            |icon, progress| {
                                icon.with_transformation(Transformation::rotate(percentage(
                                    progress,
                                )))
                            },
                        )
                        .into_any_element()
                })
            })
    }
    fn footer(&self, cx: &Context<Self>) -> AnyElement {
        let t = self.theme;
        div()
            .flex_shrink_0()
            .flex()
            .justify_end()
            .gap(px(8.))
            .px(px(20.))
            .py(px(14.))
            .border_t_1()
            .border_color(t.divider)
            .bg(crate::theme::oklab_mix(t.surface, t.surface2, 0.92))
            .rounded_b(px(13.))
            .child(
                controls::button(
                    "provider-tools-cancel",
                    self.locale.text("common.cancel"),
                    false,
                    t,
                )
                .border_color(gpui::rgba(0))
                .bg(gpui::rgba(0))
                .disabled(self.mutating())
                .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
            )
            .when(self.panel == Some(Panel::Usage), |el| {
                el.child(
                    self.action_button(
                        "usage-test",
                        "providerDetail.testUsage",
                        "refresh-cw",
                        false,
                        Pending::Usage,
                        cx,
                    )
                    .disabled(self.busy())
                    .on_click(cx.listener(|this, _, window, cx| this.test_usage(window, cx)))
                    .observe("usage-test"),
                )
                .child(
                    self.action_button(
                        "usage-apply",
                        "providerDetail.applyUsage",
                        "check",
                        true,
                        Pending::Apply,
                        cx,
                    )
                    .disabled(
                        self.busy()
                            || self.usage.as_ref().is_none_or(|r| {
                                !aipass_desktop_native::provider_tools::can_apply(r)
                            }),
                    )
                    .on_click(cx.listener(|this, _, window, cx| this.apply(window, cx)))
                    .observe("usage-apply"),
                )
            })
            .when(self.panel == Some(Panel::Reset), |el| {
                el.child(
                    controls::button(
                        "provider-reset-confirm",
                        self.locale.text(if self.busy() {
                            "common.loading"
                        } else {
                            "titlebar.resetProvider"
                        }),
                        true,
                        t,
                    )
                    .map(|mut button| {
                        button.interactivity().clear_pointer_styles();
                        button
                    })
                    .bg(t.danger)
                    .border_color(t.danger)
                    .text_color(if t.bg.r < 0.2 {
                        t.bg
                    } else {
                        gpui::rgb(0xffffff)
                    })
                    .hover(move |s| s.bg(t.danger.opacity(0.88)).border_color(t.danger))
                    .disabled(self.busy())
                    .on_click(cx.listener(|this, _, window, cx| this.confirm_reset(window, cx)))
                    .observe("provider-reset-confirm"),
                )
            })
            .observe("usage-footer")
            .into_any_element()
    }
    fn body(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme;
        div()
            .id("provider-tools-body")
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(14.))
            .pt(px(16.))
            .px(px(20.))
            .pb(px(18.))
            .when(self.panel == Some(Panel::Reset), |el| {
                el.child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(19.5))
                        .child(self.locale.text("confirm.resetProvider").replace(
                            "{title}",
                            self.entry.as_ref().map_or("", |e| e.title.as_str()),
                        ))
                        .observe("provider-reset-description"),
                )
            })
            .when(self.panel == Some(Panel::Usage), |el| {
                el.child(self.modes(cx))
                    .when(
                        self.entry.as_ref().is_some_and(|e| e.secret_refs.len() > 1),
                        |el| el.child(self.key_picker(window, cx)),
                    )
                    .child(self.field(0, window, cx))
                    .when(self.mode == UsageProbeMode::NewApiAdvanced, |el| {
                        el.child(
                            div()
                                .flex()
                                .gap(px(12.))
                                .child(div().flex_1().min_w_0().child(self.field(1, window, cx)))
                                .child(
                                    div()
                                        .w(px(150.))
                                        .flex_shrink_0()
                                        .child(self.field(2, window, cx)),
                                ),
                        )
                    })
            })
            .when(!self.error.is_empty(), |el| {
                el.child(
                    div()
                        .rounded(px(8.))
                        .px(px(12.))
                        .py(px(8.))
                        .bg(t.danger_soft)
                        .text_color(t.danger)
                        .text_size(px(12.))
                        .line_height(px(16.8))
                        .child(self.error.clone())
                        .observe("provider-tools-error"),
                )
            })
            .when(self.panel == Some(Panel::Usage), |el| {
                el.child(self.preview())
            })
            .observe("usage-body")
            .into_any_element()
    }
}
impl Render for Tools {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.panel.is_none() {
            return div().into_any_element();
        }
        let t = self.theme;
        if !self.motion_initialized {
            self.popup_motion
                .sample(0., Transition::new(Duration::ZERO), window, cx);
            self.overlay_motion
                .sample(0., Transition::new(Duration::ZERO), window, cx);
            self.motion_initialized = true;
        }
        let popup = self
            .popup_motion
            .sample(
                1.,
                Transition::new(Duration::from_millis(220)).ease(|x| {
                    aipass_desktop_native::motion::cubic_bezier(f64::from(x), 0.22, 1., 0.36, 1.)
                        as f32
                }),
                window,
                cx,
            )
            .value;
        let overlay = self
            .overlay_motion
            .sample(
                1.,
                Transition::new(Duration::from_millis(180)).ease(|x| {
                    aipass_desktop_native::motion::cubic_bezier(f64::from(x), 0.25, 0.1, 0.25, 1.)
                        as f32
                }),
                window,
                cx,
            )
            .value;
        let owner = cx.entity().downgrade();
        let content = DialogPopup::new()
            .w(px(if self.panel == Some(Panel::Reset) {
                460.
            } else {
                640.
            }))
            .max_h(window.viewport_size().height - px(32.))
            .rounded(px(14.))
            .border_1()
            .border_color(t.border)
            .bg(t.surface)
            .text_color(t.text)
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
                    .max_h(window.viewport_size().height - px(32.))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .min_h_0()
                    .child(self.header(cx))
                    .child(self.body(window, cx))
                    .child(self.footer(cx)),
            )
            .observe("provider-tools-dialog");
        Dialog::new(cx)
            .focus_handle(self.focus.clone())
            .accessibility_label(self.title())
            .close_on_escape(!self.mutating())
            .close_on_backdrop_press(!self.mutating())
            .on_ok(|_, _, _| false)
            .on_open_change(move |open, _, window, cx| {
                if !open {
                    let _ = owner.update(cx, |this, cx| {
                        if this.key_open {
                            this.key_open = false;
                            this.key_focus.trigger.focus(window, cx);
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
                    .bg(gpui::rgb(0x0f1110).opacity(0.44))
                    .backdrop_blur(px(4.))
                    .opacity(overlay),
            )
            .popup(
                crate::composite::Scale::new(
                    div().opacity(popup).child(content),
                    0.98 + 0.02 * popup,
                )
                .translate(gpui::point(px(0.), px(8. * (1. - popup)))),
            )
            .into_any_element()
    }
}
