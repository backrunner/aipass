//! Provider action menu presentation and focus ownership.
use super::Workspace;
use crate::{menu::motion, parity::Observe};
use aipass_desktop_native::catalog::Action;
use gpui::{div, prelude::*, px, svg, Anchor, Context, IntoElement, Window};
use gpui_base::{Button, Popover};

impl Workspace {
    pub(super) fn provider_actions(
        &self,
        id: uuid::Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = self.theme;
        let open = self.provider_menu_open && !self.provider_busy;
        let frame = motion::sample("provider-actions-motion", open, window, cx);
        let origin = motion::Origin::new(true);
        let track_origin = origin.clone();
        let owner = cx.entity().downgrade();
        let close = owner.clone();
        let focus = self.provider_menu_focus.clone();
        let has_keys = self
            .catalog
            .find(id)
            .is_some_and(|e| !e.secret_refs.is_empty());
        let options = [
            ("wifi", self.locale.text("providerDetail.probeEndpoint")),
            ("gauge", self.locale.text("providerDetail.refreshUsage")),
            ("archive", self.locale.text("sidebar.archive")),
            ("trash-2", self.locale.text("providerDetail.moveToTrash")),
        ];
        Popover::new("provider-actions")
            .on_position(move |position, trigger| track_origin.resolve(position, trigger))
            .open(open)
            .retain_content(frame.visible)
            .track_focus(&focus.container)
            .on_open_change(cx.listener(|this, open: &bool, _, cx| {
                this.provider_menu_open = *open && !this.provider_busy;
                cx.notify();
            }))
            .anchor(Anchor::TopRight)
            .offset(px(6.))
            .trigger(
                Button::new("provider-more")
                    .track_focus(&focus.trigger)
                    .disabled(self.provider_busy)
                    .size(px(32.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(t.border)
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(move |s| s.bg(t.surface2))
                    .accessibility_label(self.locale.text("providerDetail.moreActions"))
                    .child(svg().path("ellipsis").size(px(16.)).text_color(t.secondary)),
            )
            .content(move |_, _, _| {
                let menu = focus
                    .surface(
                        "provider-actions-surface",
                        focus.trigger.clone(),
                        focus.trigger.clone(),
                        move |_, cx| {
                            let _ = close.update(cx, |this, cx| {
                                this.provider_menu_open = false;
                                cx.notify();
                            });
                        },
                    )
                    .w(px(200.))
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
                        options
                            .into_iter()
                            .enumerate()
                            .map(|(index, (icon, label))| {
                                let owner = owner.clone();
                                let color = if index == 3 { t.danger } else { t.text };
                                let item = Button::new(("provider-action", index))
                                    .role(gpui::Role::MenuItem)
                                    .disabled(!open || (index < 2 && !has_keys))
                                    .styles(|s| s.disabled(|s| s.opacity(0.45)))
                                    .track_focus(&focus.items[index])
                                    .w_full()
                                    .px(px(10.))
                                    .py(px(7.))
                                    .rounded(px(6.))
                                    .text_size(px(13.))
                                    .line_height(px(16.))
                                    .text_color(color)
                                    .flex()
                                    .items_center()
                                    .justify_start()
                                    .gap(px(8.))
                                    .cursor_pointer()
                                    .hover(move |s| s.bg(t.accent_soft))
                                    .focus(move |s| s.bg(t.accent_soft))
                                    .accessibility_label(label.clone())
                                    .child(svg().path(icon).size(px(14.)).text_color(color))
                                    .child(div().child(label))
                                    .on_click(move |_, window, cx| {
                                        let _ = owner.update(cx, |this, cx| {
                                            if !this.provider_menu_open || this.provider_busy {
                                                return;
                                            }
                                            this.provider_menu_focus.trigger.focus(window, cx);
                                            this.provider_menu_open = false;
                                            match index {
                                                0 if has_keys => this
                                                    .tools
                                                    .update(cx, |view, cx| view.probe(window, cx)),
                                                1 if has_keys => {
                                                    this.tools.update(cx, |view, cx| {
                                                        view.show_usage(window, cx)
                                                    })
                                                }
                                                2 => this.request_provider(Action::Archive(id), cx),
                                                3 => this.request_provider(Action::Trash(id), cx),
                                                _ => (),
                                            }
                                            cx.notify();
                                        });
                                    })
                                    .observe(format!("menu-provider-action-{index}"));
                                div()
                                    .flex_shrink_0()
                                    .when(index == 2, |el| {
                                        el.child(div().h(px(1.)).my(px(4.)).bg(t.divider))
                                    })
                                    .child(item)
                            }),
                    )
                    .observe("menu-provider-actions-content");
                frame.wrap_positioned(menu, origin)
            })
            .observe("menu-provider-actions-trigger")
    }
}
