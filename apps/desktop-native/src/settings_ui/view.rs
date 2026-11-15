use super::*;
use crate::parity::Observe;
use gpui::{div, px, FontWeight};
use gpui_base::{Button, Dialog};
pub(super) const DRAWER_WIDTH: f32 = 560.;
impl Render for Settings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let frame = self.motion.sample(self.open, window, cx);
        if !frame.visible {
            if !self.open {
                self.fields.clear(window, cx);
            }
            return div().into_any_element();
        }
        if self
            .access_code
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed().as_secs() >= 60)
        {
            self.access_code = None;
        }
        self.save_blurred_preferences(cx);
        self.fields.disabled(self.pending, cx);
        let t = self.theme;
        let content = if self.loading {
            admin_ui::hint(self.locale.text("common.loading"), t).into_any_element()
        } else {
            match self.tab {
                0 => self.general(window, cx),
                1 => self.security(window, cx),
                2 => self.sync(window, cx),
                3 => self.server(window, cx),
                4 => self.backup(window, cx),
                _ => self.about(cx),
            }
        };
        let body = div()
            .w(px(DRAWER_WIDTH))
            .h(window.viewport_size().height - px(16.))
            .min_h_0()
            .rounded(px(16.))
            .border_1()
            .border_color(t.border)
            .bg(t.surface)
            .text_color(t.text)
            .text_size(px(13.))
            .line_height(px(16.))
            .font_weight(FontWeight::NORMAL)
            .font_family(".AppleSystemUIFont")
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                admin_ui::row()
                    .flex_shrink_0()
                    .pt(px(18.))
                    .pb(px(14.))
                    .px(px(20.))
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.locale.text("settings.title")),
                    )
                    .child(
                        crate::providers::controls::icon(
                            "settings-close",
                            "x",
                            self.locale.text("settings.close"),
                            t,
                        )
                        .size(px(30.))
                        .rounded_full()
                        .track_focus(&self.close_focus)
                        .disabled(self.pending && !self.pending_read)
                        .on_click(cx.listener(|s, _, w, cx| s.close(w, cx))),
                    ),
            )
            .when(!self.error.is_empty() || !self.notice.is_empty(), |el| {
                el.child(self.feedback(cx))
            })
            .child(
                div()
                    .id("settings-tabs")
                    .role(gpui::Role::TabList)
                    .flex_shrink_0()
                    .flex()
                    .gap(px(4.))
                    .px(px(16.))
                    .pb(px(12.))
                    .children(TABS.iter().enumerate().map(|(i, key)| {
                        Button::new(("settings-tab", i))
                            .track_focus(&self.tab_focus[i])
                            .tab_stop(i == self.tab)
                            .role(gpui::Role::Tab)
                            .aria_selected(i == self.tab)
                            .accessibility_label(self.locale.text(key))
                            .disabled(self.pending && !self.pending_read)
                            .h(px(30.))
                            .px(px(12.))
                            .rounded_full()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .whitespace_nowrap()
                            .bg(if i == self.tab {
                                t.accent_soft
                            } else {
                                gpui::rgba(0)
                            })
                            .text_color(if i == self.tab { t.accent } else { t.secondary })
                            .hover(move |s| s.bg(t.surface2))
                            .focus_visible(move |s| s.shadow(t.outline(t.focus_ring, 2., 1.)))
                            .child(self.locale.text(key))
                            .on_click(cx.listener(move |s, _, w, cx| s.select_tab(i, w, cx)))
                            .on_key_down(cx.listener(move |s, e: &gpui::KeyDownEvent, w, cx| {
                                let next = match e.keystroke.key.as_str() {
                                    "right" => (i + 1) % 6,
                                    "left" => (i + 5) % 6,
                                    "home" => 0,
                                    "end" => 5,
                                    _ => return,
                                };
                                if !s.pending || s.pending_read {
                                    s.select_tab(next, w, cx);
                                    s.tab_focus[next].focus(w, cx);
                                }
                                cx.stop_propagation();
                            }))
                            .observe(format!("settings-tab-{i}"))
                    })),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("settings-scroll")
                            .role(gpui::Role::TabPanel)
                            .track_scroll(&self.scroll)
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .pt(px(16.))
                            .px(px(20.))
                            .pb(px(24.))
                            .border_t_1()
                            .border_color(t.divider)
                            .bg(t.bg)
                            .rounded_b(px(15.))
                            .child(content)
                            .observe("settings-body"),
                    )
                    .child(crate::scroll_fade::rounded_bottom(
                        t.bg,
                        self.scroll.clone(),
                        15.,
                    )),
            )
            .observe("settings-drawer");
        let owner = cx.entity().downgrade();
        let drawer = Dialog::new(cx)
            .focus_handle(self.focus.clone())
            .accessibility_label(self.locale.text("settings.title"))
            .items_center()
            .justify_end()
            .p(px(8.))
            .close_on_escape((!self.pending || self.pending_read) && self.open)
            .close_on_backdrop_press((!self.pending || self.pending_read) && self.open)
            .on_ok(|_, _, _| false)
            .on_open_change(move |open, _, w, cx| {
                if !open {
                    let _ = owner.update(cx, |s, cx| s.close(w, cx));
                }
            })
            .backdrop(
                div()
                    .absolute()
                    .inset_0()
                    .bg(gpui::rgb(0x080c18).opacity(0.36))
                    .backdrop_blur(px(6.))
                    .opacity(frame.overlay),
            )
            .popup(
                crate::composite::Scale::new(div().opacity(frame.popup).child(body), frame.scale)
                    .scale_y(frame.scale_y)
                    .origin(gpui::point(1., 0.5))
                    .translate_relative(gpui::point(frame.offset, 0.)),
            );
        div()
            .child(drawer)
            .when(self.confirm.is_some(), |el| el.child(self.confirmation(cx)))
            .into_any_element()
    }
}
impl Settings {
    pub(super) fn toggle(
        &self,
        key: &str,
        on: bool,
        change: impl Fn(&mut Self, bool, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let owner = cx.entity().downgrade();
        admin_ui::row()
            .min_h(px(36.))
            .child(div().flex_1().min_w_0().child(self.locale.text(key)))
            .child(
                admin_ui::switch(
                    key.to_owned(),
                    self.locale.text(key),
                    on,
                    self.pending
                        || (key == "panel.enable"
                            && !on
                            && self.panel.as_ref().is_none_or(|p| !p.has_access_code)),
                    self.theme,
                    window,
                    cx,
                )
                .on_change(move |value, _, _, cx| {
                    let _ = owner.update(cx, |s, cx| change(s, value, cx));
                }),
            )
    }

    pub(super) fn segmented(
        &mut self,
        key: &str,
        selected: &str,
        options: Vec<(&str, String)>,
        change: impl Fn(&mut Self, &str, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let t = self.theme;
        let change = std::rc::Rc::new(change);
        let handles = self
            .segments
            .entry(key.into())
            .or_insert_with(|| (0..options.len()).map(|_| cx.focus_handle()).collect())
            .clone();
        let values = options
            .iter()
            .map(|(v, _)| v.to_string())
            .collect::<Vec<_>>();
        div()
            .id(key.to_owned())
            .role(gpui::Role::TabList)
            .grid()
            .grid_cols_min_content(options.len() as u16)
            .pixel_snap(false)
            .p(px(2.))
            .gap(px(2.))
            .rounded(px(8.))
            .border_1()
            .border_color(t.border)
            .bg(t.surface2)
            .children(options.into_iter().enumerate().map(|(i, (value, label))| {
                let value = value.to_owned();
                let chosen = value == selected;
                let click = change.clone();
                let navigate = change.clone();
                let values = values.clone();
                let handles = handles.clone();
                Button::new(("settings-segment", i))
                    .track_focus(&handles[i])
                    .tab_stop(chosen)
                    .role(gpui::Role::Tab)
                    .aria_selected(chosen)
                    .accessibility_label(label.clone())
                    .disabled(
                        self.pending
                            || (key == "update-channel"
                                && self.update.as_ref().is_some_and(|u| u.busy)),
                    )
                    .h(px(26.))
                    .px(px(12.))
                    .rounded(px(4.))
                    .font_weight(FontWeight::MEDIUM)
                    .flex_shrink_0()
                    .bg(if chosen { t.surface } else { gpui::rgba(0) })
                    .when(chosen, |el| {
                        el.shadow(vec![gpui_base::box_shadow(
                            0.,
                            1.,
                            2.,
                            0.,
                            gpui::rgb(0x0f1110).opacity(0.08).into(),
                        )])
                    })
                    .text_color(if chosen { t.text } else { t.secondary })
                    .text_size(px(12.))
                    .whitespace_nowrap()
                    .focus_visible(move |s| s.shadow(t.outline(t.focus_ring, 2., 1.)))
                    .when(!chosen, |el| {
                        el.hover(move |style| style.text_color(t.text))
                    })
                    .child(label)
                    .on_click(cx.listener(move |s, _, _, cx| click(s, &value, cx)))
                    .on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, w, cx| {
                        if s.pending {
                            return;
                        }
                        let n = values.len();
                        let next = match event.keystroke.key.as_str() {
                            "right" => (i + 1) % n,
                            "left" => (i + n - 1) % n,
                            "home" => 0,
                            "end" => n - 1,
                            _ => return,
                        };
                        navigate(s, &values[next], cx);
                        handles[next].focus(w, cx);
                        cx.stop_propagation();
                    }))
            }))
            .observe(key.to_owned())
            .into_any_element()
    }
    fn confirmation(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let kind = self.confirm.unwrap_or_default();
        let t = self.theme;
        let (title, desc, action) = match kind {
            "import" => (
                "settings.import",
                "settings.importDesc",
                "settings.importAndLock",
            ),
            "rotate" => (
                "settings.rotateKeys",
                "settings.rotateKeysDesc",
                "settings.rotate",
            ),
            _ => (
                "settings.ccSwitchConfirmTitle",
                "settings.ccSwitchConfirmBody",
                "settings.ccSwitchConfirmEnable",
            ),
        };
        let owner = cx.entity().downgrade();
        Dialog::new(cx)
            .focus_handle(self.confirm_focus.clone())
            .accessibility_label(self.locale.text(title))
            .on_ok(|_, _, _| false)
            .on_open_change(move |open, _, w, cx| {
                if !open {
                    let _ = owner.update(cx, |s, cx| s.close_confirmation(w, cx));
                }
            })
            .backdrop(div().absolute().inset_0().bg(gpui::black().opacity(0.25)))
            .popup(
                admin_ui::rows()
                    .w(px(420.))
                    .rounded(px(12.))
                    .bg(t.surface)
                    .text_color(t.text)
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.locale.text(title)),
                    )
                    .child(admin_ui::hint(self.locale.text(desc), t))
                    .child(
                        admin_ui::row()
                            .justify_end()
                            .child(
                                self.button("settings-confirm-cancel", "common.cancel", false)
                                    .track_focus(&self.confirm_cancel)
                                    .on_click(
                                        cx.listener(|s, _, w, cx| s.close_confirmation(w, cx)),
                                    ),
                            )
                            .child(
                                self.button("settings-confirm-apply", action, true)
                                    .on_click(cx.listener(move |s, _, w, cx| {
                                        s.close_confirmation(w, cx);
                                        match kind {
                                            "rotate" => s.apply(
                                                AgentRequest::VaultRotate {
                                                    reason: "manual".into(),
                                                },
                                                cx,
                                            ),
                                            "import" => s.import_backup(w, cx),
                                            _ => {
                                                s.preferences.official_accounts_import = true;
                                                s.save_preferences(cx);
                                            }
                                        }
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }
}
