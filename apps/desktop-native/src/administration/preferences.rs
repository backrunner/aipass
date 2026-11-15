//! Acknowledge each durable owner separately; a later failure is not a rollback.
use super::*;
use std::path::Path;

pub(super) fn save(
    agent: &impl Client,
    path: Option<&Path>,
    mut confirmed: AppPreferences,
    mut requested: AppPreferences,
) -> Outcome {
    requested.auto_lock_minutes = requested.auto_lock_minutes.min(1440);
    requested.clipboard_clear_seconds =
        aipass_desktop_platform::clipboard::normalize_seconds(requested.clipboard_clear_seconds);
    if policy(&requested) != policy(&confirmed) {
        match agent.request(&AgentRequest::SessionPolicySet {
            policy: policy(&requested),
        }) {
            Ok(value) => apply_policy(&mut confirmed, value),
            Err(error) => {
                // A lost reply can follow a successful write. Re-read only that
                // owner, retain the error, and never infer an implicit rollback.
                if let Ok(value) = agent.request(&AgentRequest::SessionPolicyGet) {
                    apply_policy(&mut confirmed, value);
                }
                return outcome(confirmed, Some(error));
            }
        }
    }
    if requested.locale != confirmed.locale {
        match agent.request(&AgentRequest::LanguageSettingsSet {
            locale: requested.locale,
        }) {
            Ok(value) => apply_language(&mut confirmed, value),
            Err(error) => {
                if let Ok(value) = agent.request(&AgentRequest::LanguageSettingsGet {
                    legacy_locale: None,
                }) {
                    apply_language(&mut confirmed, value);
                }
                return outcome(confirmed, Some(error));
            }
        }
    }
    // The local file mirrors the acknowledged Agent values. Local-only controls
    // (theme, clipboard and import preferences) never rewrite Agent policy.
    apply_policy(&mut requested, policy(&confirmed));
    requested.locale = confirmed.locale;
    requested.resolved_locale = confirmed.resolved_locale;
    if let Some(path) = path {
        if let Err(error) = aipass_desktop_platform::preferences::save(path, &requested) {
            return outcome(confirmed, Some(local_error(error)));
        }
    }
    outcome(requested, None)
}

fn policy(preferences: &AppPreferences) -> SessionPolicy {
    SessionPolicy {
        idle_lock_minutes: preferences.auto_lock_minutes,
        lock_on_sleep: preferences.lock_on_sleep,
        lock_on_screen_lock: preferences.lock_on_screen_lock,
    }
}
fn apply_policy(preferences: &mut AppPreferences, policy: SessionPolicy) {
    preferences.auto_lock_minutes = policy.idle_lock_minutes;
    preferences.lock_on_sleep = policy.lock_on_sleep;
    preferences.lock_on_screen_lock = policy.lock_on_screen_lock;
}
fn apply_language(preferences: &mut AppPreferences, language: LanguageSettings) {
    preferences.locale = language.locale;
    preferences.resolved_locale = Some(language.resolved_locale);
}
fn outcome(preferences: AppPreferences, error: Option<AgentCommandError>) -> Outcome {
    Outcome::PreferencesSaved { preferences, error }
}
