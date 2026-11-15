//! Shared settings/proxy form presentation; field entities survive rerenders.
use crate::{form_ui, locale::Locale, parity::Observe, theme::Theme};
use gpui::{div, prelude::*, px, Context, Entity, FontWeight, Subscription, Window};
use gpui_base::{
    input::{InputEvent, InputState},
    Switch,
};
use std::collections::BTreeMap;
mod numbers;
#[derive(Default)]
pub struct Fields {
    values: BTreeMap<String, Entity<InputState>>,
    subscriptions: BTreeMap<String, (Subscription, std::rc::Rc<std::cell::RefCell<String>>)>,
    blurs: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
    plain_passwords: bool,
}
impl Fields {
    pub fn with_plain_passwords() -> Self {
        Self {
            plain_passwords: true,
            ..Self::default()
        }
    }
    #[cfg(all(test, feature = "parity-render"))]
    pub(crate) fn input(&self, key: &str) -> Entity<InputState> {
        self.values[key].clone()
    }
    pub fn contains(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }
    pub fn set_value(&self, key: &str, value: String, window: &mut Window, cx: &mut gpui::App) {
        if let Some(state) = self.values.get(key) {
            state.update(cx, |s, cx| s.set_value(value, window, cx));
        }
    }
    pub fn value(&self, key: &str, cx: &gpui::App) -> String {
        self.values
            .get(key)
            .map(|s| s.read(cx).value().to_string())
            .unwrap_or_default()
    }
    pub fn clear(&mut self, window: &mut Window, cx: &mut gpui::App) {
        for state in self.values.values() {
            state.update(cx, |s, cx| {
                s.set_value("", window, cx);
            });
        }
        self.values.clear();
        self.subscriptions.clear();
        self.blurs.borrow_mut().clear();
    }
    pub fn disabled(&self, disabled: bool, cx: &mut gpui::App) {
        for state in self.values.values() {
            state.update(cx, |s, cx| {
                if s.presentation().is_disabled() != disabled {
                    s.set_disabled(disabled, cx);
                }
            });
        }
    }
    pub fn appearance(&self, t: Theme, cx: &mut gpui::App) {
        for s in self.values.values() {
            s.update(cx, |s, cx| {
                s.set_editor_style(t.input_style());
                cx.notify();
            });
        }
    }
    pub fn configure(
        &self,
        key: &str,
        placeholder: &str,
        disabled: bool,
        window: &mut Window,
        cx: &mut gpui::App,
    ) {
        if let Some(state) = self.values.get(key) {
            state.update(cx, |s, cx| {
                if s.presentation().placeholder().as_ref() != placeholder {
                    s.set_placeholder(placeholder, window, cx);
                }
                s.set_disabled(disabled, cx);
            });
        }
    }
    pub fn reset(&mut self, key: &str, window: &mut Window, cx: &mut gpui::App) {
        if let Some(state) = self.values.get(key) {
            state.update(cx, |s, cx| s.set_value("", window, cx));
        }
    }
    pub fn remove_indexed_row(
        &mut self,
        prefix: &str,
        index: usize,
        window: &mut Window,
        cx: &mut gpui::App,
    ) {
        let mut moves = vec![];
        for key in self.values.keys().cloned().collect::<Vec<_>>() {
            let Some(rest) = key.strip_prefix(&format!("{prefix}-")) else {
                continue;
            };
            let (head, n, tail) = if let Some(n) = rest.strip_prefix("model-") {
                ("model-", n, "")
            } else if let Some((n, tail)) = rest.split_once('-') {
                ("", n, tail)
            } else {
                continue;
            };
            let Ok(n) = n.parse::<usize>() else {
                continue;
            };
            if n < index {
                continue;
            }
            let next = if n == index {
                None
            } else if head.is_empty() {
                Some(format!("{prefix}-{}-{tail}", n - 1))
            } else {
                Some(format!("{prefix}-{head}{}", n - 1))
            };
            let value = self.values.remove(&key).unwrap();
            let subscription = self.subscriptions.remove(&key);
            moves.push((next, value, subscription));
        }
        for (key, value, subscription) in moves {
            if let Some(key) = key {
                if let Some((subscription, tracked_key)) = subscription {
                    *tracked_key.borrow_mut() = key.clone();
                    self.subscriptions
                        .insert(key.clone(), (subscription, tracked_key));
                }
                self.values.insert(key, value);
            } else {
                value.update(cx, |s, cx| s.set_value("", window, cx));
            }
        }
        self.blurs
            .borrow_mut()
            .retain(|key| !key.starts_with(&format!("{prefix}-")));
    }
    pub fn remove(&mut self, key: &str) {
        self.values.remove(key);
        self.subscriptions.remove(key);
        self.blurs.borrow_mut().retain(|value| value != key);
    }
    pub fn field<T: 'static>(
        &mut self,
        spec: (&str, &str, &str, bool),
        t: Theme,
        locale: &Locale,
        window: &mut Window,
        cx: &mut Context<T>,
    ) -> gpui::AnyElement {
        let (key, label, _, _) = spec;
        let input = self.editor(spec, t, locale, window, cx);
        div()
            .id(key.to_owned())
            .flex()
            .flex_col()
            .gap(px(6.))
            .min_w_0()
            .child(form_ui::label(locale.text(label), t))
            .child(input)
            .observe(key.to_owned())
            .into_any_element()
    }
    pub fn take_blurs(&self) -> Vec<String> {
        std::mem::take(&mut self.blurs.borrow_mut())
    }
    pub(crate) fn ensure_input<T: 'static>(
        &mut self,
        spec: (&str, &str, &str, bool),
        t: Theme,
        locale: &Locale,
        window: &mut Window,
        cx: &mut Context<T>,
    ) -> Entity<InputState> {
        let (key, label, value, secret) = spec;
        if !self.values.contains_key(key) {
            let input = form_ui::input(label, value, secret, t, locale, window, cx);
            let blurs = self.blurs.clone();
            let tracked_key = std::rc::Rc::new(std::cell::RefCell::new(key.to_owned()));
            let callback_key = tracked_key.clone();
            let subscription = cx.subscribe(&input, move |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Blur) {
                    blurs.borrow_mut().push(callback_key.borrow().clone());
                    cx.notify();
                }
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            });
            self.subscriptions
                .insert(key.into(), (subscription, tracked_key));
            self.values.insert(key.into(), input);
        }
        self.values[key].update(cx, |s, _| {
            s.set_accessibility(
                gpui_base::input::InputAccessibility::new(Some(gpui::Role::TextInput))
                    .with_label(Some(locale.text(label).into()))
                    .with_value_exposed(!secret),
            )
        });
        self.values[key].clone()
    }
    pub fn editor<T: 'static>(
        &mut self,
        spec: (&str, &str, &str, bool),
        t: Theme,
        locale: &Locale,
        window: &mut Window,
        cx: &mut Context<T>,
    ) -> gpui::AnyElement {
        let (key, label, _, secret) = spec;
        self.ensure_input(spec, t, locale, window, cx);
        if let Some(limits) = numbers::limits(key) {
            return numbers::editor(
                &self.values[key],
                limits,
                key == "clipboard-seconds",
                locale.text(label),
                t,
                window,
                cx,
            );
        }
        div()
            .id(key.to_owned())
            .child(form_ui::field_sized(
                &self.values[key],
                form_ui::FieldStyle {
                    id: "admin-input",
                    secret,
                    height: if key == "clipboard-seconds" || key.starts_with("search-url-") {
                        32.
                    } else if secret && !self.plain_passwords {
                        38.
                    } else {
                        34.
                    },
                    padding: 10.,
                    reveal: !self.plain_passwords,
                },
                t,
                locale,
                window,
                cx,
            ))
            .into_any_element()
    }
}
pub fn stack() -> gpui::Div {
    div().flex().flex_col().gap(px(14.)).min_w_0()
}
pub fn rows() -> gpui::Div {
    stack().gap(px(12.)).p(px(16.))
}
pub fn row() -> gpui::Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(16.))
        .min_h(px(32.))
}
pub fn card(title: String, t: Theme, body: impl IntoElement) -> gpui::Div {
    card_actions(title, t, body, div())
}
pub fn card_actions(
    title: String,
    t: Theme,
    body: impl IntoElement,
    actions: impl IntoElement,
) -> gpui::Div {
    div()
        .flex_shrink_0()
        .min_w_0()
        .rounded(px(12.))
        .border_1()
        .border_color(t.border)
        .bg(t.surface)
        .overflow_hidden()
        .child(
            row()
                .min_h(px(0.))
                .line_height(px(16.))
                .px(px(16.))
                .py(px(12.))
                .border_b_1()
                .border_color(t.divider)
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title)
                .child(actions),
        )
        .child(div().py(px(4.)).child(body))
}
pub fn hint(text: String, t: Theme) -> gpui::Div {
    div()
        .text_size(px(12.))
        .line_height(px(17.4))
        .text_color(t.tertiary)
        .child(text)
}
pub fn switch(
    id: impl Into<gpui::ElementId>,
    label: String,
    checked: bool,
    disabled: bool,
    t: Theme,
    window: &mut Window,
    cx: &mut gpui::App,
) -> Switch {
    let id = id.into();
    let progress = gpui_base::motion::transition(
        id.clone(),
        if checked { 1_f32 } else { 0. },
        gpui_base::Transition::new(std::time::Duration::from_millis(150)).ease(|time| {
            aipass_desktop_native::motion::cubic_bezier(f64::from(time), 0.25, 0.1, 0.25, 1.) as f32
        }),
        window,
        cx,
    );
    Switch::new(id)
        .checked(checked)
        .disabled(disabled)
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
            a: 1.,
        })
        .child(
            div()
                .size(px(16.))
                .ml(px(16. * progress))
                .rounded_full()
                .bg(gpui::white())
                .shadow(vec![gpui_base::box_shadow(
                    0.,
                    1.,
                    crate::theme::shadow_blur(3.),
                    0.,
                    gpui::black().opacity(0.15),
                )]),
        )
}
