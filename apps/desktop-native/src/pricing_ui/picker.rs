use super::*;
use crate::parity::Observe;
use gpui::{div, px, svg, Anchor, AnyElement, FontWeight};
use gpui_base::{Button, Popover};
impl Pricing {
    pub(super) fn group_picker(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme;
        let open = self.picker_open && !self.busy();
        let options: Vec<_> = std::iter::once((None, self.locale.text("pricing.listPrices")))
            .chain(
                self.config
                    .iter()
                    .flat_map(|c| c.groups.iter())
                    .map(|g| (Some(g.id), g.name.clone())),
            )
            .collect();
        let selected = options
            .iter()
            .position(|(id, _)| *id == self.selected_group)
            .unwrap_or(0);
        let value = options
            .get(selected)
            .map(|(_, label)| label.clone())
            .unwrap_or_default();
        let frame = crate::menu::motion::sample("pricing-source-motion", open, window, cx);
        let origin = crate::menu::motion::Origin::new(false);
        let track = origin.clone();
        let bounds = self.picker_bounds.clone();
        let width = bounds.get().size.width.max(px(1.));
        let scroll = window
            .use_keyed_state("pricing-sources-scroll", cx, |_, _| {
                gpui::ScrollHandle::new()
            })
            .read(cx)
            .clone();
        let mut focus = self.picker_focus.clone();
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
                    crate::parity::record_bounds("pricing-source-trigger", *rect, cx);
                }
            })
            .child(
                self.label("pricing.basePrices")
                    .font_weight(FontWeight::MEDIUM),
            )
            .child(
                Popover::new("pricing-source-picker")
                    .open(open)
                    .retain_content(frame.visible)
                    .anchor(Anchor::TopLeft)
                    .offset(px(4.))
                    .track_focus(&focus.container)
                    .on_position(move |position, trigger| track.resolve(position, trigger))
                    .on_open_change(cx.listener(move |this, open, window, cx| {
                        this.picker_open = *open && !this.busy();
                        if this.picker_open {
                            initial.focus_item(selected, window, cx);
                        }
                        cx.notify();
                    }))
                    .trigger(
                        Button::new("pricing-source")
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
                            .font_weight(FontWeight::NORMAL)
                            .line_height(px(20.))
                            .when(open, |el| el.shadow(t.outline(t.focus_ring, 3., 0.)))
                            .focus_visible(move |s| s.shadow(t.outline(t.focus_ring, 3., 0.)))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .accessibility_label(self.locale.text("pricing.basePrices"))
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
                                        this.picker_open = true;
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
                            "pricing-sources-surface",
                            focus.trigger.clone(),
                            focus.trigger.clone(),
                            move |_, cx| {
                                let _ = close.update(cx, |this, cx| {
                                    this.picker_open = false;
                                    cx.notify();
                                });
                            },
                        );
                        let items = div()
                            .id("pricing-sources-scroll")
                            .max_h(px(278.))
                            .overflow_y_scroll()
                            .track_scroll(&scroll)
                            .flex()
                            .flex_col()
                            .children(options.into_iter().enumerate().map(
                                |(index, (id, label))| {
                                    let owner = owner.clone();
                                    crate::menu::select::item(
                                        ("pricing-source-option", index),
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
                                                if !this.picker_open || this.busy() {
                                                    return;
                                                }
                                                this.selected_group = id;
                                                this.error.clear();
                                                this.picker_open = false;
                                                this.picker_focus.trigger.focus(window, cx);
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
                                    .observe("pricing-source-content"),
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
