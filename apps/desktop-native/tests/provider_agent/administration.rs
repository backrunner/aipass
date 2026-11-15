//! Exercise the native administration adapter against the isolated real Agent.
use super::*;
use aipass_agent_protocol::{LocalePreference, SyncMode, SyncSettingsUpdate, UiLocale};
use aipass_desktop_native::administration::{Operation, Outcome, Request, Scope, Surface};
use aipass_desktop_platform::preferences::{AppPreferences, ThemePreference};

fn request(transport: &mut AgentTransport, operation: Operation) -> Outcome {
    let scope = Scope {
        surface: Surface::Settings,
        generation: 23,
    };
    let Reply::Administration(completion) = transport
        .execute(Command::Administration(Box::new(Request {
            scope,
            operation,
        })))
        .unwrap()
    else {
        panic!("administration reply expected")
    };
    assert_eq!(completion.scope, scope);
    completion.result.unwrap()
}

fn load(transport: &mut AgentTransport, path: &Path) -> AppPreferences {
    let Outcome::Preferences(preferences) = request(
        transport,
        Operation::LoadPreferences {
            path: Some(path.into()),
            preferences: AppPreferences::default(),
        },
    ) else {
        panic!("preferences expected")
    };
    preferences
}

pub(super) fn workflow(transport: &mut AgentTransport, root: &Path) {
    let path = root.join("native-preferences.json");
    let previous = load(transport, &path);
    let desired = AppPreferences {
        auto_lock_minutes: 15,
        clipboard_clear_seconds: 90,
        theme: ThemePreference::Dark,
        locale: LocalePreference::ZhCn,
        ..previous.clone()
    };
    let Outcome::PreferencesSaved { preferences, error } = request(
        transport,
        Operation::Preferences {
            path: Some(path.clone()),
            previous,
            preferences: desired,
        },
    ) else {
        panic!("save acknowledgement expected")
    };
    assert!(error.is_none());
    assert_eq!(preferences.resolved_locale, Some(UiLocale::ZhCn));
    let reloaded = load(transport, &path);
    assert_eq!(reloaded.theme, ThemePreference::Dark);
    assert_eq!(reloaded.auto_lock_minutes, 15);
    assert_eq!(reloaded.clipboard_clear_seconds, 90);

    // The directory is an invalid preferences file, after both Agent-owned
    // settings have been saved. The adapter must retain those acknowledgements.
    let desired = AppPreferences {
        auto_lock_minutes: 30,
        theme: ThemePreference::Light,
        locale: LocalePreference::En,
        ..reloaded.clone()
    };
    let Outcome::PreferencesSaved { preferences, error } = request(
        transport,
        Operation::Preferences {
            path: Some(root.into()),
            previous: reloaded,
            preferences: desired.clone(),
        },
    ) else {
        panic!("partial acknowledgement expected")
    };
    assert!(error.is_some());
    assert_eq!(preferences.theme, ThemePreference::Dark);
    assert_eq!(preferences.auto_lock_minutes, 30);
    assert_eq!(preferences.resolved_locale, Some(UiLocale::En));
    let reloaded = load(transport, &path);
    assert_eq!(reloaded.auto_lock_minutes, 30);
    assert_eq!(reloaded.locale, LocalePreference::En);
    assert_eq!(reloaded.theme, ThemePreference::Dark);
    let Outcome::PreferencesSaved { error, .. } = request(
        transport,
        Operation::Preferences {
            path: Some(path.clone()),
            previous: preferences,
            preferences: desired,
        },
    ) else {
        panic!("retry acknowledgement expected")
    };
    assert!(error.is_none());
    assert_eq!(load(transport, &path).theme, ThemePreference::Light);

    // A local file cannot be a sync directory. Saving the setting still
    // succeeds; running the configured sync fails without erasing that result.
    let invalid = root.join("not-a-sync-directory");
    fs::write(&invalid, b"synthetic file").unwrap();
    let settings = SyncSettingsUpdate {
        mode: SyncMode::Local,
        sync_folder: Some(invalid.clone()),
        webdav_url: None,
        webdav_username: None,
        webdav_password: None,
        clear_webdav_password: false,
    };
    let Outcome::SyncSaved {
        settings: saved,
        error,
    } = request(
        transport,
        Operation::SaveSync {
            settings: settings.clone(),
            run: true,
        },
    )
    else {
        panic!("sync acknowledgement expected")
    };
    assert_eq!(saved.sync_folder.as_deref(), Some(invalid.as_path()));
    assert!(error.is_some());
    // Restore a usable synthetic target and verify the complete local sync.
    let destination = root.join("native-sync");
    let Outcome::SyncSaved { error, .. } = request(
        transport,
        Operation::SaveSync {
            settings: SyncSettingsUpdate {
                sync_folder: Some(destination.clone()),
                ..settings.clone()
            },
            run: true,
        },
    ) else {
        panic!("sync retry acknowledgement expected")
    };
    assert!(error.is_none(), "local sync retry failed: {error:?}");
    let Outcome::Sync {
        settings: saved, ..
    } = request(transport, Operation::LoadSync)
    else {
        panic!("sync read expected")
    };
    assert_eq!(saved.sync_folder.as_deref(), Some(destination.as_path()));
    // Retire the synthetic target watcher before the remaining provider checks.
    let Outcome::SyncSaved { error, .. } = request(
        transport,
        Operation::SaveSync {
            settings: SyncSettingsUpdate {
                sync_folder: None,
                ..settings
            },
            run: false,
        },
    ) else {
        panic!("clear sync target expected")
    };
    assert!(error.is_none());
}
