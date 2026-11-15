//! Shared SelectField surface, option spacing and selected/focused presentation.
use crate::theme::Theme;
use gpui::{div, prelude::*, px, svg, Div, ElementId, IntoElement, Stateful};
use gpui_base::Button;

pub fn surface(menu: Stateful<Div>, width: gpui::Pixels, theme: Theme) -> Stateful<Div> {
    menu.w(width)
        .max_h(px(288.))
        .overflow_hidden()
        .p(px(4.))
        .rounded(px(8.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .shadow(vec![gpui_base::box_shadow(
            0.,
            4.,
            crate::theme::shadow_blur(16.),
            0.,
            theme.popup_shadow.into(),
        )])
        .flex()
        .flex_col()
}
pub fn item(id: impl Into<ElementId>, active: bool, theme: Theme) -> Button {
    Button::new(id)
        .role(gpui::Role::MenuItemRadio)
        .aria_selected(active)
        .w_full()
        .flex_shrink_0()
        .min_h(px(32.))
        .px(px(8.))
        .py(px(6.))
        .rounded(px(6.))
        .text_size(px(13.))
        .line_height(px(20.))
        .text_color(theme.text)
        .flex()
        .items_center()
        .gap(px(8.))
        .justify_between()
        .cursor_pointer()
        .when(active, |el| el.bg(theme.accent_soft))
        .hover(move |s| {
            s.bg(if active {
                theme.accent_soft
            } else {
                theme.surface2
            })
            .shadow(vec![gpui_base::box_shadow(
                0.,
                0.,
                0.,
                1.,
                theme.focus_ring.into(),
            )
            .inset()])
        })
        .focus(move |s| {
            s.bg(if active {
                theme.accent_soft
            } else {
                theme.surface2
            })
            .shadow(vec![gpui_base::box_shadow(
                0.,
                0.,
                0.,
                1.,
                theme.focus_ring.into(),
            )
            .inset()])
        })
}
pub fn indicator(active: bool, theme: Theme) -> impl IntoElement {
    div()
        .w(px(16.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .child(svg().path("check").size(px(14.)).text_color(if active {
            theme.accent
        } else {
            gpui::rgba(0)
        }))
}

/// Menus have an opaque surface, so fading content into that same surface is
/// equivalent to the reference viewport mask, including the whole exit opacity.
pub fn fade(child: impl IntoElement, scroll: gpui::ScrollHandle, theme: Theme) -> impl IntoElement {
    div().relative().child(child).child(
        gpui::canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let offset = -f32::from(scroll.offset().y);
                let maximum = f32::from(scroll.max_offset().y);
                for (remaining, top) in [(offset, true), (maximum - offset, false)] {
                    if remaining <= 1. {
                        continue;
                    }
                    let depth = remaining.min(16.).min(f32::from(bounds.size.height) / 4.);
                    let scale = window.scale_factor();
                    let rows = (depth * scale).ceil() as u32;
                    for row in 0..rows {
                        let alpha = (row as f32 + 0.5) / rows as f32;
                        let y = if top {
                            bounds.top() + px(row as f32 / scale)
                        } else {
                            bounds.bottom() - px(depth) + px(row as f32 / scale)
                        };
                        window.paint_quad(gpui::fill(
                            gpui::Bounds::new(
                                gpui::point(bounds.left(), y),
                                gpui::size(bounds.size.width, px(1. / scale)),
                            ),
                            theme.surface.opacity(if top { 1. - alpha } else { alpha }),
                        ));
                    }
                }
            },
        )
        .absolute()
        .top(px(5.))
        .bottom(px(5.))
        .left(px(5.))
        .right(px(5.)),
    )
}
