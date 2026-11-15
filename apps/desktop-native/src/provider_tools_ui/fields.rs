use super::*;
use crate::parity::Observe;
use gpui::{div, px, svg, Anchor, AnyElement, Focusable, FontWeight};
use gpui_base::{input::Input, Button, Popover};
const MODES: [(UsageProbeMode, &str); 4] = [
    (UsageProbeMode::Auto, "providerDetail.usageProbeAuto"),
    (UsageProbeMode::NewApi, "providerDetail.usageProbeNewApi"),
    (UsageProbeMode::SubApi, "providerDetail.usageProbeSubApi"),
    (
        UsageProbeMode::NewApiAdvanced,
        "providerDetail.usageProbeNewApiAdvanced",
    ),
];
impl Tools {
    fn set_mode(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        self.mode = MODES[index].0;
        self.usage = None;
        self.error.clear();
        if self.mode != UsageProbeMode::NewApiAdvanced {
            self.inputs[1].update(cx, |s, cx| s.set_value("", window, cx));
        }
        cx.notify();
    }
    pub(super) fn label(&self, key: &str) -> gpui::Div {
        div()
            .text_size(px(12.))
            .line_height(px(15.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(self.theme.secondary)
            .child(self.locale.text(key))
    }
    pub(super) fn modes(&self, cx: &Context<Self>) -> AnyElement {
        let t = self.theme;
        div()
            .flex()
            .flex_col()
            .gap(px(7.))
            .child(self.label("providerDetail.usageProbeMode"))
            .child(
                div()
                    .id("usage-mode-group")
                    .role(gpui::Role::TabList)
                    .border_1()
                    .border_color(t.border)
                    .rounded(px(8.))
                    .bg(t.surface2)
                    .p(px(2.))
                    .flex()
                    .gap(px(2.))
                    .children(MODES.into_iter().enumerate().map(|(index, (mode, key))| {
                        let active = mode == self.mode;
                        Button::new(("usage-mode", index))
                            .role(gpui::Role::Tab)
                            .aria_selected(active)
                            .track_focus(&self.mode_focus[index])
                            .tab_index(if active { 0 } else { -1 })
                            .disabled(self.busy())
                            .flex_1()
                            .min_w_0()
                            .h(px(26.))
                            .px(px(12.))
                            .rounded(px(4.))
                            .text_size(px(12.))
                            .line_height(px(14.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if active { t.text } else { t.secondary })
                            .bg(if active { t.surface } else { gpui::rgba(0) })
                            .when(active, |el| {
                                el.shadow(vec![gpui_base::box_shadow(
                                    0.,
                                    1.,
                                    crate::theme::shadow_blur(2.),
                                    0.,
                                    gpui::rgb(0x0f1110).opacity(0.08).into(),
                                )])
                            })
                            .hover(move |s| s.text_color(t.text))
                            .focus_visible(move |s| s.shadow(t.outline(t.accent, 2., -2.)))
                            .accessibility_label(self.locale.text(key))
                            .child(self.locale.text(key))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.set_mode(index, window, cx)
                            }))
                            .on_key_down(cx.listener(
                                move |this, event: &gpui::KeyDownEvent, window, cx| {
                                    let mods = event.keystroke.modifiers;
                                    if mods.control || mods.platform || mods.alt || this.busy() {
                                        return;
                                    }
                                    let next = match event.keystroke.key.as_str() {
                                        "right" => (index + 1) % 4,
                                        "left" => (index + 3) % 4,
                                        "home" => 0,
                                        "end" => 3,
                                        _ => return,
                                    };
                                    cx.stop_propagation();
                                    this.mode_focus[next].focus(window, cx);
                                    this.set_mode(next, window, cx);
                                },
                            ))
                            .observe(format!("usage-mode-{index}"))
                    })),
            )
            .observe("usage-mode-field")
            .into_any_element()
    }
    pub(super) fn field(&self, index: usize, window: &Window, cx: &Context<Self>) -> AnyElement {
        let t = self.theme;
        let focused = self.inputs[index]
            .read(cx)
            .focus_handle(cx)
            .is_focused(window);
        let placeholder = match index {
            1 => self.locale.text("providerDetail.accessTokenPlaceholder"),
            2 => self.locale.text("providerDetail.userIdPlaceholder"),
            _ => String::new(),
        };
        div()
            .flex()
            .flex_col()
            .gap(px(7.))
            .min_w_0()
            .w_full()
            .child(self.label(LABELS[index]))
            .child(
                div()
                    .relative()
                    .h(px(34.))
                    .px(px(11.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(t.border)
                    .bg(t.surface)
                    .when(focused, |el| {
                        el.border_color(t.accent)
                            .shadow(t.outline(t.focus_ring, 3., 0.))
                    })
                    .text_size(px(13.))
                    .line_height(px(16.))
                    .when(
                        self.inputs[index].read(cx).value().is_empty() && !placeholder.is_empty(),
                        |el| {
                            el.child(
                                div()
                                    .absolute()
                                    .left(px(11.))
                                    .top(px(8.))
                                    .text_color(t.tertiary)
                                    .child(placeholder),
                            )
                        },
                    )
                    .child(Input::new(&self.inputs[index]))
                    .observe(format!("usage-input-{index}")),
            )
            .observe(format!("usage-field-{index}"))
            .into_any_element()
    }
    pub(super) fn key_picker(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme;
        let open = self.key_open && !self.busy();
        let options = self
            .entry
            .as_ref()
            .unwrap()
            .secret_refs
            .iter()
            .map(|s| (s.id.clone(), s.label.clone()))
            .collect::<Vec<_>>();
        let selected = options
            .iter()
            .position(|(id, _)| Some(id) == self.secret_id.as_ref())
            .unwrap_or(0);
        let value = options
            .get(selected)
            .map(|(_, label)| label.clone())
            .unwrap_or_default();
        let frame = crate::menu::motion::sample("usage-key-motion", open, window, cx);
        let origin = crate::menu::motion::Origin::new(false);
        let track = origin.clone();
        let bounds = self.key_bounds.clone();
        let width = bounds.get().size.width.max(px(1.));
        let scroll = window
            .use_keyed_state("usage-keys-scroll", cx, |_, _| gpui::ScrollHandle::new())
            .read(cx)
            .clone();
        let mut focus = self.key_focus.clone();
        focus.scroll = Some(scroll.clone());
        let initial = focus.clone();
        let keyboard = focus.clone();
        let owner = cx.entity().downgrade();
        let close = owner.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .on_children_prepainted(move |children, _, cx| {
                if let Some(rect) = children.last() {
                    bounds.set(*rect);
                    crate::parity::record_bounds("usage-key-trigger", *rect, cx);
                }
            })
            .child(
                self.label("providerDetail.usageProbeKey")
                    .font_weight(FontWeight::MEDIUM),
            )
            .child(
                Popover::new("usage-key-picker")
                    .open(open)
                    .retain_content(frame.visible)
                    .anchor(Anchor::TopLeft)
                    .offset(px(4.))
                    .track_focus(&focus.container)
                    .on_position(move |position, trigger| track.resolve(position, trigger))
                    .on_open_change(cx.listener(move |this, open, window, cx| {
                        this.key_open = *open && !this.busy();
                        if this.key_open {
                            initial.focus_item(selected, window, cx);
                        }
                        cx.notify();
                    }))
                    .trigger(
                        Button::new("usage-key")
                            .disabled(self.busy())
                            .track_focus(&focus.trigger)
                            .w_full()
                            .h(px(34.))
                            .px(px(10.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(if open { t.accent } else { t.border })
                            .bg(t.surface)
                            .text_color(t.text)
                            .text_size(px(13.))
                            .line_height(px(20.))
                            .when(open, |el| el.shadow(t.outline(t.focus_ring, 3., 0.)))
                            .focus_visible(move |s| s.shadow(t.outline(t.focus_ring, 3., 0.)))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .accessibility_label(self.locale.text("providerDetail.usageProbeKey"))
                            .child(div().flex_1().min_w_0().text_ellipsis().child(value))
                            .child(
                                svg()
                                    .path("chevron-down")
                                    .size(px(14.))
                                    .text_color(t.secondary),
                            )
                            .on_key_down(cx.listener(
                                move |this, event: &gpui::KeyDownEvent, window, cx| {
                                    if matches!(event.keystroke.key.as_str(), "down" | "up")
                                        && !this.busy()
                                    {
                                        this.key_open = true;
                                        let focus = keyboard.clone();
                                        window.on_next_frame(move |window, cx| {
                                            focus.focus_item(selected, window, cx)
                                        });
                                        cx.stop_propagation();
                                        cx.notify();
                                    }
                                },
                            )),
                    )
                    .content(move |_, _, _| {
                        let surface = focus.surface(
                            "usage-keys-surface",
                            focus.trigger.clone(),
                            focus.trigger.clone(),
                            move |_, cx| {
                                let _ = close.update(cx, |this, cx| {
                                    this.key_open = false;
                                    cx.notify();
                                });
                            },
                        );
                        let items = div()
                            .id("usage-keys-scroll")
                            .max_h(px(278.))
                            .overflow_y_scroll()
                            .track_scroll(&scroll)
                            .flex()
                            .flex_col()
                            .children(options.into_iter().enumerate().map(
                                |(index, (id, label))| {
                                    let owner = owner.clone();
                                    crate::menu::select::item(
                                        ("usage-key-option", index),
                                        index == selected,
                                        t,
                                    )
                                    .disabled(!open)
                                    .track_focus(&focus.items[index])
                                    .accessibility_label(label.clone())
                                    .child(div().flex_1().min_w_0().text_ellipsis().child(label))
                                    .child(crate::menu::select::indicator(index == selected, t))
                                    .on_click(
                                        move |_, window, cx| {
                                            let _ = owner.update(cx, |this, cx| {
                                                if !this.key_open || this.busy() {
                                                    return;
                                                }
                                                this.secret_id = Some(id.clone());
                                                this.usage = None;
                                                this.key_open = false;
                                                this.key_focus.trigger.focus(window, cx);
                                                cx.notify();
                                            });
                                        },
                                    )
                                },
                            ));
                        frame.wrap_positioned(
                            crate::menu::select::fade(
                                crate::menu::select::surface(surface, width, t)
                                    .child(items)
                                    .observe("usage-key-content"),
                                scroll,
                                t,
                            ),
                            origin,
                        )
                    }),
            )
            .into_any_element()
    }
}
