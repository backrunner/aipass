use super::*;
use aipass_desktop_platform::preferences::ThemePreference;
use gpui::TestAppContext;

fn failure() -> aipass_agent::AgentCommandError {
    aipass_agent::AgentCommandError {
        code: None,
        message: "synthetic request failure".into(),
    }
}
fn sync_settings(mode: SyncMode) -> SyncSettings {
    SyncSettings {
        mode,
        sync_folder: Some("/tmp/fixture".into()),
        webdav_url: Some("https://example.invalid/dav".into()),
        webdav_username: None,
        has_webdav_password: true,
    }
}

#[gpui::test]
fn transport_failure_restores_preferences_and_numeric_editor(cx: &mut TestAppContext) {
    let (view, cx) = tests::view(cx);
    cx.update(|w, cx| view.update(cx, |s, cx| s.show(1, false, w, cx)));
    cx.update(|w, cx| w.draw(cx).clear(cx));
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            s.connected = true;
            s.fields.set_value("clipboard-seconds", "90".into(), w, cx);
            s.preferences.clipboard_clear_seconds = 90;
            s.preferences.theme = ThemePreference::Dark;
            s.save_preferences(cx);
            let request = s.take_request().unwrap();
            s.complete(
                Completion {
                    scope: request.scope,
                    result: Err(failure()),
                },
                w,
                cx,
            );
            assert_eq!(s.preferences.theme, ThemePreference::System);
            assert_eq!(s.preferences.clipboard_clear_seconds, 45);
            assert_eq!(s.fields.value("clipboard-seconds", cx), "45");
            assert!(!s.pending);
            assert!(!s.error.is_empty());
            assert!(s.take_request().is_none());
        })
    });
}

#[gpui::test]
fn partial_preferences_result_updates_live_state_and_preserves_error(cx: &mut TestAppContext) {
    let (view, cx) = tests::view(cx);
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            s.show(0, false, w, cx);
            s.connected = true;
            s.preferences.theme = ThemePreference::Dark;
            s.preferences.locale = LocalePreference::ZhCn;
            s.save_preferences(cx);
            let request = s.take_request().unwrap();
            let confirmed = AppPreferences {
                auto_lock_minutes: 30,
                locale: LocalePreference::ZhCn,
                resolved_locale: Some(UiLocale::ZhCn),
                ..Default::default()
            };
            s.complete(
                Completion {
                    scope: request.scope,
                    result: Ok(Outcome::PreferencesSaved {
                        preferences: confirmed,
                        error: Some(failure()),
                    }),
                },
                w,
                cx,
            );
            assert_eq!(s.preferences.auto_lock_minutes, 30);
            assert_eq!(s.preferences.theme, ThemePreference::System);
            assert_eq!(
                cx.global::<crate::preferences::Preferences>()
                    .0
                    .resolved_locale,
                Some(UiLocale::ZhCn)
            );
            assert!(!s.error.is_empty());
            assert!(s.notice.is_empty());
            assert!(s.take_request().is_none());
        })
    });
}

#[gpui::test]
fn sync_failure_after_save_retires_password_and_retains_acknowledged_target(
    cx: &mut TestAppContext,
) {
    let (view, cx) = tests::view(cx);
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            s.show(2, false, w, cx);
            s.sync_mode = SyncMode::WebDav;
            let _ = s.field(
                "webdav-url",
                "settings.url",
                "https://example.invalid/dav",
                false,
                w,
                cx,
            );
            let _ = s.field(
                "webdav-password",
                "settings.password",
                "synthetic password",
                true,
                w,
                cx,
            );
            s.save_sync(false, true, cx);
            let request = s.take_request().unwrap();
            s.complete(
                Completion {
                    scope: request.scope,
                    result: Ok(Outcome::SyncSaved {
                        settings: sync_settings(SyncMode::WebDav),
                        error: Some(failure()),
                    }),
                },
                w,
                cx,
            );
            assert!(s.sync_settings.has_webdav_password);
            assert_eq!(s.fields.value("webdav-password", cx), "");
            assert_eq!(s.notice, s.locale.text("settings.syncSettingsSaved"));
            assert!(!s.error.is_empty());
            assert!(
                s.take_request().is_none(),
                "failed sync is not hidden behind a read refresh"
            );
            s.save_sync(false, true, cx);
            let Operation::SaveSync { settings, run } = s.take_request().unwrap().operation else {
                panic!("expected retry")
            };
            assert!(run);
            assert!(
                settings.webdav_password.is_none(),
                "retry keeps the saved credential"
            );
            assert_eq!(settings.mode, SyncMode::WebDav);
        })
    });
}

