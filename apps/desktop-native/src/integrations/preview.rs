use crate::{
    locale::Locale,
    providers::controls::{button, icon},
    theme::Theme,
};
use aipass_agent_protocol::{ToolConfigMode, ToolConfigPreviewFile, ToolConfigPreviewResponse};
use aipass_desktop_native::integration::diff;
use gpui::{
    div, prelude::*, px, AnyElement, Context, EventEmitter, FocusHandle, FontWeight, ScrollHandle,
    Window,
};
use gpui_base::{Button, Dialog};
mod code;
pub(super) const MONO: &str = if cfg!(target_os = "macos") {
    ".AppleSystemUIFontMonospaced"
} else {
    "monospace"
};
#[cfg(all(test, feature = "parity-render"))]
mod tests;

pub enum Event {
    Close,
    Confirm,
}
impl EventEmitter<Event> for Preview {}
pub struct Preview {
    response: ToolConfigPreviewResponse,
    files: Vec<ToolConfigPreviewFile>,
    rows: Vec<Vec<diff::Row>>,
    active: usize,
    full: bool,
    change: usize,
    scroll: ScrollHandle,
    focus: FocusHandle,
    panes: [FocusHandle; 2],
    allow_confirm: bool,
    pub busy: bool,
    theme: Theme,
    locale: Locale,
}
impl Preview {
    pub fn new(
        response: ToolConfigPreviewResponse,
        allow_confirm: bool,
        dark: bool,
        chinese: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let files = if response.files.is_empty() {
            vec![ToolConfigPreviewFile {
                path: response.target_path.clone(),
                content: response.preview.clone(),
                diff: response.preview.clone(),
            }]
        } else {
            response.files.clone()
        };
        let rows = files.iter().map(|f| diff::parse(&f.diff)).collect();
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            response,
            files,
            rows,
            active: 0,
            full: false,
            change: 0,
            scroll: ScrollHandle::new(),
            focus,
            panes: std::array::from_fn(|_| cx.focus_handle()),
            allow_confirm,
            busy: false,
            theme: Theme::new(dark),
            locale: Locale::new(chinese),
        }
    }
    pub fn can_confirm(&self) -> bool {
        self.allow_confirm && !self.busy
    }
    fn reset_scroll(&mut self) {
        self.scroll = ScrollHandle::new();
        self.change = 0;
    }
    fn choose_file(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.files.len() {
            self.active = index;
            self.reset_scroll();
            cx.notify();
        }
    }
    fn choose_mode(&mut self, full: bool, cx: &mut Context<Self>) {
        self.full = full;
        self.reset_scroll();
        cx.notify();
    }
    fn navigate(&mut self, forward: bool, cx: &mut Context<Self>) {
        let changes = diff::changes(&self.rows[self.active]);
        if changes.is_empty() {
            return;
        }
        self.change = if forward {
            (self.change + 1) % changes.len()
        } else {
            (self.change + changes.len() - 1) % changes.len()
        };
        let top = px(changes[self.change] as f32 * 24. + 4.).min(self.scroll.max_offset().y);
        self.scroll
            .set_offset(gpui::point(self.scroll.offset().x, -top));
        cx.notify();
    }
    fn access_note(&self) -> (String, Option<String>) {
        let env = self.files.iter().find(|f| {
            let p = f.path.replace('\\', "/");
            p.contains("/.aipass/tools/") && (p.ends_with(".env") || p.ends_with(".ps1"))
        });
        let key = match self.response.mode {
            ToolConfigMode::Plaintext => "integration.plaintextNote",
            ToolConfigMode::Helper if env.is_none() => "integration.helperNote",
            ToolConfigMode::Helper | ToolConfigMode::Env => "integration.envNote",
            ToolConfigMode::Official => "integration.officialNote",
        };
        let launch = env.map(|f| {
            if f.path.ends_with(".ps1") {
                format!(". '{}'", f.path.replace('\'', "''"))
            } else {
                format!("source '{}'", f.path.replace('\'', "'\\''"))
            }
        });
        (self.locale.text(key), launch)
    }
    #[cfg(feature = "parity-render")]
    pub fn presentation(&self) -> serde_json::Value {
        let rows = &self.rows[self.active];
        serde_json::json!({"activeFile":self.active,"full":self.full,"files":self.files.len(),"rows":rows.len(),"changes":diff::changes(rows),"change":self.change,"confirm":self.allow_confirm,"busy":self.busy,"scroll":[f32::from(self.scroll.offset().x),f32::from(self.scroll.offset().y)]})
    }
    #[cfg(feature = "parity-render")]
    pub fn fixture_state(&mut self, scenario: &str, cx: &mut Context<Self>) {
        if scenario.ends_with("-full") {
            self.choose_mode(true, cx);
        }
        if scenario.ends_with("-file2") {
            self.choose_file(1, cx);
        }
        if scenario.ends_with("-unchanged") {
            self.choose_file(3, cx);
        }
    }
}
impl Render for Preview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let (note, launch) = self.access_note();
        let measure_tab = |name: &str| {
            window
                .text_system()
                .shape_line(
                    name.to_owned().into(),
                    px(12.),
                    &[gpui::TextRun {
                        len: name.len(),
                        font: gpui::Font {
                            weight: FontWeight::MEDIUM,
                            ..gpui::font(".AppleSystemUIFont")
                        },
                        color: t.text.into(),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    }],
                    None,
                )
                .width
                + px(24.)
        };
        let mode_width = ["integration.showDiff", "integration.showFull"]
            .iter()
            .map(|key| measure_tab(&self.locale.text(key)))
            .max()
            .unwrap_or(px(80.));
        let tab_width = self
            .files
            .iter()
            .map(|file| measure_tab(file.path.rsplit(['/', '\\']).next().unwrap_or(&file.path)))
            .max()
            .unwrap_or(px(80.));
        let content = div()
            .on_children_prepainted(crate::parity::record(&[
                "integration-dialog-header",
                "integration-dialog-filebar",
                "integration-dialog-code",
                "integration-dialog-footer",
            ]))
            .w_full()
            .h(px(720.).min(window.viewport_size().height - px(48.)))
            .px(px(20.))
            .pt(px(18.))
            .pb(px(16.))
            .rounded(px(14.))
            .border_1()
            .border_color(t.border)
            .bg(t.surface)
            .text_color(t.text)
            .font_weight(FontWeight::NORMAL)
            .flex()
            .flex_col()
            .gap(px(12.))
            .overflow_hidden()
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div().flex_1().min_w_0().child(
                            div()
                                .text_size(px(15.))
                                .line_height(px(18.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(self.locale.text("providerDetail.preview")),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .flex_shrink_0()
                            .child(
                                div()
                                    .p(px(2.))
                                    .border_1()
                                    .border_color(t.border)
                                    .rounded(px(8.))
                                    .bg(t.surface2)
                                    .flex()
                                    .gap(px(2.))
                                    .children(
                                        ["integration.showDiff", "integration.showFull"]
                                            .into_iter()
                                            .enumerate()
                                            .map(|(i, key)| {
                                                Button::new(("integration-view-mode", i))
                                                    .w(mode_width)
                                                    .h(px(26.))
                                                    .px(px(12.))
                                                    .rounded(px(4.))
                                                    .text_size(px(12.))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .bg(if self.full == (i == 1) {
                                                        t.surface
                                                    } else {
                                                        gpui::rgba(0)
                                                    })
                                                    .text_color(if self.full == (i == 1) {
                                                        t.text
                                                    } else {
                                                        t.secondary
                                                    })
                                                    .hover(move |s| s.text_color(t.text))
                                                    .when(self.full == (i == 1), |el| {
                                                        el.shadow(vec![gpui::BoxShadow {
                                                            color: gpui::rgb(0x0f1110)
                                                                .opacity(0.08)
                                                                .into(),
                                                            offset: gpui::point(px(0.), px(1.)),
                                                            blur_radius: px(2.),
                                                            spread_radius: px(0.),
                                                            inset: false,
                                                        }])
                                                    })
                                                    .accessibility_label(self.locale.text(key))
                                                    .child(self.locale.text(key))
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.choose_mode(i == 1, cx)
                                                    }))
                                            }),
                                    ),
                            )
                            .child(
                                icon(
                                    "integration-dialog-close",
                                    "x",
                                    self.locale.text("common.cancel"),
                                    t,
                                )
                                .disabled(self.busy)
                                .on_click(cx.listener(|_, _, _, cx| cx.emit(Event::Close))),
                            ),
                    ),
            )
            .child(
                div()
                    .mb(px(-4.))
                    .flex_shrink_0()
                    .min_h(px(30.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .when(self.files.len() > 1, |el| {
                        el.child(
                            div()
                                .p(px(2.))
                                .border_1()
                                .border_color(t.border)
                                .rounded(px(8.))
                                .bg(t.surface2)
                                .flex()
                                .gap(px(2.))
                                .children(self.files.iter().enumerate().map(|(i, file)| {
                                    let name = file
                                        .path
                                        .rsplit(['/', '\\'])
                                        .next()
                                        .unwrap_or(&file.path)
                                        .to_owned();
                                    Button::new(("integration-file", i))
                                        .w(tab_width)
                                        .h(px(26.))
                                        .px(px(12.))
                                        .rounded(px(4.))
                                        .text_size(px(12.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(if self.active == i {
                                            t.text
                                        } else {
                                            t.secondary
                                        })
                                        .hover(move |s| s.text_color(t.text))
                                        .when(self.active == i, |el| {
                                            el.shadow(vec![gpui::BoxShadow {
                                                color: gpui::rgb(0x0f1110).opacity(0.08).into(),
                                                offset: gpui::point(px(0.), px(1.)),
                                                blur_radius: px(2.),
                                                spread_radius: px(0.),
                                                inset: false,
                                            }])
                                        })
                                        .bg(if self.active == i {
                                            t.surface
                                        } else {
                                            gpui::rgba(0)
                                        })
                                        .accessibility_label(name.clone())
                                        .child(name)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.choose_file(i, cx)
                                        }))
                                })),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .font_family(MONO)
                            .text_size(px(11.))
                            .line_height(px(13.2))
                            .text_color(t.tertiary)
                            .child(self.files[self.active].path.clone()),
                    ),
            )
            .child(self.code(window, cx))
            .child(
                div()
                    .flex_shrink_0()
                    .pt(px(12.))
                    .border_t_1()
                    .border_color(t.divider)
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(11.))
                            .line_height(px(13.2))
                            .text_color(t.tertiary)
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .line_height(px(16.5))
                                    .text_color(t.secondary)
                                    .child(note),
                            )
                            .when_some(launch, |el, text| {
                                el.child(div().font_family(MONO).child(text))
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .flex_shrink_0()
                            .child(
                                button(
                                    "integration-dialog-cancel",
                                    self.locale.text("common.cancel"),
                                    false,
                                    t,
                                )
                                .border_color(gpui::rgba(0))
                                .disabled(self.busy)
                                .on_click(cx.listener(|_, _, _, cx| cx.emit(Event::Close))),
                            )
                            .when(self.allow_confirm, |el| {
                                el.child(
                                    button(
                                        "integration-confirm",
                                        self.locale.text("server.writeConfig"),
                                        true,
                                        t,
                                    )
                                    .disabled(self.busy)
                                    .opacity(if self.busy { 0.45 } else { 1. })
                                    .on_click(cx.listener(|_, _, _, cx| cx.emit(Event::Confirm))),
                                )
                            }),
                    ),
            );
        let owner = cx.entity().downgrade();
        Dialog::new(cx)
            .focus_handle(self.focus.clone())
            .accessibility_label(self.locale.text("providerDetail.preview"))
            .close_on_escape(!self.busy)
            .close_on_backdrop_press(!self.busy)
            .on_open_change(move |open, _, _, cx| {
                if !open {
                    let _ = owner.update(cx, |_, cx| cx.emit(Event::Close));
                }
            })
            .backdrop(div().absolute().inset_0().bg(gpui::rgba(0x0f111073)))
            .popup(
                div()
                    .w(px(1120.).min(window.viewport_size().width - px(48.)))
                    .child(content),
            )
    }
}
