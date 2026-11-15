//! Native number fields share keyboard, range and disabled behavior.
use crate::parity::Observe;
use crate::theme::Theme;
use gpui::{div, prelude::*, px, App, Entity, Focusable, Window};
use gpui_base::input::{Input, InputState};

pub(super) fn limits(key: &str) -> Option<(u64, u64)> {
    Some(match key {
        "panel-port" => (1, 65535),
        "clipboard-seconds" => (1, 600),
        "silent-retries" => (1, 20),
        "hold-initial" | "hold-max" => (1, u64::MAX),
        "hold-duration" => (0, u64::MAX),
        k if k.starts_with("member-weight-") => (1, u32::MAX.into()),
        k if k.starts_with("price-") && !k.starts_with("price-model-") => (0, u64::MAX),
        k if k.starts_with("retry-") => match k.rsplit('-').next()? {
            "0" => (1, 10),
            "1" => (1, 20),
            "2" => (1, u64::MAX),
            "3" => (100, u64::MAX),
            _ => (1000, u64::MAX),
        },
        _ => return None,
    })
}

fn step(value: &str, up: bool, (min, max): (u64, u64)) -> String {
    let current = value.trim().parse::<u64>().unwrap_or(min);
    (if up {
        current.saturating_add(1)
    } else {
        current.saturating_sub(1)
    })
    .clamp(min, max)
    .to_string()
}