#[gpui::test]
fn conflict_refresh_keeps_an_unsaved_destination_choice(cx: &mut TestAppContext) {
    let (view, cx) = tests::view(cx);
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            s.show(2, false, w, cx);
            for expected in [SyncMode::Local, SyncMode::WebDav] {
                s.request(Operation::LoadSync, false, cx);
                let request = s.take_request().unwrap();
                s.complete(
                    Completion {
                        scope: request.scope,
                        result: Ok(Outcome::Sync {
                            settings: sync_settings(SyncMode::Local),
                            conflicts: vec![],
                            devices: vec![],
                        }),
                    },
                    w,
                    cx,
                );
                assert_eq!(s.sync_mode, expected);
                s.sync_mode = SyncMode::WebDav;
            }
        })
    });
}

#[gpui::test]
fn successful_save_feedback_survives_the_followup_refresh(cx: &mut TestAppContext) {
    let (view, cx) = tests::view(cx);
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            s.show(2, false, w, cx);
            s.apply(AgentRequest::SyncConfigured, cx);
            let request = s.take_request().unwrap();
            s.complete(
                Completion {
                    scope: request.scope,
                    result: Ok(Outcome::Applied),
                },
                w,
                cx,
            );
            assert!(!s.notice.is_empty());
            let read = s.take_request().unwrap();
            s.complete(
                Completion {
                    scope: read.scope,
                    result: Ok(Outcome::Sync {
                        settings: sync_settings(SyncMode::Local),
                        conflicts: vec![],
                        devices: vec![],
                    }),
                },
                w,
                cx,
            );
            assert!(!s.notice.is_empty());
            assert!(!s.pending);
        })
    });
}

#[gpui::test]
fn long_failure_feedback_keeps_tabs_and_content_reachable(cx: &mut TestAppContext) {
    let (view, cx) = tests::view(cx);
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            s.preview("settings-sync-feedback-success", w, cx);
            s.error = "synthetic failure with a long diagnostic path /".repeat(100);
        });
        w.draw(cx).clear(cx);
    });
    cx.update(|_, cx| {
        let bounds = &cx.global::<crate::parity::Geometry>().0;
        let feedback = bounds["settings-feedback"];
        let tab = bounds["settings-tab-2"];
        let body = bounds["settings-body"];
        assert!(feedback[3] <= 120.);
        assert!(tab[1] >= feedback[1] + feedback[3]);
        assert!(body[1] >= tab[1] + tab[3]);
        assert!(body[1] + body[3] <= 632.);
        assert!(
            view.read(cx).feedback_scroll.max_offset().y > gpui::px(0.),
            "long feedback must scroll instead of shrinking the banner"
        );
    });
}

#[gpui::test]
fn failed_initial_read_exposes_retry_without_repeating_a_write(cx: &mut TestAppContext) {
    let (view, cx) = tests::view(cx);
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            s.show(2, true, w, cx);
            let request = s.take_request().unwrap();
            s.complete(
                Completion {
                    scope: request.scope,
                    result: Err(failure()),
                },
                w,
                cx,
            );
            assert!(s.load_failed);
            s.load(cx);
            assert!(matches!(
                s.take_request().unwrap().operation,
                Operation::LoadSync
            ));
            assert!(!s.load_failed);
        });
    });
}

#[gpui::test]
fn preference_success_uses_the_acknowledged_language_before_global_observers_run(
    cx: &mut TestAppContext,
) {
    let (view, cx) = tests::view(cx);
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            s.show(0, false, w, cx);
            s.connected = true;
            s.preferences.locale = LocalePreference::ZhCn;
            s.save_preferences(cx);
            let scope = s.take_request().unwrap().scope;
            let mut confirmed = s.preferences.clone();
            confirmed.resolved_locale = Some(UiLocale::ZhCn);
            s.complete(
                Completion {
                    scope,
                    result: Ok(Outcome::PreferencesSaved {
                        preferences: confirmed,
                        error: None,
                    }),
                },
                w,
                cx,
            );
            assert_eq!(
                s.notice,
                s.locale.text_in(UiLocale::ZhCn, "providerRuntime.saved")
            );
            assert_ne!(
                s.notice,
                s.locale.text_in(UiLocale::En, "providerRuntime.saved")
            );
        });
    });
}
