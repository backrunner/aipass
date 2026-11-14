use super::*;
use gpui::{Context, Render, TestAppContext};

struct Probe(Backdrop, Option<bool>);
impl Render for Probe {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(dark) = self.1 {
            if self.0.prepare_appearance(dark, window, cx) {
                self.0.apply_appearance(dark);
                self.1 = None;
            }
        }
        self.0.render(window, cx)
    }
}

#[gpui::test]
fn appearance_retains_outgoing_material_and_background_animation_clock(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, _| Probe(Backdrop::new(false, true), None));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let started = cx.update(|_, cx| view.read(cx).0.started);
    for dark in [true, false, true] {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.1 = Some(dark);
                cx.notify();
            });
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.update(|_, cx| {
            let backdrop = &view.read(cx).0;
            assert!(view.read(cx).1.is_none());
            assert_eq!(backdrop.started, started);
            assert_eq!(backdrop.scene.as_ref().unwrap().source.dark, dark);
            assert_eq!(backdrop.alternate.as_ref().unwrap().source.dark, !dark);
        });
    }
}

#[gpui::test]
fn returning_to_the_original_size_retires_an_inflight_scene(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_reduce_motion(true));
    let (view, cx) = cx.add_window_view(|_, _| Probe(Backdrop::new(false, false), None));
    let original = gpui::size(px(960.), px(640.));
    cx.simulate_resize(original);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));

    cx.simulate_resize(gpui::size(px(1280.), px(820.)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|_, cx| assert!(view.read(cx).0.pending.is_some()));
    // Return before the background executor completes the intermediate scene.
    cx.simulate_resize(original);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|_, cx| {
        let backdrop = &view.read(cx).0;
        assert!(backdrop.ready().unwrap());
        assert_eq!(
            backdrop.scene.as_ref().unwrap().source.dimensions(),
            (960., 640.)
        );
    });

    cx.simulate_resize(gpui::size(px(1280.), px(820.)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_resize(gpui::size(px(1100.), px(700.)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|_, cx| {
        // The completed 1280-wide scene must never replace the retained scene.
        assert_ne!(
            view.read(cx).0.scene.as_ref().unwrap().source.dimensions(),
            (1280., 820.)
        );
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|_, cx| {
        let backdrop = &view.read(cx).0;
        assert!(backdrop.ready().unwrap());
        assert_eq!(
            backdrop.scene.as_ref().unwrap().source.dimensions(),
            (1100., 700.)
        );
    });
}
