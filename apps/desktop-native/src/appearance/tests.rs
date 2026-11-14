use super::*;
use gpui::{div, prelude::*, px, Context, Render, TestAppContext};

#[test]
fn default_follows_all_system_appearances_and_explicit_preferences_override() {
    assert_eq!(
        aipass_desktop_platform::preferences::AppPreferences::default().theme,
        ThemePreference::System
    );
    assert_eq!(forced(ThemePreference::System), None);
    for appearance in [
        WindowAppearance::Light,
        WindowAppearance::Dark,
        WindowAppearance::VibrantLight,
        WindowAppearance::VibrantDark,
    ] {
        assert_eq!(
            resolve(ThemePreference::System, appearance),
            is_dark(appearance)
        );
        assert!(!resolve(ThemePreference::Light, appearance));
        assert!(resolve(ThemePreference::Dark, appearance));
    }
}

#[test]
fn transition_is_monotonic_symmetric_interruptible_and_reduced_motion_is_immediate() {
    let mut appearance = Appearance::new(false);
    appearance.requested = true;
    appearance.begin(false);
    let slot = appearance.slot;
    let mut last = 0.;
    for milliseconds in 0..=220 {
        appearance.frozen = Some(Duration::from_millis(milliseconds));
        let progress = appearance.progress();
        assert!(progress >= last && progress <= 1.);
        appearance.frozen = Some(Duration::from_millis(220 - milliseconds));
        assert!((progress + appearance.progress() - 1.).abs() < 0.00001);
        last = progress;
    }
    appearance.frozen = Some(Duration::ZERO);
    appearance.requested = false;
    appearance.begin(false);
    assert_ne!(appearance.slot, slot);
    assert!(!appearance.dark);
    assert_eq!(appearance.progress(), 0.);
    appearance.begin(true);
    assert_eq!(appearance.progress(), 1.);
}

struct Probe(Appearance);
impl Render for Probe {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.0.frame(false, false, window, cx);
        div().size_full().bg(if self.0.dark {
            gpui::black()
        } else {
            gpui::white()
        })
    }
}

#[gpui::test]
fn outgoing_span_survives_each_frame_and_is_retired_at_endpoint(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, _| Probe(Appearance::new(false)));
    let draw = |cx: &mut gpui::VisualTestContext| cx.update(|window, cx| window.draw(cx).clear(cx));
    draw(cx);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.0.requested = true;
            view.0.frozen = Some(Duration::from_millis(110));
            view.0.begin(false);
            cx.notify();
        })
    });
    for _ in 0..5 {
        draw(cx);
        cx.update(|window, _| {
            assert!(window.rendered_span(span(0)).is_some());
            assert!(window.rendered_span(span(1)).is_some());
        });
    }
    // Interrupting starts from the complete previous composite, including its
    // outgoing material, instead of jumping back to a palette endpoint.
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.0.requested = false;
            view.0.frozen = Some(Duration::ZERO);
            view.0.begin(false);
            cx.notify();
        })
    });
    draw(cx);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.0.frozen = Some(DURATION);
            cx.notify();
        })
    });
    draw(cx);
    cx.update(|window, _| {
        assert!(window.rendered_span(span(0)).is_some());
        assert!(window.rendered_span(span(1)).is_none());
    });
}

#[gpui::test]
fn resize_lock_and_reduce_motion_cannot_replay_stale_contents(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, _| Probe(Appearance::new(false)));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.0.frame(false, false, window, cx);
            for (workspace, covered) in [(true, false), (false, true)] {
                view.0.begin(false);
                view.0.frozen = Some(Duration::ZERO);
                assert_eq!(view.0.frame(workspace, covered, window, cx), 1.);
            }
            view.0.begin(false);
        });
    });
    cx.simulate_resize(gpui::size(px(960.), px(640.)));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            assert_eq!(view.0.frame(false, false, window, cx), 1.);
            view.0.begin(false);
            cx.set_reduce_motion(true);
            assert_eq!(view.0.frame(false, false, window, cx), 1.);
        })
    });
}

struct InteractiveProbe {
    appearance: Appearance,
    clicks: Vec<bool>,
}
impl Render for InteractiveProbe {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.appearance.frame(false, false, window, cx);
        let dark = self.appearance.dark;
        div().size_full().child(gpui::deferred(
            div()
                .id("deferred-theme-button")
                .size(px(100.))
                .bg(if dark { gpui::black() } else { gpui::white() })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.clicks.push(dark);
                    cx.notify();
                })),
        ))
    }
}

#[gpui::test]
fn outgoing_deferred_popup_cannot_duplicate_or_receive_input(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, _| InteractiveProbe {
        appearance: Appearance::new(false),
        clicks: Vec::new(),
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.appearance.requested = true;
            view.appearance.begin(false);
            view.appearance.frozen = Some(Duration::from_millis(110));
            cx.notify();
        });
        window.draw(cx).clear(cx);
    });
    cx.simulate_click(gpui::point(px(20.), px(20.)), Default::default());
    assert_eq!(view.read_with(cx, |view, _| view.clicks.clone()), [true]);
}
