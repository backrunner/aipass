use super::*;
use crate::parity::Observe;
use crate::providers::{
    controls::{button, icon},
    presentation,
};
use aipass_desktop_native::credentials::INTERFACES;
use gpui::{div, px, svg, Anchor, AnyElement, BoxShadow, Div};
use gpui_base::{input::Input, Button, Popover};

impl Credentials {
    fn input(&self, index: usize, window: &Window, cx: &Context<Self>) -> Div {
        let t = self.theme;
        let focused = self.inputs[index]
            .read(cx)
            .focus_handle(cx)
            .is_focused(window);
        div()
            .relative()
            .min_w_0()
            .h(px(if index == 2 || index >= 5 { 34. } else { 32. }))
            .px(px(10.))
            .rounded(px(8.))
            .border_1()
            .border_color(if focused { t.accent } else { t.border })
            .bg(t.surface)
            .when(focused, |el| {
                el.shadow(vec![
                    BoxShadow::new(px(0.), px(0.), t.focus_ring.into()).spread_radius(px(3.))
                ])
            })
            .text_size(px(13.))
            .line_height(px(20.))
            .when(
                index == 1 && self.editor.as_ref().is_some_and(Option::is_some),
                |el| el.pr(px(34.)),
            )
            .when(
                index == 1 && !self.visible && !self.inputs[index].read(cx).value().is_empty(),
                |el| el.font_family("AIPass Password Dots"),
            )
            .child(Input::new(&self.inputs[index]))
            .when(
                index == 1 && self.editor.as_ref().is_some_and(Option::is_some),
                |el| {
                    el.child(
                        icon(
                            "credential-value-eye",
                            if self.visible { "eye-off" } else { "eye" },
                            self.locale.text(if self.visible {
                                "providerForm.hideApiKey"
                            } else {
                                "providerForm.showApiKey"
                            }),
                            t,
                        )
                        .absolute()
                        .right(px(4.))
                        .top(px(2.))
                        .size(px(26.))
                        .disabled(self.busy())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.visible = !this.visible;
                            this.inputs[1].update(cx, |input, cx| {
                                input.set_masked(!this.visible, window, cx)
                            });
                            cx.notify();
                        })),
                    )
                },
            )
    }

    fn field(&self, index: usize, window: &Window, cx: &Context<Self>) -> Div {
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(15.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(self.theme.secondary)
                    .child(self.locale.text(LABELS[index])),
            )
            .child(self.input(index, window, cx))
    }

    fn picker(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::menu::select;
        let t = self.theme;
        let mut focus = self.picker_focus.clone();
        let scroll_state = window.use_keyed_state("credential-picker-scroll", cx, |_, _| {
            gpui::ScrollHandle::new()
        });
        let scroll = scroll_state.read(cx).clone();
        focus.scroll = Some(scroll.clone());
        let before = self.inputs[1].read(cx).focus_handle(cx);
        let after = self.inputs[2].read(cx).focus_handle(cx);
        let owner = cx.entity().downgrade();
        let close = owner.clone();
        let selected = INTERFACES
            .iter()
            .position(|i| *i == self.interface)
            .unwrap_or(0);
        let initial_focus = focus.clone();
        let keyboard_focus = initial_focus.clone();
        let options: Vec<_> = INTERFACES
            .iter()
            .map(|i| self.locale.text(presentation::protocol_key(i)))
            .collect();
        let open = self.picker_open && !self.busy();
        let motion = crate::menu::motion::sample("credential-picker-motion", open, window, cx);
        let origin = crate::menu::motion::Origin::new(false);
        let track_origin = origin.clone();
        let bounds = self.picker_bounds.clone();
        let width = bounds.get().size.width.max(px(1.));
        div()
            .w_full()
            .on_children_prepainted(move |children, _, cx| {
                if let Some(rect) = children.first() {
                    bounds.set(*rect);
                    crate::parity::record_bounds("menu-credential-trigger", *rect, cx);
                }
            })
            .child(
                Popover::new("credential-interface-picker")
                    .on_position(move |position, trigger| track_origin.resolve(position, trigger))
                    .track_focus(&self.picker_focus.container)
                    .anchor(Anchor::TopLeft)
                    .offset(px(4.))
                    .open(open)
                    .retain_content(motion.visible)
                    .on_open_change(cx.listener(move |this, open: &bool, window, cx| {
                        this.picker_open = *open && !this.busy();
                        if this.picker_open {
                            initial_focus.focus_item(selected, window, cx);
                        }
                        cx.notify();
                    }))
                    .trigger(
                        Button::new("credential-interface")
                            .track_focus(&self.picker_focus.trigger)
                            .disabled(self.busy())
                            .w_full()
                            .h(px(34.))
                            .px(px(10.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(t.border)
                            .bg(t.surface)
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(13.))
                            .line_height(px(16.))
                            .text_color(t.text)
                            .hover(move |s| {
                                s.border_color(if open { t.accent } else { t.border_strong })
                            })
                            .focus_visible(move |s| {
                                s.border_color(t.accent)
                                    .shadow(t.outline(t.focus_ring, 3., 0.))
                            })
                            .when(open, |el| {
                                el.border_color(t.accent)
                                    .shadow(t.outline(t.focus_ring, 3., 0.))
                            })
                            .accessibility_label(self.locale.text("providerDetail.keyFormat"))
                            .on_key_down(cx.listener(
                                move |this, event: &gpui::KeyDownEvent, window, cx| {
                                    if !this.busy()
                                        && !this.picker_open
                                        && matches!(event.keystroke.key.as_str(), "down" | "up")
                                    {
                                        this.picker_open = true;
                                        let focus = keyboard_focus.clone();
                                        window.on_next_frame(move |window, cx| {
                                            focus.focus_item(selected, window, cx)
                                        });
                                        cx.stop_propagation();
                                        cx.notify();
                                    }
                                },
                            ))
                            .child(
                                div().flex_1().min_w_0().text_ellipsis().child(
                                    self.locale
                                        .text(presentation::protocol_key(&self.interface)),
                                ),
                            )
                            .child(
                                svg()
                                    .path("chevron-down")
                                    .size(px(14.))
                                    .text_color(t.tertiary),
                            ),
                    )
                    .content(move |_, _, _| {
                        let menu =
                            focus.surface("credential-interfaces", after, before, move |_, cx| {
                                let _ = close.update(cx, |this, cx| {
                                    this.picker_open = false;
                                    cx.notify();
                                });
                            });
                        let content = select::surface(menu, width, t)
                            .child(
                                div()
                                    .id("credential-select-viewport")
                                    .flex_shrink_0()
                                    .max_h(px(278.))
                                    .overflow_y_scroll()
                                    .track_scroll(&scroll)
                                    .flex()
                                    .flex_col()
                                    .children(options.into_iter().enumerate().map(
                                        |(index, label)| {
                                            let owner = owner.clone();
                                            select::item(
                                                ("credential-interface-option", index),
                                                index == selected,
                                                t,
                                            )
                                            .disabled(!open)
                                            .track_focus(&focus.items[index])
                                            .accessibility_label(label.clone())
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .text_ellipsis()
                                                    .child(label),
                                            )
                                            .child(select::indicator(index == selected, t))
                                            .on_click(move |_, window, cx| {
                                                let _ = owner.update(cx, |this, cx| {
                                                    if this.busy() || !this.picker_open {
                                                        return;
                                                    }
                                                    this.interface = INTERFACES[index].clone();
                                                    this.picker_open = false;
                                                    this.picker_focus.trigger.focus(window, cx);
                                                    cx.notify();
                                                });
                                            })
                                            .observe(format!("menu-credential-option-{index}"))
                                        },
                                    )),
                            )
                            .observe("menu-credential-content");
                        motion
                            .wrap_positioned(crate::menu::select::fade(content, scroll, t), origin)
                    }),
            )
    }

    fn editor_view(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme;
        let editing = self.editor.as_ref().is_some_and(Option::is_some);
        let billing: Vec<_> = (5..8)
            .filter_map(|index| {
                let value = self.inputs[index].read(cx).value();
                let value = value.trim();
                let value = if index == 5 {
                    value.trim_end_matches(['x', 'X', '×']).trim()
                } else {
                    value
                };
                (!value.is_empty()).then(|| {
                    if index == 5 {
                        format!("{value}×")
                    } else {
                        value.to_owned()
                    }
                })
            })
            .collect();
        let can_save = !self.busy()
            && !self.inputs[0].read(cx).value().trim().is_empty()
            && !self.inputs[1].read(cx).value().trim().is_empty();
        div()
            .on_children_prepainted(crate::parity::record(&[
                "credential-label-input",
                "credential-key-row",
                "credential-protocol-row",
                "credential-overrides",
                "credential-inherit-hint",
                "credential-billing-panel",
            ]))
            .id("credential-editor")
            .w_full()
            .flex()
            .flex_col()
            .gap(px(8.))
            .min_w_0()
            .child(self.input(0, window, cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().flex_1().min_w_0().child(self.input(1, window, cx)))
                    .child(
                        button("credential-save", self.locale.text("common.save"), false, t)
                            .h(px(26.))
                            .px(px(9.))
                            .text_size(px(12.))
                            .opacity(if can_save { 1. } else { 0.45 })
                            .disabled(!can_save)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.save(cx);
                            })),
                    )
                    .child(
                        icon(
                            "credential-cancel",
                            "undo-2",
                            self.locale.text("common.cancel"),
                            t,
                        )
                        .size(px(24.))
                        .on_click(cx.listener(|this, _, window, cx| this.cancel(window, cx))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(div().flex_1().min_w_0().child(self.picker(window, cx)))
                    .child(div().flex_1().min_w_0().child(self.input(2, window, cx))),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(self.field(3, window, cx))
                    .child(self.field(4, window, cx)),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .line_height(px(13.))
                    .text_color(t.tertiary)
                    .child(self.locale.text("credential.inheritDefaults")),
            )
            .when(editing, |el| {
                el.child(
                    div()
                        .mt(px(2.))
                        .rounded(px(8.))
                        .overflow_hidden()
                        .border_1()
                        .border_color(t.border)
                        .bg(t.surface)
                        .child(
                            Button::new("credential-billing")
                                .aria_expanded(self.billing_open)
                                .disabled(self.busy())
                                .w_full()
                                .px(px(12.))
                                .py(px(10.))
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .text_size(px(12.))
                                .line_height(px(17.))
                                .border_b_1()
                                .border_color(gpui::rgba(0))
                                .accessibility_label(
                                    self.locale.text("credential.billingReference"),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_left()
                                        .child(self.locale.text("credential.billingReference")),
                                )
                                .child(
                                    div()
                                        .min_w_0()
                                        .max_w(gpui::relative(0.45))
                                        .overflow_hidden()
                                        .flex()
                                        .items_center()
                                        .gap(px(6.))
                                        .text_size(px(11.))
                                        .text_color(t.tertiary)
                                        .opacity(if self.billing_open { 0. } else { 1. })
                                        .children(billing.into_iter().map(|value| {
                                            div().min_w_0().text_ellipsis().child(value)
                                        })),
                                )
                                .child(self.disclosure_frame.chevron(t.tertiary, 15.))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.billing_open = !this.billing_open;
                                    cx.notify();
                                })),
                        )
                        .child(
                            self.disclosure_frame.body(
                                "credential-billing-content",
                                div()
                                    .p(px(12.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .flex()
                                            .gap(px(10.))
                                            .child(self.field(5, window, cx))
                                            .child(self.field(6, window, cx)),
                                    )
                                    .child(self.field(7, window, cx)),
                            ),
                        ),
                )
            })
            .into_any_element()
    }

    fn row(&self, secret: &SecretRef, index: usize, cx: &Context<Self>) -> Div {
        let t = self.theme;
        let id = secret.id.clone();
        let edit = secret.clone();
        let remove_id = id.clone();
        let eye_id = id.clone();
        let revealed = self.revealed.get(&id);
        let label = self
            .locale
            .text(if revealed.is_some() {
                "providerDetail.hideSecret"
            } else {
                "providerDetail.revealSecret"
            })
            .replace("{label}", &secret.label);
        let entry = self.entry.as_ref().unwrap();
        let format = secret
            .interface_type
            .as_ref()
            .unwrap_or(&entry.interface_type);
        let format = match format {
            InterfaceType::OpenAiCompatible => "OpenAI",
            InterfaceType::AnthropicMessages => "Anthropic",
            InterfaceType::Gemini => "Gemini",
            InterfaceType::AzureOpenAi => "Azure OpenAI",
            InterfaceType::Bedrock => "Bedrock",
            InterfaceType::CustomHttp => "HTTP",
        };
        let group = secret
            .group
            .as_ref()
            .or_else(|| entry.gateway.as_ref().and_then(|g| g.group.as_ref()));
        div()
            .relative()
            .px(px(16.))
            .py(px(12.))
            .bg(t.surface2)
            .border_b_1()
            .border_color(t.divider)
            .flex()
            .flex_col()
            .gap(px(5.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(11.))
                    .line_height(px(13.))
                    .text_color(t.tertiary)
                    .child(svg().path("key-round").size(px(13.)).text_color(t.tertiary))
                    .child(div().min_w_0().text_ellipsis().child(secret.label.clone())),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .min_w_0()
                    .child(if let Some(value) = revealed {
                        div()
                            .id(("credential-visible", index))
                            .track_scroll(&value.scroll)
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .h(px(30.))
                            .px(px(8.))
                            .py(px(4.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(t.border)
                            .bg(t.surface)
                            .overflow_x_scroll()
                            .overflow_y_hidden()
                            .text_size(px(13.))
                            .line_height(px(20.))
                            .font_family("Menlo")
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .whitespace_nowrap()
                                    .child(value.value.expose().to_owned()),
                            )
                            .into_any_element()
                    } else {
                        Button::new(("credential-reveal-value", index))
                            .flex_1()
                            .min_w_0()
                            .h(px(28.))
                            .disabled(self.busy())
                            .text_size(px(13.))
                            .justify_start()
                            .accessibility_label(label.clone())
                            .child(
                                div()
                                    .w_full()
                                    .min_w_0()
                                    .text_ellipsis()
                                    .font_family("AIPass Password Dots")
                                    .child("••••••••••••••••"),
                            )
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.reveal(id.clone(), cx)),
                            )
                            .into_any_element()
                    })
                    .child(
                        div()
                            .flex()
                            .gap(px(4.))
                            .flex_shrink_0()
                            .when(self.editing_provider, |el| {
                                el.child(
                                    icon(
                                        ("credential-remove", index),
                                        "trash-2",
                                        self.locale.text("providerDetail.removeKey"),
                                        t,
                                    )
                                    .size(px(28.))
                                    .disabled(self.busy())
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| this.remove(remove_id.clone(), cx),
                                    )),
                                )
                            })
                            .child(
                                icon(
                                    ("credential-edit", index),
                                    "pencil",
                                    self.locale.text("providerDetail.editKey"),
                                    t,
                                )
                                .size(px(28.))
                                .disabled(self.busy())
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.begin(Some(edit.clone()), window, cx)
                                    },
                                )),
                            )
                            .child(
                                icon(
                                    ("credential-eye", index),
                                    if revealed.is_some() { "eye-off" } else { "eye" },
                                    label,
                                    t,
                                )
                                .size(px(28.))
                                .disabled(self.busy())
                                .on_click(cx.listener(
                                    move |this, _, _, cx| this.reveal(eye_id.clone(), cx),
                                )),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .pt(px(3.))
                    .child(
                        div()
                            .px(px(8.))
                            .py(px(2.))
                            .rounded_full()
                            .border_1()
                            .border_color(t.accent.opacity(0.24))
                            .bg(t.accent_soft)
                            .text_color(t.accent)
                            .text_size(px(10.))
                            .line_height(px(15.))
                            .child(format),
                    )
                    .when_some(group, |el, group| {
                        el.child(
                            div()
                                .min_w_0()
                                .px(px(8.))
                                .py(px(2.))
                                .rounded_full()
                                .border_1()
                                .border_color(t.border)
                                .text_color(t.secondary)
                                .text_size(px(10.))
                                .line_height(px(15.))
                                .text_ellipsis()
                                .child(group.clone()),
                        )
                    }),
            )
    }
}

