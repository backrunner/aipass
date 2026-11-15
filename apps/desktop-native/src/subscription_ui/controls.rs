use super::*;
use crate::parity::Observe;
use gpui::{div, px, svg, Anchor, AnyElement, Div, Focusable, FontWeight};
use gpui_base::{input::Input, Button, Popover};

impl Subscriptions {
    pub(super) fn button(
        &self,
        id: impl Into<gpui::ElementId>,
        label: impl Into<String>,
        icon: &'static str,
        primary: bool,
    ) -> Button {
        let t = self.theme;
        let label = label.into();
        let color = if primary { gpui::rgb(0xffffff) } else { t.text };
        crate::providers::controls::base_button(id, primary, t)
            .flex_shrink_0()
            .gap(px(6.))
            .accessibility_label(label.clone())
            .when(!icon.is_empty(), |el| {
                el.child(svg().path(icon).size(px(14.)).text_color(color))
            })
            .child(label)
    }
    pub(super) fn ghost_button(&self, mut button: Button) -> Button {
        let t = self.theme;
        button.interactivity().clear_pointer_styles();
        button
            .bg(gpui::rgba(0))
            .border_color(gpui::rgba(0))
            .hover(move |s| s.bg(t.accent_soft))
    }
    pub(super) fn icon_button(
        &self,
        id: impl Into<gpui::ElementId>,
        icon: &'static str,
        label: String,
        small: bool,
        danger: bool,
    ) -> Button {
        let t = self.theme;
        Button::new(id)
            .flex()
            .items_center()
            .justify_center()
            .flex_shrink_0()
            .size(px(if small { 26. } else { 32. }))
            .rounded(px(8.))
            .border_1()
            .border_color(gpui::rgba(0))
            .text_color(t.secondary)
            .styles(|s| s.disabled(|s| s.opacity(0.4)))
            .focus_visible(move |s| s.shadow(t.outline(t.focus_ring, 2., 1.)))
            .hover(move |s| {
                s.bg(if danger { t.danger_soft } else { t.accent_soft })
                    .text_color(if danger { t.danger } else { t.text })
            })
            .accessibility_label(label)
            .child(
                svg()
                    .path(icon)
                    .size(px(if small { 14. } else { 17. }))
                    .inherit_text_color(),
            )
    }
    pub(super) fn paragraph(&self, text: String, size: f32) -> Div {
        div()
            .text_size(px(size))
            .line_height(px(if size == 13. { 19.5 } else { size * 1.6 }))
            .text_color(self.theme.tertiary)
            .child(text)
    }
    pub(super) fn banner(&self, text: String, warning: bool) -> Div {
        let t = self.theme;
        div()
            .px(px(12.))
            .py(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(gpui::rgba(0))
            .bg(if warning {
                t.warning.opacity(if self.dark { 0.14 } else { 0.1 })
            } else {
                t.danger_soft
            })
            .text_size(px(12.))
            .line_height(px(16.8))
            .text_color(if warning { t.warning } else { t.danger })
            .child(text)
    }

    pub(super) fn field(
        &self,
        id: String,
        label: String,
        input: &Entity<InputState>,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let focused = input.read(cx).focus_handle(cx).is_focused(window);
        let disabled = input.read(cx).presentation().is_disabled();
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .w_full()
            .min_w_0()
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(15.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(self.theme.secondary)
                    .child(label),
            )
            .child(
                div()
                    .w_full()
                    .h(px(34.))
                    .px(px(10.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(self.theme.border)
                    .bg(self.theme.surface)
                    .when(focused, |el| {
                        el.border_color(self.theme.accent)
                            .shadow(self.theme.outline(self.theme.focus_ring, 3., 0.))
                    })
                    .when(disabled, |el| el.opacity(0.55))
                    .text_size(px(13.))
                    .line_height(px(16.))
                    .child(Input::new(input))
                    .observe(format!("{id}-input")),
            )
            .observe(id)
            .into_any_element()
    }
    pub(super) fn search(&self) -> AnyElement {
        div()
            .h(px(33.))
            .px(px(10.))
            .py(px(8.))
            .flex_shrink_0()
            .rounded(px(8.))
            .border_1()
            .border_color(self.theme.border)
            .bg(self.theme.surface)
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px(12.))
            .line_height(px(15.))
            .child(
                svg()
                    .path("search")
                    .size(px(14.))
                    .flex_shrink_0()
                    .text_color(self.theme.tertiary),
            )
            .child(div().flex_1().min_w_0().child(Input::new(&self.query)))
            .observe("subscription-search")
            .into_any_element()
    }
    pub(super) fn picker(
        &self,
        key: String,
        label: String,
        value: String,
        options: Vec<(String, String)>,
        render: (&mut Window, &mut Context<Self>),
    ) -> AnyElement {
        let (window, cx) = render;
        let t = self.theme;
        let owner = cx.entity().downgrade();
        let close = owner.clone();
        let observe_key = key.clone();
        let choose_key = key.clone();
        let arrow_key = key.clone();
        let mut focus = self.picker_focus.clone();
        if let Some(trigger) = self.picker_triggers.get(&key) {
            focus.trigger = trigger.clone();
        }
        focus.items.truncate(options.len().max(1));
        let selected_index = options
            .iter()
            .position(|(item, _)| item == &value)
            .unwrap_or(0);
        let initial_focus = focus.items[selected_index].clone();
        let keyboard_focus = initial_focus.clone();
        let disabled = self.busy()
            || self.import_busy()
            || (key.starts_with("runtime-")
                && (self.runtime_options.pending
                    || (key == "runtime-interval"
                        && self
                            .runtime_options
                            .value
                            .as_ref()
                            .is_some_and(|v| !v.quota_tracking))));
        let widths = self.picker_widths.clone();
        let measure_key = key.clone();
        let popup_width = widths.borrow().get(&key).copied().unwrap_or(px(300.));
        let selected = value.clone();
        let open = self.picker.as_deref() == Some(&key);
        let motion = crate::menu::motion::sample(format!("picker-motion-{key}"), open, window, cx);
        let origin = crate::menu::motion::Origin::new(false);
        let track_origin = origin.clone();
        let current = options
            .iter()
            .find(|(v, _)| v == &value)
            .map(|(_, label)| label.clone())
            .unwrap_or_default();
        div()
            .on_children_prepainted(move |bounds, _, cx| {
                if let Some(bounds) = bounds.last() {
                    crate::parity::record_bounds(
                        &format!("subscription-trigger-{measure_key}"),
                        *bounds,
                        cx,
                    );
                    widths
                        .borrow_mut()
                        .insert(measure_key.clone(), bounds.size.width);
                }
            })
            .flex()
            .flex_col()
            .gap(px(6.))
            .when(!label.is_empty(), |el| {
                el.child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(15.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.secondary)
                        .child(label.clone()),
                )
            })
            .child(
                Popover::new(format!("subscription-picker-{key}"))
                    .on_position(move |position, trigger| track_origin.resolve(position, trigger))
                    .anchor(Anchor::TopLeft)
                    .offset(px(4.))
                    .track_focus(&focus.container)
                    .open(open)
                    .retain_content(motion.visible)
                    .on_open_change(cx.listener(move |this, open: &bool, window, cx| {
                        this.picker = open.then(|| key.clone());
                        if *open {
                            initial_focus.focus(window, cx);
                        }
                        cx.notify();
                    }))
                    .trigger(
                        Button::new(format!("subscription-trigger-{choose_key}"))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(t.border)
                            .bg(t.surface)
                            .text_color(t.text)
                            .text_size(px(13.))
                            .cursor_pointer()
                            .w_full()
                            .h(px(34.))
                            .px(px(10.))
                            .gap(px(8.))
                            .font_weight(FontWeight::NORMAL)
                            .line_height(px(16.))
                            .styles(move |s| s.disabled(|s| s.opacity(0.5)))
                            .focus_visible(move |s| {
                                s.border_color(t.accent)
                                    .shadow(t.outline(t.focus_ring, 3., 0.))
                            })
                            .when(open, |el| {
                                el.border_color(t.accent)
                                    .shadow(t.outline(t.focus_ring, 3., 0.))
                            })
                            .hover(move |s| {
                                s.bg(t.surface).border_color(if open {
                                    t.accent
                                } else {
                                    t.border_strong
                                })
                            })
                            .justify_between()
                            .child(current)
                            .child(
                                svg()
                                    .path("chevron-down")
                                    .size(px(14.))
                                    .text_color(t.secondary),
                            )
                            .track_focus(&focus.trigger)
                            .accessibility_label(if label.is_empty() {
                                self.text("subscriptionImport.provider")
                            } else {
                                label
                            })
                            .disabled(disabled)
                            .on_key_down(cx.listener(
                                move |this, event: &gpui::KeyDownEvent, window, cx| {
                                    if !disabled
                                        && this.picker.as_deref() != Some(arrow_key.as_str())
                                        && matches!(event.keystroke.key.as_str(), "down" | "up")
                                    {
                                        this.picker = Some(arrow_key.clone());
                                        let focus = keyboard_focus.clone();
                                        // The controlled popover installs its focus scope in render.
                                        window.on_next_frame(move |window, cx| {
                                            focus.focus(window, cx)
                                        });
                                        cx.stop_propagation();
                                        cx.notify();
                                    }
                                },
                            )),
                    )
                    .content(move |_, _, _| {
                        let content = focus
                            .surface(
                                "subscription-picker-options",
                                focus.trigger.clone(),
                                focus.trigger.clone(),
                                move |_, cx| {
                                    let _ = close.update(cx, |this, cx| {
                                        this.picker = None;
                                        cx.notify();
                                    });
                                },
                            )
                            .w(popup_width)
                            .max_h(px(288.))
                            .overflow_y_scroll()
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
                            .children(options.into_iter().enumerate().map(
                                |(index, (value, label))| {
                                    let active = value == selected;
                                    let owner = owner.clone();
                                    let key = choose_key.clone();
                                    let restore = focus.trigger.clone();
                                    Button::new(("subscription-option", index))
                                        .disabled(!open)
                                        .role(gpui::Role::MenuItemRadio)
                                        .aria_selected(active)
                                        .track_focus(&focus.items[index])
                                        .min_h(px(32.))
                                        .px(px(8.))
                                        .py(px(6.))
                                        .rounded(px(6.))
                                        .text_size(px(13.))
                                        .line_height(px(20.))
                                        .text_color(t.text)
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .justify_between()
                                        .cursor_pointer()
                                        .accessibility_label(label.clone())
                                        .child(
                                            div().flex_1().min_w_0().text_ellipsis().child(label),
                                        )
                                        .child(
                                            div()
                                                .w(px(16.))
                                                .flex_shrink_0()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .child(
                                                    svg().path("check").size(px(14.)).text_color(
                                                        if active {
                                                            t.accent
                                                        } else {
                                                            gpui::rgba(0)
                                                        },
                                                    ),
                                                ),
                                        )
                                        .when(active, |el| el.bg(t.accent_soft))
                                        .hover(move |s| {
                                            s.bg(if active { t.accent_soft } else { t.surface2 })
                                                .shadow(vec![gpui_base::box_shadow(
                                                    0.,
                                                    0.,
                                                    0.,
                                                    1.,
                                                    t.focus_ring.into(),
                                                )
                                                .inset()])
                                        })
                                        .focus(move |s| {
                                            s.bg(if active { t.accent_soft } else { t.surface2 })
                                                .shadow(vec![gpui_base::box_shadow(
                                                    0.,
                                                    0.,
                                                    0.,
                                                    1.,
                                                    t.focus_ring.into(),
                                                )
                                                .inset()])
                                        })
                                        .on_click(move |_, window, cx| {
                                            let _ = owner.update(cx, |this, cx| {
                                                if key.starts_with("runtime-") {
                                                    this.options_choice(&key, &value, window, cx);
                                                } else if key == "method" {
                                                    if let Ok(method) = value.parse() {
                                                        this.method = method;
                                                        this.setup_inputs(window, cx);
                                                    }
                                                } else if key == "import-provider" {
                                                    this.import_provider = value.clone();
                                                } else {
                                                    this.choices.insert(key.clone(), value.clone());
                                                }
                                                this.picker = None;
                                                restore.focus(window, cx);
                                                cx.notify();
                                            });
                                        })
                                        .observe(format!("subscription-option-{index}"))
                                },
                            ))
                            .observe("subscription-picker-menu");
                        motion.wrap_positioned(content, origin)
                    }),
            )
            .observe(format!("subscription-picker-field-{}", observe_key))
            .into_any_element()
    }
}
