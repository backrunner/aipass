use crate::theme::Theme;
use gpui::{prelude::*, px, rgb, svg, ElementId, FontWeight};
use gpui_base::Button;

pub fn button(id: impl Into<ElementId>, label: String, primary: bool, t: Theme) -> Button {
    Button::new(id)
        .h(px(32.))
        .px(px(12.))
        .rounded(px(8.))
        .border_1()
        .border_color(if primary { gpui::rgba(0) } else { t.border })
        .bg(if primary { rgb(0x2563eb) } else { t.surface })
        .text_color(if primary { rgb(0xffffff) } else { t.text })
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(move |style| style.bg(if primary { rgb(0x1f4fd0) } else { t.surface2 }))
        .accessibility_label(label.clone())
        .child(label)
}

pub fn icon(id: impl Into<ElementId>, name: &'static str, label: String, t: Theme) -> Button {
    Button::new(id)
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
