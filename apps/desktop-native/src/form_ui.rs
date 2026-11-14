//! Shared compact form controls for native setup and management pages.
use crate::parity::Observe;
use crate::{locale::Locale, theme::Theme};
use gpui::{div, prelude::*, px, svg, App, Context, Entity, Focusable, Window};
use gpui_base::{
    input::{Input, InputAccessibility, InputState},
    Checkbox,
};
pub mod select;
pub fn input<T: 'static>(
    label: &str,
    value: &str,
    secret: bool,
    t: Theme,
    locale: &Locale,
    window: &mut Window,
    cx: &mut Context<T>,
) -> Entity<InputState> {
    cx.new(|cx| {
        let mut s = InputState::new(window, cx).masked(secret);
        s.set_editor_style(t.input_style());
        s.set_accessibility(
            InputAccessibility::new(Some(gpui::Role::TextInput))
                .with_label(Some(locale.text(label).into()))
                .with_value_exposed(!secret),
        );
        s.set_value(value, window, cx);
        s
    })
}
pub fn label(text: String, t: Theme) -> gpui::Div {
    div()
        .text_size(px(12.))
        .line_height(px(15.))
        .text_color(t.secondary)
        .child(text)
}
pub fn field(
    input: &Entity<InputState>,
    id: &'static str,
    secret: bool,
    t: Theme,
    locale: &Locale,
    window: &Window,
    cx: &App,
) -> impl IntoElement {
    let focused = input.read(cx).focus_handle(cx).is_focused(window);
    let masked = input.read(cx).presentation().is_masked();
    let state = input.clone();
    div()
        .w_full()
        .min_w_0()
        .h(px(if secret { 38. } else { 36. }))
        .px(px(if secret { 0. } else { 12. }))
        .rounded(px(8.))
        .border_1()
        .border_color(t.border)
        .bg(t.surface)
        .text_color(t.text)
        .text_size(px(13.))
        .line_height(px(18.))
        .flex()
        .items_center()
        .gap(px(0.))
        .when(focused, |el| {
            el.border_color(t.accent)
                .shadow(t.outline(t.focus_ring, 3., 0.))
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .when(secret, |el| el.px(px(12.)))
                .when(secret && masked, |el| {
                    el.font_family("AIPass Password Dots")
                })
                .child(Input::new(input)),
        )
        .when(secret, |el| {
            el.child(
                crate::providers::controls::icon(
                    id,
                    if masked { "eye" } else { "eye-off" },
                    locale.text(if masked {
                        "password.show"
                    } else {
                        "password.hide"
                    }),
                    t,
                )
                .size(px(36.))
                .rounded(px(0.))
                .border_l_1()
                .border_color(t.divider)
                .disabled(input.read(cx).presentation().is_disabled())
                .on_click(move |_, window, cx| {
                    state.update(cx, |s, cx| s.toggle_masked(window, cx))
                }),
            )
        })
        .observe(id)
}
pub fn check(id: &'static str, text: String, checked: bool, t: Theme) -> Checkbox {
    Checkbox::new(id)
        .checked(checked)
        .accessibility_label(text.clone())
        .flex()
        .items_start()
        .gap(px(10.))
        .text_size(px(12.))
        .line_height(px(17.4))
        .text_color(t.secondary)
        .focus_visible(move |s| s.shadow(t.outline(t.focus_ring, 2., 1.)))
        .child(
            div()
                .mt(px(2.))
                .size(px(14.))
                .flex_shrink_0()
                .rounded(px(4.))
                .border_1()
                .border_color(if checked { t.accent } else { t.border_strong })
                .bg(if checked { t.accent } else { t.surface })
                .when(checked, |el| {
                    el.child(
                        svg()
                            .path("checkbox-check")
                            .size(px(12.))
                            .text_color(gpui::rgb(0xffffff)),
                    )
                }),
        )
        .child(div().flex_1().min_w_0().child(text))
}
