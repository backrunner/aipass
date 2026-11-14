//! Root window composition and recovery presentation.
#[cfg(feature = "parity-render")]
use super::ReplayToast;
use super::{CloseWindow, Desktop, LockVault};
use crate::parity::Observe;
use gpui::{div, prelude::*, px, rgb, Context, Window};

impl Render for Desktop {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.prepare_appearance(window, cx);
        self.appearance
            .frame(self.show_workspace, self.curtain.is_some(), window, cx);
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
            .capture_key_down(cx.listener(|this, _, _, _| {
                if let Some(session) = &mut this.session {
                    session.record_activity();
                }
            }))
            .capture_any_mouse_down(cx.listener(|this, _, _, _| {
                if let Some(session) = &mut this.session {
                    session.record_activity();
                }
            }))
            .on_action(cx.listener(|this, _: &LockVault, window, cx| this.lock(window, cx)))
            .child(super::activity::capture_scroll(cx))
            .child(
                if self.loading_preview
                    || self
                        .session
                        .as_ref()
                        .is_some_and(|s| s.status.as_ref().is_none_or(|s| s.initial_sync_pending))
                {
                    self.loading(window, cx).into_any_element()
                } else if self.show_workspace {
                    self.workspace.clone().into_any_element()
                } else {
                    self.auth.clone().into_any_element()
                },
            )
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
            .child(self.recovery.clone())
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

impl Desktop {
    fn loading(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui::Div {
        let spinner = gpui::svg()
            .path("loading")
            .size(px(16.))
            .text_color(self.theme.accent);
        use gpui::AnimationExt;
        let spinner = if cx.reduce_motion() {
            spinner.into_any_element()
        } else {
            spinner
                .with_animation(
                    "boot-spinner",
                    gpui::Animation::new(std::time::Duration::from_millis(800)).repeat(),
                    |el, delta| {
                        el.with_transformation(gpui::Transformation::rotate(gpui::radians(
                            delta * std::f32::consts::TAU,
                        )))
                    },
                )
                .into_any_element()
        };
        let chrome = self
            .workspace
            .update(cx, |view, cx| view.loading_chrome(window, cx));
        let error = self
            .session
            .as_ref()
            .and_then(|s| s.error.as_ref())
            .map(|e| e.message.clone());
        div()
            .size_full()
            .relative()
            .child(chrome)
            .pt(px(48.))
            .pb(px(24.))
            .bg(self.theme.bg)
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .relative()
                    .gap(px(22.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .text_size(px(15.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(self.theme.text)
                            .child(crate::logo::logo(28., window))
                            .child("AIPass"),
                    )
                    .child(
                        div()
                            .id("boot-status")
                            .role(gpui::Role::Status)
                            .aria_label(self.locale.text("common.loading"))
                            .min_h(px(20.))
                            .flex()
                            .items_center()
                            .child(spinner)
                            .observe("boot-status"),
                    )
                    .when_some(error, |el, error| {
                        el.child(
                            div()
                                .max_w(px(420.))
                                .text_size(px(13.))
                                .text_color(self.theme.danger)
                                .child(error),
                        )
                        .child(
                            crate::providers::controls::button(
                                "boot-retry",
                                self.locale.text("providerRuntime.retry"),
                                false,
                                self.theme,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(s) = &mut this.session {
                                    s.submit(aipass_desktop_native::session::Command::Refresh);
                                }
                                cx.notify();
                            })),
                        )
                    })
                    .observe("boot-content"),
            )
    }
}
