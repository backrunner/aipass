use super::Form;
use crate::{menu::select, parity::Observe};
use gpui::{div, prelude::*, px, svg, Anchor, AnyElement, Context};
use gpui_base::{Button, Popover};

impl Form {
    pub(super) fn picker(
        &self,
        kind: &'static str,
        label: String,
        value: String,
        options: Vec<(usize, String)>,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = self.theme;
        let editing = self.is_editing();
        let owner = cx.entity().downgrade();
        let close = owner.clone();
        let open = self.picker == Some(kind) && !self.busy;
        let motion =
            crate::menu::motion::sample(format!("form-picker-motion-{kind}"), open, window, cx);
        let origin = crate::menu::motion::Origin::new(false);
        let track_origin = origin.clone();
        let mut focus = self.picker_focus[kind].clone();
        let scroll_state =
            window.use_keyed_state(format!("form-picker-scroll-{kind}"), cx, |_, _| {
                gpui::ScrollHandle::new()
            });
        let scroll = scroll_state.read(cx).clone();
        focus.scroll = Some(scroll.clone());
        // Auth options are filtered; keep navigation on the rendered choices.
        focus.items = options
            .iter()
            .map(|(index, _)| focus.items[*index].clone())
            .collect();
        let selected = options
            .iter()
            .position(|(_, label)| *label == value)
            .unwrap_or(0);
        let initial_focus = focus.clone();
        let keyboard_focus = initial_focus.clone();
        let bounds = self.picker_bounds[kind].clone();
        let width = bounds.get().size.width.max(px(1.));
        div()
            .px(px(16.))
            .py(px(if editing { 12. } else { 13. }))
            .border_b_1()
            .border_color(t.divider)
            .flex()
            .flex_col()
            .gap(px(if editing { 5. } else { 4. }))
            .on_children_prepainted(move |children, _, cx| {
                if let Some(rect) = children.last() {
                    bounds.set(*rect);
                    crate::parity::record_bounds(&format!("menu-form-{kind}-trigger"), *rect, cx);
                }
            })
            .child(
                div()
                    .text_size(px(11.))
                    .line_height(px(13.))
                    .text_color(t.tertiary)
                    .child(label.clone())
                    .observe(format!("provider-edit-{kind}-label")),
            )
            .child(
                Popover::new(kind)
                    .on_position(move |position, trigger| track_origin.resolve(position, trigger))
                    .anchor(Anchor::TopLeft)
                    .offset(px(4.))
                    .track_focus(&focus.container)
                    .open(open)
                    .retain_content(motion.visible)
                    .on_open_change(cx.listener(move |this, open: &bool, window, cx| {
                        this.picker = (*open && !this.busy).then_some(kind);
                        if this.picker.is_some() {
                            initial_focus.focus_item(selected, window, cx);
                        }
                        cx.notify();
                    }))
                    .trigger(
                        Button::new((kind, 1000_usize))
                            .disabled(self.busy)
                            .track_focus(&focus.trigger)
                            .w_full()
                            .when(!editing, |el| {
                                el.h(px(28.))
                                    .rounded(px(4.))
                                    .border_1()
                                    .border_color(gpui::rgba(0))
                                    .shadow(vec![gpui_base::box_shadow(
                                        0.,
                                        -1.,
                                        0.,
                                        0.,
                                        t.border.opacity(0.58).into(),
                                    )
                                    .inset()])
                                    .when(open, |el| {
                                        el.bg(t.accent_soft.opacity(0.44)).shadow(vec![
                                            gpui_base::box_shadow(0., -2., 0., 0., t.accent.into())
                                                .inset(),
                                        ])
                                    })
                                    .styles(|s| s.disabled(|s| s.opacity(0.5)))
                                    .text_size(px(13.))
                                    .line_height(px(20.))
                                    .text_color(t.text)
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .cursor_pointer()
                                    .hover(move |s| {
                                        s.bg(if open {
                                            t.accent_soft.opacity(0.44)
                                        } else {
                                            t.surface2.opacity(0.56)
                                        })
                                        .shadow(vec![
                                            gpui_base::box_shadow(
                                                0.,
                                                if open { -2. } else { -1. },
                                                0.,
                                                0.,
                                                if open {
                                                    t.accent.into()
                                                } else {
                                                    t.border_strong.into()
                                                },
                                            )
                                            .inset(),
                                        ])
                                    })
                                    .focus_visible(move |s| {
                                        let mut shadows = t.outline(t.accent.opacity(0.58), 1., 1.);
                                        shadows.push(
                                            gpui_base::box_shadow(
                                                0.,
                                                if open { -2. } else { -1. },
                                                0.,
                                                0.,
                                                if open {
                                                    t.accent.into()
                                                } else {
                                                    t.border.opacity(0.58).into()
                                                },
                                            )
                                            .inset(),
                                        );
                                        s.shadow(shadows)
                                    })
                            })
                            .when(editing, |el| {
                                el.h(px(34.))
                                    .px(px(10.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(if open { t.accent } else { t.border })
                                    .bg(t.surface)
                                    .text_size(px(13.))
                                    .line_height(px(16.))
                                    .text_color(t.text)
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .cursor_pointer()
                                    .styles(|s| s.disabled(|s| s.opacity(0.5)))
                                    .when(open, |el| el.shadow(t.outline(t.focus_ring, 3., 0.)))
                                    .hover(move |s| {
                                        s.border_color(if open {
                                            t.accent
                                        } else {
                                            t.border_strong
                                        })
                                    })
                                    .focus_visible(move |s| {
                                        s.border_color(t.accent).shadow(t.outline(
                                            t.focus_ring,
                                            3.,
                                            0.,
                                        ))
                                    })
                            })
                            .accessibility_label(label)
                            .on_key_down(cx.listener(
                                move |this, event: &gpui::KeyDownEvent, window, cx| {
                                    if !this.busy
                                        && this.picker != Some(kind)
                                        && matches!(event.keystroke.key.as_str(), "down" | "up")
                                    {
                                        this.picker = Some(kind);
                                        let focus = keyboard_focus.clone();
                                        window.on_next_frame(move |window, cx| {
                                            focus.focus_item(selected, window, cx)
                                        });
                                        cx.stop_propagation();
                                        cx.notify();
                                    }
                                },
                            ))
                            .child(div().flex_1().min_w_0().text_ellipsis().child(value))
                            .child(svg().path("chevron-down").size(px(14.)).text_color(t.text)),
                    )
                    .content(move |_, _, _| {
                        let menu = focus.surface(
                            kind,
                            focus.trigger.clone(),
                            focus.trigger.clone(),
                            move |_, cx| {
                                let _ = close.update(cx, |form, cx| {
                                    form.picker = None;
                                    cx.notify();
                                });
                            },
                        );
                        let content = select::surface(menu, width, t)
                            .child(
                                div()
                                    .id("form-select-viewport")
                                    .flex_shrink_0()
                                    .max_h(px(278.))
                                    .overflow_y_scroll()
                                    .track_scroll(&scroll)
                                    .flex()
                                    .flex_col()
                                    .children(options.into_iter().enumerate().map(
                                        |(position, (index, label))| {
                                            let owner = owner.clone();
                                            let restore = focus.trigger.clone();
                                            select::item((kind, index), position == selected, t)
                                                .disabled(!open)
                                                .track_focus(&focus.items[position])
                                                .accessibility_label(label.clone())
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .text_ellipsis()
                                                        .child(label),
                                                )
                                                .child(select::indicator(position == selected, t))
                                                .on_click(move |_, window, cx| {
                                                    let _ = owner.update(cx, |form, cx| {
                                                        if form.busy || form.picker != Some(kind) {
                                                            return;
                                                        }
                                                        form.choose(kind, index, window, cx);
                                                        restore.focus(window, cx);
                                                    });
                                                })
                                                .observe(format!("menu-form-{kind}-option-{index}"))
                                        },
                                    )),
                            )
                            .observe(format!("menu-form-{kind}-content"));
                        motion
                            .wrap_positioned(crate::menu::select::fade(content, scroll, t), origin)
                    }),
            )
            .observe(format!("provider-edit-{kind}-row"))
            .into_any_element()
    }
}