#[derive(Default)]
struct Repeat {
    task: Option<gpui::Task<()>>,
}
fn apply(
    input: &Entity<InputState>,
    up: bool,
    limits: (u64, u64),
    window: &mut Window,
    cx: &mut App,
) {
    input.update(cx, |s, cx| {
        if s.presentation().is_disabled() {
            return;
        }
        let value = step(&s.value(), up, limits);
        s.focus(window, cx);
        s.replace_all(value, window, cx);
    });
}
pub(super) fn editor(
    input: &Entity<InputState>,
    limits: (u64, u64),
    right_aligned: bool,
    label: String,
    t: Theme,
    window: &mut Window,
    cx: &mut App,
) -> gpui::AnyElement {
    input.update(cx, |s, cx| {
        s.set_text_align(
            if right_aligned {
                gpui::TextAlign::Right
            } else {
                gpui::TextAlign::Left
            },
            cx,
        )
    });
    let focused = input.focus_handle(cx).is_focused(window);
    let disabled = input.read(cx).presentation().is_disabled();
    let repeat = window.use_keyed_state(("admin-number-repeat", input.entity_id()), cx, |_, _| {
        Repeat::default()
    });
    if disabled {
        repeat.update(cx, |s, _| s.task = None);
    }
    let keyboard = input.clone();
    let mut buttons = div()
        .w(px(12.))
        .h(px(16.))
        .rounded(px(3.))
        .bg(t.border.opacity(0.35))
        .flex()
        .flex_col();
    for up in [true, false] {
        let state = input.clone();
        let pressed = repeat.clone();
        let released = repeat.clone();
        let outside = repeat.clone();
        buttons = buttons.child(
            gpui_base::Button::new(if up { "increment" } else { "decrement" })
                .focusable(false)
                .disabled(disabled)
                .flex_1()
                .min_h_0()
                .w_full()
                .p(px(0.))
                .accessibility_label(format!("{label} {}1", if up { "+" } else { "−" }))
                .hover(move |s| s.bg(t.border))
                .child(
                    gpui::svg()
                        .path(if up { "chevron-up" } else { "chevron-down" })
                        .size(px(12.))
                        .text_color(t.secondary),
                )
                .on_mouse_down(gpui::MouseButton::Left, move |_, w, cx| {
                    if state.read(cx).presentation().is_disabled() {
                        return;
                    }
                    apply(&state, up, limits, w, cx);
                    let input = state.downgrade();
                    pressed.update(cx, |s, cx| {
                        s.task = Some(cx.spawn_in(w, async move |this, cx| {
                            cx.background_executor()
                                .timer(std::time::Duration::from_millis(500))
                                .await;
                            loop {
                                let keep = this
                                    .update_in(cx, |_, w, cx| {
                                        let Some(input) = input.upgrade() else {
                                            return false;
                                        };
                                        if !w.is_window_active()
                                            || !input.focus_handle(cx).is_focused(w)
                                            || input.read(cx).presentation().is_disabled()
                                        {
                                            return false;
                                        }
                                        apply(&input, up, limits, w, cx);
                                        true
                                    })
                                    .unwrap_or(false);
                                if !keep {
                                    break;
                                }
                                cx.background_executor()
                                    .timer(std::time::Duration::from_millis(50))
                                    .await;
                            }
                        }))
                    });
                    cx.stop_propagation();
                })
                .on_mouse_up(gpui::MouseButton::Left, move |_, _, cx| {
                    released.update(cx, |s, _| s.task = None)
                })
                .on_mouse_up_out(gpui::MouseButton::Left, move |_, _, cx| {
                    outside.update(cx, |s, _| s.task = None)
                })
                .observe(format!("admin-number-{label}-{up}")),
        );
    }
    div()
        .id(("admin-number", input.entity_id()))
        .w_full()
        .min_w_0()
        .h(px(if right_aligned { 32. } else { 34. }))
        .px(px(10.))
        .flex()
        .items_center()
        .border_1()
        .border_color(if focused { t.accent } else { t.border })
        .rounded(px(8.))
        .bg(t.surface)
        .text_color(t.text)
        .text_size(px(13.))
        .line_height(px(18.))
        .when(disabled, |el| el.opacity(0.5))
        .when(focused, |el| el.shadow(t.outline(t.focus_ring, 3., 0.)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .pr(px(if right_aligned { 0. } else { 8. }))
                .child(Input::new(input)),
        )
        .child(buttons)
        .capture_key_down(move |e: &gpui::KeyDownEvent, w, cx| {
            if disabled {
                return;
            }
            let up = match e.keystroke.key.as_str() {
                "up" => true,
                "down" => false,
                _ => return,
            };
            apply(&keyboard, up, limits, w, cx);
            cx.stop_propagation();
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn integer_stepping_preserves_precision_and_bounds() {
        assert_eq!(
            step("18446744073709551614", true, (0, u64::MAX)),
            u64::MAX.to_string()
        );
        assert_eq!(step("65535", true, (1, 65535)), "65535");
        assert_eq!(step("1", false, (1, 65535)), "1");
        assert_eq!(step("", false, (1, 65535)), "1");
    }
}

#[cfg(all(test, feature = "parity-render"))]
mod interaction_tests {
    use super::*;
    struct Probe {
        input: Entity<InputState>,
    }
    impl gpui::Render for Probe {
        fn render(&mut self, w: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
            div().w(px(200.)).child(editor(
                &self.input,
                (1, 20),
                false,
                "count".into(),
                Theme::new(false),
                w,
                cx,
            ))
        }
    }
    #[gpui::test]
    fn press_repeats_stays_in_editor_and_stops_on_outside_release(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_base::init);
        let mut input = None;
        let (_, cx) = cx.add_window_view(|w, cx| {
            let state = cx.new(|cx| {
                let mut s = InputState::new(w, cx);
                s.set_value("1", w, cx);
                s
            });
            input = Some(state.clone());
            let probe = cx.new(|_| Probe { input: state });
            gpui_base::Root::new(probe, w, cx)
        });
        let input = input.unwrap();
        cx.update(|w, cx| {
            w.activate_window();
            w.draw(cx).clear(cx);
        });
        let arrow = cx.update(|_, cx| {
            let b = cx.global::<crate::parity::Geometry>().0["admin-number-count-true"];
            gpui::point(px(b[0] + b[2] / 2.), px(b[1] + b[3] / 2.))
        });
        cx.simulate_mouse_down(arrow, gpui::MouseButton::Left, Default::default());
        assert_eq!(input.read_with(cx, |s, _| s.value().to_string()), "2");
        cx.run_until_parked();
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(500));
        cx.run_until_parked();
        assert_eq!(input.read_with(cx, |s, _| s.value().to_string()), "3");
        cx.simulate_mouse_up(
            gpui::point(px(400.), px(300.)),
            gpui::MouseButton::Left,
            Default::default(),
        );
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(500));
        cx.run_until_parked();
        assert_eq!(input.read_with(cx, |s, _| s.value().to_string()), "3");
        cx.update(|w, cx| {
            assert!(input.focus_handle(cx).is_focused(w));
            w.draw(cx).clear(cx);
        });
        cx.simulate_keystrokes("down");
        assert_eq!(input.read_with(cx, |s, _| s.value().to_string()), "2");
        cx.update(|w, cx| {
            input.update(cx, |s, cx| s.set_disabled(true, cx));
            w.draw(cx).clear(cx);
        });
        cx.simulate_keystrokes("up");
        assert_eq!(input.read_with(cx, |s, _| s.value().to_string()), "2");
    }
}
