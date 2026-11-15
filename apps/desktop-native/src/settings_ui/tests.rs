use super::*;
use gpui::{px, size, Entity, TestAppContext, VisualTestContext};
pub(super) fn view(cx: &mut TestAppContext) -> (Entity<Settings>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_base::init(cx);
        crate::menu::init(cx);
        cx.set_global(crate::preferences::Preferences(
            AppPreferences::default(),
            None,
        ));
    });
    let mut entity = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Settings::new(false, false, cx));
        entity = Some(view.clone());
        gpui_base::Root::new(view, window, cx)
    });
    cx.simulate_resize(size(px(960.), px(640.)));
    (entity.unwrap(), cx)
}

#[gpui::test]
fn update_channel_only_changes_after_worker_acknowledgement(cx: &mut TestAppContext) {
    use aipass_desktop_native::updates::Snapshot;
    let (view, cx) = view(cx);
    cx.update(|_, cx| {
        view.update(cx, |s, cx| {
            s.select_update_channel(UpdateChannel::Beta, cx);
            assert_eq!(s.update_channel, UpdateChannel::Official);
            assert!(!s.error.is_empty());
            s.sync_update(
                Some(Snapshot {
                    channel: Some(UpdateChannel::Nightly),
                    ..Default::default()
                }),
                cx,
            );
            assert_eq!(s.update_channel, UpdateChannel::Nightly);
            s.select_update_channel(UpdateChannel::Beta, cx);
            assert_eq!(s.update_channel, UpdateChannel::Nightly);
            s.sync_update(
                Some(Snapshot {
                    channel: Some(UpdateChannel::Nightly),
                    error: Some("cannot clear cache".into()),
                    ..Default::default()
                }),
                cx,
            );
            assert_eq!(s.update_channel, UpdateChannel::Nightly);
            s.sync_update(
                Some(Snapshot {
                    channel: Some(UpdateChannel::Beta),
                    ..Default::default()
                }),
                cx,
            );
            assert_eq!(s.update_channel, UpdateChannel::Beta);
        })
    });
}
#[gpui::test]
fn populated_panel_actions_render_and_retire_the_visible_code(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|w, cx| view.update(cx, |s, cx| s.preview("settings-server-access-code", w, cx)));
    // Exercises the conditional revoke action as well as the generated-code row.
    // Re-registering hover on an already styled button panics during rendering.
    cx.update(|w, cx| w.draw(cx).clear(cx));
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            assert!(s.panel.as_ref().unwrap().remote_unlock_enabled);
            assert_eq!(
                s.access_code.as_ref().unwrap().0.expose(),
                "fixture-access-code-only"
            );
            s.clear(w, cx);
            assert!(s.access_code.is_none());
        })
    });
}

#[gpui::test]
fn lock_and_reopen_reject_old_completions_and_erase_secret_inputs(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|window, cx| {
        view.update(cx, |s, cx| {
            s.show(4, false, window, cx);
            let _ = s.field(
                "export-password",
                "settings.exportPassword",
                "synthetic-secret",
                true,
                window,
                cx,
            );
            let generation = s.generation;
            s.clear(window, cx);
            assert!(s.fields.value("export-password", cx).is_empty());
            s.show(3, false, window, cx);
            s.complete(
                Completion {
                    scope: Scope {
                        surface: Surface::Settings,
                        generation,
                    },
                    result: Ok(Outcome::AccessCode(ControlPanelAccessCode {
                        access_code: SensitiveString::new("old-code"),
                    })),
                },
                window,
                cx,
            );
            assert!(s.access_code.is_none());
            assert_eq!(s.tab, 3);
        })
    });
}
#[gpui::test]
fn completion_preserves_other_drafts_and_failure_can_be_retried(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|window, cx| {
        view.update(cx, |s, cx| {
            s.show(4, false, window, cx);
            let _ = s.field(
                "import-path",
                "settings.inputFile",
                "/tmp/keep-this-draft",
                false,
                window,
                cx,
            );
            s.apply(
                AgentRequest::VaultExport {
                    output: "/tmp/export".into(),
                    export_password: SensitiveString::new("synthetic-only"),
                },
                cx,
            );
            let scope = s.take_request().unwrap().scope;
            s.complete(
                Completion {
                    scope,
                    result: Err(aipass_agent::AgentCommandError {
                        code: None,
                        message: "fixture failure".into(),
                    }),
                },
                window,
                cx,
            );
            assert!(!s.pending);
            assert_eq!(s.fields.value("import-path", cx), "/tmp/keep-this-draft");
            s.apply(
                AgentRequest::VaultExport {
                    output: "/tmp/export".into(),
                    export_password: SensitiveString::new("synthetic-only"),
                },
                cx,
            );
            let scope = s.take_request().unwrap().scope;
            s.complete(
                Completion {
                    scope,
                    result: Ok(Outcome::Applied),
                },
                window,
                cx,
            );
            assert_eq!(s.fields.value("import-path", cx), "/tmp/keep-this-draft");
        })
    });
}
#[gpui::test]
fn preview_preferences_do_not_gain_a_storage_path(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|window, cx| {
        view.update(cx, |s, cx| {
            s.show(0, false, window, cx);
            s.preferences.theme = aipass_desktop_platform::preferences::ThemePreference::Dark;
            s.save_preferences(cx);
            assert!(cx.global::<crate::preferences::Preferences>().1.is_none());
            assert_eq!(
                cx.global::<crate::preferences::Preferences>().0.theme,
                aipass_desktop_platform::preferences::ThemePreference::Dark
            );
            assert!(s.take_request().is_none());
        })
    });
}