impl Render for Credentials {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.disclosure_frame = self.disclosure.sample(self.billing_open, window, cx);
        if self.entry.is_none() {
            return div();
        }
        let t = self.theme;
        div()
            .w_full()
            .flex_shrink_0()
            .min_w_0()
            .flex()
            .flex_col()
            .when(!self.error.is_empty(), |el| {
                el.child(
                    div()
                        .px(px(16.))
                        .py(px(10.))
                        .bg(t.danger_soft)
                        .text_color(t.danger)
                        .text_size(px(12.))
                        .child(self.error.clone()),
                )
            })
            .when(!self.notice.is_empty(), |el| {
                el.child(
                    div()
                        .px(px(16.))
                        .py(px(10.))
                        .text_color(t.success)
                        .text_size(px(12.))
                        .child(self.notice.clone()),
                )
            })
            .children(
                self.entry
                    .iter()
                    .flat_map(|entry| &entry.secret_refs)
                    .enumerate()
                    .map(|(index, secret)| {
                        if self.editor.as_ref().and_then(Option::as_deref)
                            == Some(secret.id.as_str())
                        {
                            div()
                                .px(px(16.))
                                .py(px(12.))
                                .bg(t.surface2)
                                .border_b_1()
                                .border_color(t.divider)
                                .child(self.editor_view(window, cx))
                                .into_any_element()
                        } else {
                            self.row(secret, index, cx).into_any_element()
                        }
                    }),
            )
            .child(
                div()
                    .flex()
                    .px(px(16.))
                    .py(px(12.))
                    .border_b_1()
                    .border_color(t.divider)
                    .child(if self.editor == Some(None) {
                        self.editor_view(window, cx).into_any_element()
                    } else {
                        Button::new("credential-add")
                            .disabled(self.busy())
                            .h(px(26.))
                            .px(px(10.))
                            .rounded_full()
                            .border_1()
                            .border_color(t.border)
                            .bg(t.surface)
                            .text_color(t.secondary)
                            .text_size(px(12.))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .accessibility_label(self.locale.text("providerDetail.addKey"))
                            .child(svg().path("plus").size(px(12.)).text_color(t.secondary))
                            .child(self.locale.text("providerDetail.addKey"))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.begin(None, window, cx)),
                            )
                            .into_any_element()
                    }),
            )
    }
}
