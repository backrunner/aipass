use super::*;
use crate::parity::Observe;
use gpui::{div, px, AnyElement, Focusable, FontWeight};
use gpui_base::{input::Input, Button};

pub(super) const RULE_LABELS: [&str; 13] = [
    "pricing.modelPattern",
    "pricing.inputPrice",
    "pricing.outputPrice",
    "pricing.cacheReadPrice",
    "pricing.cacheCreationPrice",
    "pricing.cacheCreation1hPrice",
    "pricing.offPeakFrom",
    "pricing.offPeakTo",
    "pricing.inputPrice",
    "pricing.outputPrice",
    "pricing.cacheReadPrice",
    "pricing.cacheCreationPrice",
    "pricing.cacheCreation1hPrice",
];
pub(super) fn input(
    value: &str,
    label: &str,
    theme: Theme,
    locale: &Locale,
    window: &mut Window,
    cx: &mut Context<Pricing>,
) -> Entity<InputState> {
    cx.new(|cx| {
        let mut s = InputState::new(window, cx);
        s.set_editor_style(theme.input_style());
        s.set_accessibility(
            InputAccessibility::new(Some(gpui::Role::TextInput))
                .with_label(Some(locale.text(label).into())),
        );
        s.set_value(value, window, cx);
        s
    })
}
fn step(
    input: &Entity<InputState>,
    prefix: bool,
    up: bool,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    input.update(cx, |s, cx| {
        let step = if prefix { 1. } else { 0.01 };
        let value = draft::multiplier(&s.value()).unwrap_or(0.);
        let next = (value + if up { step } else { -step }).max(0.);
        if next.is_finite() {
            // Keep tiny increments stable without overflowing large multipliers.
            let next = if next < 1e9 {
                (next * 1e6).round() / 1e6
            } else {
                next
            };
            s.set_value(next.to_string(), window, cx);
        }
        s.focus_handle(cx).focus(window, cx);
    });
}
impl Pricing {
    pub(super) fn number(
        &self,
        input: &Entity<InputState>,
        id: String,
        label: &str,
        prefix: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let t = self.theme;
        let focused = input.read(cx).focus_handle(cx).is_focused(window);
        let keyboard = input.clone();
        let blocked = self.busy();
        div()
            .id(id.clone())
            .relative()
            .capture_key_down(move |event, window, cx| {
                let m = event.keystroke.modifiers;
                if blocked
                    || m.control
                    || m.platform
                    || m.alt
                    || m.shift
                    || !keyboard.read(cx).focus_handle(cx).is_focused(window)
                {
                    return;
                }
                let up = match event.keystroke.key.as_str() {
                    "up" => true,
                    "down" => false,
                    _ => return,
                };
                cx.stop_propagation();
                step(&keyboard, prefix, up, window, cx);
            })
            .min_w_0()
            .w_full()
            .h(px(34.))
            .px(px(if prefix { 10. } else { 9. }))
            .border_1()
            .border_color(t.border)
            .rounded(px(8.))
            .bg(t.surface)
            .text_color(t.text)
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(12.))
            .line_height(px(15.))
            .font_weight(if prefix {
                FontWeight::MEDIUM
            } else {
                FontWeight::NORMAL
            })
            .when(focused, |el| {
                el.border_color(t.accent)
                    .shadow(t.outline(t.focus_ring, 3., 0.))
            })
            .when(prefix, |el| {
                el.child(div().text_color(t.secondary).child("×"))
            })
            .when(
                label == "pricing.cacheCreation1hPrice" && input.read(cx).value().is_empty(),
                |el| {
                    el.child(
                        div()
                            .absolute()
                            .left(px(9.))
                            .top(px(8.5))
                            .text_color(t.tertiary)
                            .child("—"),
                    )
                },
            )
            .child(div().flex_1().min_w_0().child(Input::new(input)))
            .child(
                div()
                    .w(px(11.))
                    .h(px(16.))
                    .flex_shrink_0()
                    .rounded(px(3.))
                    .overflow_hidden()
                    .bg(t.surface2)
                    .flex()
                    .flex_col()
                    .children([true, false].into_iter().map(|up| {
                        let input = input.clone();
                        Button::new(format!("{id}-{}", if up { "up" } else { "down" }))
                            .w_full()
                            .h(px(8.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .tab_index(-1)
                            .disabled(self.busy())
                            .accessibility_label(format!(
                                "{} {}",
                                self.locale.text(label),
                                if up { "↑" } else { "↓" }
                            ))
                            .child(
                                gpui::svg()
                                    .path(if up { "chevron-up" } else { "chevron-down" })
                                    .size(px(9.))
                                    .text_color(t.text),
                            )
                            .on_click(move |_, window, cx| step(&input, prefix, up, window, cx))
                    })),
            )
            .observe(id)
            .into_any_element()
    }
    pub(super) fn label(&self, key: &str) -> gpui::Div {
        div()
            .text_size(px(12.))
            .line_height(px(15.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(self.theme.secondary)
            .child(self.locale.text(key))
    }
    pub(super) fn field(
        &self,
        input: &Entity<InputState>,
        id: impl Into<String>,
        window: &Window,
        cx: &gpui::App,
    ) -> AnyElement {
        let t = self.theme;
        div()
            .min_w_0()
            .w_full()
            .h(px(34.))
            .px(px(9.))
            .py(px(8.))
            .border_1()
            .border_color(t.border)
            .rounded(px(8.))
            .bg(t.surface)
            .text_size(px(12.))
            .line_height(px(16.))
            .text_color(t.text)
            .when(input.read(cx).focus_handle(cx).is_focused(window), |el| {
                el.border_color(t.accent)
                    .shadow(t.outline(t.focus_ring, 3., 0.))
            })
            .child(Input::new(input))
            .observe(id)
            .into_any_element()
    }
    pub(super) fn button(&self, id: &'static str, key: &str, primary: bool) -> Button {
        crate::providers::controls::button(id, self.locale.text(key), primary, self.theme)
            .disabled(self.busy())
    }
    pub(super) fn hint(&self, icon: &'static str, text: String) -> gpui::Div {
        div()
            .flex()
            .items_center()
            .gap(px(7.))
            .text_size(px(11.))
            .line_height(px(16.5))
            .text_color(self.theme.tertiary)
            .child(
                gpui::svg()
                    .path(icon)
                    .size(px(13.))
                    .flex_shrink_0()
                    .text_color(self.theme.tertiary),
            )
            .child(div().flex_1().min_w_0().child(text))
    }
}
