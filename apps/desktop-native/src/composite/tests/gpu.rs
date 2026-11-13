//! Synthetic Metal targets only: never opens or captures a native application window.
use gpui::{
    point, size, Backdrop, BorderStyle, Bounds, ContentMask, DevicePixels, Filter, Quad,
    ScaledPixels, Scene,
};

fn bounds(x: f32, y: f32, w: f32, h: f32) -> Bounds<ScaledPixels> {
    Bounds::new(
        point(ScaledPixels(x), ScaledPixels(y)),
        size(ScaledPixels(w), ScaledPixels(h)),
    )
}
fn quad(rect: Bounds<ScaledPixels>, color: u32, viewport: Bounds<ScaledPixels>) -> Quad {
    Quad {
        order: 0,
        border_style: BorderStyle::Solid,
        bounds: rect,
        content_mask: ContentMask { bounds: viewport },
        background: gpui::rgb(color).into(),
        border_color: gpui::transparent_black(),
        corner_radii: Default::default(),
        border_widths: Default::default(),
        corner_smoothing: 0.,
        pad: 0,
    }
}

#[test]
fn current_frame_backdrop_blurs_foreground_and_discards_previous_frame() {
    let mut renderer = gpui_platform::current_headless_renderer()
        .unwrap()
        .expect("Metal renderer");
    for scale in [1., 2.] {
        let viewport = bounds(0., 0., 128. * scale, 96. * scale);
        let extent = size(
            DevicePixels((128. * scale) as i32),
            DevicePixels((96. * scale) as i32),
        );
        for color in [0xff0000, 0x0000ff] {
            let mut scene = Scene::default();
            scene.insert_primitive(quad(viewport, color, viewport));
            scene.insert_primitive(quad(
                bounds(60. * scale, 0., 8. * scale, 96. * scale),
                0xffffff,
                viewport,
            ));
            scene.insert_primitive(Backdrop {
                bounds: viewport,
                content_mask: ContentMask { bounds: viewport },
                blur: 4. * scale,
                opacity: 1.,
                ..Default::default()
            });
            scene.finish();
            let image = renderer.render_scene_to_image(&scene, extent).unwrap();
            let beside = image
                .get_pixel((57. * scale) as u32, (48. * scale) as u32)
                .0;
            assert!(
                beside[1] > 5 && beside[1] < 200,
                "foreground must spread into the backdrop: {beside:?}"
            );
            let corner = image
                .get_pixel((20. * scale) as u32, (48. * scale) as u32)
                .0;
            let channel = if color == 0xff0000 { 0 } else { 2 };
            assert!(
                beside[channel] > 240 && beside[2 - channel] < 200,
                "blurred foreground must also use the current background: {beside:?}"
            );
            assert!(
                corner[channel] > 240 && corner[2 - channel] < 10,
                "stale frame: {corner:?}"
            );
        }
    }
}

#[test]
fn compositor_scales_the_complete_subtree_at_each_display_scale() {
    let mut renderer = gpui_platform::current_headless_renderer()
        .unwrap()
        .expect("Metal renderer");
    for device_scale in [1., 2.] {
        let viewport = bounds(0., 0., 128. * device_scale, 96. * device_scale);
        let source = bounds(
            20. * device_scale,
            20. * device_scale,
            80. * device_scale,
            40. * device_scale,
        );
        let mut scene = Scene::default();
        scene.insert_primitive(quad(viewport, 0x000000, viewport));
        scene.push_filter(
            source,
            source.center(),
            viewport,
            Filter {
                scale: 0.5,
                ..Default::default()
            },
        );
        scene.insert_primitive(quad(source, 0xffffff, viewport));
        scene.insert_primitive(quad(
            bounds(
                40. * device_scale,
                20. * device_scale,
                20. * device_scale,
                40. * device_scale,
            ),
            0xff0000,
            viewport,
        ));
        scene.pop_filter();
        scene.finish();
        let image = renderer
            .render_scene_to_image(
                &scene,
                size(
                    DevicePixels((128. * device_scale) as i32),
                    DevicePixels((96. * device_scale) as i32),
                ),
            )
            .unwrap();
        let pixel = |x, y| {
            image
                .get_pixel((x * device_scale) as u32, (y * device_scale) as u32)
                .0
        };
        assert!(pixel(30., 40.)[0] < 10, "old bounds must be empty");
        assert!(pixel(45., 40.)[1] > 240, "parent must be scaled");
        assert!(
            pixel(55., 40.)[0] > 240 && pixel(55., 40.)[1] < 10,
            "child must share its parent's transform"
        );
    }
}

