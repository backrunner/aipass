use super::*;
use crate::parity::Observe;
use gpui::{div, px, FontWeight};
use gpui_base::Dialog;
impl Proxy {
    pub(super) fn dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let frame = self.modal_motion.sample(self.modal.is_some(), window, cx);
        if !frame.visible {
            if self.closing_modal.take().is_some() {
                self.fields.clear(window, cx);
                self.route = None;
            }
            return div().absolute().into_any_element();
        }
        let Some(modal) = self.modal.or(self.closing_modal) else {
            return div().absolute().into_any_element();
        };
        if modal == Modal::Logs {
            return self.log_dialog(frame, window, cx);
        }
        let t = self.theme;
        let title = match modal {
            Modal::Route => {
                if self.route.as_ref().is_some_and(|r| {
                    self.config
                        .as_ref()
                        .is_some_and(|c| c.routes.iter().any(|existing| existing.id == r.id))
                }) {
                    "server.editGroup"
                } else {
                    "server.addGroup"
                }
            }
            Modal::Logs => "server.viewLogs",
            Modal::Search => "search.title",
            Modal::ClearUsage => "server.clearUsageConfirmTitle",
            Modal::DeleteRoute => "server.deleteGroup",
        };
        let body = match modal {
            Modal::Route => self.route_form(window, cx),
            Modal::Search => self.search_form(window, cx),
            Modal::Logs => unreachable!(),
            Modal::ClearUsage => {
                admin_ui::hint(self.locale.text("server.clearUsageConfirmDescription"), t)
                    .into_any_element()
            }
            Modal::DeleteRoute => admin_ui::hint(
                self.config
                    .as_ref()
                    .and_then(|c| c.routes.iter().find(|r| Some(r.id) == self.selected))
                    .map(|r| r.name.clone())
                    .unwrap_or_default(),
                t,
            )
            .into_any_element(),
        };
        let width = match modal {
            Modal::Logs => 780.,
            Modal::Search => 720.,
            Modal::Route => {
                if self.bind_models {
                    840.
                } else {
                    640.
                }
            }
            _ => 460.,
        };
        let width = self
            .route_width
            .sample(
                width,
                gpui_base::Transition::new(Duration::from_millis(200)).ease(|v| {
                    aipass_desktop_native::motion::cubic_bezier(v as f64, 0.25, 0.1, 0.25, 1.)
                        as f32
                }),
                window,
                cx,
            )
            .value;
        let max_height =
            window.viewport_size().height - px(if modal == Modal::Route { 32. } else { 64. });
        // The reference route form has its own viewport cap inside the bordered
        // dialog. Keep that inner constraint separate, including at minimum size.
        let form = div()
            .max_h(max_height - px(if modal == Modal::Route { 0. } else { 2. }))
            .flex()
            .flex_col()
            .child(
                admin_ui::row()
                    .min_h(px(if modal == Modal::Search { 26. } else { 28. }))
                    .px(px(20.))
                    .py(px(if modal == Modal::Search { 18. } else { 16. }))
                    .flex_shrink_0()
                    .border_b_1()
                    .border_color(t.divider)
                    .child(
                        div()
                            .text_size(px(if modal == Modal::Route { 15. } else { 16. }))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.locale.text(title)),
                    )
                    .child(
                        crate::providers::controls::icon(
                            "proxy-dialog-close",
                            "x",
                            self.locale.text("common.close"),
                            t,
                        )
                        .size(px(if modal == Modal::Search { 26. } else { 28. }))
                        .track_focus(&self.close_focus)
                        .disabled(self.busy())
                        .border_color(gpui::rgba(0))
                        .on_click(cx.listener(|s, _, w, cx| s.close_modal(w, cx))),
                    ),
            )
            .child(
                div()
                    .relative()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("proxy-modal-body")
                            .track_scroll(&self.modal_scroll)
                            .min_h_0()
                            .overflow_y_scroll()
                            .p(px(20.))
                            .child(body)
                            .when(!self.error.is_empty(), |el| {
                                el.child(div().text_color(t.danger).child(self.error.clone()))
                            })
                            .observe("proxy-dialog-body"),
                    )
                    .child(crate::scroll_fade::fade(
                        t.surface,
                        self.modal_scroll.clone(),
                    )),
            )
            .child(
                admin_ui::row()
                    .flex_shrink_0()
                    .justify_end()
                    .gap(px(8.))
                    .px(px(20.))
                    .py(px(14.))
                    .border_t_1()
                    .border_color(t.divider)
                    .child(
                        self.button(
                            "proxy-cancel",
                            if modal == Modal::Logs {
                                "common.close"
                            } else {
                                "common.cancel"
                            },
                            false,
                        )
                        .border_color(gpui::rgba(0))
                        .on_click(cx.listener(|s, _, w, cx| s.close_modal(w, cx))),
                    )
                    .when(modal != Modal::Logs, |el| {
                        el.child(
                            self.button(
                                "proxy-save",
                                if modal == Modal::DeleteRoute {
                                    "common.remove"
                                } else if modal == Modal::ClearUsage {
                                    "server.clearUsageConfirm"
                                } else {
                                    "common.save"
                                },
                                true,
                            )
                            .disabled(
                                self.busy()
                                    || (modal == Modal::Route
                                        && (self.fields.value("route-name", cx).trim().is_empty()
                                            || self
                                                .route
                                                .as_ref()
                                                .is_none_or(|r| r.targets.is_empty()))),
                            )
                            .on_click(cx.listener(
                                move |s, _, _, cx| match modal {
                                    Modal::Route => s.save_route(cx),
                                    Modal::Search => s.save_search(cx),
                                    Modal::ClearUsage => {
                                        s.apply(AgentRequest::ServerUsageClear, cx)
                                    }
                                    Modal::DeleteRoute => {
                                        if let Some(mut config) = s.config.clone() {
                                            config.routes.retain(|r| Some(r.id) != s.selected);
                                            s.apply(AgentRequest::ServerConfigSet { config }, cx);
                                        }
                                    }
                                    Modal::Logs => {}
                                },
                            )),
                        )
                    })
                    .observe("proxy-dialog-footer"),
            );
        let content = div()
            .w(px(width).min(window.viewport_size().width - px(32.)))
            .max_h(max_height)
            .rounded(px(14.))
            .border_1()
            .border_color(t.border)
            .bg(t.surface)
            .text_color(t.text)
            .text_size(px(13.))
            .line_height(px(15.6))
            .font_weight(FontWeight::NORMAL)
            .overflow_hidden()
            .child(form)
            .observe("proxy-dialog");
        let owner = cx.entity().downgrade();
        Dialog::new(cx)
            .focus_handle(self.focus.clone())
            .accessibility_label(self.locale.text(title))
            .close_on_escape(!self.busy() && self.modal.is_some())
            .close_on_backdrop_press(!self.busy() && self.modal.is_some())
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
                    .bg(gpui::black().opacity(0.4))
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
