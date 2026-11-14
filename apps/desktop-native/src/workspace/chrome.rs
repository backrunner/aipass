use super::{Workspace, WorkspaceEvent, FILTERS};
use crate::parity::Observe;
use gpui::{
    div, prelude::*, px, rgb, svg, Anchor, BoxShadow, Context, Focusable, FontWeight, Window,
};
use gpui_base::{input::Input, Button, Popover};

impl Workspace {
    pub(super) fn button(
        &self,
        id: &'static str,
        label: &str,
        icon: &'static str,
        primary: bool,
        small: bool,
    ) -> Button {
        let t = self.theme;
        let color = if primary { rgb(0xffffff) } else { t.text };
        Button::new(id)
            .flex()
            .items_center()
            .justify_center()
            .flex_shrink_0()
            .gap(px(6.))
            .h(px(if small { 26. } else { 32. }))
            .px(px(if small { 9. } else { 12. }))
            .rounded(px(8.))
            .border_1()
            .border_color(if primary { gpui::rgba(0) } else { t.border })
            .bg(if primary { rgb(0x2563eb) } else { t.surface })
            .text_color(color)
            .text_size(px(if small { 12. } else { 13. }))
            .line_height(px(if small { 12. } else { 13. }))
            .font_weight(FontWeight::MEDIUM)
            .cursor_pointer()
            .hover(move |s| s.bg(if primary { rgb(0x1f4fd0) } else { t.surface2 }))
            .child(svg().path(icon).size(px(14.)).text_color(color))
            .accessibility_label(self.locale.text(label))
            .child(self.locale.text(label))
    }

