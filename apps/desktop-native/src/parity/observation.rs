//! Observe the actual child layout without introducing a layout box or changing
//! its identity, event handlers or accessibility tree. No control values are read.
use gpui::{IntoElement, Styled};

pub trait Observe: IntoElement + Styled {
    fn observe(self, name: impl Into<String>) -> impl IntoElement {
        #[cfg(feature = "parity-render")]
        {
            let mut element = self;
            let style = element.style().clone();
            Observation {
                inner: element.into_any_element(),
                name: name.into(),
                style,
            }
        }
        #[cfg(not(feature = "parity-render"))]
        {
            let _ = name;
            self
        }
    }
}
impl<T: IntoElement + Styled> Observe for T {}

#[cfg(feature = "parity-render")]
use gpui::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, LayoutId,
    Pixels, Window,
};
#[cfg(feature = "parity-render")]
struct Observation {
    inner: AnyElement,
    name: String,
    style: gpui::StyleRefinement,
}
#[cfg(feature = "parity-render")]
impl IntoElement for Observation {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
#[cfg(feature = "parity-render")]
impl Element for Observation {
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
        (self.inner.request_layout(window, cx), ())
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
        super::record_bounds(&self.name, window.input_transform().bounds(bounds), cx);
        if !cx.has_global::<super::Styles>() {
            cx.set_global(super::Styles::default());
        }
        cx.global_mut::<super::Styles>().0.insert(
            self.name.clone(),
            serde_json::to_value(&self.style).expect("GPUI style serialization"),
        );
        self.inner.prepaint(window, cx);
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.inner.paint(window, cx);
    }
}
