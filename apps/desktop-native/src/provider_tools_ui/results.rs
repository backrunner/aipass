use super::*;
use crate::parity::Observe;
use gpui::{div, px, svg, AnyElement, FontWeight};
const MONO: &str = ".AppleSystemUIFontMonospaced";
impl Tools {
    fn dot(&self, ok: bool) -> gpui::Div {
        div()
            .size(px(6.))
            .flex_shrink_0()
            .rounded_full()
            .mr(px(2.))
            .bg(if ok {
                self.theme.success
            } else {
                self.theme.danger
            })
    }
    pub(super) fn preview(&self) -> AnyElement {
        let t = self.theme;
        let panel = div()
            .flex_shrink_0()
            .p(px(14.))
            .rounded(px(8.))
            .border_1()
            .border_color(t.divider)
            .bg(t.surface2)
            .flex()
            .flex_col()
            .gap(px(12.));
        let Some(result) = &self.usage else {
            return panel
                .min_h(px(86.))
                .items_center()
                .justify_center()
                .text_color(t.tertiary)
                .text_size(px(12.))
                .line_height(px(17.4))
                .child(svg().path("gauge").size(px(18.)).text_color(t.tertiary))
                .child(self.locale.text("providerDetail.usageProbeEmpty"))
                .observe("usage-preview")
                .into_any_element();
        };
        panel
            .when(!result.ok, |el| {
                el.bg(crate::theme::oklab_mix(t.danger, t.surface, 0.06))
                    .border_color(crate::theme::oklab_mix(t.danger, t.divider, 0.28))
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(self.dot(result.ok))
                    .child(
                        div()
                            .min_w_0()
                            .text_ellipsis()
                            .text_size(px(13.))
                            .line_height(px(16.))
                            .font_weight(FontWeight::BOLD)
                            .child(self.locale.text(if result.ok {
                                aipass_desktop_native::provider_tools::source_label(result.source)
                            } else {
                                "providerDetail.checkFailed"
                            })),
                    )
                    .when_some(result.status, |el, status| {
                        el.child(
                            div()
                                .text_size(px(11.))
                                .line_height(px(13.))
                                .font_family(MONO)
                                .text_color(t.tertiary)
                                .child(format!("HTTP {status}")),
                        )
                    }),
            )
            .when_some(result.endpoint.clone(), |el, url| {
                el.child(
                    div()
                        .text_size(px(11.))
                        .line_height(px(15.95))
                        .font_family(MONO)
                        .text_color(t.tertiary)
                        .child(url),
                )
            })
            .when_some(result.error.clone(), |el, error| {
                el.child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(17.4))
                        .text_color(t.danger)
                        .child(error),
                )
            })
            .when_some(result.quota.as_ref(), |el, q| {
                let values = [
                    (
                        "providerDetail.quota",
                        q.label.as_ref().or(result.plan_name.as_ref()),
                    ),
                    ("providerDetail.remaining", q.remaining.as_ref()),
                    ("providerDetail.limit", q.limit.as_ref()),
                    ("providerDetail.used", q.used.as_ref()),
                ];
                el.child(
                    div().flex().gap(px(8.)).children(
                        values
                            .into_iter()
                            .enumerate()
                            .filter(|(i, (_, v))| *i != 3 || v.is_some())
                            .map(|(i, (label, value))| {
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .p(px(10.))
                                    .rounded(px(6.))
                                    .border_1()
                                    .border_color(t.divider)
                                    .bg(t.surface)
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .line_height(px(13.))
                                            .text_color(t.tertiary)
                                            .child(self.locale.text(label)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .line_height(px(16.))
                                            .font_weight(FontWeight(650.))
                                            .text_ellipsis()
                                            .child(value.cloned().unwrap_or_else(|| "—".into())),
                                    )
                                    .observe(format!("usage-quota-{i}"))
                            }),
                    ),
                )
                .when(q.unit.is_some() || q.reset_at.is_some(), |el| {
                    el.child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_x(px(14.))
                            .gap_y(px(8.))
                            .text_size(px(12.))
                            .line_height(px(15.))
                            .text_color(t.tertiary)
                            .children(
                                [
                                    ("providerDetail.unit", q.unit.as_ref()),
                                    ("providerDetail.resets", q.reset_at.as_ref()),
                                ]
                                .into_iter()
                                .filter_map(|(key, v)| {
                                    v.map(|v| {
                                        div().child(format!("{}: {v}", self.locale.text(key)))
                                    })
                                }),
                            ),
                    )
                })
            })
            .when_some(result.gateway.as_ref(), |el, g| {
                el.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(7.))
                        .child(self.label("providerDetail.gateway"))
                        .child(
                            div().flex().flex_wrap().gap(px(4.)).children(
                                [
                                    ("providerDetail.gatewayGroup", g.group.as_ref()),
                                    ("providerDetail.gatewayRate", g.rate.as_ref()),
                                ]
                                .into_iter()
                                .filter_map(|(key, v)| {
                                    v.map(|v| {
                                        div()
                                            .px(px(8.))
                                            .py(px(3.))
                                            .rounded_full()
                                            .bg(t.surface)
                                            .text_color(t.secondary)
                                            .text_size(px(11.))
                                            .line_height(px(13.))
                                            .child(format!("{}: {v}", self.locale.text(key)))
                                    })
                                }),
                            ),
                        ),
                )
            })
            .when(
                result.ok && !aipass_desktop_native::provider_tools::can_apply(result),
                |el| {
                    el.child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(17.4))
                            .text_color(t.tertiary)
                            .child(self.locale.text("providerDetail.noUsageData")),
                    )
                },
            )
            .observe("usage-preview")
            .into_any_element()
    }
    pub fn status(&self) -> AnyElement {
        let t = self.theme;
        let row = |label: &str, value: gpui::Div| {
            div()
                .px(px(16.))
                .py(px(13.))
                .border_t_1()
                .border_color(t.divider)
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .text_size(px(11.))
                        .line_height(px(13.))
                        .text_color(t.tertiary)
                        .child(self.locale.text(label)),
                )
                .child(value)
        };
        div()
            .flex()
            .flex_col()
            .when_some(self.probe.as_ref(), |el, r| {
                el.child(
                    row(
                        "providerDetail.status",
                        div()
                            .flex()
                            .items_center()
                            .flex_wrap()
                            .gap(px(8.))
                            .text_size(px(12.))
                            .line_height(px(18.))
                            .child(self.dot(r.ok))
                            .child(self.locale.text(if r.ok {
                                "providerDetail.healthy"
                            } else {
                                "providerDetail.checkFailed"
                            }))
                            .when_some(r.model_count, |el, count| {
                                el.child(
                                    div().text_color(t.tertiary).child(
                                        self.locale
                                            .text("providerDetail.modelCount")
                                            .replace("{count}", &count.to_string()),
                                    ),
                                )
                            })
                            .when_some(r.websocket.as_ref(), |el, ws| {
                                el.child(div().text_color(t.tertiary).child(self.locale.text(
                                    match ws.supported {
                                        Some(true) => "providerDetail.wsSupported",
                                        Some(false) => "providerDetail.wsUnsupported",
                                        None => "providerDetail.wsUnknown",
                                    },
                                )))
                            })
                            .when_some(r.error.clone(), |el, error| {
                                el.child(div().text_color(t.danger).child(error))
                            }),
                    )
                    .observe("provider-probe-result"),
                )
            })
            .when_some(self.usage.as_ref(), |el, r| {
                el.child(
                    row(
                        "providerDetail.usage",
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(12.))
                            .line_height(px(18.))
                            .child(self.dot(r.ok))
                            .child(self.locale.text(if r.ok {
                                aipass_desktop_native::provider_tools::source_label(r.source)
                            } else {
                                "providerDetail.checkFailed"
                            }))
                            .children(
                                r.quota
                                    .as_ref()
                                    .into_iter()
                                    .flat_map(|q| {
                                        [
                                            ("providerDetail.remaining", q.remaining.as_ref()),
                                            ("providerDetail.used", q.used.as_ref()),
                                        ]
                                    })
                                    .chain(r.gateway.as_ref().into_iter().flat_map(|g| {
                                        [
                                            ("providerDetail.gatewayGroup", g.group.as_ref()),
                                            ("providerDetail.gatewayRate", g.rate.as_ref()),
                                        ]
                                    }))
                                    .filter_map(|(key, value)| {
                                        value.map(|v| {
                                            div()
                                                .text_color(t.tertiary)
                                                .child(format!("{}: {v}", self.locale.text(key)))
                                        })
                                    }),
                            )
                            .when_some(r.error.clone(), |el, error| {
                                el.child(div().text_color(t.danger).child(error))
                            }),
                    )
                    .observe("provider-usage-result"),
                )
            })
            .into_any_element()
    }
    pub fn notice(&self) -> Option<AnyElement> {
        self.notice.map(|(key, _)| {
            div()
                .rounded(px(8.))
                .px(px(12.))
                .py(px(8.))
                .bg(self.theme.accent_soft)
                .text_color(self.theme.text)
                .text_size(px(12.))
                .line_height(px(16.8))
                .child(self.locale.text(key))
                .into_any_element()
        })
    }
}
