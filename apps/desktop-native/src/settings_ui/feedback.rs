//! Bounded, accessible operation feedback above the drawer tabs.
use super::*;
use crate::parity::Observe;
use gpui::{div, px, Role};

impl Settings {
    pub(super) fn feedback(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = self.theme;
        let banner = |id: &'static str, message: &str, error: bool| {
            div()
                .id(id)
                .role(if error { Role::Alert } else { Role::Status })
                .pixel_snap(false)
                .flex_shrink_0()
                .w_full()
                .min_w_0()
                .px(px(12.))
                .py(px(8.))
                .border_1()
                .border_color(gpui::rgba(0))
                .rounded(px(8.))
                .bg(if error { t.danger_soft } else { t.success_soft })
                .text_color(if error { t.danger } else { t.success })
                .text_size(px(12.))
                .line_height(px(16.8))
                .child(message.to_owned())
                .observe(id)
        };
        div()
            .relative()
            .pixel_snap(false)
            .flex_shrink_0()
            .child(
                div()
                    .id("settings-feedback-scroll")
                    .track_scroll(&self.feedback_scroll)
                    .max_h(px(120.))
                    .overflow_y_scroll()
                    .pixel_snap(false)
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .px(px(20.))
                    .py(px(12.))
                    // A saved destination and a failed sync are separate results.
                    .when(!self.notice.is_empty(), |el| {
                        el.child(banner("settings-notice", &self.notice, false))
                    })
                    .when(!self.error.is_empty(), |el| {
                        el.child(banner("settings-error", &self.error, true))
                    })
                    .when(self.load_failed, |el| {
                        el.child(
                            admin_ui::row().justify_end().child(
                                self.link("settings-retry", "settings.refresh")
                                    .on_click(cx.listener(|s, _, _, cx| s.load(cx))),
                            ),
                        )
                    }),
            )
            .child(crate::scroll_fade::fade(
                t.surface,
                self.feedback_scroll.clone(),
            ))
            .observe("settings-feedback")
            .into_any_element()
    }
}
