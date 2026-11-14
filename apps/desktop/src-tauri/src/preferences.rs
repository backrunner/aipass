//! Desktop-local preferences; no vault secrets are stored here.
use crate::commands::load_language_settings;
use crate::models::SavePreferencesRequest;
use crate::*;
use aipass_agent_protocol::SessionPolicy;

pub(crate) use aipass_desktop_platform::clipboard::normalize_seconds as normalize_clipboard_seconds;

pub(crate) fn load_preferences(app: &AppHandle) -> Result<AppPreferences, String> {
    aipass_desktop_platform::preferences::load_and_migrate(&preferences_path(app)?)
}

fn save_preferences(app: &AppHandle, preferences: &AppPreferences) -> Result<(), String> {
    aipass_desktop_platform::preferences::save(&preferences_path(app)?, preferences)
}

fn preferences_path(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(path) = runtime_check::path("preferences.json") {
        return Ok(path);
    }
    let dir = app.path().app_config_dir().map_err(|err| err.to_string())?;
    Ok(dir.join("preferences.json"))
}

#[tauri::command]
pub(crate) async fn preferences_load(app: AppHandle) -> Result<AppPreferences, String> {
    run_blocking(move || {
        let local = load_preferences(&app)?;
        let language = load_language_settings(&app, local.locale)?;
        let policy = agent_request_no_unlock::<SessionPolicy>(&app, AgentRequest::SessionPolicyGet)
            .unwrap_or_default();
        Ok(AppPreferences {
            auto_lock_minutes: policy.idle_lock_minutes,
            clipboard_clear_seconds: normalize_clipboard_seconds(local.clipboard_clear_seconds),
            credential_click_action: local.credential_click_action,
            confirm_secret_copy: local.confirm_secret_copy,
            lock_on_sleep: policy.lock_on_sleep,
            lock_on_screen_lock: policy.lock_on_screen_lock,
            theme: local.theme,
            locale: language.locale,
            resolved_locale: Some(language.resolved_locale),
            official_accounts_import: local.official_accounts_import,
        })
    })
    .await
}

#[tauri::command]
pub(crate) async fn preferences_save(
    app: AppHandle,
    request: SavePreferencesRequest,
) -> Result<AppPreferences, String> {
    run_blocking(move || {
        let current_policy =
            agent_request_no_unlock::<SessionPolicy>(&app, AgentRequest::SessionPolicyGet)
                .unwrap_or_default();
        let stored = load_preferences(&app)?;
        let language = load_language_settings(&app, stored.locale)?;
        let preferences = AppPreferences {
            auto_lock_minutes: request.auto_lock_minutes.min(1_440),
            clipboard_clear_seconds: normalize_clipboard_seconds(request.clipboard_clear_seconds),
            credential_click_action: request
                .credential_click_action
                .unwrap_or(stored.credential_click_action),
            confirm_secret_copy: request
                .confirm_secret_copy
                .unwrap_or(stored.confirm_secret_copy),
            lock_on_sleep: request
                .lock_on_sleep
                .unwrap_or(current_policy.lock_on_sleep),
            lock_on_screen_lock: request
                .lock_on_screen_lock
                .unwrap_or(current_policy.lock_on_screen_lock),
            theme: request.theme.unwrap_or(stored.theme),
            locale: request.locale.unwrap_or(language.locale),
            resolved_locale: None,
            official_accounts_import: request
                .official_accounts_import
                .unwrap_or(stored.official_accounts_import),
        };

        let _: SessionPolicy = agent_request_no_unlock(
            &app,
            AgentRequest::SessionPolicySet {
                policy: SessionPolicy {
                    idle_lock_minutes: preferences.auto_lock_minutes,
                    lock_on_sleep: preferences.lock_on_sleep,
                    lock_on_screen_lock: preferences.lock_on_screen_lock,
                },
            },
        )?;
        let language: aipass_agent_protocol::LanguageSettings = agent_request_no_unlock(
            &app,
            AgentRequest::LanguageSettingsSet {
                locale: preferences.locale,
            },
        )?;
        crate::tray_i18n::set_locale(language.resolved_locale);
        let _ = app.emit(crate::tray::REFRESH_PROXY_TRAY_EVENT, ());
        let preferences = AppPreferences {
            resolved_locale: Some(language.resolved_locale),
            ..preferences
        };
        save_preferences(&app, &preferences)?;
        Ok(preferences)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::CredentialClickAction;
    #[test]
    fn old_preferences_get_safe_credential_defaults() {
        let preferences: AppPreferences = serde_json::from_value(serde_json::json!({
            "autoLockMinutes": 60, "clipboardClearSeconds": 0,
            "lockOnSleep": true, "lockOnScreenLock": true
        }))
        .unwrap();
        assert_eq!(
            preferences.credential_click_action,
            CredentialClickAction::Reveal
        );
        assert!(preferences.confirm_secret_copy);
        assert_eq!(
            normalize_clipboard_seconds(preferences.clipboard_clear_seconds),
            45
        );
    }
    #[test]
    fn clipboard_duration_is_always_bounded_and_enabled() {
        assert_eq!(normalize_clipboard_seconds(0), 45);
        assert_eq!(normalize_clipboard_seconds(1), 1);
        assert_eq!(normalize_clipboard_seconds(u16::MAX), 600);
    }
    #[test]
    fn explicit_copy_settings_round_trip() {
        let preferences = AppPreferences {
            credential_click_action: CredentialClickAction::Copy,
            confirm_secret_copy: false,
            ..Default::default()
        };
        let loaded: AppPreferences =
            serde_json::from_slice(&serde_json::to_vec(&preferences).unwrap()).unwrap();
        assert_eq!(loaded.credential_click_action, CredentialClickAction::Copy);
        assert!(!loaded.confirm_secret_copy);
    }
}
