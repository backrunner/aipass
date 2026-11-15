use gpui::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, Window,
};

/// Measure closed content without registering its focus, hitboxes or AX nodes.
/// MotionReveal still receives its natural height before the opening frame.
pub(super) struct MeasuredBody {
    visible: bool,
    child: AnyElement,
}

impl MeasuredBody {
    pub fn new(visible: bool, child: AnyElement) -> Self {
        Self { visible, child }
    }
}

impl IntoElement for MeasuredBody {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for MeasuredBody {
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
        if self.visible {
            self.child.prepaint_at(bounds.origin, window, cx);
        }
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
        if self.visible {
            self.child.paint(window, cx);
        }
    }
}
