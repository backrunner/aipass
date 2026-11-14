use super::*;
use crate::parity::Observe;
use gpui::{div, px, svg, AnyElement, FontWeight};
use gpui_base::Button;

impl Pricing {
    pub(super) fn assignment_body(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = self.theme;
        let Some(secret) = &self.secret else {
            return div().into_any_element();
        };
        let assignment = self.assignment(cx);
        let changed = assignment != self.original;
        let cost = aipass_desktop_native::pricing::presentation::estimate(
            assignment.map(|a| a.multiplier),
        );
        div()
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(12.))
                    .line_height(px(15.))
                    .text_color(t.secondary)
                    .child(
                        svg()
                            .path("key-round")
                            .size(px(14.))
                            .flex_shrink_0()
                            .text_color(t.tertiary),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .font_weight(FontWeight::MEDIUM)
                            .child(secret.label.clone()),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .ml_auto()
                            .font_family("Menlo")
                            .text_size(px(11.))
                            .line_height(px(13.))
                            .text_color(t.tertiary)
                            .child(secret.masked.clone())
                            .observe("pricing-identity-mask"),
                    )
                    .observe("pricing-identity"),
            )
            .child(
                div()
                    .p(px(16.))
                    .border_1()
                    .border_color(t.border)
                    .rounded(px(10.))
                    .bg(gpui::Rgba {
                        r: t.surface2.r * 0.45 + t.surface.r * 0.55,
                        g: t.surface2.g * 0.45 + t.surface.g * 0.55,
                        b: t.surface2.b * 0.45 + t.surface.b * 0.55,
                        a: 1.,
                    })
                    .child(
                        div()
                            .flex()
                            .gap(px(14.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(self.group_picker(window, cx)),
                            )
                            .child(
                                div()
                                    .w(px(104.))
                                    .flex_shrink_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .child(self.label("pricing.multiplier"))
                                    .child(self.number(
                                        &self.multiplier,
                                        "pricing-multiplier".into(),
                                        "pricing.multiplier",
                                        true,
                                        window,
                                        cx,
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(9.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(10.))
                            .text_size(px(10.))
                            .line_height(px(14.))
                            .text_color(t.tertiary)
                            .child(div().flex_1().min_w_0().child(self.locale.text(
                                if self.selected_group.is_some() {
                                    "pricing.customSourceHint"
                                } else {
                                    "pricing.defaultSourceHint"
                                },
                            )))
                            .child(
                                Button::new("pricing-manage")
                                    .whitespace_nowrap()
                                    .flex_shrink_0()
                                    .py(px(2.))
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .text_size(px(11.))
                                    .line_height(px(13.))
                                    .font_weight(FontWeight::NORMAL)
                                    .text_color(t.accent)
                                    .cursor_pointer()
                                    .focus_visible(move |s| {
                                        s.shadow(t.outline(t.focus_ring, 2., 3.))
                                    })
                                    .disabled(self.busy() || changed)
                                    .accessibility_label(self.locale.text(if changed {
                                        "pricing.saveBeforeRules"
                                    } else if self.selected_group.is_some() {
                                        "pricing.manageRules"
                                    } else {
                                        "pricing.newGroup"
                                    }))
                                    .child(
                                        svg()
                                            .path(if self.selected_group.is_some() {
                                                "pencil"
                                            } else {
                                                "plus"
                                            })
                                            .size(px(11.))
                                            .text_color(t.accent),
                                    )
                                    .child(self.locale.text(if self.selected_group.is_some() {
                                        "pricing.manageRules"
                                    } else {
                                        "pricing.newGroup"
                                    }))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.edit_group(window, cx)
                                    }))
                                    .observe("pricing-manage"),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(16.))
                            .whitespace_nowrap()
                            .pt(px(14.))
                            .border_t_1()
                            .border_color(t.divider)
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .text_size(px(11.))
                            .line_height(px(16.))
                            .text_color(t.tertiary)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(5.))
                                    .child(self.locale.text("pricing.previewBase"))
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(t.secondary)
                                            .child("$1.00"),
                                    ),
                            )
                            .child(
                                svg()
                                    .path("arrow-right")
                                    .size(px(14.))
                                    .text_color(t.tertiary),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(5.))
                                    .child(self.locale.text("pricing.previewEstimate"))
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(t.accent)
                                            .child(cost),
                                    ),
                            )
                            .child(
                                div()
                                    .px(px(7.))
                                    .py(px(3.))
                                    .border_1()
                                    .border_color(t.border)
                                    .rounded_full()
                                    .text_size(px(10.))
                                    .line_height(px(10.))
                                    .ml_auto()
                                    .child("USD"),
                            )
                            .observe("pricing-cost-preview"),
                    )
                    .observe("pricing-settings"),
            )
            .when(secret.billing.is_some(), |el| {
                el.child(self.reference(window, cx))
            })
            .child(
                self.hint("history", self.locale.text("pricing.assignmentImpact"))
                    .observe("pricing-assignment-impact"),
            )
            .into_any_element()
    }
    fn reference(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme;
        let billing = self
            .secret
            .as_ref()
            .and_then(|s| s.billing.as_ref())
            .unwrap();
        let frame = self
            .reference_disclosure
            .sample(self.reference_open, window, cx);
        let rate = billing.rate.as_deref().and_then(draft::multiplier);
        let fields = [
            (
                "pricing.multiplier",
                billing.rate.as_ref().map(|s| format!("×{s}")),
            ),
            ("providerForm.billingCurrency", billing.currency.clone()),
            ("providerForm.billingUnitPrice", billing.unit_price.clone()),
        ];
        div()
            .border_1()
            .border_color(t.border)
            .rounded(px(8.))
            .overflow_hidden()
            .child(
                Button::new("pricing-reference")
                    .h(px(37.8))
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(12.))
                    .py(px(10.))
                    .text_size(px(12.))
                    .line_height(px(16.8))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(t.text)
                    .accessibility_label(self.locale.text("pricing.referenceBilling"))
                    .aria_expanded(self.reference_open)
                    .border_b_1()
                    .border_color(if self.reference_open {
                        t.divider
                    } else {
                        gpui::rgba(0)
                    })
                    .child(
                        div()
                            .flex_1()
                            .child(self.locale.text("pricing.referenceBilling")),
                    )
                    .child(div().ml_auto().flex_shrink_0().h(px(15.4)).child({
                        let (height, top) =
                            crate::typography::inline_badge_metrics(window, 10., 11., 15.4, 4.);
                        div()
                            .relative()
                            .top(top)
                            .flex_shrink_0()
                            .h(height + px(8.))
                            .px(px(7.))
                            .py(px(3.))
                            .border_1()
                            .border_color(t.border)
                            .rounded_full()
                            .text_size(px(10.))
                            .whitespace_nowrap()
                            .line_height(height)
                            .font_weight(FontWeight::NORMAL)
                            .text_color(t.tertiary)
                            .child(self.locale.text("pricing.referenceOnly"))
                            .observe("pricing-reference-badge")
                    }))
                    .child(frame.chevron(t.tertiary, 15.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.reference_open = !this.reference_open;
                        cx.notify();
                    })),
            )
            .child(
                frame.body(
                    "pricing-reference-body",
                    div()
                        .p(px(12.))
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex()
                                .gap(px(24.))
                                .children(fields.into_iter().filter_map(|(key, value)| {
                                    value.filter(|v| !v.is_empty()).map(|value| {
                                        div()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap(px(5.))
                                            .child(
                                                div()
                                                    .text_size(px(11.))
                                                    .line_height(px(13.))
                                                    .text_color(t.tertiary)
                                                    .child(self.locale.text(key)),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(14.))
                                                    .line_height(px(17.))
                                                    .child(value),
                                            )
                                    })
                                })),
                        )
                        .children(billing.note.as_ref().map(|note| {
                            div()
                                .text_size(px(11.))
                                .line_height(px(16.5))
                                .text_color(t.tertiary)
                                .child(note.clone())
                        }))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(px(11.))
                                        .line_height(px(16.5))
                                        .text_color(t.tertiary)
                                        .child(self.locale.text("pricing.referenceNote")),
                                )
                                .when_some(rate, |el, rate| {
                                    el.child(
                                        self.button(
                                            "pricing-reference-rate",
                                            "pricing.useReferenceRate",
                                            false,
                                        )
                                        .h(px(26.))
                                        .px(px(10.))
                                        .text_size(px(12.))
                                        .flex_shrink_0()
                                        .on_click(
                                            cx.listener(move |this, _, window, cx| {
                                                this.multiplier.update(cx, |s, cx| {
                                                    s.set_value(rate.to_string(), window, cx)
                                                });
                                                cx.notify();
                                            }),
                                        ),
                                    )
                                }),
                        ),
                ),
            )
            .observe("pricing-reference")
            .into_any_element()
    }
}