#[test]
fn layer_opacity_preserves_opaque_children_and_nested_alpha() {
    let mut renderer = gpui_platform::current_headless_renderer()
        .unwrap()
        .expect("Metal renderer");
    for scale in [1., 2.] {
        let viewport = bounds(0., 0., 128. * scale, 96. * scale);
        let extent = size(
            DevicePixels((128. * scale) as i32),
            DevicePixels((96. * scale) as i32),
        );
        let source = bounds(20. * scale, 20. * scale, 80. * scale, 50. * scale);
        for opacity in [0., 0.45, 1.] {
            let mut scene = Scene::default();
            scene.insert_primitive(quad(viewport, 0xffffff, viewport));
            scene.push_filter(
                source,
                source.center(),
                viewport,
                Filter {
                    opacity,
                    ..Default::default()
                },
            );
            scene.insert_primitive(quad(source, 0x2563eb, viewport));
            scene.insert_primitive(quad(
                bounds(40. * scale, 30. * scale, 20. * scale, 20. * scale),
                0xffffff,
                viewport,
            ));
            scene.pop_filter();
            scene.finish();
            let image = renderer.render_scene_to_image(&scene, extent).unwrap();
            let glyph = image
                .get_pixel((50. * scale) as u32, (40. * scale) as u32)
                .0;
            assert!(
                glyph.iter().all(|v| *v >= 253),
                "group opacity must not tint white foreground: {glyph:?}"
            );
            let background = image
                .get_pixel((30. * scale) as u32, (40. * scale) as u32)
                .0;
            let expected =
                [37., 99., 235.].map(|v| (v * opacity + 255. * (1. - opacity)).round() as i32);
            for channel in 0..3 {
                assert!(
                    (i32::from(background[channel]) - expected[channel]).abs() <= 2,
                    "wrong opacity {opacity}: {background:?}, expected {expected:?}"
                );
            }
        }
        let mut scene = Scene::default();
        scene.insert_primitive(quad(viewport, 0x000000, viewport));
        scene.push_filter(
            source,
            source.center(),
            viewport,
            Filter {
                opacity: 0.5,
                ..Default::default()
            },
        );
        scene.insert_primitive(quad(source, 0xff0000, viewport));
        scene.push_filter(
            source,
            source.center(),
            viewport,
            Filter {
                opacity: 0.5,
                ..Default::default()
            },
        );
        scene.insert_primitive(quad(source, 0xffffff, viewport));
        scene.pop_filter();
        scene.pop_filter();
        scene.finish();
        let image = renderer.render_scene_to_image(&scene, extent).unwrap();
        let pixel = image
            .get_pixel((50. * scale) as u32, (40. * scale) as u32)
            .0;
        for (actual, expected) in pixel[..3].iter().zip([128, 64, 64]) {
            assert!(
                (i32::from(*actual) - expected).abs() <= 2,
                "nested alpha: {pixel:?}"
            );
        }
    }
}

#[test]
fn translucent_siblings_keep_independent_composites() {
    let mut renderer = gpui_platform::current_headless_renderer()
        .unwrap()
        .expect("Metal renderer");
    let viewport = bounds(0., 0., 128., 96.);
    let mut scene = Scene::default();
    scene.insert_primitive(quad(viewport, 0x000000, viewport));
    for color in [0xff0000, 0x0000ff] {
        scene.push_filter(
            viewport,
            viewport.center(),
            viewport,
            Filter {
                opacity: 0.5,
                ..Default::default()
            },
        );
        scene.insert_primitive(quad(viewport, color, viewport));
        scene.pop_filter();
    }
    scene.finish();
    let image = renderer
        .render_scene_to_image(&scene, size(DevicePixels(128), DevicePixels(96)))
        .unwrap();
    let pixel = image.get_pixel(50, 40).0;
    for (actual, expected) in pixel[..3].iter().zip([64, 0, 128]) {
        assert!(
            (i32::from(*actual) - expected).abs() <= 2,
            "sibling layers must not merge: {pixel:?}"
        );
    }
}
