use super::*;
use crate::parity::Observe;
use gpui::{div, px, FontWeight};
impl Proxy {
    pub(super) fn update_logs(&mut self, logs: Vec<ProxyLogEntry>) {
        let equal = |a: &ProxyLogEntry, b: &ProxyLogEntry| {
            a.timestamp == b.timestamp && a.level == b.level && a.message == b.message
        };
        if self.logs.len() == logs.len() && self.logs.iter().zip(&logs).all(|(a, b)| equal(a, b)) {
            return;
        }
        if self.logs.is_empty() {
            self.log_list.set_follow_mode(gpui::FollowMode::Tail);
        }
        // The bounded history trims its prefix. Splice the common suffix/prefix
        // instead of resetting the list, retaining the reader's current row.
        let overlap = (0..=self.logs.len().min(logs.len()))
            .rev()
            .find(|n| {
                self.logs[self.logs.len() - n..]
                    .iter()
                    .zip(&logs[..*n])
                    .all(|(a, b)| equal(a, b))
            })
            .unwrap_or(0);
        self.log_list.splice(0..self.logs.len() - overlap, 0);
        self.log_list.splice(overlap..overlap, logs.len() - overlap);
        self.logs = logs;
    }

    pub(super) fn log_dialog(
        &self,
        frame: crate::subscription_ui::motion::Frame,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let t = self.theme;
        let empty = self.logs.is_empty();
        let owner = cx.entity().downgrade();
        let body = div()
            .id("proxy-log-scroll")
            .track_focus(&self.log_focus)
            .tab_stop(true)
            .role(gpui::Role::Region)
            .aria_label(self.locale.text("server.proxyLogs"))
            .flex_1()
            .min_h_0()
            .focus_visible(move |s| s.shadow(t.outline(t.focus_ring, 2., -2.)))
            .on_key_down(cx.listener(|s, e: &gpui::KeyDownEvent, _, cx| {
                match e.keystroke.key.as_str() {
                    "down" => s.log_list.scroll_by(px(38.4)),
                    "up" => s.log_list.scroll_by(px(-38.4)),
                    "pagedown" => s.log_list.scroll_by(px(300.)),
                    "pageup" => s.log_list.scroll_by(px(-300.)),
                    "home" => s.log_list.scroll_to(gpui::ListOffset {
                        item_ix: 0,
                        offset_in_item: px(0.),
                    }),
                    "end" => s.log_list.set_follow_mode(gpui::FollowMode::Tail),
                    _ => return,
                }
                cx.stop_propagation();
                cx.notify();
            }))
            .child(
                gpui::list(self.log_list.clone(), move |index, _, cx| {
                    let Some(view) = owner.upgrade() else {
                        return div().into_any_element();
                    };
                    let s = view.read(cx);
                    let Some(entry) = s.logs.get(index) else {
                        return div().into_any_element();
                    };
                    let (text, highlights) =
                        super::log_format::line(entry, &s.entries, s.config.as_ref(), t);
                    div()
                        .px(px(20.))
                        .text_size(px(12.))
                        .line_height(px(19.2))
                        .font_family(".AppleSystemUIFontMonospaced")
                        .text_color(t.secondary)
                        .child(
                            gpui_base::SelectableText::new(("proxy-log-line", index), text)
                                .document_order(index as u64)
                                .highlights(highlights)
                                .selection_color(t.focus_ring.into()),
                        )
                        .into_any_element()
                })
                .size_full()
                .py(px(18.)),
            );
        let owner = cx.entity().downgrade();
        let content = div()
            .w(px(780.).min(window.viewport_size().width - px(32.)))
            .max_h(window.viewport_size().height - px(64.))
            .rounded(px(14.))
            .border_1()
            .border_color(t.border)
            .bg(t.surface)
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap(px(16.))
                    .px(px(20.))
                    .py(px(16.))
                    .border_b_1()
                    .border_color(t.divider)
                    .child(
                        admin_ui::stack()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(t.text)
                                    .child(self.locale.text("server.proxyLogs")),
                            )
                            .child(admin_ui::hint(self.locale.text("server.proxyLogsDesc"), t)),
                    )
                    .child(
                        crate::providers::controls::icon(
                            "proxy-dialog-close",
                            "x",
                            self.locale.text("common.close"),
                            t,
                        )
                        .size(px(16.))
                        .track_focus(&self.close_focus)
                        .on_click(cx.listener(|s, _, w, cx| s.close_modal(w, cx))),
                    ),
            )
            .when(!self.error.is_empty() && !empty, |el| {
                el.child(
                    div()
                        .px(px(20.))
                        .py(px(8.))
                        .text_size(px(12.))
                        .text_color(t.danger)
                        .child(self.locale.text("server.proxyLogsRefreshFailed")),
                )
            })
            .child(
                div()
                    .h(px(480.).min(window.viewport_size().height - px(180.)))
                    .min_h_0()
                    .bg(t.surface2)
                    .rounded_b(px(13.))
                    .flex()
                    .flex_col()
                    .when(!empty, |el| el.child(body))
                    .when(empty, |el| {
                        el.child(
                            div()
                                .flex_1()
                                .flex()
                                .items_center()
                                .justify_center()
                                .px(px(20.))
                                .text_size(px(13.))
                                .text_color(t.tertiary)
                                .child(self.locale.text(if self.pending {
                                    "common.loading"
                                } else if !self.error.is_empty() {
                                    "server.proxyLogsRefreshFailed"
                                } else {
                                    "server.proxyLogsEmpty"
                                })),
                        )
                    }),
            )
            .observe("proxy-dialog");
        gpui_base::Dialog::new(cx)
            .focus_handle(self.focus.clone())
            .accessibility_label(self.locale.text("server.proxyLogs"))
            .on_ok(|_, _, _| false)
            .on_open_change(move |open, _, w, cx| {
                if !open {
                    let _ = owner.update(cx, |s, cx| s.close_modal(w, cx));
                }
            })
            .backdrop(
                div()
                    .absolute()
                    .inset_0()
                    .bg(gpui::rgb(0x0f1110).opacity(0.5))
                    .backdrop_blur(px(4.))
                    .opacity(frame.overlay),
            )
            .popup(
                crate::composite::Scale::new(
                    div().opacity(frame.popup).child(content),
                    frame.scale,
                )
                .translate_relative(gpui::point(0., frame.offset)),
            )
            .into_any_element()
    }
}
