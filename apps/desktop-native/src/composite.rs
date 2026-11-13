//! Compositor scaling with matching input, accessibility and text-service geometry.
use gpui::{
    px, AnyElement, App, Bounds, Element, ElementId, ElementTransform, GlobalElementId,
    InspectorElementId, IntoElement, LayerFilter, LayoutId, Pixels, Window,
};

pub(crate) struct Scale {
    child: AnyElement,
    factor: f32,
    origin: gpui::Point<f32>,
    tracked_origin: Option<std::rc::Rc<std::cell::Cell<gpui::Point<f32>>>>,
    offset: gpui::Point<Pixels>,
}

impl Scale {
    pub fn new(child: impl IntoElement, factor: f32) -> Self {
        assert!(factor.is_finite() && factor > 0.);
        Self {
            child: child.into_any_element(),
            factor: if filters_enabled() { factor } else { 1. },
            origin: gpui::point(0.5, 0.5),
            tracked_origin: None,
            offset: gpui::Point::default(),
        }
    }
    pub fn origin(mut self, origin: gpui::Point<f32>) -> Self {
        self.origin = origin;
        self
    }
    pub fn origin_from(mut self, origin: std::rc::Rc<std::cell::Cell<gpui::Point<f32>>>) -> Self {
        self = self.origin(origin.get());
        self.tracked_origin = Some(origin);
        self
    }
    fn resolved_origin(&self) -> gpui::Point<f32> {
        self.tracked_origin
            .as_ref()
            .map_or(self.origin, |origin| origin.get())
    }
    pub fn translate(mut self, offset: gpui::Point<Pixels>) -> Self {
        if filters_enabled() {
            self.offset = offset;
        }
        self
    }
    fn input_transform(&self, bounds: Bounds<Pixels>) -> ElementTransform {
        let origin = bounds.origin
            + gpui::point(
                bounds.size.width * self.resolved_origin().x,
                bounds.size.height * self.resolved_origin().y,
            );
        ElementTransform::scale_around(origin, self.factor).translated(self.offset)
    }
}

// Diagnostic renderer bypass must apply to both painting and input geometry.
fn filters_enabled() -> bool {
    #[cfg(target_os = "macos")]
    {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ENABLED.get_or_init(|| std::env::var("GPUI_FILTERS").as_deref() != Ok("0"))
    }
    #[cfg(not(target_os = "macos"))]
    true
}

impl IntoElement for Scale {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Scale {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_input_transform(self.input_transform(bounds), |w| {
            self.child.prepaint(w, cx);
        });
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_input_transform(self.input_transform(bounds), |w| {
            w.with_filter(
                bounds,
                LayerFilter {
                    scale: Some(self.factor),
                    scale_origin: Some(self.resolved_origin()),
                    translate: Some(self.offset),
                    ..Default::default()
                },
                // Preserve the popup's 64 px shadow and its 24 px vertical offset.
                px(128.),
                |w| self.child.paint(w, cx),
            );
        });
    }
}

#[cfg(all(test, feature = "parity-render"))]
mod tests;
