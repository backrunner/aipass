//! Inline address editing stays open until the Agent accepts the new config.
use super::*;
use crate::parity::Observe;
use gpui::{div, px, Focusable};
use gpui_base::{
    input::{Enter, Escape, Input},
    Button,
};

impl Proxy {
    pub(super) fn begin_bind(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy() || self.status.as_ref().is_some_and(|s| s.running) {
            return;
        }
        let Some(config) = &self.config else { return };
        let input = self.fields.ensure_input(
            ("proxy-bind", "server.bindAddress", &config.bind_addr, false),
            self.theme,
            &self.locale,
            window,
            cx,
        );
        self.editing_bind = true;
        input.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }
    pub(super) fn cancel_bind(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving_bind {
            return;
        }
        self.editing_bind = false;
        self.fields.remove("proxy-bind");
        self.bind_focus.focus(window, cx);
        cx.notify();
    }
    fn save_bind(&mut self, cx: &mut Context<Self>) {
        if !self.editing_bind || self.busy() || self.saving_bind {
            return;
        }
        let value = self.fields.value("proxy-bind", cx).trim().to_owned();
        if value.is_empty() {
            return;
        }
        if let Some(mut config) = self.config.clone() {
            config.bind_addr = value;
            self.saving_bind = true;
            self.apply(AgentRequest::ServerConfigSet { config }, cx);
        }
    }
    pub(super) fn bind_chip(
        &mut self,
        config: &ProxyConfig,
        running: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let t = self.theme;
        let mut chip = div()
            .id("proxy-bind-chip")
            .flex()
            .items_center()
            .gap(px(6.))
            .min_w_0()
            .px(px(10.))
            .py(px(4.))
            .rounded_full()
            .border_1()
            .border_color(t.divider)
            .bg(t.surface2)
            .text_size(px(12.))
            .line_height(px(18.))
            .text_color(t.secondary)
            .font_family(".AppleSystemUIFontMonospaced");
        if self.editing_bind && !running {
            let input = self.fields.ensure_input(
                ("proxy-bind", "server.bindAddress", &config.bind_addr, false),
                t,
                &self.locale,
                window,
                cx,
            );
            input.update(cx, |s, cx| s.set_disabled(self.saving_bind, cx));
            chip = chip
                .on_action(cx.listener(|s, _: &Enter, _, cx| s.save_bind(cx)))
                .on_action(cx.listener(|s, _: &Escape, w, cx| s.cancel_bind(w, cx)))
                .child(
                    div()
                        .w(px(140.))
                        .min_w_0()
                        .h(px(22.))
                        .px(px(2.))
                        .flex()
                        .items_center()
                        .text_color(t.text)
                        .child(Input::new(&input))
                        .observe("proxy-bind-input"),
                )
                .child(
                    self.bind_button("proxy-save-bind", "check", "common.save", true)
                        .disabled(
                            self.busy() || self.fields.value("proxy-bind", cx).trim().is_empty(),
                        )
                        .on_click(cx.listener(|s, _, _, cx| s.save_bind(cx))),
                )
                .child(
                    self.bind_button("proxy-cancel-bind", "x", "common.cancel", false)
                        .disabled(self.busy())
                        .on_click(cx.listener(|s, _, w, cx| s.cancel_bind(w, cx))),
                );
        } else {
            let address = if running {
                self.status
                    .as_ref()
                    .map(|s| &s.bind_addr)
                    .unwrap_or(&config.bind_addr)
            } else {
                &config.bind_addr
            };
            chip = chip
                .child(div().min_w_0().truncate().child(address.clone()))
                .when(!running, |el| {
                    el.child(
                        self.bind_button(
                            "proxy-edit-bind",
                            "pencil",
                            "server.editBindAddress",
                            false,
                        )
                        .track_focus(&self.bind_focus)
                        .disabled(self.busy())
                        .on_click(cx.listener(|s, _, w, cx| s.begin_bind(w, cx))),
                    )
                });
        }
        chip.observe("proxy-bind-chip").into_any_element()
    }
    fn bind_button(
        &self,
        id: &'static str,
        icon: &'static str,
        label: &str,
        primary: bool,
    ) -> Button {
        let t = self.theme;
        Button::new(id)
            .accessibility_label(self.locale.text(label))
            .h(px(16.))
            .px(px(if primary { 8. } else { 0. }))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .when(primary, |s| s.bg(t.accent))
            .hover(move |s| {
                s.bg(if primary {
                    t.accent.opacity(0.9)
                } else {
                    t.accent_soft
                })
            })
            .focus_visible(move |s| s.shadow(t.outline(t.focus_ring, 2., 1.)))
            .child(
                gpui::svg()
                    .path(icon)
                    .size(px(if icon == "pencil" { 12. } else { 13. }))
                    .text_color(if primary {
                        gpui::rgb(0xffffff)
                    } else {
                        t.tertiary
                    }),
            )
    }
}
