//! Root window composition and recovery presentation.
#[cfg(feature = "parity-render")]
use super::ReplayToast;
use super::{CloseWindow, Desktop, LockVault};
use gpui::{div, prelude::*, px, rgb, Context, Window};

impl Render for Desktop {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.motion_only {
            return div()
                .size_full()
                .relative()
                .rounded(px(12.))
                .overflow_hidden()
                .bg(self.theme.bg)
                .children(self.curtain.clone())
                .into_any_element();
        }
        let recovery = self
            .session
            .as_ref()
            .and_then(|s| s.recovery.as_ref())
            .map(|key| key.expose().to_owned());
        let root = div()
            .id("desktop")
            .track_focus(&self.focus)
            .size_full()
            .font_weight(gpui::FontWeight::NORMAL)
            .font_features(gpui::FontFeatures(std::sync::Arc::new(vec![
                ("ss01".into(), 1),
                ("cv11".into(), 1),
            ])))
            .relative()
            .rounded(px(12.))
            .overflow_hidden()
            .on_action(cx.listener(|this, _: &CloseWindow, window, cx| {
                if this.request_close(window, cx) {
                    window.remove_window();
                    cx.quit();
                }
            }))
            .on_action(cx.listener(|this, _: &LockVault, window, cx| this.lock(window, cx)))
            .child(if self.show_workspace {
                self.workspace.clone().into_any_element()
            } else {
                self.auth.clone().into_any_element()
            })
            .when(!self.show_workspace, |el| {
                el.child(
                    div()
                        .absolute()
                        .left(px(19.))
                        .top(px(19.))
                        .flex()
                        .gap(px(8.))
                        .children([0xff5f57, 0xfebc2e, 0x28c840].into_iter().enumerate().map(
                            |(index, color)| {
                                div()
                                    .id(("window-control", index))
                                    .size(px(12.))
                                    .rounded_full()
                                    .bg(rgb(color))
                                    .border_1()
                                    .border_color(gpui::rgba(0x00000026))
                                    .on_click(move |_, window, cx| match index {
                                        0 => window.dispatch_action(Box::new(CloseWindow), cx),
                                        1 => window.minimize_window(),
                                        _ => window.zoom_window(),
                                    })
                            },
                        )),
                )
            })
            .when_some(recovery, |el, key| {
                el.child(
                    div()
                        .absolute()
                        .inset_0()
                        .bg(gpui::rgba(0x080c188c))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .w(px(440.))
                                .p(px(28.))
                                .rounded(px(14.))
                                .bg(self.theme.surface)
                                .text_color(self.theme.text)
                                .flex()
                                .flex_col()
                                .gap(px(14.))
                                .child(
                                    div()
                                        .text_size(px(18.))
                                        .child(self.locale.text("recovery.title")),
                                )
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .child(self.locale.text("recovery.shortDesc")),
                                )
                                .child(
                                    div()
                                        .p(px(14.))
                                        .rounded(px(8.))
                                        .bg(self.theme.surface2)
                                        .font_family("Menlo")
                                        .text_size(px(13.))
                                        .child(key),
                                )
                                .child(
                                    div()
                                        .id("recovery-confirm")
                                        .cursor_pointer()
                                        .text_size(px(12.))
                                        .child(format!(
                                            "{} {}",
                                            if self.recovery_confirmed {
                                                "☑"
                                            } else {
                                                "☐"
                                            },
                                            self.locale.text("recovery.ack")
                                        ))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.recovery_confirmed = !this.recovery_confirmed;
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    div()
                                        .id("recovery-continue")
                                        .h(px(36.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(8.))
                                        .bg(rgb(0x2563eb))
                                        .text_color(rgb(0xffffff))
                                        .when(!self.recovery_confirmed, |el| el.opacity(0.5))
                                        .child(self.locale.text("recovery.continue"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if this.recovery_confirmed {
                                                if let Some(session) = &mut this.session {
                                                    session.recovery = None;
                                                }
                                                this.recovery_confirmed = false;
                                                cx.notify();
                                            }
                                        })),
                                ),
                        ),
                )
            })
            .child(self.update_banner(cx))
            .children(self.curtain.clone())
            .child(self.toast.clone());
        #[cfg(feature = "parity-render")]
        let root = root.on_action(cx.listener(|this, _: &ReplayToast, window, cx| {
            if let Some(long) = this.toast_fixture.filter(|_| this.session.is_none()) {
                this.toast.update(cx, |toast, cx| {
                    toast.show(
                        crate::toast::fixture_message(this.chinese, long),
                        window,
                        cx,
                    )
                });
            }
        }));
        #[cfg(not(feature = "parity-render"))]
        let _ = self.toast_fixture;
        root.into_any_element()
    }
}
