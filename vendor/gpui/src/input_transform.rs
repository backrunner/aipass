//! AIPass extension: keep input geometry in step with compositor-only scaling.
use crate::{Bounds, Pixels, Point, Window};

/// A uniform scale and translation from element coordinates to window coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ElementTransform {
    scale: f32,
    offset: Point<Pixels>,
}

impl Default for ElementTransform {
    fn default() -> Self {
        Self {
            scale: 1.,
            offset: Point::default(),
        }
    }
}

impl ElementTransform {
    /// Scale around an origin expressed in logical pixels.
    pub fn scale_around(origin: Point<Pixels>, scale: f32) -> Self {
        assert!(scale.is_finite() && scale > 0.);
        Self {
            scale,
            offset: origin * (1. - scale),
        }
    }

    /// Translate after scaling, in logical destination pixels.
    pub fn translated(mut self, offset: Point<Pixels>) -> Self {
        self.offset += offset;
        self
    }

    /// Convert an element position into window coordinates.
    pub fn point(self, value: Point<Pixels>) -> Point<Pixels> {
        value * self.scale + self.offset
    }

    /// Convert a window position into element coordinates.
    pub fn unmap(self, value: Point<Pixels>) -> Point<Pixels> {
        (value - self.offset) / self.scale
    }

    /// Convert a physical movement into element coordinates.
    pub fn delta(self, value: Point<Pixels>) -> Point<Pixels> {
        value / self.scale
    }

    /// Convert element bounds into window coordinates.
    pub fn bounds(self, bounds: Bounds<Pixels>) -> Bounds<Pixels> {
        Bounds::from_corners(self.point(bounds.origin), self.point(bounds.bottom_right()))
    }

    pub(crate) fn then(self, parent: Self) -> Self {
        Self {
            scale: self.scale * parent.scale,
            offset: parent.point(self.offset),
        }
    }
}

impl Window {
    /// Input transform active for the element currently being drawn or handling input.
    pub fn input_transform(&self) -> ElementTransform {
        self.input_transform
    }

    /// Register a composited subtree's input, text-service and accessibility geometry.
    ///
    /// Call in both prepaint and paint with the same transform. This does not paint
    /// a transform itself: pair it with `with_filter` or `Styled::layer_scale`.
    /// Layout remains unscaled; mouse listeners receive coordinates in that layout.
    pub fn with_input_transform<R>(
        &mut self,
        transform: ElementTransform,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        if transform == ElementTransform::default() {
            return f(self);
        }
        let parent = self.input_transform;
        let clip = self.input_clip;
        self.input_clip = Some(clip.unwrap_or_else(|| parent.bounds(self.content_mask().bounds)));
        self.input_transform = transform.then(parent);
        let result = f(self);
        self.input_transform = parent;
        self.input_clip = clip;
        result
    }
}
