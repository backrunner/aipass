//! Presentation-only strength meter matching the reference's scoring. This is
//! not a password acceptance policy and never retains the input.
use super::AuthView;
use gpui::{div, prelude::*, px, Context, FontWeight};

fn assessment(value: &str) -> (usize, &'static str, Vec<(&'static str, usize)>) {
    // JavaScript String.length counts UTF-16 code units, not UTF-8 bytes.
    let length = value.encode_utf16().count();
    if length == 0 {
        return (0, "password.enter", Vec::new());
    }
    let lower = value.chars().any(|c| c.is_ascii_lowercase());
    let upper = value.chars().any(|c| c.is_ascii_uppercase());
    let digit = value.chars().any(|c| c.is_ascii_digit());
    let symbol = value.chars().any(|c| !c.is_ascii_alphanumeric());
    let variety = [lower, upper, digit, symbol]
        .into_iter()
        .filter(|v| *v)
        .count();
    let score = [8, 12, 16, 20].into_iter().filter(|n| length >= *n).count()
        + [2, 3, 4].into_iter().filter(|n| variety >= *n).count();
    let (filled, label) = if length < 8 {
        (1, "password.tooShort")
    } else {
        match score {
            0..=2 => (1, "password.weak"),
            3..=4 => (2, "password.fair"),
            5 => (3, "password.good"),
            _ => (4, "password.strong"),
        }
    };
    let mut hints = Vec::new();
    if length < 12 {
        hints.push(("password.moreChars", 12 - length));
    }
    if !lower || !upper {
        hints.push(("password.mixCase", 0));
    }
    if !digit {
        hints.push(("password.addDigit", 0));
    }
    if !symbol {
        hints.push(("password.addSymbol", 0));
    }
    hints.truncate(2);
    (filled, label, hints)
}

impl AuthView {
    pub(super) fn strength_meter(&self, cx: &Context<Self>) -> impl IntoElement {
        let value = self.password.read(cx).value();
        let (filled, label, hints) = assessment(&value);
        let t = self.theme;
        let color = [t.tertiary, t.danger, t.warning, t.accent, t.success][filled];
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(div().flex().gap(px(4.)).children((0..4).map(|i| {
                div()
                    .flex_1()
                    .h(px(4.))
                    .rounded_full()
                    .border_1()
                    .border_color(if i < filled { color } else { t.divider })
                    .bg(if i < filled { color } else { t.surface2 })
            })))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .justify_between()
                    .gap(px(8.))
                    .text_size(px(11.))
                    .line_height(px(11.))
                    .child(
                        div()
                            .text_color(color)
                            .font_weight(if filled == 0 {
                                FontWeight::MEDIUM
                            } else {
                                FontWeight::SEMIBOLD
                            })
                            .child(self.locale.text(label)),
                    )
                    // Keep Chinese guidance in separate fields, per repository copy rules.
                    .child(div().flex().gap(px(8.)).text_color(t.tertiary).children(
                        hints.into_iter().map(|(key, count)| {
                            div()
                                .child(self.locale.text(key).replace("{count}", &count.to_string()))
                        }),
                    )),
            )
    }

    pub(super) fn confirmation_feedback(&self, cx: &Context<Self>) -> impl IntoElement {
        let password = self.password.read(cx).value();
        let confirmation = self.confirmation.read(cx).value();
        let matched = !password.is_empty() && password == confirmation;
        div().when(!confirmation.is_empty(), |el| {
            el.mt(px(-4.))
                .font_weight(FontWeight::MEDIUM)
                .text_size(px(11.))
                .line_height(px(14.))
                .text_color(if matched {
                    self.theme.success
                } else {
                    self.theme.danger
                })
                .child(self.locale.text(if matched {
                    "auth.passwordMatch"
                } else {
                    "auth.passwordMismatch"
                }))
        })
    }
}

#[cfg(test)]
#[test]
fn strength_uses_reference_utf16_length_and_ascii_character_classes() {
    assert_eq!(assessment("").0, 0);
    assert_eq!(assessment("aA1!").1, "password.tooShort");
    assert_eq!(assessment("abcdefghijkl").0, 1);
    assert_eq!(assessment("Aa1!abcdefghijklmnop").0, 4);
    assert_eq!(assessment("密码密码").1, "password.tooShort");
    assert_eq!(assessment("🔑🔑🔑🔑").1, "password.weak");
}
