//! Backgrounds paint a complete cached scene on the first frame. Subsequent
//! resize preparation retains the last scene; motion only transforms its layers.
mod glass;
mod motion;
mod scene;
#[cfg(all(test, feature = "parity-render"))]
mod tests;
use gpui::{canvas, div, fill, prelude::*, px, App, Asset, Bounds, Pixels, Window};
use motion::movement;
use scene::{Scene, Source};
use std::{cell::RefCell, future::Future, rc::Rc, sync::Arc, time::Instant};

#[derive(Clone)]
pub(crate) struct Frame {
    scene: Arc<Scene>,
    viewport: Bounds<Pixels>,
    elapsed: f64,
    reduced: bool,
}
impl Frame {
    pub fn paint(&self, clip: Bounds<Pixels>, radius: f32, frosted: bool, window: &mut Window) {
        self.scene.paint(self, clip, radius, frosted, window);
    }
}
pub(crate) type Surface = Rc<RefCell<Option<Frame>>>;
pub struct Backdrop {
    dark: bool,
    hero: bool,
    started: Instant,
    scene: Option<Arc<Scene>>,
    pending: Option<Source>,
    surface: Surface,
    error: Option<String>,
}
impl Backdrop {
    pub fn new(dark: bool, hero: bool) -> Self {
        Self {
            dark,
            hero,
            started: Instant::now(),
            scene: None,
            pending: None,
            surface: Rc::default(),
            error: None,
        }
    }
    pub fn source(&self) -> Surface {
        self.surface.clone()
    }
    #[cfg(feature = "parity-render")]
    pub fn ready(&self) -> Result<bool, String> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        Ok(self.scene.is_some() && self.pending.is_none())
    }
    pub fn render(&mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let size = window.viewport_size();
        let source = Source {
            width: f32::from(size.width).to_bits(),
            height: f32::from(size.height).to_bits(),
            dark: self.dark,
            hero: self.hero,
        };
        #[cfg(feature = "parity-render")]
        let preparation = Instant::now();
        if self.scene.is_none() && self.error.is_none() {
            match Scene::build(source, cx.svg_renderer()) {
                Ok(scene) => {
                    self.scene = Some(scene);
                    self.started = Instant::now();
                }
                Err(error) => self.error = Some(error),
            }
        }
        // Retire every completed job, even if a resize returned to the original
        // size. Never install a scene prepared for an intermediate viewport.
        if let Some(pending) = self.pending {
            if let Some(result) = window.use_asset::<SceneAsset>(&pending, cx) {
                match result {
                    Ok(scene) if pending == source => {
                        if let Some(previous) = self.scene.replace(scene) {
                            previous.release_atlas(window);
                        }
                    }
                    Err(error) if pending == source => self.error = Some(error),
                    _ => {}
                }
                cx.remove_asset::<SceneAsset>(&pending);
                self.pending = None;
            }
        }
        if self.pending.is_none()
            && self.error.is_none()
            && self
                .scene
                .as_ref()
                .is_some_and(|scene| scene.source != source)
        {
            self.pending = Some(source);
            let _ = window.use_asset::<SceneAsset>(&source, cx);
        }
        let reduced = cx.reduce_motion();
        let elapsed = self.started.elapsed().as_secs_f64();
        #[cfg(feature = "parity-render")]
        let elapsed = {
            if !cx.has_global::<crate::parity::Backgrounds>() {
                cx.set_global(crate::parity::Backgrounds::default());
            }
            let observations = cx.global_mut::<crate::parity::Backgrounds>();
            let elapsed = observations.time_ms.map(|ms| ms / 1000.).unwrap_or(elapsed);
            let key = if self.hero { "auth" } else { "workspace" };
            let first = !observations.frames.contains_key(key);
            let first_ready = observations
                .frames
                .get(key)
                .and_then(|v| v["firstFrameComplete"].as_bool())
                .unwrap_or(self.scene.is_some());
            let first_ms = observations
                .frames
                .get(key)
                .and_then(|v| v["firstPreparationMs"].as_f64())
                .unwrap_or(preparation.elapsed().as_secs_f64() * 1000.);
            let transforms: Vec<_> = if self.hero {
                (0..5)
                    .map(|i| {
                        let (x, y, scale) = movement(
                            i,
                            elapsed,
                            reduced,
                            f32::from(size.width),
                            f32::from(size.height),
                        );
                        [scale, 0., 0., scale, x, y]
                    })
                    .collect()
            } else {
                Vec::new()
            };
            observations.frames.insert(key.into(),serde_json::json!({"elapsedMs":elapsed*1000., "reduced":reduced,
                "firstFrameComplete":first_ready,"firstPreparationMs":first_ms,"isFirstFrame":first,
                "complete":self.scene.is_some(),"resizePending":self.pending.is_some(),"transforms":transforms}));
            elapsed
        };
        if self.hero && !reduced {
            window.request_animation_frame();
        }
        *self.surface.borrow_mut() = self.scene.as_ref().map(|scene| Frame {
            scene: scene.clone(),
            viewport: Bounds::new(gpui::point(px(0.), px(0.)), size),
            elapsed,
            reduced,
        });
        let surface = self.surface.clone();
        div().absolute().inset_0().size_full().child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    if let Some(frame) = surface.borrow().as_ref() {
                        frame.paint(bounds, 12., false, window);
                    }
                },
            )
            .size_full(),
        )
    }
    /// Sample this app's own background at the surface's actual window position.
    /// Foreground controls stay sharp; no desktop/screen capture is involved.
    pub fn glass(&self, tint: gpui::Rgba, radius: f32) -> impl IntoElement {
        let surface = self.surface.clone();
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let bounds = bounds.dilate(px(-1.));
                if let Some(frame) = surface.borrow().as_ref() {
                    frame.paint(bounds, (radius - 1.).max(0.), true, window);
                }
                window.paint_quad(fill(bounds, tint).corner_radii(px((radius - 1.).max(0.))));
            },
        )
        .absolute()
        .inset_0()
        .size_full()
    }
}
struct SceneAsset;
impl Asset for SceneAsset {
    type Source = Source;
    type Output = Result<Arc<Scene>, String>;
    fn load(source: Source, cx: &mut App) -> impl Future<Output = Self::Output> + Send + 'static {
        let renderer = cx.svg_renderer();
        cx.background_executor()
            .spawn(async move { Scene::build(source, renderer) })
    }
}

