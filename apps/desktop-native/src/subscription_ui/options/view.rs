use super::*;
use crate::parity::Observe;
use gpui::{div, px, svg, AnyElement, FontWeight};
use gpui_base::Button;
impl Subscriptions {
    pub(in crate::subscription_ui) fn options_content(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = self.theme;
        div()
            .h(px(566.))
            .w_full()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_shrink_0()
                    .px(px(24.))
                    .py(px(20.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .size(px(36.))
                            .flex_shrink_0()
                            .rounded(px(10.))
                            .border_1()
                            .border_color(t.border)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                svg()
                                    .path("sliders-horizontal")
                                    .size(px(18.))
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
                                    .text_size(px(15.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(self.text("providerRuntime.title")),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(15.))
                                    .text_color(t.tertiary)
                                    .text_ellipsis()
                                    .child(
                                        self.entry
                                            .as_ref()
                                            .map(|e| e.title.clone())
                                            .unwrap_or_default(),
                                    ),
                            ),
                    )
                    .child(
                        self.icon_button(
                            "runtime-close",
                            "x",
                            self.text("common.close"),
                            false,
                            false,
                        )
                        .track_focus(&self.focus)
                        .disabled(self.runtime_options.busy())
                        .on_click(cx.listener(|this, _, window, cx| this.close(window, cx)))
                        .observe("runtime-close"),
                    )
                    .observe("subscription-runtime-header"),
            )
            .child(
                div()
                    .id("runtime-tabs")
                    .role(gpui::Role::TabList)
                    .flex_shrink_0()
                    .px(px(24.))
                    .flex()
                    .gap(px(24.))
                    .border_b_1()
                    .border_color(t.divider)
                    .children(SECTIONS.iter().enumerate().map(|(index, (label, icon))| {
                        Button::new(format!("runtime-tab-{index}"))
                            .track_focus(&self.runtime_options.tab_focus[index])
                            .tab_stop(self.runtime_options.tab == index)
                            .focus_visible(move |s| {
                                s.rounded(px(3.)).shadow(t.outline(t.focus_ring, 2., 3.))
                            })
                            .styles(|s| s.disabled(|s| s.opacity(0.5)))
                            .role(gpui::Role::Tab)
                            .aria_selected(self.runtime_options.tab == index)
                            .pb(px(13.))
                            .border_b_2()
                            .border_color(if self.runtime_options.tab == index {
                                t.accent
                            } else {
                                gpui::rgba(0)
                            })
                            .text_size(px(12.))
                            .line_height(px(15.))
                            .text_color(if self.runtime_options.tab == index {
                                t.text
                            } else {
                                t.tertiary
                            })
                            .flex()
                            .gap(px(7.))
                            .items_center()
                            .disabled(self.runtime_options.busy())
                            .accessibility_label(self.text(label))
                            .child(svg().path(*icon).size(px(15.)).text_color(
                                if self.runtime_options.tab == index {
                                    t.text
                                } else {
                                    t.tertiary
                                },
                            ))
                            .child({
                                let label = self.text(label);
                                let width = window
                                    .text_system()
                                    .shape_line(
                                        label.clone().into(),
                                        px(12.),
                                        &[gpui::TextRun {
                                            len: label.len(),
                                            font: gpui::font(".AppleSystemUIFont"),
                                            color: t.text.into(),
                                            background_color: None,
                                            underline: None,
                                            strikethrough: None,
                                        }],
                                        None,
                                    )
                                    .width;
                                // Preserve fractional glyph advances between adjacent tabs.
                                div()
                                    .w(width)
                                    .flex_shrink_0()
                                    .whitespace_nowrap()
                                    .child(label)
                            })
                            .on_key_down(cx.listener(
                                move |this, event: &gpui::KeyDownEvent, window, cx| {
                                    if this.runtime_options.busy() {
                                        return;
                                    }
                                    let index = this.runtime_options.tab;
                                    let next = match event.keystroke.key.as_str() {
                                        "right" => (index + 1) % 4,
                                        "left" => (index + 3) % 4,
                                        "home" => 0,
                                        "end" => 3,
                                        _ => return,
                                    };
                                    this.runtime_options.tab = next;
                                    this.runtime_options.tab_focus[next].focus(window, cx);
                                    this.body_scroll.set_offset(gpui::point(px(0.), px(0.)));
                                    cx.stop_propagation();
                                    cx.notify();
                                },
                            ))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.runtime_options.tab = index;
                                this.body_scroll.set_offset(gpui::point(px(0.), px(0.)));
                                cx.notify();
                            }))
                            .observe(format!("runtime-tab-{index}"))
                    }))
                    .observe("subscription-runtime-tabs"),
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
                            .id("runtime-editor-body")
                            .track_scroll(&self.body_scroll)
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .p(px(24.))
                            .bg(t.bg)
                            .flex()
                            .flex_col()
                            .gap(px(16.))
                            .when(!self.runtime_options.error.is_empty(), |el| {
                                el.child(self.banner(self.runtime_options.error.clone(), false))
                            })
                            .when(!self.runtime_options.notice.is_empty(), |el| {
                                el.child(self.paragraph(self.runtime_options.notice.clone(), 12.))
                            })
                            .when(self.runtime_options.value.is_none(), |el| {
                                el.when(self.runtime_options.pending, |el| {
                                    el.child(self.paragraph(self.text("common.loading"), 12.))
                                })
                                .when(
                                    !self.runtime_options.pending,
                                    |el| {
                                        el.child(
                                            self.button(
                                                "runtime-retry",
                                                self.text("providerRuntime.retry"),
                                                "",
                                                false,
                                            )
                                            .on_click(
                                                cx.listener(|this, _, _, cx| {
                                                    this.options_request(Action::Load, cx)
                                                }),
                                            ),
                                        )
                                    },
                                )
                            })
                            .when(self.runtime_options.value.is_some(), |el| {
                                el.child(match self.runtime_options.tab {
                                    1 => self.options_proxy(window, cx),
                                    2 => self.options_balance(window, cx),
                                    3 => self.options_hooks(window, cx),
                                    _ => self.options_quota(window, cx),
                                })
                            })
                            .observe("subscription-runtime-body"),
                    )
                    .child(scroll::fade(t.bg, self.body_scroll.clone())),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .px(px(24.))
                    .py(px(14.))
                    .border_t_1()
                    .border_color(t.divider)
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(11.))
                            .line_height(px(14.))
                            .text_color(t.tertiary)
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                svg()
                                    .path("shield-check")
                                    .size(px(14.))
                                    .text_color(t.tertiary),
                            )
                            .child(self.text(if self.runtime_options.dirty {
                                "providerRuntime.unsaved"
                            } else {
                                "communityConnect.localNote"
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                self.ghost_button(self.button(
                                    "runtime-cancel",
                                    self.text(if self.runtime_options.dirty {
                                        "common.cancel"
                                    } else {
                                        "common.close"
                                    }),
                                    "",
                                    false,
                                ))
                                .disabled(self.runtime_options.busy())
                                .on_click(cx.listener(|this, _, window, cx| this.close(window, cx)))
                                .observe("runtime-cancel"),
                            )
                            .child(
                                self.button("runtime-save", self.text("common.save"), "", true)
                                    .disabled(
                                        self.runtime_options.pending
                                            || !self.runtime_options.dirty
                                            || self.runtime_options.value.is_none(),
                                    )
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.options_save(window, cx)
                                    }))
                                    .observe("runtime-save"),
                            ),
                    )
                    .observe("subscription-runtime-footer"),
            )
            .into_any_element()
    }
}
