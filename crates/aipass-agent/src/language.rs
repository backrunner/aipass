use aipass_agent_protocol::{LanguageSettings, LocalePreference, UiLocale};
use aipass_storage::atomic_write_bytes;
use anyhow::Result;
use std::{fs, io::ErrorKind, path::Path, sync::Mutex};

// Serialize first-run migration against writes from other authenticated clients.
static SETTINGS_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn load(vault_dir: &Path, legacy: Option<LocalePreference>) -> Result<LanguageSettings> {
    let _guard = SETTINGS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let path = crate::session::policy_path(vault_dir).with_file_name("language.json");
    let locale = match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            // A browser ping must not install a default before desktop migration.
            if let Some(locale) = legacy {
                write(vault_dir, locale)?;
            }
            legacy.unwrap_or_default()
        }
        Err(err) => return Err(err.into()),
    };
    Ok(resolve(locale))
}

pub(crate) fn save(vault_dir: &Path, locale: LocalePreference) -> Result<LanguageSettings> {
    let _guard = SETTINGS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    write(vault_dir, locale)?;
    Ok(resolve(locale))
}

fn write(vault_dir: &Path, locale: LocalePreference) -> Result<()> {
    let path = crate::session::policy_path(vault_dir).with_file_name("language.json");
    fs::create_dir_all(path.parent().expect("settings parent"))?;
    atomic_write_bytes(&path, &serde_json::to_vec(&locale)?)?;
    Ok(())
}

fn resolve(locale: LocalePreference) -> LanguageSettings {
    LanguageSettings {
        locale,
        resolved_locale: resolve_locale(locale, sys_locale::get_locale().as_deref()),
    }
}

fn resolve_locale(preference: LocalePreference, system: Option<&str>) -> UiLocale {
    match preference {
        LocalePreference::En => UiLocale::En,
        LocalePreference::ZhCn => UiLocale::ZhCn,
        LocalePreference::System => {
            let language = system
                .unwrap_or("en")
                .split(['-', '_'])
                .next()
                .unwrap_or("en");
            if language.eq_ignore_ascii_case("zh") {
                UiLocale::ZhCn
            } else {
                UiLocale::En
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_survives_browser_first_and_never_overwrites_a_saved_choice() {
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path().join("vault");
        assert_eq!(load(&vault, None).unwrap().locale, LocalePreference::System);
        assert_eq!(
            load(&vault, Some(LocalePreference::ZhCn))
                .unwrap()
                .resolved_locale,
            UiLocale::ZhCn
        );
        save(&vault, LocalePreference::En).unwrap();
        assert_eq!(
            load(&vault, Some(LocalePreference::ZhCn))
                .unwrap()
                .resolved_locale,
            UiLocale::En
        );
        save(&vault, LocalePreference::System).unwrap();
        assert_eq!(
            load(&vault, Some(LocalePreference::ZhCn)).unwrap().locale,
            LocalePreference::System
        );
    }

    #[test]
    fn all_clients_use_the_os_language_and_explicit_choice_wins() {
        assert_eq!(
            resolve_locale(LocalePreference::System, Some("zh_Hans_CN")),
            UiLocale::ZhCn
        );
        assert_eq!(
            resolve_locale(LocalePreference::System, Some("en-US")),
            UiLocale::En
        );
        assert_eq!(resolve_locale(LocalePreference::System, None), UiLocale::En);
        assert_eq!(
            resolve_locale(LocalePreference::En, Some("zh-CN")),
            UiLocale::En
        );
        assert_eq!(
            resolve_locale(LocalePreference::ZhCn, Some("en-US")),
            UiLocale::ZhCn
        );
    }
}
