use super::*;
use crate::parity::Observe;
use aipass_proxy::UpstreamProxyMode;
use gpui::{div, px, svg, AnyElement, FontWeight};
use gpui_base::Button;
impl Subscriptions {
    pub fn options_card(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme;
        let frame =
            self.runtime_options
                .disclosure
                .sample(self.runtime_options.expanded, window, cx);
        let summary = self
            .runtime_options
            .saved_value
            .as_ref()
            .or(self.runtime_options.value.as_ref())
            .map(|value| {
                [
                    if value.quota_tracking {
                        self.text("providerRuntime.everyMinutes")
                            .replace("{count}", &(value.quota_refresh_seconds / 60).to_string())
                    } else {
                        self.text("providerRuntime.disabled")
                    },
                    self.text(match value.proxy.as_ref().map(|p| p.mode) {
                        None => "providerRuntime.inherit",
                        Some(UpstreamProxyMode::System) => "proxy.useSystemProxy",
                        Some(UpstreamProxyMode::Direct) => "providerRuntime.direct",
                        Some(UpstreamProxyMode::Environment) => "providerRuntime.environment",
                        Some(UpstreamProxyMode::Custom) => "providerRuntime.custom",
                    }),
                    self.text(if value.balance.is_some() {
                        "providerRuntime.configured"
                    } else {
                        "providerRuntime.notConfigured"
                    }),
                    self.text("providerRuntime.activeHooks").replace(
                        "{count}",
                        &value
                            .webhooks
                            .iter()
                            .filter(|h| h.enabled)
                            .count()
                            .to_string(),
                    ),
                ]
            });
        let body = div()
            .flex()
            .flex_col()
            .when_some(summary, |el, summary| {
                el.px(px(8.))
                    .py(px(4.))
                    .children(SECTIONS.iter().enumerate().map(|(index, (label, icon))| {
                        let label = if index == 1 {
                            "proxy.configuration"
                        } else {
                            *label
                        };
                        Button::new(format!("runtime-overview-{index}"))
                            .w_full()
                            .px(px(8.))
                            .py(px(10.))
                            .rounded(px(8.))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .hover(move |s| s.bg(t.surface2))
                            .focus_visible(move |s| {
                                s.shadow(vec![gpui_base::box_shadow(
                                    0.,
                                    0.,
                                    0.,
                                    2.,
                                    t.focus_ring.into(),
                                )
                                .inset()])
                            })
                            .accessibility_label(self.text(label))
                            .child(
                                div()
                                    .size(px(30.))
                                    .flex_shrink_0()
                                    .border_1()
                                    .border_color(t.border)
                                    .rounded(px(8.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(svg().path(*icon).size(px(16.)).text_color(t.secondary))
                                    .observe(format!("runtime-icon-{index}")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(3.))
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .line_height(px(15.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(t.text)
                                            .child(self.text(label))
                                            .observe(format!("runtime-label-{index}")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .line_height(px(13.))
                                            .text_color(t.tertiary)
                                            .text_ellipsis()
                                            .child(summary[index].clone())
                                            .observe(format!("runtime-summary-{index}")),
                                    ),
                            )
                            .child(
                                svg()
                                    .path("chevron-right")
                                    .size(px(14.))
                                    .text_color(t.tertiary),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.show_options(index, window, cx)
                            }))
                            .observe(format!("runtime-row-{index}"))
                    }))
            })
            .when(self.runtime_options.value.is_none(), |el| {
                el.px(px(16.))
                    .py(px(12.))
                    .gap(px(12.))
                    .when(self.runtime_options.pending, |el| {
                        el.child(self.paragraph(self.text("common.loading"), 12.))
                    })
                    .when(!self.runtime_options.error.is_empty(), |el| {
                        el.child(self.banner(self.runtime_options.error.clone(), false))
                    })
                    .when(!self.runtime_options.pending, |el| {
                        el.child(
                            self.usage_button(
                                "runtime-load",
                                self.text("providerRuntime.retry"),
                                "",
                                false,
                            )
                            .on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.options_request(Action::Load, cx)
                                }),
                            ),
                        )
                    })
            })
            .observe("runtime-overview");
        self.card()
            .child(
                div()
                    .flex()
                    .items_center()
                    .min_w_0()
                    .border_b_1()
                    .border_color(if self.runtime_options.expanded {
                        t.divider
                    } else {
                        gpui::rgba(0x00000000)
                    })
                    .child(
                        Button::new("runtime-expand")
                            .rounded_tl(px(11.))
                            .when(!self.runtime_options.expanded, |s| s.rounded_bl(px(11.)))
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .px(px(16.))
                            .py(px(12.))
                            .gap(px(8.))
                            .text_color(t.text)
                            .hover(move |s| s.bg(t.surface2))
                            .focus_visible(move |s| {
                                s.shadow(vec![gpui_base::box_shadow(
                                    0.,
                                    0.,
                                    0.,
                                    2.,
                                    t.focus_ring.into(),
                                )
                                .inset()])
                            })
                            .accessibility_label(self.text("providerRuntime.title"))
                            .aria_expanded(self.runtime_options.expanded)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .items_start()
                                    // The inline-flex heading occupies a text line with its baseline descent.
                                    .pb(px(2.078125))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(
                                                svg()
                                                    .path("sliders-horizontal")
                                                    .size(px(14.))
                                                    .flex_shrink_0()
                                                    .text_color(t.tertiary)
                                                    .observe("runtime-heading-icon"),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .line_height(px(18.2))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .child(self.text("providerRuntime.title")),
                                            )
                                            .observe("runtime-heading"),
                                    )
                                    .observe("runtime-title"),
                            )
                            .child(
                                div()
                                    .size(px(15.))
                                    .flex_shrink_0()
                                    .child(frame.chevron(t.tertiary, 15.))
                                    .observe("runtime-chevron"),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.runtime_options.expanded = !this.runtime_options.expanded;
                                if this.runtime_options.expanded
                                    && this.runtime_options.value.is_none()
                                {
                                    this.options_request(Action::Load, cx);
                                }
                                cx.notify();
                            }))
                            .observe("runtime-trigger"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .flex_shrink_0()
                            .pr(px(16.))
                            .child(
                                Button::new("runtime-configure")
                                    .group("runtime-configure")
                                    .size(px(26.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .border_1()
                                    .border_color(gpui::rgba(0x00000000))
                                    .rounded(px(8.))
                                    .text_color(t.secondary)
                                    .hover(move |s| s.bg(t.accent_soft).text_color(t.text))
                                    .focus_visible(move |s| {
                                        s.shadow(vec![
                                            gpui_base::box_shadow(
                                                0.,
                                                0.,
                                                0.,
                                                3.,
                                                t.focus_ring.into(),
                                            ),
                                            gpui_base::box_shadow(0., 0., 0., 1., t.surface.into()),
                                        ])
                                    })
                                    .accessibility_label(self.text("providerRuntime.configure"))
                                    .child(
                                        svg()
                                            .path("settings-2")
                                            .size(px(14.))
                                            .text_color(t.secondary)
                                            .group_hover("runtime-configure", move |s| {
                                                s.text_color(t.text)
                                            })
                                            .observe("runtime-configure-icon"),
                                    )
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.show_options(0, window, cx)
                                    }))
                                    .observe("runtime-configure"),
                            )
                            .observe("runtime-actions"),
                    )
                    .observe("runtime-card-header"),
            )
            .child(frame.body(
                "runtime-overview-body",
                div().py(px(4.)).child(body).observe("runtime-card-body"),
            ))
            .observe("subscription-runtime-card")
            .into_any_element()
    }
}
