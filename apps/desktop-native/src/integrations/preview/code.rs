use super::*;
use aipass_desktop_native::integration::syntax;
use gpui::{FontStyle, HighlightStyle, Pixels, Rgba, StyledText, TextRun};

fn mix(a: Rgba, b: Rgba, weight: f32) -> Rgba {
    Rgba {
        r: a.r * weight + b.r * (1. - weight),
        g: a.g * weight + b.g * (1. - weight),
        b: a.b * weight + b.b * (1. - weight),
        a: 1.,
    }
}

fn missing_cell(t: Theme) -> impl IntoElement {
    gpui::canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let width = f32::from(bounds.size.width);
            let height = f32::from(bounds.size.height);
            let mut x = -height;
            let mut path = gpui::PathBuilder::stroke(px(1.));
            while x < width {
                let start = x.max(0.);
                let end = (x + height).min(width);
                path.move_to(bounds.origin + gpui::point(px(start), px(height - (start - x))));
                path.line_to(bounds.origin + gpui::point(px(end), px(height - (end - x))));
                x += 5. * std::f32::consts::SQRT_2;
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, t.tertiary.opacity(0.06));
            }
        },
    )
    .absolute()
    .inset_0()
    .size_full()
}

fn styled(
    text: &str,
    path: &str,
    t: Theme,
    changed: Option<std::ops::Range<usize>>,
    color: Rgba,
) -> StyledText {
    let spans = syntax::spans(text, path);
    let mut boundaries = vec![0, text.len()];
    for (r, _) in &spans {
        boundaries.extend([r.start, r.end]);
    }
    if let Some(r) = &changed {
        boundaries.extend([r.start, r.end]);
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    let highlights = boundaries
        .windows(2)
        .filter(|r| r[0] < r[1])
        .map(|r| {
            let kind = spans
                .iter()
                .find(|(range, _)| range.contains(&r[0]))
                .map(|(_, kind)| *kind);
            let color = match kind {
                Some(
                    syntax::Kind::Key
                    | syntax::Kind::Keyword
                    | syntax::Kind::Section
                    | syntax::Kind::Variable,
                ) => t.accent,
                Some(syntax::Kind::String) => t.success,
                Some(syntax::Kind::Number) => t.warning,
                Some(syntax::Kind::Comment) => t.tertiary,
                None => t.text,
            };
            (
                r[0]..r[1],
                HighlightStyle {
                    color: Some(color.into()),
                    font_style: (kind == Some(syntax::Kind::Comment)).then_some(FontStyle::Italic),
                    font_weight: matches!(
                        kind,
                        Some(syntax::Kind::Keyword | syntax::Kind::Section)
                    )
                    .then_some(FontWeight::MEDIUM),
                    ..Default::default()
                },
            )
        })
        .map(|(range, mut style)| {
            if changed.as_ref().is_some_and(|r| r.contains(&range.start)) {
                style.background_color = Some(color.opacity(0.22).into());
            }
            (range, style)
        });
    StyledText::new(text.to_owned()).with_highlights(highlights)
}

impl Preview {
    fn scroll_key(&mut self, event: &gpui::KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let mut offset = self.scroll.offset();
        let max = self.scroll.max_offset();
        match event.keystroke.key.as_str() {
            "down" => offset.y -= px(24.),
            "up" => offset.y += px(24.),
            "pagedown" => offset.y -= px(240.),
            "pageup" => offset.y += px(240.),
            "home" => offset.y = px(0.),
            "end" => offset.y = -max.y,
            "left" => offset.x += px(24.),
            "right" => offset.x -= px(24.),
            _ => return,
        }
        offset.x = offset.x.clamp(-max.x, px(0.));
        offset.y = offset.y.clamp(-max.y, px(0.));
        self.scroll.set_offset(offset);
        cx.stop_propagation();
        cx.notify();
    }
    fn text_width(&self, text: &str, window: &Window) -> Pixels {
        window
            .text_system()
            .shape_line(
                text.to_owned().into(),
                px(12.),
                &[TextRun {
                    len: text.len(),
                    font: gpui::font(MONO),
                    color: self.theme.text.into(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }],
                None,
            )
            .width
    }
    pub(super) fn code(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let file = &self.files[self.active];
        let t = self.theme;
        if !self.full && file.diff.trim() == "(no changes)" {
            return div()
                .flex_1()
                .min_h(px(120.))
                .border_1()
                .border_color(t.divider)
                .rounded(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(12.))
                .text_color(t.tertiary)
                .child(self.locale.text("integration.noChanges"))
                .into_any_element();
        }
        if self.full || file.diff.is_empty() {
            let lines: Vec<_> = file.content.split('\n').collect();
            let width = lines
                .iter()
                .map(|text| self.text_width(text, window))
                .max()
                .unwrap_or(px(0.));
            return div()
                .id("integration-full-code")
                .track_scroll(&self.scroll)
                .track_focus(&self.panes[0])
                .tab_stop(true)
                .flex_1()
                .min_h_0()
                .min_w_0()
                .border_1()
                .border_color(t.divider)
                .rounded(px(8.))
                .overflow_scroll()
                .p(px(12.))
                .px(px(14.))
                .font_family(MONO)
                .text_size(px(12.))
                .line_height(px(19.2))
                .on_key_down(cx.listener(Self::scroll_key))
                .child(
                    div()
                        .w(width)
                        .min_w_full()
                        .flex()
                        .flex_col()
                        .children(lines.iter().map(|text| {
                            div()
                                .h(px(19.2))
                                .flex_shrink_0()
                                .whitespace_nowrap()
                                .child(styled(text, &file.path, t, None, t.text))
                        })),
                )
                .into_any_element();
        }
        let rows = &self.rows[self.active];
        let changes = diff::changes(rows);
        let widths = rows
            .iter()
            .flat_map(|r| [r.old.as_ref(), r.next.as_ref()])
            .flatten()
            .map(|s| self.text_width(&s.text, window));
        let width = widths.max().unwrap_or(px(0.)) + px(80.);
        let counts = [
            rows.iter().filter(|r| r.changed && r.old.is_some()).count(),
            rows.iter()
                .filter(|r| r.changed && r.next.is_some())
                .count(),
        ];
        div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .border_1()
            .border_color(t.border)
            .rounded(px(8.))
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(35.))
                    .flex_shrink_0()
                    .px(px(12.))
                    .border_b_1()
                    .border_color(t.border)
                    .bg(t.surface2)
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_size(px(11.))
                    .text_color(t.secondary)
                    .child(
                        self.locale
                            .text("integration.diffChangeCount")
                            .replace("{count}", &changes.len().to_string()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .when(!changes.is_empty(), |el| {
                                el.child(div().mr(px(6.)).text_color(t.tertiary).child(format!(
                                    "{} / {}",
                                    self.change + 1,
                                    changes.len()
                                )))
                            })
                            .child(
                                icon(
                                    "integration-diff-previous",
                                    "chevron-up",
                                    self.locale.text("integration.diffPrevious"),
                                    t,
                                )
                                .size(px(25.))
                                .disabled(changes.is_empty())
                                .on_click(cx.listener(|this, _, _, cx| this.navigate(false, cx))),
                            )
                            .child(
                                icon(
                                    "integration-diff-next",
                                    "chevron-down",
                                    self.locale.text("integration.diffNext"),
                                    t,
                                )
                                .size(px(25.))
                                .disabled(changes.is_empty())
                                .on_click(cx.listener(|this, _, _, cx| this.navigate(true, cx))),
                            ),
                    ),
            )
            .child(
                div()
                    .h(px(33.))
                    .flex_shrink_0()
                    .border_b_1()
                    .border_color(t.divider)
                    .flex()
                    .children((0..2).map(|side| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .px(px(12.))
                            .border_l_1()
                            .border_color(if side == 1 { t.border } else { gpui::rgba(0) })
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.secondary)
                            .child(self.locale.text(if side == 0 {
                                "integration.diffBefore"
                            } else {
                                "integration.diffAfter"
                            }))
                            .child(
                                div()
                                    .font_family(MONO)
                                    .text_color(if side == 0 { t.danger } else { t.success })
                                    .child(format!(
                                        "{}{}",
                                        if side == 0 { "−" } else { "+" },
                                        counts[side]
                                    )),
                            )
                    })),
            )
            .child(div().flex_1().min_h_0().flex().children((0..2).map(|side| {
                let change_color = if side == 0 { t.danger } else { t.success };
                // Both sides use an identical content extent and the same offset.
                // A shorter side cannot clamp a programmatic synchronization event.
                div()
                    .id(("integration-diff-pane", side))
                    .track_scroll(&self.scroll)
                    .track_focus(&self.panes[side])
                    .tab_stop(true)
                    .role(gpui::Role::Region)
                    .aria_label(self.locale.text(if side == 0 {
                        "integration.diffBefore"
                    } else {
                        "integration.diffAfter"
                    }))
                    .border_l_1()
                    .border_color(if side == 1 { t.border } else { gpui::rgba(0) })
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_scroll()
                    .font_family(MONO)
                    .text_size(px(12.))
                    .line_height(px(24.))
                    .on_key_down(cx.listener(Self::scroll_key))
                    .child(
                        div()
                            .w(width)
                            .min_w_full()
                            .py(px(4.))
                            .flex()
                            .flex_col()
                            .children(rows.iter().map(|row| {
                                let cell = if side == 0 {
                                    row.old.as_ref()
                                } else {
                                    row.next.as_ref()
                                };
                                let bg = if cell.is_none() {
                                    t.surface2
                                } else if row.changed {
                                    mix(change_color, t.surface, 0.09)
                                } else {
                                    t.surface
                                };
                                div()
                                    .relative()
                                    .h(px(24.))
                                    .flex_shrink_0()
                                    .bg(bg)
                                    .when(cell.is_none(), |el| el.child(missing_cell(t)))
                                    .child(
                                        div()
                                            .ml(px(66.))
                                            .pr(px(14.))
                                            .h_full()
                                            .flex()
                                            .items_center()
                                            .line_height(px(14.4))
                                            .whitespace_nowrap()
                                            .children(cell.map(|cell| {
                                                styled(
                                                    &cell.text,
                                                    &file.path,
                                                    t,
                                                    cell.changed.clone(),
                                                    change_color,
                                                )
                                            })),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .left(-self.scroll.offset().x)
                                            .top_0()
                                            .w(px(62.))
                                            .h(px(24.))
                                            .px(px(7.))
                                            .gap(px(7.))
                                            .flex()
                                            .bg(bg)
                                            .text_color(if row.changed && cell.is_some() {
                                                change_color
                                            } else {
                                                t.tertiary
                                            })
                                            .child(
                                                div().w(px(30.)).text_right().child(
                                                    cell.and_then(|c| c.line)
                                                        .map(|n| n.to_string())
                                                        .unwrap_or_default(),
                                                ),
                                            )
                                            .child(div().w(px(10.)).child(
                                                if row.changed && cell.is_some() {
                                                    if side == 0 {
                                                        "−"
                                                    } else {
                                                        "+"
                                                    }
                                                } else {
                                                    ""
                                                },
                                            )),
                                    )
                            })),
                    )
            })))
            .into_any_element()
    }
}
