use super::*;

impl Settings {
    pub(super) fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = self.theme;
        let mode = match self.sync_mode {
            SyncMode::Local => "local",
            SyncMode::ICloud => "icloud",
            SyncMode::OneDrive => "onedrive",
            SyncMode::WebDav => "webdav",
        };
        let mut modes = vec![("local", self.locale.text("settings.local"))];
        if cfg!(target_os = "macos") {
            modes.push(("icloud", "iCloud".into()));
        }
        modes.extend([("onedrive", "OneDrive".into()), ("webdav", "WebDAV".into())]);
        let mut target = presentation::rows().child(self.segmented(
            "sync-mode",
            mode,
            modes,
            |s, v, cx| {
                s.sync_mode = match v {
                    "icloud" => SyncMode::ICloud,
                    "onedrive" => SyncMode::OneDrive,
                    "webdav" => SyncMode::WebDav,
                    _ => SyncMode::Local,
                };
                cx.notify();
            },
            cx,
        ));
        match self.sync_mode {
            SyncMode::Local => {
                let value = self
                    .sync_settings
                    .sync_folder
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                target = target.child(self.field(
                    "sync-folder",
                    "settings.folder",
                    &value,
                    false,
                    window,
                    cx,
                ));
            }
            SyncMode::ICloud => {
                target = target.child(admin_ui::hint(self.locale.text("settings.icloudDesc"), t))
            }
            SyncMode::OneDrive => {
                target = target.child(admin_ui::hint(self.locale.text("settings.onedriveDesc"), t))
            }
            SyncMode::WebDav => {
                let url = self.sync_settings.webdav_url.clone().unwrap_or_default();
                let username = self
                    .sync_settings
                    .webdav_username
                    .clone()
                    .unwrap_or_default();
                target = target
                    .child(self.field("webdav-url", "settings.url", &url, false, window, cx))
                    .child(self.field(
                        "webdav-user",
                        "settings.username",
                        &username,
                        false,
                        window,
                        cx,
                    ))
                    .child(self.field(
                        "webdav-password",
                        "settings.password",
                        "",
                        true,
                        window,
                        cx,
                    ));
                if self.sync_settings.has_webdav_password {
                    target = target.child(admin_ui::hint(
                        self.locale.text("settings.savedPasswordHint"),
                        t,
                    ));
                }
            }
        }
        target = target.child(
            admin_ui::row()
                .gap(gpui::px(8.))
                .justify_end()
                .child(
                    self.button("sync-save", "common.save", false)
                        .on_click(cx.listener(|s, _, _, cx| s.save_sync(false, false, cx))),
                )
                .when(
                    self.sync_mode == SyncMode::WebDav && self.sync_settings.has_webdav_password,
                    |el| {
                        el.child(
                            self.button("sync-clear-password", "settings.clearPassword", false)
                                .on_click(cx.listener(|s, _, _, cx| s.save_sync(true, false, cx))),
                        )
                    },
                )
                .child(
                    self.icon_button("sync-now", "settings.syncNow", "wifi", true)
                        .disabled(
                            self.pending
                                || match self.sync_mode {
                                    SyncMode::Local => {
                                        self.fields.value("sync-folder", cx).trim().is_empty()
                                    }
                                    SyncMode::WebDav => {
                                        self.fields.value("webdav-url", cx).trim().is_empty()
                                    }
                                    _ => false,
                                },
                        )
                        .on_click(cx.listener(|s, _, _, cx| s.save_sync(false, true, cx))),
                ),
        );
        let refresh = self
            .link("conflicts-refresh", "settings.refresh")
            .on_click(cx.listener(|s, _, _, cx| s.request(Operation::LoadSync, false, cx)));
        let conflicts = self.conflict_rows(window, cx);
        let devices = self.device_rows(cx);
        presentation::cards()
            .child(admin_ui::card(
                self.locale.text("settings.syncTarget"),
                t,
                target,
            ))
            .child(admin_ui::card_actions(
                self.locale.text("settings.conflicts"),
                t,
                conflicts,
                refresh,
            ))
            .child(admin_ui::card(
                self.locale.text("settings.trustedDevices"),
                t,
                devices,
            ))
            .into_any_element()
    }
    pub(super) fn save_sync(
        &mut self,
        clear_webdav_password: bool,
        run: bool,
        cx: &mut Context<Self>,
    ) {
        let value = |key, fallback: Option<String>| {
            if !self.fields.contains(key) {
                return fallback;
            }
            let value = self.fields.value(key, cx);
            if value.trim().is_empty() {
                None
            } else {
                Some(value.trim().to_string())
            }
        };
        let password = self.fields.value("webdav-password", cx);
        let settings = SyncSettingsUpdate {
            mode: self.sync_mode,
            sync_folder: value(
                "sync-folder",
                self.sync_settings
                    .sync_folder
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned()),
            )
            .map(Into::into),
            webdav_url: value("webdav-url", self.sync_settings.webdav_url.clone()),
            webdav_username: value("webdav-user", self.sync_settings.webdav_username.clone()),
            webdav_password: (!password.is_empty()).then(|| SensitiveString::new(password)),
            clear_webdav_password,
        };
        if (settings.mode == SyncMode::Local && settings.sync_folder.is_none())
            || (settings.mode == SyncMode::WebDav && settings.webdav_url.is_none())
        {
            self.error = self.locale.text("settings.saveFailed");
            cx.notify();
            return;
        }
        self.request(Operation::SaveSync { settings, run }, true, cx);
    }
}
