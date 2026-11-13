use aipass_desktop_native::updates::Command;
#[cfg(feature = "parity-render")]
use aipass_desktop_native::updates::Worker;
use gpui::{div, prelude::*, px, Context};
use gpui_base::{Button, Dialog, DialogPopup};

impl crate::desktop::Desktop {
    #[cfg(feature = "parity-render")]
    pub(crate) fn enable_updates(
        &mut self,
        worker: Worker,
        gate: Option<std::sync::Arc<aipass_desktop_native::runtime::Gate>>,
    ) {
        self.runtime_gate = gate;
        self.updater = Some(worker);
    }

    pub(crate) fn update_banner(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(worker) = &self.updater else {
            return div().into_any_element();
        };
        let state = worker.snapshot();
        let Some(release) = state.available else {
            return div().into_any_element();
        };
        if !self.show_workspace || self.curtain.is_some() {
            return div().into_any_element();
        }
        if let Some(token) = state.confirmation {
            let owner = cx.entity().downgrade();
            return Dialog::new(cx)
                .focus_handle(self.update_focus.clone())
                .accessibility_label(self.locale.text("updates.proxyRestartTitle"))
                .on_ok(|_, _, _| false)
                .on_open_change(move |open, _, window, cx| {
                    if !open {
                        let _ = owner.update(cx, |this, cx| {
                            if let Some(worker) = &this.updater {
                                worker.cancel_confirmation();
                            }
                            this.focus.focus(window, cx);
                            cx.notify();
                        });
                    }
                })
                .backdrop(
                    div()
                        .absolute()
                        .inset_0()
                        .bg(gpui::rgba(0x0f111080))
                        .backdrop_blur(px(4.)),
                )
                .popup(
                    DialogPopup::new()
                        .w(px(420.))
                        .p(px(24.))
                        .rounded(px(14.))
                        .bg(self.theme.surface)
                        .text_color(self.theme.text)
                        .flex()
                        .flex_col()
                        .gap(px(16.))
                        .child(
                            div()
                                .text_size(px(18.))
                                .child(self.locale.text("updates.proxyRestartTitle")),
                        )
                        .child(
                            div()
                                .text_size(px(13.))
                                .child(self.locale.text("updates.proxyRestartDescription")),
                        )
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap(px(12.))
                                .child(
                                    Button::new("update-confirm-cancel")
                                        .accessibility_label(
                                            self.locale.text("updates.proxyRestartCancel"),
                                        )
                                        .child(self.locale.text("updates.proxyRestartCancel"))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.focus.focus(window, cx);
                                            if let Some(worker) = &this.updater {
                                                worker.cancel_confirmation();
                                            }
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    Button::new("update-confirm-continue")
                                        .disabled(state.busy)
                                        .accessibility_label(
                                            self.locale.text("updates.proxyRestartConfirm"),
                                        )
                                        .child(self.locale.text("updates.proxyRestartConfirm"))
                                        .on_click(cx.listener(move |this, _, _, _| {
                                            if this.show_workspace && this.curtain.is_none() {
                                                if let Some(worker) = &this.updater {
                                                    worker.send(Command::Confirm(token));
                                                }
                                            }
                                        })),
                                ),
                        ),
                )
                .into_any_element();
        }
        let title = self
            .locale
            .text("updates.bannerTitle")
            .replace("{version}", &release.version.to_string());
        div()
            .absolute()
            .bottom(px(28.))
            .right(px(28.))
            .max_w(px(570.))
            .p(px(12.))
            .rounded(px(10.))
            .border_1()
            .border_color(self.theme.border)
            .bg(self.theme.surface)
            .text_color(self.theme.text)
            .text_size(px(13.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(title)
                    .child(
                        Button::new("update-install")
                            .accessibility_label(self.locale.text("updates.installRestart"))
                            .disabled(state.busy)
                            .child(self.locale.text(if state.busy {
                                "updates.installing"
                            } else {
                                "updates.installRestart"
                            }))
                            .on_click(cx.listener(|this, _, _, _| {
                                if this.show_workspace && this.curtain.is_none() {
                                    if let Some(worker) = &this.updater {
                                        worker.send(Command::Install);
                                    }
                                }
                            })),
                    )
                    .child(
                        Button::new("update-later")
                            .accessibility_label(self.locale.text("updates.later"))
                            .disabled(state.busy)
                            .child(self.locale.text("updates.later"))
                            .on_click(cx.listener(|this, _, _, _| {
                                if let Some(worker) = &this.updater {
                                    worker.send(Command::Dismiss);
                                }
                            })),
                    ),
            )
            .when_some(state.error, |el, error| {
                let key = if error.contains("update_signature_invalid") {
                    "error.updateSignatureInvalid"
                } else if error.contains("update_package_invalid") {
                    "error.updatePackageInvalid"
                } else {
                    "error.updateInstallFailed"
                };
                el.child(self.locale.text(key))
            })
            .into_any_element()
    }
}