    fn icon_button(&self, id: &'static str, icon: &'static str, small: bool) -> Button {
        let t = self.theme;
        Button::new(id)
            .size(px(if small { 26. } else { 32. }))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(8.))
            .cursor_pointer()
            .hover(move |s| s.bg(t.accent_soft))
            .child(svg().path(icon).size(px(14.)).text_color(t.secondary))
    }

    pub(super) fn toolbar(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let focused = self.search.read(cx).focus_handle(cx).is_focused(window);
        let filter_open = self.filter_open;
        let filter_motion =
            crate::menu::motion::sample("filter-menu-motion", filter_open, window, cx);
        let filter_motion_origin = crate::menu::motion::Origin::new(true);
        let filter_motion_origin_track = filter_motion_origin.clone();
        let filter_enabled = !matches!(self.selected, 1 | 9 | 10);
        let selected = self.selected;
        let selected_label = self.locale.text("common.selected");
        let selected_tag = self.tag_filter.clone();
        let mut options: Vec<_> = FILTERS
            .into_iter()
            .map(|(index, label)| (index, self.locale.text(label), None))
            .collect();
        let owner = cx.entity().downgrade();
        let mut filter_focus = self.filter_focus.clone();
        let tags: std::collections::BTreeSet<_> = self
            .catalog
            .active
            .iter()
            .flat_map(|entry| entry.tags.iter())
            .collect();
        for tag in tags {
            let focus =
                window.use_keyed_state(format!("filter-tag-{tag}"), cx, |_, cx| cx.focus_handle());
            filter_focus.items.push(focus.read(cx).clone());
            options.push((
                0,
                self.locale.text("providerList.tag").replace("{value}", tag),
                Some(tag.clone()),
            ));
        }
        let close_filter = owner.clone();
        let after_filter = self.refresh_focus.clone();
        let before_filter = self.search.read(cx).focus_handle(cx);
        div()
            .on_children_prepainted(crate::parity::record(if self.selected == 10 {
                &["toolbar.search", "toolbar.empty-trash"]
            } else {
                &[
                    "toolbar.search",
                    "toolbar.refresh",
                    "toolbar.connect",
                    "toolbar.add",
                ]
            }))
            .pt(px(38.))
            .px(px(16.))
            .pb(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h(px(34.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .pl(px(12.))
                    .pr(px(6.))
                    .border_1()
                    .border_color(if focused { t.accent } else { t.surface2 })
                    .rounded(px(8.))
                    .bg(if focused { t.surface } else { t.surface2 })
                    .when(focused, |el| {
                        el.shadow(vec![BoxShadow::new(px(0.), px(0.), t.focus_ring.into())
                            .spread_radius(px(3.))])
                    })
                    .child(
                        svg()
                            .path("search")
                            .size(px(14.))
                            .flex_shrink_0()
                            .text_color(t.secondary),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(13.))
                            .line_height(px(16.))
                            .when(self.search.read(cx).value().is_empty(), |el| {
                                el.child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .overflow_hidden()
                                        .text_color(t.tertiary)
                                        .child(self.locale.text("providerList.search")),
                                )
                            })
                            .child(Input::new(&self.search))
                            .observe("provider-search-input"),
                    )
                    .child(
                        Popover::new("provider-filter-popover")
                            .on_position(move |position, trigger| {
                                filter_motion_origin_track.resolve(position, trigger)
                            })
                            .track_focus(&self.filter_focus.container)
                            .anchor(Anchor::TopRight)
                            .offset(px(8.))
                            .open(filter_open)
                            .retain_content(filter_motion.visible)
                            .trigger(
                                self.icon_button("filter", "sliders-horizontal", true)
                                    .track_focus(&self.filter_focus.trigger)
                                    .accessibility_label(self.locale.text("providerList.filter"))
                                    .disabled(!filter_enabled)
                                    .when(!filter_enabled, |el| el.opacity(0.4)),
                            )
                            .on_open_change(cx.listener(move |this, open, _, cx| {
                                this.filter_open = *open && filter_enabled;
                                if this.filter_open {
                                    this.menu_open = false;
                                }
                                cx.notify();
                            }))
                            .content(move |_, _, _| {
                                let content = {
                                    filter_focus
                                        .surface(
                                            "filter-menu",
                                            after_filter,
                                            before_filter,
                                            move |_, cx| {
                                                let _ = close_filter.update(cx, |this, cx| {
                                                    this.filter_open = false;
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
                                        .shadow(vec![BoxShadow::new(
                                            px(0.),
                                            px(4.),
                                            t.popup_shadow.into(),
                                        )
                                        .blur_radius(px(8.))])
                                        .flex()
                                        .flex_col()
                                        .children(options.into_iter().enumerate().map(
                                            |(position, (index, label, tag))| {
                                                let owner = owner.clone();
                                                let selected_label = selected_label.clone();
                                                let active = if let Some(tag) = &tag {
                                                    selected_tag.as_ref() == Some(tag)
                                                } else {
                                                    selected_tag.is_none() && selected == index
                                                };
                                                Button::new(("filter-option", position))
                                                    .disabled(!filter_open)
                                                    .role(gpui::Role::MenuItem)
                                                    .track_focus(&filter_focus.items[position])
                                                    .justify_start()
                                                    .px(px(10.))
                                                    .py(px(8.))
                                                    .text_size(px(13.))
                                                    .line_height(px(16.))
                                                    .rounded(px(6.))
                                                    .cursor_pointer()
                                                    .hover(move |s| s.bg(t.accent_soft))
                                                    .focus(move |s| s.bg(t.accent_soft))
                                                    .accessibility_label(label.clone())
                                                    .child(label)
                                                    .when(active, |el| {
                                                        el.child(div().flex_1()).child(
                                                            div()
                                                                .text_size(px(11.))
                                                                .text_color(t.tertiary)
                                                                .child(selected_label),
                                                        )
                                                    })
                                                    .on_click(move |_, window, cx| {
                                                        let _ = owner.update(cx, |this, cx| {
                                                            if this.provider_busy {
                                                                return;
                                                            }
                                                            this.close_provider_form(window, cx);
                                                            this.selected = index;
                                                            this.tag_filter = tag.clone();
                                                            this.select_visible(window, cx);
                                                            this.filter_open = false;
                                                            cx.notify();
                                                        });
                                                    })
                                            },
                                        ))
                                };
                                filter_motion.wrap_positioned(
                                    content.observe("menu-filter-content"),
                                    filter_motion_origin,
                                )
                            })
                            .observe("menu-filter-trigger"),
                    )
                    .observe("provider-search"),
            )
            .when(self.selected != 10, |el| {
                el.child(
                    self.icon_button("refresh", "refresh-cw", false)
                        .track_focus(&self.refresh_focus)
                        .accessibility_label(self.locale.text("providerList.refreshAccounts"))
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(WorkspaceEvent::Refresh))),
                )
                .child(
                    self.icon_button("connect", "plug", false)
                        .accessibility_label(self.locale.text("oauthConnect.title"))
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(WorkspaceEvent::Connect))),
                )
                .child(
                    self.button("add", "providerList.add", "plus", true, false)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_provider_form(None, window, cx)
                        })),
                )
            })
            .when(self.selected == 10, |el| {
                el.child(
                    crate::providers::controls::ghost_button("empty-trash", true, t)
                        .flex_shrink_0()
                        .gap(px(6.))
                        .accessibility_label(self.locale.text("providerList.emptyTrash"))
                        .child(svg().path("trash-2").size(px(14.)).text_color(t.danger))
                        .child(self.locale.text("providerList.emptyTrash"))
                        .disabled(self.provider_busy || self.catalog.trash.is_empty())
                        .on_click(
                            cx.listener(|this, _, window, cx| this.confirm_trash(None, window, cx)),
                        ),
                )
            })
    }

    pub(super) fn titlebar(
        &self,
        sidebar: f32,
        list: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = self.theme;
        let menu_open = self.menu_open;
        let menu_motion = crate::menu::motion::sample("app-menu-motion", menu_open, window, cx);
        let menu_motion_origin = crate::menu::motion::Origin::new(true);
        let menu_motion_origin_track = menu_motion_origin.clone();
        let settings = self
            .menu_item("settings", "titlebar.settings", "settings")
            .disabled(!menu_open)
            .track_focus(&self.menu_focus.items[0])
            .on_click(cx.listener(|this, _, _, cx| {
                this.menu_open = false;
                cx.emit(WorkspaceEvent::Settings);
                cx.notify();
            }));
        let can_reset = self.selected_provider.is_some() && !matches!(self.selected, 9 | 10);
        let reset = self
            .menu_item("reset-provider", "titlebar.resetProvider", "rotate-ccw")
            .disabled(!menu_open || self.provider_busy)
            .track_focus(&self.menu_focus.items[1])
            .on_click(cx.listener(|this, _, window, cx| {
                if this.menu_open {
                    this.reset_provider(window, cx);
                }
            }));
        let lock = self
            .menu_item("lock", "titlebar.lock", "lock")
            .disabled(!menu_open)
            .track_focus(&self.menu_focus.items[2])
            .on_click(cx.listener(|this, _, window, cx| {
                this.menu_open = false;
                window.dispatch_action(Box::new(crate::desktop::LockVault), cx);
                cx.notify();
            }));
        let mut menu_focus = self.menu_focus.clone();
        if !can_reset {
            menu_focus.items.remove(1);
        }
        let owner = cx.entity().downgrade();
        let after_menu = self.nav_focus[0].clone();
        let before_menu = self.window_controls_focus[2].clone();
        div()
            .on_children_prepainted(crate::parity::record(&[
                "titlebar.logo",
                "titlebar.title",
                "titlebar.spacer",
                "titlebar.menu",
            ]))
            .absolute()
            .left(px(sidebar + 16.))
            .top(px(8.))
            .w(px(list))
            .h(px(34.))
            // AppTitleBar uses pane-content-inset (13 px), independently of
            // the bordered search toolbar's 16 px padding.
            .px(px(13.))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(crate::logo::logo(16., window))
            .child(crate::logo::wordmark(t.secondary, window))
            .child(div().flex_1())
            .child(
                Popover::new("app-menu-popover")
                    .on_position(move |position, trigger| {
                        menu_motion_origin_track.resolve(position, trigger)
                    })
                    .track_focus(&self.menu_focus.container)
                    .mr(px(-6.))
                    .anchor(Anchor::TopRight)
                    .offset(px(6.))
                    .open(menu_open)
                    .retain_content(menu_motion.visible)
                    .trigger(
                        Button::new("app-menu")
                            .track_focus(&self.menu_focus.trigger)
                            .size(px(28.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .accessibility_label(self.locale.text("titlebar.menu"))
                            .rounded(px(8.))
                            .cursor_pointer()
                            .hover(move |s| s.bg(t.accent_soft))
                            .child(svg().path("menu").size(px(16.)).text_color(t.secondary)),
                    )
                    .on_open_change(cx.listener(|this, open, _, cx| {
                        this.menu_open = *open;
                        if *open {
                            this.filter_open = false;
                        }
                        cx.notify();
                    }))
                    .content(move |_, _, _| {
                        let content = {
                            menu_focus
                                .surface(
                                    "app-menu-surface",
                                    after_menu,
                                    before_menu,
                                    move |_, cx| {
                                        let _ = owner.update(cx, |this, cx| {
                                            this.menu_open = false;
                                            cx.notify();
                                        });
                                    },
                                )
                                .w(px(150.))
                                .p(px(4.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(t.border)
                                .bg(t.surface)
                                .shadow(vec![BoxShadow::new(px(0.), px(4.), t.popup_shadow.into())
                                    .blur_radius(px(8.))])
                                .flex()
                                .flex_col()
                                .child(settings)
                                .when(can_reset, |el| el.child(reset))
                                .child(lock)
                        };
                        menu_motion.wrap_positioned(
                            content.observe("menu-app-content"),
                            menu_motion_origin,
                        )
                    }),
            )
    }

    fn menu_item(&self, id: &'static str, label: &str, icon: &'static str) -> Button {
        let t = self.theme;
        Button::new(id)
            .role(gpui::Role::MenuItem)
            .px(px(10.))
            .py(px(7.))
            .flex()
            .items_center()
            .justify_start()
            .gap(px(8.))
            .rounded(px(6.))
            .text_size(px(13.))
            .line_height(px(16.))
            .text_color(t.text)
            .cursor_pointer()
            .hover(move |s| s.bg(t.accent_soft))
            .focus(move |s| s.bg(t.accent_soft))
            .child(svg().path(icon).size(px(14.)).text_color(t.text))
            .accessibility_label(self.locale.text(label))
            .child(self.locale.text(label))
    }
}
