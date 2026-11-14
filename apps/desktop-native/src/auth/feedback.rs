use super::*;
use gpui::{percentage, relative, Animation, AnimationExt, Transformation};
use std::time::Duration;

pub(super) fn spinner(color: gpui::Rgba, reduced: bool) -> gpui::AnyElement {
    let icon = svg()
        .path("spinner")
        .size(px(12.))
        .absolute()
        .left(relative(0.5))
        .top(relative(0.5))
        .ml(px(-6.))
        .mt(px(-6.))
        .text_color(color);
    if reduced {
        icon.into_any_element()
    } else {
        icon.with_animation(
            "auth-spinner",
            Animation::new(Duration::from_millis(700)).repeat(),
            |icon, progress| icon.with_transformation(Transformation::rotate(percentage(progress))),
        )
        .into_any_element()
    }
}

impl AuthView {
    pub(crate) fn preview_state(
        &mut self,
        scenario: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if scenario.starts_with("setup-cloud") || scenario == "create-local" {
            self.cloud_default = true;
            self.cloud_failed = scenario.ends_with("failed");
        }
        #[cfg(feature = "parity-render")]
        if let Some(source) = scenario.strip_prefix("setup-import-") {
            self.import
                .update(cx, |s, cx| s.preview(source, window, cx));
        }
        if matches!(scenario, "reset" | "reset-bottom") {
            self.open_reset(window, cx);
            self.scroll_to_end = scenario == "reset-bottom";
        }
        if matches!(scenario, "locked-busy" | "locked-error") {
            self.password.update(cx, |input, cx| {
                input.set_value("ParityExample123!", window, cx)
            });
            self.focus.focus(window, cx);
            if scenario == "locked-busy" {
                self.set_busy(true, cx);
            } else {
                self.error = self.locale.text("error.incorrectMasterPassword");
            }
        }
    }

    pub(super) fn error_notice(&self) -> impl IntoElement {
        div()
            .flex_shrink_0()
            .min_w_0()
            .rounded(px(8.))
            .border_1()
            .border_color(crate::theme::oklab_mix(
                self.theme.danger,
                self.theme.border,
                0.2,
            ))
            .bg(crate::theme::oklab_mix(
                self.theme.danger,
                self.theme.surface,
                0.04,
            ))
            .p(px(10.))
            .px(px(12.))
            .flex()
            .items_start()
            .gap(px(8.))
            .child(
                svg()
                    .path("circle-alert")
                    .size(px(16.))
                    .mt(px(1.))
                    .flex_shrink_0()
                    .text_color(self.theme.danger),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .font_weight(FontWeight::NORMAL)
                    .text_color(self.theme.text)
                    .child(gpui::text!(id = "auth-error-message", self.error.clone())),
            )
    }
}
