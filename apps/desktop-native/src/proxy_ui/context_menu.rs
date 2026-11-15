use super::*;
use gpui::{div, px};
impl Proxy {
    pub(super) fn route_context_menu(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some((id, position)) = self.context_menu else {
            return div().absolute().into_any_element();
        };
        let t = self.theme;
        let focus = self.context_focus.clone();
        let owner = cx.entity().downgrade();
        let menu = focus
            .surface(
                "proxy-route-context",
                focus.trigger.clone(),
                focus.trigger.clone(),
                move |w, cx| {
                    let _ = owner.update(cx, |s, cx| s.close_context_menu(w, cx));
                },
            )
            .on_mouse_down_out(cx.listener(|s, _, w, cx| s.close_context_menu(w, cx)))
            .w(px(188.))
            .p(px(4.))
            .rounded(px(8.))
            .border_1()
            .border_color(t.border)
            .bg(t.surface)
            .shadow(vec![gpui_base::box_shadow(
                0.,
                4.,
                crate::theme::shadow_blur(16.),
                0.,
                t.popup_shadow.into(),
            )])
            .flex()
            .flex_col()
            .children(
                [
                    ("pencil", "server.editGroup"),
                    ("trash-2", "server.deleteGroup"),
                ]
                .into_iter()
                .enumerate()
                .map(|(index, (icon, label))| {
                    gpui_base::Button::new(("route-context-action", index))
                        .track_focus(&focus.items[index])
                        .role(gpui::Role::MenuItem)
                        .w_full()
                        .h(px(32.))
                        .px(px(10.))
                        .rounded(px(6.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_size(px(13.))
                        .text_color(if index == 1 { t.danger } else { t.text })
                        .hover(move |s| s.bg(t.accent_soft))
                        .focus(move |s| s.bg(t.accent_soft))
                        .child(
                            gpui::svg()
                                .path(icon)
                                .size(px(14.))
                                .text_color(if index == 1 { t.danger } else { t.text }),
                        )
                        .child(self.locale.text(label))
                        .on_click(cx.listener(move |s, _, w, cx| {
                            s.close_context_menu(w, cx);
                            if s.busy() {
                                return;
                            }
                            s.selected = Some(id);
                            if index == 0 {
                                let route = s
                                    .config
                                    .as_ref()
                                    .and_then(|c| c.routes.iter().find(|r| r.id == id))
                                    .cloned();
                                if let Some(route) = route {
                                    s.edit_route(Some(route), w, cx);
                                }
                            } else {
                                s.open_modal(Modal::DeleteRoute, w, cx);
                            }
                        }))
                }),
            );
        gpui::deferred(
            gpui::anchored()
                .position(position)
                .snap_to_window_with_margin(gpui::Edges::all(px(8.)))
                .child(menu),
        )
        .into_any_element()
    }
    fn close_context_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.context_menu = None;
        if let Some(focus) = self.previous_focus.take() {
            focus.focus(window, cx);
        }
        cx.notify();
    }
}
