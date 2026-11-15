use super::*;
use gpui::{div, px, FontWeight};
impl Settings {
    pub(super) fn about(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = self.theme;
        let refresh = self
            .link("extension-refresh", "settings.refresh")
            .on_click(cx.listener(|s, _, _, cx| {
                s.request(Operation::BrowserExtension { install: false }, false, cx)
            }));
        let mut extension = presentation::rows();
        if let Some(status) = &self.extension {
            let ready = status.extension_installed && status.native_host_configured;
            let key = if ready {
                "settings.extensionReady"
            } else if !status.chrome_installed {
                "settings.extensionBrowserMissing"
            } else if !status.zip_exists {
                "settings.extensionPackageMissing"
            } else {
                "settings.extensionAvailable"
            };
            extension = extension.child(
                presentation::row()
                    .min_h(px(40.))
                    .gap(px(12.))
                    .child(
                        div()
                            .size(px(32.))
                            .flex_shrink_0()
                            .rounded(px(6.))
                            .bg(t.surface2)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                gpui::svg()
                                    .path("puzzle")
                                    .size(px(18.))
                                    .text_color(t.accent),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.locale.text(key)),
                    )
                    .when(!ready, |el| {
                        el.child(
                            self.icon_button(
                                "extension-install",
                                if status.extension_installed {
                                    "settings.extensionRepair"
                                } else {
                                    "settings.extensionInstall"
                                },
                                "puzzle",
                                false,
                            )
                            .disabled(
                                self.pending || !status.chrome_installed || !status.zip_exists,
                            )
                            .on_click(cx.listener(|s, _, _, cx| {
                                s.request(Operation::BrowserExtension { install: true }, false, cx)
                            })),
                        )
                    }),
            );
        } else {
            extension = extension.child(admin_ui::hint(
                self.locale.text(if self.pending {
                    "common.loading"
                } else {
                    "settings.extensionPackageMissing"
                }),
                t,
            ));
        }
        let channel = self.segmented(
            "update-channel",
            self.update_channel.as_str(),
            vec![
                (
                    "official",
                    self.locale.text("settings.updateChannelOfficial"),
                ),
                ("beta", self.locale.text("settings.updateChannelBeta")),
                ("nightly", self.locale.text("settings.updateChannelNightly")),
            ],
            |s, value, cx| {
                let channel = match value {
                    "beta" => UpdateChannel::Beta,
                    "nightly" => UpdateChannel::Nightly,
                    _ => UpdateChannel::Official,
                };
                s.select_update_channel(channel, cx);
            },
            cx,
        );
        let mut updates = presentation::rows()
            .child(
                presentation::row()
                    .child(self.locale.text("settings.currentVersion"))
                    .child(admin_ui::hint(
                        format!("AIPass v{}", env!("CARGO_PKG_VERSION")),
                        t,
                    )),
            )
            .child(
                presentation::row()
                    .child(self.locale.text("settings.buildTime"))
                    .child(admin_ui::hint(self.build_time.clone(), t)),
            )
            .child(
                presentation::row()
                    .child(
                        admin_ui::stack()
                            .gap(px(2.))
                            .flex_1()
                            .min_w_0()
                            .child(self.locale.text("settings.updateChannel"))
                            .child(presentation::description(
                                self.locale.text("settings.updateChannelDesc"),
                                t,
                            )),
                    )
                    .child(channel),
            );
        let busy = self.update.as_ref().is_some_and(|u| u.busy);
        if let Some(update) = &self.update {
            if let Some(error) = &update.error {
                updates = updates.child(div().text_color(t.danger).child(error.clone()));
            }
            if let Some(release) = &update.available {
                updates = updates.child(
                    presentation::row()
                        .child(
                            admin_ui::stack()
                                .gap(px(4.))
                                .child(self.locale.text("settings.updateAvailable"))
                                .child(admin_ui::hint(format!("v{}", release.version), t)),
                        )
                        .child(
                            self.button(
                                "update-install",
                                if update.restarting {
                                    "updates.installing"
                                } else {
                                    "settings.install"
                                },
                                true,
                            )
                            .disabled(busy)
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(Event::InstallUpdate))),
                        ),
                );
                if let Some(notes) = &release.notes {
                    updates = updates.child(admin_ui::hint(notes.clone(), t));
                }
            }
            if busy && update.downloaded > 0 {
                updates = updates.child(admin_ui::hint(
                    format!(
                        "{}  {:.1} MB",
                        self.locale.text("updates.downloading"),
                        update.downloaded as f64 / 1_048_576.
                    ),
                    t,
                ));
            }
        }
        let checked = self
            .update
            .as_ref()
            .is_some_and(|u| u.checked && u.available.is_none() && u.error.is_none());
        updates = updates.child(
            presentation::row()
                .child(self.locale.text(if checked {
                    "settings.upToDate"
                } else {
                    "settings.checkUpdates"
                }))
                .child(
                    self.icon_button(
                        "update-check",
                        if busy {
                            "settings.checking"
                        } else {
                            "settings.checkNow"
                        },
                        "refresh-cw",
                        false,
                    )
                    .disabled(busy)
                    .on_click(cx.listener(|s, _, _, cx| {
                        if s.update.is_none() {
                            s.error = s.locale.text("error.updateReleaseUnavailable");
                        }
                        cx.emit(Event::CheckUpdates);
                        cx.notify();
                    })),
                ),
        );
        presentation::cards()
            .child(admin_ui::card_actions(
                self.locale.text("settings.browserExtension"),
                t,
                extension,
                refresh,
            ))
            .child(admin_ui::card(
                self.locale.text("settings.updates"),
                t,
                updates,
            ))
            .into_any_element()
    }
}