#[gpui::test]
fn sync_now_submits_current_target_and_keeps_inactive_settings(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            s.show(2, false, w, cx);
            s.sync_settings.webdav_url = Some("https://example.invalid/dav".into());
            let _ = s.field(
                "sync-folder",
                "settings.folder",
                "/tmp/new-sync-target",
                false,
                w,
                cx,
            );
            s.save_sync(false, true, cx);
            let Operation::SaveSync { settings, run } = s.take_request().unwrap().operation else {
                panic!("save then sync")
            };
            assert!(run);
            assert_eq!(
                settings.sync_folder.unwrap(),
                std::path::PathBuf::from("/tmp/new-sync-target")
            );
            assert_eq!(
                settings.webdav_url.as_deref(),
                Some("https://example.invalid/dav")
            );
        })
    });
}

#[gpui::test]
fn confirmation_escape_restores_trigger_and_keeps_drawer_open(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            s.show(4, false, w, cx);
            s.close_focus.focus(w, cx);
            s.show_confirmation("import", w, cx);
        })
    });
    cx.update(|w, cx| w.draw(cx).clear(cx));
    cx.simulate_keystrokes("escape");
    cx.update(|w, cx| {
        view.update(cx, |s, _| {
            assert!(s.open);
            assert!(s.confirm.is_none());
            assert!(s.close_focus.is_focused(w));
        })
    });
}

#[gpui::test]
fn removing_pricing_row_moves_editors_and_blur_subscriptions_together(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            s.show(3, false, w, cx);
            for i in 0..12 {
                let _ = s.field(
                    &format!("price-model-{i}"),
                    "settings.modelPrefix",
                    &format!("model-{i}"),
                    false,
                    w,
                    cx,
                );
            }
            let original = s.fields.input("price-model-10").entity_id();
            s.fields.remove_indexed_row("price", 2, w, cx);
            assert_eq!(s.fields.input("price-model-9").entity_id(), original);
            assert_eq!(s.fields.value("price-model-2", cx), "model-3");
            s.pending = true; // Leave the blur queued for inspection instead of autosaving during render.
            s.fields
                .input("price-model-9")
                .update(cx, |_, cx| cx.emit(gpui_base::input::InputEvent::Blur));
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |s, _| {
            assert_eq!(s.fields.take_blurs(), vec!["price-model-9"])
        })
    });
}

#[gpui::test]
fn pending_reads_allow_navigation_and_ignore_retired_replies(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            s.show(2, true, w, cx);
            let old = s.take_request().unwrap().scope;
            assert!(s.loading && s.pending);
            s.select_tab(3, w, cx);
            assert_eq!(s.tab, 3);
            s.complete(
                Completion {
                    scope: old,
                    result: Err(aipass_agent::AgentCommandError {
                        code: None,
                        message: "stale".into(),
                    }),
                },
                w,
                cx,
            );
            assert!(s.pending && s.loading);
            assert!(s.error.is_empty());
            s.close(w, cx);
            assert!(!s.open);
            assert!(!s.pending);
            assert!(s.take_request().is_none());
            s.show(4, false, w, cx);
            s.apply(AgentRequest::ControlPanelStop, cx);
            s.close(w, cx);
            assert!(s.open, "do not dismiss an in-flight mutation");
        })
    });
}
