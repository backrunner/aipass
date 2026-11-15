//! Settings-specific spacing and actions, matching the shared Svelte settings.
use super::*;
use gpui::{div, px, FontWeight};
pub(super) fn cards() -> gpui::Div {
    admin_ui::stack()
        .pixel_snap(false)
        .on_children_prepainted(|children, _, cx| {
            for (index, bounds) in children.iter().enumerate() {
                crate::parity::record_bounds(&format!("settings-card-{index}"), *bounds, cx);
            }
        })
}
pub(super) fn rows() -> gpui::Div {
    admin_ui::rows().gap(px(14.))
}
pub(super) fn row() -> gpui::Div {
    admin_ui::row().min_h(px(36.))
}
pub(super) fn description(text: String, theme: Theme) -> gpui::Div {
    admin_ui::hint(text, theme).line_height(px(15.6))
}
impl Settings {
    pub(super) fn link(&self, id: &'static str, label: &str) -> gpui_base::Button {
        let t = self.theme;
        gpui_base::Button::new(id)
            .accessibility_label(self.locale.text(label))
            .text_size(px(12.))
            .font_weight(FontWeight::NORMAL)
            .text_color(t.accent)
            .focus_visible(move |s| s.shadow(t.outline(t.focus_ring, 2., 1.)))
            .child(self.locale.text(label))
            .disabled(self.pending)
    }
    pub(super) fn icon_button(
        &self,
        id: &str,
        label: &str,
        icon: &'static str,
        primary: bool,
    ) -> gpui_base::Button {
        crate::providers::controls::base_button(id.to_owned(), primary, self.theme)
            .accessibility_label(self.locale.text(label))
            .disabled(self.pending)
            .gap(px(6.))
            .child(gpui::svg().path(icon).size(px(14.)).text_color(if primary {
                gpui::rgb(0xffffff)
            } else {
                self.theme.text
            }))
            .child(self.locale.text(label))
    }
    pub(super) fn described_toggle(
        &self,
        key: &str,
        desc: &str,
        on: bool,
        change: impl Fn(&mut Self, bool, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let owner = cx.entity().downgrade();
        row()
            .gap(px(12.))
            .child(
                admin_ui::stack()
                    .gap(px(2.))
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .child(self.locale.text(key)),
                    )
                    .child(description(self.locale.text(desc), self.theme)),
            )
            .child(
                admin_ui::switch(
                    key.to_owned(),
                    self.locale.text(key),
                    on,
                    self.pending,
                    self.theme,
                    window,
                    cx,
                )
                .on_change(move |value, _, _, cx| {
                    let _ = owner.update(cx, |s, cx| change(s, value, cx));
                }),
            )
    }
}

/// CSS flex shrinks the inline 1fr grid against the copy's max-content width,
/// while retaining each option's min-content width. Taffy's intrinsic grid
/// contribution differs here; calculate the same constraint from shaped text.
pub(super) fn inline_segments_width(
    window: &Window,
    labels: &[String],
    description: &str,
) -> gpui::Pixels {
    let measure = |text: &str, weight| {
        let mut font = gpui::font(".AppleSystemUIFont");
        font.weight = weight;
        window
            .text_system()
            .shape_line(
                text.to_owned().into(),
                px(12.),
                &[gpui::TextRun {
                    len: text.len(),
                    font,
                    color: gpui::black(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }],
                None,
            )
            .width
    };
    let widths = labels
        .iter()
        .map(|label| measure(label, FontWeight::MEDIUM) + px(24.))
        .collect::<Vec<_>>();
    let edges = px(6. + (labels.len() - 1) as f32 * 2.);
    let minimum = widths.iter().copied().sum::<gpui::Pixels>() + edges;
    let maximum =
        widths.iter().copied().fold(px(0.), |a, b| a.max(b)) * labels.len() as f32 + edges;
    let copy = measure(description, FontWeight::NORMAL);
    // Drawer border, body inset and card border/padding, then the row gap.
    let available = px(super::view::DRAWER_WIDTH - 2. - 40. - 2. - 32. - 16.);
    let excess = (maximum + copy - available).max(px(0.));
    (maximum - excess * (maximum / (maximum + copy))).max(minimum)
}
