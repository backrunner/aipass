//! Confirm before requesting plaintext; late reads cannot write after invalidation.
use super::*;
use aipass_desktop_platform::{clipboard::CopyPermit, preferences::CredentialClickAction};

impl Credentials {
    pub(super) fn copy_on_click(cx: &gpui::App) -> bool {
        cx.try_global::<crate::preferences::Preferences>()
            .is_some_and(|p| p.0.credential_click_action == CredentialClickAction::Copy)
    }
    pub(super) fn cancel_copy(&mut self) {
        if let Some(cancel) = self.copy_cancellation.take() {
            cancel.cancel();
        }
        self.copy_task = None;
    }
    pub(super) fn copy_key(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        let Some(entry) = &self.entry else {
            return;
        };
        if !entry.secret_refs.iter().any(|key| key.id == id) {
            return;
        }
        let entry_id = entry.id;
        let permit = match CopyPermit::begin() {
            Ok(Some(permit)) => permit,
            Ok(None) => return,
            Err(error) => {
                self.error = error;
                cx.notify();
                return;
            }
        };
        let policy = cx
            .try_global::<crate::preferences::Preferences>()
            .map(|p| p.0.clone())
            .unwrap_or_default();
        self.cancel_copy();
        self.copied = None;
        self.notice_timer = None;
        self.copy_cancellation = Some(permit.cancellation());
        if !policy.confirm_secret_copy {
            self.send(
                Operation::Copy {
                    secret_id: id.clone(),
                    permit,
                    seconds: policy.clipboard_clear_seconds,
                },
                Pending::Copy(id),
                cx,
            );
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.pending = Some(Pending::Copy(id.clone()));
        self.error.clear();
        self.notice.clear();
        self.update_disabled(cx);
        let (title, detail, copy, cancel) =
            aipass_desktop_platform::clipboard::confirmation(self.locale.language());
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            title,
            Some(detail),
            &[copy, cancel],
            cx,
        );
        self.copy_task = Some(cx.spawn(async move |this, cx| {
            let accepted = answer.await == Ok(0);
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation
                    || this.entry.as_ref().map(|e| e.id) != Some(entry_id)
                {
                    return;
                }
                this.pending = None;
                if accepted && permit.is_current() && !this.blocked {
                    this.send(
                        Operation::Copy {
                            secret_id: id.clone(),
                            permit,
                            seconds: policy.clipboard_clear_seconds,
                        },
                        Pending::Copy(id),
                        cx,
                    );
                } else {
                    this.copy_cancellation = None;
                    this.update_disabled(cx);
                    cx.emit(Event::Changed);
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }
}
impl Drop for Credentials {
    fn drop(&mut self) {
        self.cancel_copy();
    }
}

impl Credentials {
    pub(super) fn copy_control(
        &self,
        secret: &SecretRef,
        index: usize,
        visible: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        use gpui::{div, px, svg};
        let t = self.theme;
        if self.copied.as_ref() == Some(&secret.id) {
            return div()
                .flex_shrink_0()
                .h(px(28.))
                .flex()
                .items_center()
                .gap(px(4.))
                .text_size(px(11.))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(t.accent)
                .child(svg().path("check").size(px(13.)).text_color(t.accent))
                .child(self.locale.text("providerDetail.copied"))
                .into_any_element();
        }
        let opacity = gpui_base::transition(
            format!("credential-copy-opacity-{}", secret.id),
            if visible { 1. } else { 0. },
            gpui_base::Transition::new(Duration::from_millis(120)).ease(|x| {
                aipass_desktop_native::motion::cubic_bezier(f64::from(x), 0.25, 0.1, 0.25, 1.)
                    as f32
            }),
            window,
            cx,
        );
        let id = secret.id.clone();
        gpui_base::Button::new(("credential-copy", index))
            .size(px(28.))
            .flex_shrink_0()
            .rounded(px(8.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .opacity(opacity)
            .focus_visible(move |s| s.shadow(t.outline(t.focus_ring, 2., 1.)))
            .hover(move |s| s.bg(t.accent_soft))
            .accessibility_label(
                self.locale
                    .text("providerDetail.copySecret")
                    .replace("{label}", &secret.label),
            )
            .disabled(self.busy())
            .child(svg().path("copy").size(px(13.)).text_color(t.tertiary))
            .on_click(cx.listener(move |this, _, window, cx| this.copy_key(id.clone(), window, cx)))
            .into_any_element()
    }
}
