use super::*;
use aipass_desktop_platform::preferences::CredentialClickAction;
use gpui::{div, px};
impl Settings {
    pub(super) fn security(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let t = self.theme;
        let mut options = [
            (15, "15m"),
            (30, "30m"),
            (60, "1h"),
            (120, "2h"),
            (240, "4h"),
            (480, "8h"),
            (1440, "24h"),
            (0, "Never"),
        ]
        .into_iter()
        .map(|(n, key)| {
            (
                n.to_string(),
                self.locale.text(&format!("settings.autoLock{key}")),
            )
        })
        .collect::<Vec<_>>();
        if !options
            .iter()
            .any(|(n, _)| n == &self.preferences.auto_lock_minutes.to_string())
        {
            options.insert(
                0,
                (
                    self.preferences.auto_lock_minutes.to_string(),
                    self.locale
                        .text("settings.autoLockCustom")
                        .replace("{minutes}", &self.preferences.auto_lock_minutes.to_string()),
                ),
            );
        }
        let auto = self.choice(
            "auto-lock",
            "settings.autoLock",
            self.preferences.auto_lock_minutes.to_string(),
            options,
            |s, value, cx| {
                if let Ok(value) = value.parse() {
                    s.preferences.auto_lock_minutes = value;
                    s.save_preferences(cx);
                }
            },
            cx,
        );
        let clipboard = self.fields.editor(
            (
                "clipboard-seconds",
                "settings.clipboardClear",
                &self.preferences.clipboard_clear_seconds.to_string(),
                false,
            ),
            t,
            &self.locale,
            window,
            cx,
        );
        let click = self.choice(
            "credential-action",
            "settings.credentialClickAction",
            if self.preferences.credential_click_action == CredentialClickAction::Copy {
                "copy"
            } else {
                "reveal"
            }
            .into(),
            vec![
                (
                    "reveal".into(),
                    self.locale.text("settings.credentialClickReveal"),
                ),
                (
                    "copy".into(),
                    self.locale.text("settings.credentialClickCopy"),
                ),
            ],
            |s, value, cx| {
                s.preferences.credential_click_action = if value == "copy" {
                    CredentialClickAction::Copy
                } else {
                    CredentialClickAction::Reveal
                };
                s.save_preferences(cx);
            },
            cx,
        );
        let password = self.field("new-password", "auth.newPassword", "", true, window, cx);
        presentation::cards()
            .child(admin_ui::card(
                self.locale.text("settings.lockPolicy"),
                t,
                admin_ui::rows()
                    .child(
                        admin_ui::row()
                            .child(
                                admin_ui::stack()
                                    .gap(px(2.))
                                    .flex_1()
                                    .child(self.locale.text("settings.autoLock"))
                                    .child(presentation::description(
                                        self.locale.text("settings.autoLockDesc"),
                                        t,
                                    )),
                            )
                            .child(div().w(px(132.)).flex_shrink_0().child(auto)),
                    )
                    .child(
                        admin_ui::row()
                            .child(
                                admin_ui::stack()
                                    .gap(px(2.))
                                    .flex_1()
                                    .child(self.locale.text("settings.clipboardClear"))
                                    .child(presentation::description(
                                        self.locale.text("settings.clipboardClearDesc"),
                                        t,
                                    )),
                            )
                            .child(div().w(px(80.)).flex_shrink_0().child(clipboard)),
                    )
                    .child(
                        admin_ui::row()
                            .child(self.locale.text("settings.credentialClickAction"))
                            .child(div().w(px(132.)).flex_shrink_0().child(click)),
                    )
                    .child(self.toggle(
                        "settings.confirmSecretCopy",
                        self.preferences.confirm_secret_copy,
                        |s, v, cx| {
                            s.preferences.confirm_secret_copy = v;
                            s.save_preferences(cx);
                        },
                        window,
                        cx,
                    ))
                    .child(self.described_toggle(
                        "settings.lockOnSleep",
                        "settings.lockOnSleepDesc",
                        self.preferences.lock_on_sleep,
                        |s, v, cx| {
                            s.preferences.lock_on_sleep = v;
                            s.save_preferences(cx);
                        },
                        window,
                        cx,
                    ))
                    .child(self.described_toggle(
                        "settings.lockOnScreenLock",
                        "settings.lockOnScreenLockDesc",
                        self.preferences.lock_on_screen_lock,
                        |s, v, cx| {
                            s.preferences.lock_on_screen_lock = v;
                            s.save_preferences(cx);
                        },
                        window,
                        cx,
                    )),
            ))
            .child(admin_ui::card(
                self.locale.text("settings.masterPassword"),
                t,
                presentation::rows().child(password).child(
                    admin_ui::row().justify_end().child(
                        self.button("password-save", "settings.changePassword", false)
                            .disabled(
                                self.pending
                                    || self.fields.value("new-password", cx).trim().is_empty(),
                            )
                            .on_click(cx.listener(|s, _, w, cx| {
                                let password =
                                    SensitiveString::new(s.fields.value("new-password", cx));
                                s.fields.reset("new-password", w, cx);
                                s.apply(
                                    AgentRequest::VaultChangePassword {
                                        new_password: password,
                                    },
                                    cx,
                                );
                            })),
                    ),
                ),
            ))
            .child(admin_ui::card(
                self.locale.text("settings.rotateKeys"),
                t,
                presentation::rows()
                    .child(admin_ui::hint(
                        self.locale.text("settings.rotateKeysDesc"),
                        t,
                    ))
                    .child(
                        admin_ui::row().justify_end().child(
                            self.icon_button("rotate-keys", "settings.rotate", "rotate-cw", false)
                                .on_click(
                                    cx.listener(|s, _, w, cx| s.show_confirmation("rotate", w, cx)),
                                ),
                        ),
                    ),
            ))
            .into_any_element()
    }
}
