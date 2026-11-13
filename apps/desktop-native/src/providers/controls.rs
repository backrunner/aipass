use crate::theme::Theme;
use gpui::{prelude::*, px, rgb, svg, ElementId, FontWeight};
use gpui_base::Button;

pub fn button(id: impl Into<ElementId>, label: String, primary: bool, t: Theme) -> Button {
    base_button(id, primary, t)
        .accessibility_label(label.clone())
        .child(label)
}

pub fn base_button(id: impl Into<ElementId>, primary: bool, t: Theme) -> Button {
    Button::new(id)
        .styles(|s| s.disabled(|s| s.opacity(0.45)))
        .focus_visible(move |s| s.shadow(t.outline(t.focus_ring, 2., 1.)))
        .h(px(32.))
        .px(px(12.))
        .rounded(px(8.))
        .border_1()
        .border_color(if primary { rgb(0x2563eb) } else { t.border })
        .bg(if primary { rgb(0x2563eb) } else { t.surface })
        .text_color(if primary { rgb(0xffffff) } else { t.text })
        .text_size(px(13.))
        .line_height(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(move |style| {
            style
                .bg(if primary { rgb(0x1f4fd0) } else { t.surface2 })
                .border_color(if primary {
                    rgb(0x1f4fd0)
                } else {
                    t.border_strong
                })
        })
}

pub fn icon(id: impl Into<ElementId>, name: &'static str, label: String, t: Theme) -> Button {
    Button::new(id)
        .styles(|s| s.disabled(|s| s.opacity(0.45)))
        .focus_visible(move |s| s.shadow(t.outline(t.focus_ring, 2., 1.)))
        .size(px(28.))
        .rounded(px(8.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .text_color(t.secondary)
        .cursor_pointer()
        .hover(move |s| s.bg(t.accent_soft))
        .accessibility_label(label)
        .child(svg().path(name).size(px(14.)).text_color(t.secondary))
}
