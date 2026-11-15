use super::*;
use crate::parity::Observe;
use aipass_desktop_platform::preferences::ThemePreference;
use gpui::{div, px};
impl Settings {
    pub(super) fn general(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let t = self.theme;
        let theme = match self.preferences.theme {
            ThemePreference::System => "system",
            ThemePreference::Light => "light",
            ThemePreference::Dark => "dark",
        };
        let language = match self.preferences.locale {
            LocalePreference::System => "system",
            LocalePreference::En => "en",
            LocalePreference::ZhCn => "zh-CN",
        };
        let theme_width = presentation::inline_segments_width(
            window,
            &[
                self.locale.text("settings.themeSystem"),
                self.locale.text("settings.themeLight"),
                self.locale.text("settings.themeDark"),
            ],
            &self.locale.text("settings.themeDesc"),
        );
        let language_width = presentation::inline_segments_width(
            window,
            &[
                self.locale.text("locale.system"),
                self.locale.text("locale.en"),
                self.locale.text("locale.zhCN"),
            ],
            &self.locale.text("settings.languageDesc"),
        );
        let themes = self.segmented(
            "settings-theme",
            theme,
            vec![
                ("system", self.locale.text("settings.themeSystem")),
                ("light", self.locale.text("settings.themeLight")),
                ("dark", self.locale.text("settings.themeDark")),
            ],
            |s, value, cx| {
                s.preferences.theme = match value {
                    "light" => ThemePreference::Light,
                    "dark" => ThemePreference::Dark,
                    _ => ThemePreference::System,
                };
                s.save_preferences(cx);
            },
            cx,
        );
        let languages = self.segmented(
            "settings-language",
            language,
            vec![
                ("system", self.locale.text("locale.system")),
                ("en", self.locale.text("locale.en")),
                ("zh-CN", self.locale.text("locale.zhCN")),
            ],
            |s, value, cx| {
                s.preferences.locale = match value {
                    "en" => LocalePreference::En,
                    "zh-CN" => LocalePreference::ZhCn,
                    _ => LocalePreference::System,
                };
                s.preferences.resolved_locale = Some(match s.preferences.locale {
                    LocalePreference::ZhCn => UiLocale::ZhCn,
                    LocalePreference::En => UiLocale::En,
                    LocalePreference::System => s.locale.language(),
                });
                s.save_preferences(cx);
            },
            cx,
        );
        let detected = self
            .detection
            .as_ref()
            .is_some_and(|d| d.config_exists || d.app_installed);
        let enabled = self.preferences.official_accounts_import;
        let appearance = admin_ui::card(
            self.locale.text("settings.appearance"),
            t,
            presentation::rows()
                .child(
                    presentation::row()
                        .pixel_snap(false)
                        .child(
                            admin_ui::stack()
                                .gap(px(2.))
                                .min_w_0()
                                .child(
                                    div()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .child(self.locale.text("settings.theme")),
                                )
                                .child(presentation::description(
                                    self.locale.text("settings.themeDesc"),
                                    t,
                                )),
                        )
                        .child(
                            div()
                                .w(theme_width)
                                .flex_shrink_0()
                                .pixel_snap(false)
                                .child(themes),
                        ),
                )
                .child(
                    presentation::row()
                        .pixel_snap(false)
                        .child(
                            admin_ui::stack()
                                .gap(px(2.))
                                .min_w_0()
                                .child(
                                    div()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .child(self.locale.text("settings.language")),
                                )
                                .child(presentation::description(
                                    self.locale.text("settings.languageDesc"),
                                    t,
                                )),
                        )
                        .child(
                            div()
                                .w(language_width)
                                .flex_shrink_0()
                                .pixel_snap(false)
                                .child(languages),
                        ),
                ),
        );
        let owner = cx.entity().downgrade();
        let import =
            admin_ui::rows()
                .py(px(12.))
                .text_size(px(12.))
                .line_height(px(15.))
                .child(
                    presentation::row()
                        .min_h(px(0.))
                        .child(
                            admin_ui::stack()
                                .gap(px(4.))
                                .flex_1()
                                .child(self.locale.text("settings.ccSwitchStatus"))
                                .when_some(
                                    self.detection.as_ref().and_then(|d| d.config_path.clone()),
                                    |el, path| el.child(admin_ui::hint(path, t)),
                                ),
                        )
                        .child(
                            div()
                                .text_color(t.secondary)
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .child(self.locale.text(if detected {
                                    "settings.ccSwitchDetected"
                                } else {
                                    "settings.ccSwitchNotDetected"
                                })),
                        ),
                )
                .child(
                    presentation::row()
                        .min_h(px(36.))
                        .child(
                            admin_ui::stack()
                                .gap(px(2.))
                                .min_w_0()
                                .child(div().text_size(px(13.)).child(
                                    self.locale.text("settings.officialAccountsImportEnable"),
                                ))
                                .child(presentation::description(
                                    self.locale
                                        .text("settings.officialAccountsImportEnableDesc"),
                                    t,
                                )),
                        )
                        .child(
                            admin_ui::switch(
                                "settings-import-enabled",
                                self.locale.text("settings.officialAccountsImportEnable"),
                                enabled,
                                self.pending,
                                t,
                                window,
                                cx,
                            )
                            .on_change(move |value, _, w, cx| {
                                let _ = owner.update(cx, |s, cx| {
                                    if value && detected {
                                        s.show_confirmation("ccswitch", w, cx);
                                    } else {
                                        s.preferences.official_accounts_import = value;
                                        s.save_preferences(cx);
                                    }
                                });
                            }),
                        ),
                );
        let refresh = self
            .link("settings-detect", "settings.refresh")
            .on_click(cx.listener(|s, _, _, cx| s.request(Operation::DetectImport, false, cx)));
        let import = admin_ui::card_actions(
            self.locale.text("settings.officialAccountsImport"),
            t,
            import,
            refresh,
        )
        .child(
            presentation::row()
                .min_h(px(0.))
                .justify_end()
                .px(px(16.))
                .py(px(10.))
                .border_t_1()
                .border_color(t.divider)
                .bg(t.surface2)
                .rounded_b(px(11.))
                .child(
                    self.button(
                        "settings-import-ccswitch",
                        "settings.ccSwitchImportNow",
                        true,
                    )
                    .h(px(26.))
                    .px(px(9.))
                    .text_size(px(12.))
                    .disabled(
                        self.pending
                            || !enabled
                            || !self.detection.as_ref().is_some_and(|d| d.config_exists),
                    )
                    .on_click(cx.listener(|s, _, _, cx| s.apply(AgentRequest::CcSwitchImport, cx)))
                    .observe("settings-import-action"),
                ),
        );
        presentation::cards()
            .child(appearance)
            .child(import)
            .into_any_element()
    }
}
