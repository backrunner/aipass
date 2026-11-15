use super::*;
use crate::parity::Observe;
use gpui::{div, px, FontWeight};
use gpui_base::Button;
pub(super) fn advertised(value: &str) -> String {
    if let Some(port) = value.strip_prefix("0.0.0.0:") {
        format!("127.0.0.1:{port}")
    } else if let Some(port) = value.strip_prefix("[::]:") {
        format!("[::1]:{port}")
    } else {
        value.into()
    }
}
impl Render for Proxy {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.fields.disabled(self.busy(), cx);
        let t = self.theme;
        let (sidebar, list) = crate::theme::columns(f32::from(window.viewport_size().width));
        let detail_width = f32::from(window.viewport_size().width) - sidebar - list - 32.;
        let routes = self
            .config
            .as_ref()
            .map(|c| c.routes.clone())
            .unwrap_or_default();
        let mut entries = div()
            .id("proxy-groups")
            .track_scroll(&self.list_scroll)
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .pt(px(4.))
            .px(px(12.))
            .pb(px(12.))
            .flex()
            .flex_col();
        if !self.loaded && self.busy() {
            entries = entries.child(admin_ui::hint(self.locale.text("common.loading"), t));
        } else if routes.is_empty() {
            entries = entries.child(
                admin_ui::stack()
                    .p(px(24.))
                    .items_center()
                    .child(
                        gpui::svg()
                            .path("server")
                            .size(px(22.))
                            .text_color(t.tertiary),
                    )
                    .child(self.locale.text("server.noGroups"))
                    .child(admin_ui::hint(self.locale.text("server.noGroupsDesc"), t)),
            );
        }
        for route in &routes {
            let id = route.id;
            let edit = route.clone();
            let owner = cx.entity().downgrade();
            let selected = Some(id) == self.selected;
            entries = entries.child(
                div()
                    .id(format!("proxy-group-{id}"))
                    .on_mouse_down(
                        gpui::MouseButton::Right,
                        cx.listener(move |s, e: &gpui::MouseDownEvent, w, cx| {
                            if !s.busy() {
                                s.selected = Some(id);
                                s.context_menu = Some((id, e.position));
                                s.previous_focus = w.focused(cx);
                                s.context_focus.items[0].focus(w, cx);
                                cx.notify();
                            }
                        }),
                    )
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .min_h(px(68.))
                    .px(px(12.))
                    .py(px(10.))
                    .rounded(px(8.))
                    .bg(if selected {
                        t.accent_soft
                    } else {
                        gpui::rgba(0)
                    })
                    .hover(move |s| s.bg(if selected { t.accent_soft } else { t.surface2 }))
                    .child(
                        Button::new(format!("proxy-group-select-{id}"))
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .gap(px(12.))
                            .items_center()
                            .accessibility_label(route.name.clone())
                            .role(gpui::Role::ListBoxOption)
                            .aria_selected(selected)
                            .child(
                                div()
                                    .size(px(32.))
                                    .flex_shrink_0()
                                    .rounded(px(8.))
                                    .bg(t.surface2)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        gpui::svg()
                                            .path("server")
                                            .size(px(16.))
                                            .text_color(t.secondary),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .text_ellipsis()
                                            .text_size(px(13.))
                                            .line_height(px(16.9))
                                            .text_color(if selected { t.accent } else { t.text })
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(route.name.clone()),
                                    )
                                    .child(admin_ui::hint(
                                        format!(
                                            "{}  {}",
                                            route::strategies(&self.locale)
                                                .into_iter()
                                                .find(|(v, _)| serde_json::to_value(route.strategy)
                                                    .unwrap()
                                                    .as_str()
                                                    == Some(v.as_str()))
                                                .map(|(_, label)| label)
                                                .unwrap_or_default(),
                                            self.locale.text("server.memberCount").replace(
                                                "{count}",
                                                &route.targets.len().to_string()
                                            )
                                        ),
                                        t,
                                    )),
                            )
                            .on_click(cx.listener(move |s, e: &gpui::ClickEvent, w, cx| {
                                s.selected = Some(id);
                                if e.click_count() == 2 && !s.busy() {
                                    s.edit_route(Some(edit.clone()), w, cx);
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        admin_ui::switch(
                            format!("proxy-group-enabled-{id}"),
                            format!("{}: {}", route.name, self.locale.text("server.enabled")),
                            route.enabled,
                            self.busy(),
                            t,
                            window,
                            cx,
                        )
                        .on_change(move |enabled, _, _, cx| {
                            let _ = owner.update(cx, |s, cx| {
                                s.apply(
                                    AgentRequest::ServerRouteSetEnabled {
                                        route_id: id,
                                        enabled,
                                    },
                                    cx,
                                )
                            });
                        }),
                    ),
            );
        }
        let list_pane = div()
            .w(px(list))
            .h_full()
            .flex_shrink_0()
            .rounded(px(14.))
            .border_1()
            .border_color(t.border.opacity(0.6))
            .bg(t.surface.opacity(0.86))
            .backdrop_blur(px(8.))
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                admin_ui::row()
                    .pt(px(42.))
                    .px(px(12.))
                    .pb(px(10.))
                    .child(
                        div()
                            .pl(px(4.))
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.locale.text("server.groups")),
                    )
                    .child(
                        crate::providers::controls::base_button("proxy-add-group", true, t)
                            .gap(px(6.))
                            .accessibility_label(self.locale.text("server.addGroup"))
                            .child(
                                gpui::svg()
                                    .path("plus")
                                    .size(px(14.))
                                    .text_color(gpui::white()),
                            )
                            .child(self.locale.text("server.addGroup"))
                            .disabled(self.busy() || self.entries.is_empty())
                            .on_click(cx.listener(|s, _, w, cx| s.edit_route(None, w, cx))),
                    ),
            )
            .child(entries)
            .observe("proxy-list");
        let header = self.header(detail_width, window, cx);
        let mut body = admin_ui::stack().gap(px(14.));
        if !self.error.is_empty() {
            body = body.child(
                div().text_color(t.danger).child(self.error.clone()).child(
                    self.button("proxy-retry", "settings.refresh", false)
                        .on_click(cx.listener(|s, _, _, cx| s.refresh(cx))),
                ),
            );
        }
        if !self.notice.is_empty() {
            body = body.child(admin_ui::hint(self.notice.clone(), t));
        }
        body = body
            .child(self.status_grid(detail_width - 56.))
            .child(self.chart(detail_width - 56., window, cx))
            .child(self.breakdown(detail_width - 56., window, cx))
            .child(self.integration_card(window, cx));
        let detail = div()
            .h_full()
            .flex_1()
            .min_w_0()
            .rounded(px(14.))
            .border_1()
            .border_color(t.border.opacity(0.6))
            .bg(t.surface.opacity(0.88))
            .backdrop_blur(px(8.))
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("proxy-detail-scroll")
                            .track_scroll(&self.detail_scroll)
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .px(px(28.))
                            .pt(px(18.))
                            .pb(px(28.))
                            .child(body),
                    )
                    .child(crate::scroll_fade::fade(
                        t.surface,
                        self.detail_scroll.clone(),
                    )),
            )
            .observe("proxy-detail");
        div()
            .size_full()
            .text_size(px(13.))
            .font_weight(FontWeight::NORMAL)
            .line_height(px(15.6))
            .flex()
            .gap(px(8.))
            .child(list_pane)
            .child(detail)
            .children(self.preview.clone())
            .children(self.setup.clone())
            .child(self.dialog(window, cx))
            .child(self.route_context_menu(window, cx))
            .into_any_element()
    }
}
