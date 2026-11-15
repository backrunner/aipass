use super::*;
use aipass_desktop_platform::preferences::{self as storage, ThemePreference};
use serde_json::{json, Value};
use std::{cell::RefCell, collections::VecDeque};

type Step = (&'static str, Result<Value, AgentCommandError>);
struct Script(RefCell<VecDeque<Step>>);
impl Script {
    fn new(steps: impl IntoIterator<Item = Step>) -> Self {
        Self(RefCell::new(steps.into_iter().collect()))
    }
    fn done(&self) {
        assert!(self.0.borrow().is_empty());
    }
}
impl Client for Script {
    fn request<T: serde::de::DeserializeOwned>(
        &self,
        request: &AgentRequest,
    ) -> Result<T, AgentCommandError> {
        let (name, response) = self
            .0
            .borrow_mut()
            .pop_front()
            .expect("unexpected Agent request");
        assert_eq!(request.event_name(), name);
        response.map(|value| serde_json::from_value(value).unwrap())
    }
}
fn failure() -> AgentCommandError {
    local_error("synthetic failure".into())
}
fn policy(minutes: u16) -> Value {
    serde_json::to_value(SessionPolicy {
        idle_lock_minutes: minutes,
        ..Default::default()
    })
    .unwrap()
}
fn language(locale: LocalePreference) -> Value {
    serde_json::to_value(LanguageSettings {
        locale,
        resolved_locale: if locale == LocalePreference::ZhCn {
            UiLocale::ZhCn
        } else {
            UiLocale::En
        },
    })
    .unwrap()
}
fn saved(outcome: Outcome) -> (AppPreferences, Option<AgentCommandError>) {
    let Outcome::PreferencesSaved { preferences, error } = outcome else {
        panic!("expected preference acknowledgement")
    };
    (preferences, error)
}

#[test]
fn local_preferences_do_not_rewrite_agent_policy_or_language() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preferences.json");
    let previous = AppPreferences::default();
    let requested = AppPreferences {
        theme: ThemePreference::Dark,
        clipboard_clear_seconds: 0,
        ..previous.clone()
    };
    let agent = Script::new([]);
    let (confirmed, error) = saved(preferences::save(&agent, Some(&path), previous, requested));
    assert!(error.is_none());
    assert_eq!(confirmed.theme, ThemePreference::Dark);
    assert_eq!(confirmed.clipboard_clear_seconds, 45);
    assert_eq!(storage::load(&path).unwrap().theme, ThemePreference::Dark);
    agent.done();
}

#[test]
fn later_language_failure_keeps_acknowledged_policy_and_retries_only_missing_steps() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preferences.json");
    let previous = AppPreferences::default();
    storage::save(&path, &previous).unwrap();
    let requested = AppPreferences {
        auto_lock_minutes: 15,
        locale: LocalePreference::ZhCn,
        theme: ThemePreference::Dark,
        ..previous.clone()
    };
    let agent = Script::new([
        ("session.policy.set", Ok(policy(15))),
        ("preferences.language.set", Err(failure())),
        (
            "preferences.language.get",
            Ok(language(LocalePreference::System)),
        ),
    ]);
    let (confirmed, error) = saved(preferences::save(
        &agent,
        Some(&path),
        previous,
        requested.clone(),
    ));
    assert!(error.is_some());
    assert_eq!(confirmed.auto_lock_minutes, 15);
    assert_eq!(confirmed.locale, LocalePreference::System);
    assert_eq!(confirmed.theme, ThemePreference::System);
    assert_eq!(storage::load(&path).unwrap().auto_lock_minutes, 60);
    agent.done();
    let retry = Script::new([(
        "preferences.language.set",
        Ok(language(LocalePreference::ZhCn)),
    )]);
    let (confirmed, error) = saved(preferences::save(&retry, Some(&path), confirmed, requested));
    assert!(error.is_none());
    assert_eq!(confirmed.resolved_locale, Some(UiLocale::ZhCn));
    let stored = storage::load(&path).unwrap();
    assert_eq!(stored.auto_lock_minutes, 15);
    assert_eq!(stored.theme, ThemePreference::Dark);
    retry.done();
}

#[test]
fn local_file_failure_retains_agent_acknowledgement_without_accepting_local_draft() {
    let dir = tempfile::tempdir().unwrap();
    let previous = AppPreferences::default();
    let requested = AppPreferences {
        auto_lock_minutes: 30,
        theme: ThemePreference::Dark,
        ..previous.clone()
    };
    let agent = Script::new([("session.policy.set", Ok(policy(30)))]);
    let (confirmed, error) = saved(preferences::save(
        &agent,
        Some(dir.path()),
        previous,
        requested.clone(),
    ));
    assert!(error.is_some());
    assert_eq!(confirmed.auto_lock_minutes, 30);
    assert_eq!(confirmed.theme, ThemePreference::System);
    agent.done();
    let retry = Script::new([]);
    let path = dir.path().join("preferences.json");
    let (_, error) = saved(preferences::save(&retry, Some(&path), confirmed, requested));
    assert!(error.is_none());
    assert_eq!(storage::load(&path).unwrap().theme, ThemePreference::Dark);
    retry.done();
}

#[test]
fn failed_policy_reply_reconciles_the_authoritative_state_without_claiming_success() {
    let previous = AppPreferences::default();
    let requested = AppPreferences {
        auto_lock_minutes: 30,
        ..previous.clone()
    };
    let agent = Script::new([
        ("session.policy.set", Err(failure())),
        ("session.policy.get", Ok(policy(30))),
    ]);
    let (confirmed, error) = saved(preferences::save(&agent, None, previous, requested));
    assert!(error.is_some());
    assert_eq!(confirmed.auto_lock_minutes, 30);
    agent.done();
}

fn sync_update() -> SyncSettingsUpdate {
    SyncSettingsUpdate {
        mode: SyncMode::WebDav,
        sync_folder: None,
        webdav_url: Some("https://example.invalid/dav".into()),
        webdav_username: None,
        webdav_password: Some(SensitiveString::new("fixture-only")),
        clear_webdav_password: false,
    }
}
#[test]
fn failed_sync_retains_successfully_saved_target_and_password_acknowledgement() {
    let response = json!({"mode":"web_dav","webdavUrl":"https://example.invalid/dav","hasWebdavPassword":true});
    let agent = Script::new([
        ("sync.settings.set", Ok(response)),
        ("sync.configured", Err(failure())),
    ]);
    let outcome = Operation::SaveSync {
        settings: sync_update(),
        run: true,
    }
    .execute(&agent)
    .unwrap();
    let Outcome::SyncSaved { settings, error } = outcome else {
        panic!("expected saved sync settings")
    };
    assert!(settings.has_webdav_password);
    assert_eq!(
        settings.webdav_url.as_deref(),
        Some("https://example.invalid/dav")
    );
    assert!(error.is_some());
    agent.done();
}
#[test]
fn rejected_sync_settings_never_start_sync() {
    let agent = Script::new([("sync.settings.set", Err(failure()))]);
    assert!(Operation::SaveSync {
        settings: sync_update(),
        run: true
    }
    .execute(&agent)
    .is_err());
    agent.done();
}
