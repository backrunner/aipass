//! The tray consumes the same catalog as the desktop and browser extension.
use aipass_agent_protocol::UiLocale;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock,
    },
};

type Messages = BTreeMap<String, String>;
static CHINESE: AtomicBool = AtomicBool::new(false);
static EN: OnceLock<Messages> = OnceLock::new();
static ZH: OnceLock<Messages> = OnceLock::new();
static ALIASES: OnceLock<Messages> = OnceLock::new();

pub(crate) fn set_locale(locale: UiLocale) {
    CHINESE.store(locale == UiLocale::ZhCn, Ordering::Relaxed);
}

pub(crate) fn locale() -> UiLocale {
    if CHINESE.load(Ordering::Relaxed) {
        UiLocale::ZhCn
    } else {
        UiLocale::En
    }
}

fn catalog(locale: UiLocale) -> &'static Messages {
    match locale {
        UiLocale::En => EN.get_or_init(|| {
            serde_json::from_str(include_str!("../../../../packages/ui/src/locales/en.json"))
                .expect("English catalog")
        }),
        UiLocale::ZhCn => ZH.get_or_init(|| {
            serde_json::from_str(include_str!(
                "../../../../packages/ui/src/locales/zh-CN.json"
            ))
            .expect("Chinese catalog")
        }),
    }
}

fn translate(locale: UiLocale, key: &str) -> String {
    let aliases = ALIASES.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../../../../packages/ui/src/locales/aliases.json"
        ))
        .expect("message aliases")
    });
    let key = aliases.get(key).map(String::as_str).unwrap_or(key);
    catalog(locale)
        .get(key)
        .or_else(|| catalog(UiLocale::En).get(key))
        .cloned()
        .unwrap_or_else(|| key.to_string())
}

pub(crate) fn tr(key: &str) -> String {
    translate(locale(), key)
}

pub(crate) fn tf(key: &str, params: &[(&str, String)]) -> String {
    let mut text = tr(key);
    for (name, value) in params {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

#[cfg(target_os = "macos")]
pub(crate) fn messages() -> Messages {
    catalog(locale())
        .iter()
        .filter(|(key, _)| {
            [
                "tray.",
                "panel.",
                "auth.",
                "password.",
                "ext.",
                "server.",
                "titlebar.",
            ]
            .iter()
            .any(|prefix| key.starts_with(prefix))
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_and_extension_share_unlock_errors_in_both_languages() {
        for locale in [UiLocale::En, UiLocale::ZhCn] {
            assert_eq!(
                translate(locale, "error.incorrectMasterPassword"),
                translate(locale, "ext.unlock.wrongPassword")
            );
            assert_eq!(
                translate(locale, "auth.unlock.submit"),
                translate(locale, "ext.unlock.action")
            );
            assert_ne!(translate(locale, "tray.vaultLocked"), "tray.vaultLocked");
        }
    }
}
