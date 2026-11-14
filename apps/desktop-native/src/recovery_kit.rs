//! Recovery acknowledgment retains focus and uses the shared expiring clipboard.
use crate::parity::Observe;
use crate::{form_ui, locale::Locale, theme::Theme};
use aipass_agent_protocol::SensitiveString;
use aipass_desktop_platform::clipboard::{Cancellation, CopyPermit};
use gpui::{div, prelude::*, px, svg, Context, FocusHandle, Window};
use gpui_base::{Dialog, DialogPopup, Transition, TransitionState};
use std::time::Duration;
pub struct RecoveryKit {
    key: Option<SensitiveString>,
    confirmed: bool,
    copied: bool,
    copying: bool,
    closing: bool,
    generation: u64,
    error: String,
    cancellation: Option<Cancellation>,
    focus: FocusHandle,
    restore: Option<FocusHandle>,
    theme: Theme,
    locale: Locale,
    popup: TransitionState<f32>,
    overlay: TransitionState<f32>,
    initialized: bool,
}
impl RecoveryKit {
    pub fn new(dark: bool, chinese: bool, cx: &mut Context<Self>) -> Self {
        Self {
            key: None,
            confirmed: false,
            copied: false,
            copying: false,
            closing: false,
            generation: 0,
            error: String::new(),
            cancellation: None,
            focus: cx.focus_handle(),
            restore: None,
            theme: Theme::new(dark),
            locale: Locale::new(chinese),
            popup: Default::default(),
            overlay: Default::default(),
            initialized: false,
        }
    }
    pub fn show(&mut self, key: SensitiveString, window: &mut Window, cx: &mut Context<Self>) {
        self.clear(cx);
        self.key = Some(key);
        self.restore = window.focused(cx);
        self.focus.focus(window, cx);
        cx.notify();
    }
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        self.key = None;
        self.confirmed = false;
        self.copied = false;
        self.copying = false;
        self.closing = false;
        self.error.clear();
        self.initialized = false;
        self.restore = None;
        if let Some(cancel) = self.cancellation.take() {
            cancel.cancel();
        }
        cx.notify();
    }
    pub fn appearance(&mut self, dark: bool, cx: &mut Context<Self>) {
        self.theme = Theme::new(dark);
        cx.notify();
    }
    fn copy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.copying || self.closing || self.key.is_none() {
            return;
        }
        let permit = match CopyPermit::begin() {
            Ok(Some(p)) => p,
            Ok(None) => return,
            Err(e) => {
                self.error = e;
                cx.notify();
                return;
            }
        };
        self.cancellation = Some(permit.cancellation());
        self.copying = true;
        let generation = self.generation;
        let prefs = cx
            .try_global::<crate::preferences::Preferences>()
            .map(|p| p.0.clone())
            .unwrap_or_default();
        let prompt = if prefs.confirm_secret_copy {
            let (title, detail, copy, cancel) =
                aipass_desktop_platform::clipboard::confirmation(self.locale.language());
            Some(window.prompt(
                gpui::PromptLevel::Warning,
                title,
                Some(detail),
                &[copy, cancel],
                cx,
            ))
        } else {
            None
        };
        cx.spawn_in(window, async move |this, cx| {
            let accepted = match prompt {
                Some(answer) => answer.await == Ok(0),
                None => true,
            };
            let key = this
                .update(cx, |this, cx| {
                    if this.generation != generation {
                        return None;
                    }
                    if !accepted || !permit.is_current() {
                        this.copying = false;
                        this.cancellation = None;
                        cx.notify();
                        return None;
                    }
                    this.key.clone()
                })
                .ok()
                .flatten();
            let Some(key) = key else {
                return;
            };
            let result = cx
                .background_executor()
                .spawn(async move { permit.write(key, prefs.clipboard_clear_seconds) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.copying = false;
                this.cancellation = None;
                match result {
                    Ok(true) => {
                        this.copied = true;
                        this.error.clear();
                    }
                    Ok(false) => {}
                    Err(e) => this.error = e,
                }
                cx.notify();
            });
            cx.background_executor()
                .timer(Duration::from_millis(1800))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.generation == generation {
                    this.copied = false;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.confirmed || self.closing || self.copying {
            return;
        }
        self.closing = true;
        let generation = self.generation;
        let delay = if cx.reduce_motion() { 0 } else { 220 };
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(delay))
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.generation != generation {
                    return;
                }
                let restore = this.restore.take();
                this.clear(cx);
                if let Some(focus) = restore {
                    focus.focus(window, cx);
                }
            });
        })
        .detach();
        cx.notify();
    }
}
impl Drop for RecoveryKit {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancellation.take() {
            cancel.cancel();
        }
    }
}
impl Render for RecoveryKit {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(key) = self.key.as_ref() else {
            return div().into_any_element();
        };
        let key = key.expose().to_owned();
        let t = self.theme;
        let closing = self.closing;
        if !self.initialized {
            self.popup
                .sample(0., Transition::new(Duration::ZERO), window, cx);
            self.overlay
                .sample(0., Transition::new(Duration::ZERO), window, cx);
            self.initialized = true;
        }
        let target = if closing { 0. } else { 1. };
        let popup = self
            .popup
            .sample(
                target,
                Transition::new(Duration::from_millis(if closing { 220 } else { 280 })).ease(
                    move |x| {
                        aipass_desktop_native::motion::cubic_bezier(
                            f64::from(x),
                            if closing { 0.4 } else { 0.22 },
                            if closing { 0. } else { 1. },
                            if closing { 0.85 } else { 0.36 },
                            if closing { 0.4 } else { 1. },
                        ) as f32
                    },
                ),
                window,
                cx,
            )
            .value;
        let overlay = self
            .overlay
            .sample(
                target,
                Transition::new(Duration::from_millis(if closing { 200 } else { 220 })).ease(|x| {
                    aipass_desktop_native::motion::cubic_bezier(f64::from(x), 0.4, 0., 0.2, 1.)
                        as f32
                }),
                window,
                cx,
            )
            .value;
        let owner = cx.entity().downgrade();
        let content = DialogPopup::new()
            .w(px(440.))
            .max_w(window.viewport_size().width - px(32.))
            .max_h(window.viewport_size().height - px(32.))
            .p(px(28.))
            .pb(px(24.))
            .rounded(px(14.))
            .border_1()
            .border_color(t.border)
            .bg(t.surface)
            .text_color(t.text)
            .flex()
            .flex_col()
            .gap(px(14.))
            .shadow(vec![gpui_base::box_shadow(
                0.,
                24.,
                crate::theme::shadow_blur(56.),
                0.,
                gpui::rgba(0x080c1852).into(),
            )])
            .child(
                div()
                    .size(px(36.))
                    .rounded_full()
                    .bg(t.accent_soft)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(svg().path("key-round").size(px(18.)).text_color(t.accent)),
            )
            .child(
                div()
                    .mt(px(2.))
                    .text_size(px(18.))
                    .line_height(px(21.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(self.locale.text("recovery.title"))
                    .observe("recovery-title"),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .line_height(px(19.5))
                    .text_color(t.secondary)
                    .child(self.locale.text("recovery.shortDesc"))
                    .observe("recovery-sub"),
            )
            .child(
                div()
                    .mt(px(4.))
                    .px(px(16.))
                    .py(px(14.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(t.border)
                    .bg(t.surface2)
                    .font_family(if cfg!(target_os = "macos") {
                        ".AppleSystemUIFontMonospaced"
                    } else {
                        "monospace"
                    })
                    .text_size(px(13.))
                    .line_height(px(20.15))
                    .child(key)
                    .observe("recovery-key"),
            )
            .child(
                div().flex().child(
                    crate::providers::controls::ghost_button("recovery-copy", false, t)
                        .accessibility_label(self.locale.text(if self.copied {
                            "recovery.copied"
                        } else {
                            "common.copy"
                        }))
                        .h(px(30.))
                        .bg(t.surface2)
                        .text_color(t.secondary)
                        .text_size(px(12.))
                        .gap(px(6.))
                        .disabled(self.copying || closing)
                        .child(
                            svg()
                                .path(if self.copied { "check" } else { "copy" })
                                .size(px(14.))
                                .text_color(t.secondary),
                        )
                        .child(self.locale.text(if self.copied {
                            "recovery.copied"
                        } else {
                            "common.copy"
                        }))
                        .on_click(cx.listener(|this, _, window, cx| this.copy(window, cx))),
                ),
            )
            .child(
                form_ui::check(
                    "recovery-confirm",
                    self.locale.text("recovery.ack"),
                    self.confirmed,
                    t,
                )
                .px(px(12.))
                .py(px(10.))
                .rounded(px(8.))
                .bg(t.surface2)
                .disabled(closing)
                .on_change(move |state, _, _, cx| {
                    let _ = owner.update(cx, |this, cx| {
                        this.confirmed = state == gpui_base::CheckboxState::Checked;
                        cx.notify();
                    });
                }),
            )
            .when(!self.error.is_empty(), |el| {
                el.child(
                    div()
                        .text_size(px(12.))
                        .text_color(t.danger)
                        .child(self.error.clone()),
                )
            })
            .child(
                crate::providers::controls::button(
                    "recovery-continue",
                    self.locale.text("recovery.continue"),
                    true,
                    t,
                )
                .disabled(!self.confirmed || closing || self.copying)
                .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
            );
        Dialog::new(cx)
            .focus_handle(self.focus.clone())
            .accessibility_label(self.locale.text("recovery.title"))
            .close_on_escape(false)
            .close_on_backdrop_press(false)
            .on_ok(|_, _, _| false)
            .backdrop(
                div()
                    .absolute()
                    .inset_0()
                    .bg(gpui::rgba(0x080c188c))
                    .backdrop_blur(px(6.))
                    .opacity(overlay),
            )
            .popup(
                crate::composite::Scale::new(
                    div()
                        .opacity(popup)
                        .child(content.observe("recovery-dialog")),
                    if closing {
                        0.97 + 0.03 * popup
                    } else {
                        0.96 + 0.04 * popup
                    },
                )
                .translate_relative(gpui::point(
                    0.,
                    (if closing { 0.02 } else { 0.04 }) * (1. - popup),
                )),
            )
            .into_any_element()
    }
}

#[cfg(feature = "parity-render")]
impl RecoveryKit {
    pub fn preview(&mut self, scenario: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.show(
            SensitiveString::new("AIPASS-ABCD-EFGH-JKLM-NPQR-STUV-WXYZ"),
            window,
            cx,
        );
        self.confirmed = scenario.ends_with("confirmed");
    }
}
#[cfg(all(test, feature = "parity-render"))]
mod tests {
    use super::*;
    #[gpui::test]
    fn acknowledgment_is_required_and_lock_retires_key(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_base::init);
        let (view, cx) = cx.add_window_view(|_, cx| RecoveryKit::new(false, false, cx));
        cx.update(|w, cx| {
            view.update(cx, |v, cx| {
                v.show(SensitiveString::new("fixture-only"), w, cx);
                v.close(w, cx);
                assert!(!v.closing);
                v.appearance(true, cx);
                assert!(v.key.is_some());
                v.confirmed = true;
                v.close(w, cx);
                assert!(v.closing);
                v.clear(cx);
                assert!(v.key.is_none());
                assert!(!v.confirmed);
                assert!(!v.closing);
            })
        });
    }
}