fn shadows(w: f32, h: f32) -> String {
    let (sidebar, list) = crate::theme::columns(w);
    let mut shadows = String::new();
    let mut gradients = String::new();
    // CSS outer box shadows exclude the element's interior, even when its
    // background is transparent. GPUI's unmasked shadow fills that interior.
    gradients.push_str(r#"<filter id="pane-shadow" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="16"/><feOffset dy="12"/></filter>"#);
    for (i, (x, width)) in [
        (8., sidebar),
        (sidebar + 16., list),
        (sidebar + list + 24., w - sidebar - list - 32.),
    ]
    .into_iter()
    .enumerate()
    {
        gradients.push_str(&format!(r#"<mask id="outside{i}" maskUnits="userSpaceOnUse" x="0" y="0" width="{w}" height="{h}"><rect width="{w}" height="{h}" fill="white"/><rect x="{x}" y="8" width="{width}" height="{}" rx="14" fill="black"/></mask>"#, h-16.));
        shadows.push_str(&format!(r##"<g mask="url(#outside{i})"><rect x="{x}" y="8" width="{width}" height="{}" rx="14" fill="#080c18" opacity="0.05" filter="url(#pane-shadow)"/></g>"##, h-16.));
    }

    format!("<defs>{gradients}</defs>{shadows}")
}
