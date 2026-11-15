use super::{form::LABELS, Form};
use crate::parity::Observe;
use gpui::{
    canvas, div, point, prelude::*, px, svg, Anchor, AnyElement, BoxShadow, Context, Window,
};
use gpui_base::{input::Input, Button, Popup};

gpui::actions!(provider_fields, [Activate]);
struct KeysRegistered;
impl gpui::Global for KeysRegistered {}
pub(super) fn init(cx: &mut gpui::App) {
    if cx.has_global::<KeysRegistered>() {
        return;
    }
    cx.set_global(KeysRegistered);
    cx.bind_keys([
        gpui::KeyBinding::new("enter", Activate, Some("AIPassFieldChoice")),
        gpui::KeyBinding::new("space", Activate, Some("AIPassFieldChoice")),
    ]);
}

// Group and order match ProviderFormFields.svelte. Billing is one menu item,
// backed by the three rate/currency/unit-price inputs (13..=15).
pub(super) const OPTIONAL: [usize; 10] = [3, 4, 5, 6, 7, 8, 10, 11, 12, 13];

impl Form {
    fn toggle_field_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let opening = self.picker != Some("add-field");
        self.picker = opening.then_some("add-field");
        if opening {
            self.field_menu.container.focus(window, cx);
        } else {
            self.field_menu.trigger.focus(window, cx);
        }
        cx.notify();
    }
    pub(super) fn has_optional_fields(&self) -> bool {
        OPTIONAL.iter().any(|index| !self.visible.contains(index))
    }

    pub(super) fn remove_optional(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        for index in if index == 13 { 13..=15 } else { index..=index } {
            self.inputs[index].update(cx, |input, cx| input.set_value("", window, cx));
        }
        if index == 6 {
            self.notes
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.visible.remove(&index);
        self.field_menu.trigger.focus(window, cx);
        cx.notify();
    }

    pub(super) fn add_field_menu(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme;
        let open = self.picker == Some("add-field");
        let mut focus = self.field_menu.clone();
        let options: Vec<_> = OPTIONAL
            .into_iter()
            .filter(|i| !self.visible.contains(i))
            .map(|i| {
                (
                    i,
                    self.locale.text(if i == 13 {
                        "providerForm.billing"
                    } else {
                        LABELS[i]
                    }),
                )
            })
            .collect();
        focus.items = options
            .iter()
            .map(|(i, _)| self.field_menu.items[*i].clone())
            .collect();
        let item_focus = self.field_menu.items.clone();
        let headings = [
            self.locale.text("providerForm.details"),
            self.locale.text("providerForm.advanced"),
        ];
        let owner = cx.entity().downgrade();
        let close = owner.clone();
        let trigger = self.field_menu.trigger.clone();
        let bounds = self.field_menu_bounds.get();
        let groups = [false, true]
            .into_iter()
            .filter(|advanced| options.iter().any(|(i, _)| (*i >= 7) == *advanced))
            .count();
        let menu_height = px((14 + groups * 29 + options.len() * 32) as f32).min(px(380.));
        let below = window.viewport_size().height - bounds.bottom() - px(14.);
        let above = bounds.top() - px(14.);
        let anchor = if menu_height > below && above > below {
            Anchor::BottomLeft
        } else {
            Anchor::TopLeft
        };
        let maximum = px(380.)
            .min(if anchor == Anchor::BottomLeft {
                above
            } else {
                below
            })
            .max(px(32.));
        let measured = self.field_menu_bounds.clone();
        let popup = Popup::new(
            "add-field",
            Button::new("provider-add-field")
                .aria_expanded(self.picker == Some("add-field"))
                .track_focus(&self.field_menu.trigger)
                .disabled(self.busy)
                .h(px(33.))
                .px(px(12.))
                .rounded(px(6.))
                .border_1()
                .border_color(t.border)
                .bg(t.surface)
                .text_color(t.accent)
                .text_size(px(12.))
                .line_height(px(15.))
                .flex()
                .items_center()
                .gap(px(8.))
                .cursor_pointer()
                .hover(move |s| s.bg(t.accent_soft))
                .focus(move |s| s.bg(t.accent_soft))
                .accessibility_label(self.locale.text("providerForm.addField"))
                .child(svg().path("plus").size(px(14.)).text_color(t.accent))
                .child(self.locale.text("providerForm.addField"))
                .child(
                    svg()
                        .path("chevron-down")
                        .size(px(14.))
                        .text_color(t.accent),
                )
                .child(
                    canvas(
                        move |bounds, window, _| {
                            if measured.replace(bounds) != bounds {
                                window.request_animation_frame();
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0()
                    .size_full(),
                )
                .key_context("AIPassFieldChoice")
                .on_action(
                    cx.listener(|this, _: &Activate, window, cx| {
                        this.toggle_field_menu(window, cx)
                    }),
                )
                .on_click(cx.listener(|this, _, window, cx| this.toggle_field_menu(window, cx))),
        )
        .anchor(anchor)
        .offset(px(6.));
        // The frozen provider-field-menu has no enter/exit animation. Its
        // advanced field disclosure animates independently.
        if !open {
            return popup.into_any_element();
        }
        let content = focus
            .surface(
                "provider-field-menu",
                trigger.clone(),
                trigger,
                move |_, cx| {
                    let _ = close.update(cx, |this, cx| {
                        this.picker = None;
                        cx.notify();
                    });
                },
            )
            .w(px(220.))
            .max_h(maximum)
            .overflow_y_scroll()
            .p(px(6.))
            .rounded(px(8.))
            .border_1()
            .border_color(t.border)
            .bg(t.surface)
            .text_color(t.text)
            .flex()
            .flex_col()
            .shadow(vec![BoxShadow {
                inset: false,
                color: t.popup_shadow.into(),
                offset: point(px(0.), px(4.)),
                blur_radius: crate::theme::shadow_blur(16.),
                spread_radius: px(0.),
            }])
            .children(
                headings
                    .into_iter()
                    .enumerate()
                    .filter_map(|(group, heading)| {
                        let fields: Vec<_> = options
                            .iter()
                            .filter(|(i, _)| (*i >= 7) == (group == 1))
                            .collect();
                        if fields.is_empty() {
                            return None;
                        }
                        Some(
                            div()
                                .flex_shrink_0()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .p(px(8.))
                                        .text_size(px(11.))
                                        .line_height(px(13.))
                                        .text_color(t.tertiary)
                                        .child(heading)
                                        .observe(format!("provider-field-menu-heading-{group}")),
                                )
                                .children(fields.into_iter().map(|(index, label)| {
                                    let index = *index;
                                    let owner = owner.clone();
                                    let keyboard = owner.clone();
                                    Button::new(("provider-field-option", index))
                                        .disabled(!open)
                                        .track_focus(&item_focus[index])
                                        .role(gpui::Role::MenuItem)
                                        .w_full()
                                        .p(px(8.))
                                        .rounded(px(4.))
                                        .text_size(px(13.))
                                        .line_height(px(16.))
                                        .justify_start()
                                        .text_color(t.text)
                                        .cursor_pointer()
                                        .hover(move |s| s.bg(t.accent_soft))
                                        .focus(move |s| s.bg(t.accent_soft))
                                        .accessibility_label(label.clone())
                                        .child(label.clone())
                                        .key_context("AIPassFieldChoice")
                                        .on_action(move |_: &Activate, window, cx| {
                                            if !open {
                                                return;
                                            }
                                            let _ = keyboard.update(cx, |form, cx| {
                                                form.choose("add-field", index, window, cx)
                                            });
                                        })
                                        .on_click(move |_, window, cx| {
                                            let _ = owner.update(cx, |form, cx| {
                                                form.choose("add-field", index, window, cx)
                                            });
                                        })
                                        .observe(format!("provider-field-option-{index}"))
                                })),
                        )
                    }),
            )
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                if this.picker == Some("add-field") {
                    this.picker = None;
                    this.field_menu.trigger.focus(window, cx);
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
            .observe("provider-field-menu");
        popup.content(content).into_any_element()
    }

    pub(super) fn billing_fields(&self, _window: &Window, cx: &Context<Self>) -> AnyElement {
        let t = self.theme;
        div()
            .flex()
            .items_center()
            .px(px(16.))
            .py(px(13.))
            .gap(px(8.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .gap(px(10.))
                    .children((13..=15).map(|index| {
                        div()
                            .when(index < 15, |el| el.w(px(90.)).flex_shrink_0())
                            .when(index == 15, |el| el.flex_1().min_w_0())
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .line_height(px(13.))
                                    .text_color(t.tertiary)
                                    .child(self.locale.text(LABELS[index])),
                            )
                            .child(
                                div()
                                    .h(px(28.))
                                    .rounded(px(4.))
                                    .border_b_1()
                                    .border_color(t.border.opacity(0.58))
                                    .text_size(px(13.))
                                    .line_height(px(20.))
                                    .child(Input::new(&self.inputs[index])),
                            )
                    })),
            )
            .child(
                super::controls::icon(
                    "remove-provider-billing",
                    "x",
                    self.locale
                        .text("providerForm.removeField")
                        .replace("{label}", &self.locale.text("providerForm.billing")),
                    t,
                )
                .disabled(self.busy)
                .on_click(cx.listener(|this, _, window, cx| this.remove_optional(13, window, cx))),
            )
            .observe("provider-billing-fields")
            .into_any_element()
    }
}
