//! Inline edit controls use the shared SwitchField presentation and motion.
use super::Form;
use crate::parity::Observe;
use gpui::{div, prelude::*, px, AnyElement, Context, FontWeight, Window};
use gpui_base::Switch;

impl Form {
    pub(super) fn edit_notes(&self, focused: bool, cx: &Context<Self>) -> AnyElement {
        let t = self.theme;
        div()
            .relative()
            .flex_shrink_0()
            .mx(px(16.))
            .my(px(10.))
            .py(px(4.))
            .h(self.notes_height)
            .border_b_1()
            .border_color(if focused { t.accent } else { t.border })
            .text_size(px(12.))
            .line_height(px(19.2))
            .child(gpui_base::input::Textarea::new(&self.notes))
            .child(
                div()
                    .id("provider-edit-notes-resize")
                    .absolute()
                    .bottom_0()
                    .right_0()
                    .size(px(12.))
                    .when(!self.busy, |el| el.cursor(gpui::CursorStyle::ResizeUpDown))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                            if !this.busy {
                                this.notes_resize_start =
                                    Some((event.position.y, this.notes_height));
                                cx.stop_propagation();
                            }
                        }),
                    )
                    .when(!self.busy, |el| {
                        el.on_drag(ResizeNotes(cx.entity_id()), |drag, _, _, cx| {
                            cx.stop_propagation();
                            cx.new(|_| drag.clone())
                        })
                    })
                    .on_drag_move(cx.listener(
                        |this, event: &gpui::DragMoveEvent<ResizeNotes>, _, cx| {
                            if !this.busy && event.drag(cx).0 == cx.entity_id() {
                                if let Some((y, height)) = this.notes_resize_start {
                                    this.notes_height =
                                        (height + event.event.position.y - y).max(px(64.));
                                    cx.notify();
                                }
                            }
                        },
                    ))
                    .children([3., 7.].map(|size| {
                        div()
                            .absolute()
                            .bottom(px(1.))
                            .right(px(1.))
                            .size(px(size))
                            .border_b_1()
                            .border_r_1()
                            .border_color(t.tertiary.opacity(0.5))
                    }))
                    .observe("provider-edit-notes-resize"),
            )
            .observe("provider-edit-notes")
            .into_any_element()
    }

    pub(super) fn concurrency_input(&self, cx: &Context<Self>) -> AnyElement {
        let t = self.theme;
        div()
            .id("provider-concurrency-input")
            .flex()
            .items_center()
            .size_full()
            // Keep one accessible/focusable editor. The arrows only operate it.
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(gpui_base::input::Input::new(&self.inputs[9])),
            )
            .child(
                div()
                    .relative()
                    .w(px(12.))
                    .h(px(15.))
                    .rounded(px(6.))
                    .overflow_hidden()
                    .bg(if self.dark {
                        gpui::rgb(0xffffff)
                    } else {
                        gpui::rgb(0)
                    }
                    .opacity(12. / 255.))
                    .flex()
                    .flex_col()
                    .children([true, false].into_iter().map(|up| {
                        gpui_base::Button::new(if up {
                            "provider-concurrency-increment"
                        } else {
                            "provider-concurrency-decrement"
                        })
                        .focusable(false)
                        .disabled(self.busy)
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .accessibility_label(format!(
                            "{} {}1",
                            self.locale.text("providerForm.maxConcurrentRequests"),
                            if up { "+" } else { "−" }
                        ))
                        .flex()
                        .items_center()
                        .justify_center()
                        .hover(move |s| s.bg(t.border))
                        .child(
                            gpui::svg()
                                .path(if up { "chevron-up" } else { "chevron-down" })
                                .size(px(13.))
                                .flex_shrink_0()
                                .text_color(if self.dark {
                                    gpui::rgb(0xdddddf)
                                } else {
                                    gpui::rgb(0x252525)
                                }),
                        )
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                this.start_concurrency_repeat(up, window, cx);
                            }),
                        )
                        .on_mouse_up(
                            gpui::MouseButton::Left,
                            cx.listener(|this, _, _, _| this.concurrency_repeat = None),
                        )
                        .on_mouse_up_out(
                            gpui::MouseButton::Left,
                            cx.listener(|this, _, _, _| this.concurrency_repeat = None),
                        )
                        .on_click(
                            cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                                if event.is_keyboard() {
                                    this.step_concurrency(up, window, cx);
                                }
                            }),
                        )
                        .observe(if up {
                            "provider-concurrency-increment"
                        } else {
                            "provider-concurrency-decrement"
                        })
                    }))
                    .child(
                        div()
                            .absolute()
                            .left(px(2.))
                            .top(px(7.))
                            .w(px(8.))
                            .h(px(1.))
                            .bg(if self.dark {
                                gpui::rgb(0x343944)
                            } else {
                                gpui::rgb(0xdbdbdb)
                            }),
                    ),
            )
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                let mods = event.keystroke.modifiers;
                if this.busy || mods.alt || mods.platform || mods.control {
                    return;
                }
                let up = match event.keystroke.key.as_str() {
                    "up" => true,
                    "down" => false,
                    _ => return,
                };
                cx.stop_propagation();
                this.step_concurrency(up, window, cx);
            }))
            .into_any_element()
    }

    fn step_concurrency(&mut self, up: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.inputs[9].update(cx, |input, cx| {
            input.focus(window, cx);
            let current = input
                .value()
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite());
            let value = current.unwrap_or(0.);
            // HTML step=1 aligns fractional values in the direction of travel.
            let next = if up {
                value.floor() + 1.
            } else {
                value.ceil() - 1.
            }
            .clamp(0., f64::from(u32::MAX));
            if current.is_none() || (up && next > value) || (!up && next < value) {
                input.replace_all(format!("{next:.0}"), window, cx);
            }
        });
    }

    fn start_concurrency_repeat(&mut self, up: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.step_concurrency(up, window, cx);
        self.concurrency_repeat = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(500))
                .await;
            loop {
                let keep_going = this
                    .update_in(cx, |this, window, cx| {
                        if this.busy || !window.is_window_active() {
                            return false;
                        }
                        this.step_concurrency(up, window, cx);
                        true
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(50))
                    .await;
            }
        }));
    }

    pub(super) fn edit_websocket(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme;
        let progress = gpui_base::motion::transition(
            "provider-edit-websocket-motion",
            if self.draft.websockets { 1.0_f32 } else { 0.0 },
            gpui_base::Transition::new(std::time::Duration::from_millis(150)).ease(|time| {
                aipass_desktop_native::motion::cubic_bezier(f64::from(time), 0.25, 0.1, 0.25, 1.0)
                    as f32
            }),
            window,
            cx,
        );
        let owner = cx.entity().downgrade();
        let label = self.locale.text("providerForm.supportsWebsockets");
        div()
            .px(px(16.))
            .py(px(12.))
            .border_b_1()
            .border_color(t.divider)
            .child(
                div()
                    .min_h(px(36.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(13.))
                            .line_height(px(16.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(label.clone()),
                    )
                    .child(
                        Switch::new("provider-edit-websocket")
                            .checked(self.draft.websockets)
                            .disabled(self.busy)
                            .accessibility_label(label)
                            .styles(|s| s.disabled(|s| s.opacity(0.5)))
                            .focus_visible(move |s| s.shadow(t.outline(t.accent, 2., 3.)))
                            .w(px(36.))
                            .h(px(20.))
                            .p(px(2.))
                            .rounded_full()
                            .bg(gpui::Rgba {
                                r: t.border.r + (t.accent.r - t.border.r) * progress,
                                g: t.border.g + (t.accent.g - t.border.g) * progress,
                                b: t.border.b + (t.accent.b - t.border.b) * progress,
                                a: t.border.a + (t.accent.a - t.border.a) * progress,
                            })
                            .child(
                                div()
                                    .size(px(16.))
                                    .ml(px(16. * progress))
                                    .rounded_full()
                                    .bg(gpui::rgb(0xffffff))
                                    .shadow(vec![gpui_base::box_shadow(
                                        0.,
                                        1.,
                                        crate::theme::shadow_blur(3.),
                                        0.,
                                        gpui::black().opacity(0.15),
                                    )]),
                            )
                            .on_change(move |value, _, _, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    this.draft.websockets = value;
                                    this.draft.websocket_touched = true;
                                    cx.notify();
                                });
                            })
                            .observe("provider-edit-websocket"),
                    ),
            )
            .observe("provider-edit-websocket-row")
            .into_any_element()
    }
}

#[derive(Clone)]
struct ResizeNotes(gpui::EntityId);
impl Render for ResizeNotes {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
