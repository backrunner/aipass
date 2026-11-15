use super::*;
use gpui::{div, px, FontWeight};
impl Proxy {
    pub(super) fn header(
        &mut self,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let t = self.theme;
        let running = self.status.as_ref().is_some_and(|s| s.running);
        let degraded = self.status.as_ref().is_some_and(|s| s.degraded);
        let title = div()
            .flex()
            .items_center()
            .gap(px(9.))
            .min_w_0()
            .text_size(px(15.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(
                gpui::svg()
                    .path("server")
                    .size(px(18.))
                    .text_color(t.text)
                    .flex_shrink_0(),
            )
            .child(self.locale.text("server.localProxy"));
        let actions = div()
            .flex()
            .items_center()
            .flex_wrap()
            .gap(px(8.))
            .child(
                self.button("proxy-search", "search.title", false)
                    .h(px(26.))
                    .px(px(9.))
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .on_click(cx.listener(|s, _, w, cx| {
                        s.search_config = s
                            .config
                            .as_ref()
                            .map(|c| c.search.clone())
                            .unwrap_or_default();
                        s.open_modal(Modal::Search, w, cx);
                    })),
            )
            .child(
                crate::providers::controls::icon(
                    "proxy-logs",
                    "file-text",
                    self.locale.text("server.viewLogs"),
                    t,
                )
                .size(px(26.))
                .disabled(self.busy())
                .on_click(cx.listener(|s, _, w, cx| {
                    s.open_modal(Modal::Logs, w, cx);
                    s.request(Operation::Logs, false, cx);
                })),
            )
            .child(
                div()
                    .rounded_full()
                    .flex_shrink_0()
                    .whitespace_nowrap()
                    .px(px(10.))
                    .py(px(3.))
                    .text_size(px(11.))
                    .text_color(if degraded {
                        t.warning
                    } else if running {
                        t.success
                    } else {
                        t.tertiary
                    })
                    .bg(if degraded {
                        t.warning.opacity(0.12)
                    } else if running {
                        t.success.opacity(0.1)
                    } else {
                        t.surface2
                    })
                    .child(self.locale.text(if degraded {
                        "server.degraded"
                    } else if running {
                        "server.running"
                    } else {
                        "server.stopped"
                    })),
            )
            .child(
                self.button(
                    "proxy-toggle",
                    if running {
                        "server.stop"
                    } else {
                        "server.start"
                    },
                    !running,
                )
                .child(
                    gpui::svg()
                        .path(if running { "square" } else { "play" })
                        .size(px(14.))
                        .text_color(if running { t.text } else { gpui::rgb(0xffffff) }),
                )
                .flex_row_reverse()
                .gap(px(6.))
                .disabled(
                    self.busy()
                        || (!running
                            && self
                                .config
                                .as_ref()
                                .is_none_or(|c| !c.routes.iter().any(|r| r.enabled))),
                )
                .on_click(cx.listener(move |s, _, _, cx| {
                    s.apply(
                        if running {
                            AgentRequest::ServerStop
                        } else {
                            AgentRequest::ServerStart
                        },
                        cx,
                    )
                })),
            );
        let mut meta = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .min_w_0();
        if let Some(config) = self.config.clone() {
            for route in config.routes.iter().filter(|r| r.enabled) {
                meta = meta.child(
                    div()
                        .max_w(px(176.))
                        .border_1()
                        .border_color(t.border)
                        .rounded_full()
                        .px(px(8.))
                        .py(px(2.))
                        .text_size(px(11.))
                        .text_color(t.secondary)
                        .truncate()
                        .child(route.name.clone()),
                );
            }
            meta = meta.child(self.bind_chip(&config, running, window, cx));
        }
        let mut header = admin_ui::stack()
            .gap(px(10.))
            .px(px(28.))
            .py(px(18.))
            .border_b_1()
            .border_color(t.divider);
        if width <= 440. {
            header = header.child(title).child(actions);
        } else {
            header = header.child(admin_ui::row().min_h(px(0.)).child(title).child(actions));
        }
        header.child(meta).into_any_element()
    }
}
