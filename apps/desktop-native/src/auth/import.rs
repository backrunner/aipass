//! Setup import fields. No filesystem or vault writes happen in the view.
use crate::{
    form_ui::{
        self,
        select::{Choice, Selected},
    },
    locale::Locale,
    theme::Theme,
};
use aipass_agent_protocol::SensitiveString;
use aipass_desktop_native::onboarding::{self, Import};
use gpui::{div, prelude::*, px, Context, Entity, EventEmitter, Subscription, Window};
use gpui_base::input::{InputEvent, InputState};
pub enum Event {
    Back,
    Submit(Import),
}
pub struct ImportForm {
    pub source: Entity<Choice>,
    inputs: [Entity<InputState>; 5],
    theme: Theme,
    locale: Locale,
    busy: bool,
    picking: bool,
    generation: u64,
    picker_error: String,
    _subscriptions: Vec<Subscription>,
}
impl EventEmitter<Event> for ImportForm {}
impl ImportForm {
    pub fn new(dark: bool, chinese: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let theme = Theme::new(dark);
        let locale = Locale::new(chinese);
        let source = cx.new(|cx| {
            Choice::new(
                locale.text("auth.import.source"),
                [
                    ("backup", locale.text("auth.import.backup")),
                    ("vault", locale.text("auth.import.vault")),
                    ("icloud", "iCloud (CloudKit)".into()),
                    ("webdav", "WebDAV".into()),
                    ("local", locale.text("auth.import.folder")),
                ]
                .into_iter()
                .map(|(k, v)| (k.into(), v))
                .collect(),
                "backup",
                theme,
                cx,
            )
        });
        let inputs = std::array::from_fn(|i| {
            form_ui::input(
                [
                    "auth.import.path",
                    "auth.import.originalPassword",
                    "auth.import.webdavUrl",
                    "auth.import.username",
                    "auth.import.webdavPassword",
                ][i],
                "",
                matches!(i, 1 | 4),
                theme,
                &locale,
                window,
                cx,
            )
        });
        let mut subscriptions: Vec<_> = inputs
            .iter()
            .map(|input| {
                cx.subscribe_in(input, window, |this, _, event, window, cx| match event {
                    InputEvent::PressEnter { .. } => this.submit(window, cx),
                    InputEvent::Change => cx.notify(),
                    _ => {}
                })
            })
            .collect();
        subscriptions.push(cx.subscribe_in(
            &source,
            window,
            |this, _, selected: &Selected, window, cx| {
                debug_assert_eq!(this.source.read(cx).selected, selected.0);
                this.generation += 1;
                this.picking = false;
                this.picker_error.clear();
                this.clear_secrets(window, cx);
                this.placeholders(window, cx);
                cx.notify();
            },
        ));
        let mut form = Self {
            source,
            inputs,
            theme,
            locale,
            busy: false,
            picking: false,
            generation: 0,
            picker_error: String::new(),
            _subscriptions: subscriptions,
        };
        form.placeholders(window, cx);
        form
    }
    fn placeholders(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let source = self.source.read(cx).selected.clone();
        let backup = source == "backup";
        use gpui::Focusable;
        let next = self.inputs[if source == "icloud" {
            1
        } else if source == "webdav" {
            2
        } else {
            0
        }]
        .read(cx)
        .focus_handle(cx);
        self.source.update(cx, |s, cx| {
            s.next = Some(next);
            cx.notify();
        });
        self.inputs[0].update(cx, |s, cx| {
            s.set_placeholder(
                if backup {
                    "/…/vault.aipass-backup"
                } else {
                    "/…/AIPass"
                },
                window,
                cx,
            )
        });
        self.inputs[2].update(cx, |s, cx| {
            s.set_placeholder("https://…/AIPass", window, cx)
        });
    }
    fn clear_secrets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for i in [1, 4] {
            self.inputs[i].update(cx, |s, cx| {
                s.set_value("", window, cx);
                s.set_masked(true, window, cx);
            });
        }
    }
    pub fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.generation += 1;
        self.picking = false;
        self.picker_error.clear();
        self.set_busy(false, cx);
        for s in &self.inputs {
            s.update(cx, |s, cx| s.set_value("", window, cx));
        }
        self.clear_secrets(window, cx);
        self.source.update(cx, |s, cx| {
            s.selected = "backup".into();
            s.close(cx);
        });
        self.placeholders(window, cx);
    }
    pub fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        self.busy = busy;
        for input in &self.inputs {
            input.update(cx, |s, cx| s.set_disabled(busy, cx));
        }
        self.source.update(cx, |s, cx| {
            s.disabled = busy;
            cx.notify();
        });
        cx.notify();
    }
    pub fn appearance(&mut self, dark: bool, cx: &mut Context<Self>) {
        self.theme = Theme::new(dark);
        for input in &self.inputs {
            input.update(cx, |s, cx| {
                s.set_editor_style(self.theme.input_style());
                cx.notify();
            });
        }
        self.source.update(cx, |s, cx| {
            s.theme = self.theme;
            cx.notify();
        });
        cx.notify();
    }
    fn uses_path(&self, cx: &gpui::App) -> bool {
        matches!(
            self.source.read(cx).selected.as_str(),
            "backup" | "vault" | "local"
        )
    }
    fn ready(&self, cx: &gpui::App) -> bool {
        !self.busy
            && !self.picking
            && !self.inputs[1].read(cx).value().is_empty()
            && (!self.uses_path(cx) || !self.inputs[0].read(cx).value().trim().is_empty())
            && (self.source.read(cx).selected != "webdav"
                || !self.inputs[2].read(cx).value().trim().is_empty())
    }
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ready(cx) {
            return;
        }
        let value = |i: usize| self.inputs[i].read(cx).value().to_string();
        let source = self.source.read(cx).selected.clone();
        let password = SensitiveString::new(value(1));
        let request = if matches!(source.as_str(), "backup" | "vault") {
            Import::Archive {
                path: value(0).trim().into(),
                password,
            }
        } else {
            Import::Sync {
                settings: onboarding::sync_settings(
                    &source,
                    value(0).trim().into(),
                    value(2).trim().into(),
                    value(3).trim().into(),
                    SensitiveString::new(value(4)),
                ),
                password,
            }
        };
        self.clear_secrets(window, cx);
        self.set_busy(true, cx);
        cx.emit(Event::Submit(request));
    }
    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.picking {
            return;
        }
        self.picking = true;
        self.picker_error.clear();
        let generation = self.generation;
        let files = self.source.read(cx).selected == "backup";
        let result = cx.prompt_for_paths(gpui::PathPromptOptions {
            files,
            directories: !files,
            multiple: false,
            prompt: Some(self.locale.text("auth.import.browse").into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = result.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.generation != generation {
                    return;
                }
                this.picking = false;
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.first() {
                            this.inputs[0].update(cx, |s, cx| {
                                s.set_value(path.to_string_lossy().to_string(), window, cx)
                            });
                        }
                    }
                    Ok(Ok(None)) => {}
                    _ => {
                        this.picker_error =
                            if this.locale.language() == aipass_agent_protocol::UiLocale::ZhCn {
                                "无法打开文件选择器，请重试"
                            } else {
                                "Could not open the file picker. Try again."
                            }
                            .into()
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn field(&self, i: usize, key: &str, window: &Window, cx: &gpui::App) -> gpui::Div {
        div()
            .min_w_0()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(form_ui::label(self.locale.text(key), self.theme))
            .child(form_ui::field(
                &self.inputs[i],
                [
                    "import-path",
                    "import-password",
                    "import-url",
                    "import-username",
                    "import-webdav-password",
                ][i],
                matches!(i, 1 | 4),
                self.theme,
                &self.locale,
                window,
                cx,
            ))
    }
}
impl Render for ImportForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let source = self.source.read(cx).selected.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(22.))
                            .line_height(px(26.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.locale.text("auth.import.title")),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(19.5))
                            .text_color(t.secondary)
                            .child(self.locale.text("auth.import.desc")),
                    ),
            )
            .child(self.source.clone())
            .when(self.uses_path(cx), |el| {
                el.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(form_ui::label(self.locale.text("auth.import.path"), t))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(form_ui::field(
                                    &self.inputs[0],
                                    "import-path",
                                    false,
                                    t,
                                    &self.locale,
                                    window,
                                    cx,
                                ))
                                .child(
                                    crate::providers::controls::button(
                                        "import-browse",
                                        self.locale.text("auth.import.browse"),
                                        false,
                                        t,
                                    )
                                    .flex_shrink_0()
                                    .disabled(self.busy || self.picking)
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.browse(window, cx)),
                                    ),
                                ),
                        ),
                )
            })
            .when(source == "webdav", |el| {
                el.child(self.field(2, "auth.import.webdavUrl", window, cx))
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap(px(12.))
                            .child(self.field(3, "auth.import.username", window, cx))
                            .child(self.field(4, "auth.import.webdavPassword", window, cx)),
                    )
            })
            .child(self.field(
                1,
                if source == "backup" {
                    "auth.import.backupPassword"
                } else {
                    "auth.import.originalPassword"
                },
                window,
                cx,
            ))
            .when(!self.picker_error.is_empty(), |el| {
                el.child(
                    div()
                        .text_size(px(13.))
                        .text_color(t.danger)
                        .child(self.picker_error.clone()),
                )
            })
            .child(
                crate::providers::controls::button(
                    "import-submit",
                    self.locale.text(if self.busy {
                        "auth.import.busy"
                    } else {
                        "auth.import.submit"
                    }),
                    true,
                    t,
                )
                .disabled(!self.ready(cx))
                .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
            )
            .child(
                crate::providers::controls::ghost_button("import-back", false, t)
                    .accessibility_label(self.locale.text("auth.setup.back"))
                    .disabled(self.busy)
                    .child(self.locale.text("auth.setup.back"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.clear(window, cx);
                        cx.emit(Event::Back);
                    })),
            )
    }
}

#[cfg(feature = "parity-render")]
impl ImportForm {
    pub fn preview(&mut self, source: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.source.update(cx, |s, cx| {
            s.selected = source.into();
            cx.notify();
        });
        self.placeholders(window, cx);
    }
}
#[cfg(all(test, feature = "parity-render"))]
mod tests;
