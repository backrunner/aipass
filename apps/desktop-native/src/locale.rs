use std::collections::HashMap;

thread_local! { static SELECTED: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) }; }
pub fn select(chinese: bool) {
    SELECTED.with(|value| value.set(Some(chinese)));
}
pub struct Locale(HashMap<String, String>, bool, HashMap<String, String>);

impl Locale {
    pub fn new(chinese: bool) -> Self {
        let mut dictionary: HashMap<String, String> =
            serde_json::from_str(include_str!("../../../packages/ui/src/locales/en.json"))
                .expect("checked shared English locale JSON");
        let mut chinese_dictionary = dictionary.clone();
        chinese_dictionary.extend(
            serde_json::from_str::<HashMap<String, String>>(include_str!(
                "../../../packages/ui/src/locales/zh-CN.json"
            ))
            .expect("checked shared Chinese locale JSON"),
        );
        let aliases: HashMap<String, String> = serde_json::from_str(include_str!(
            "../../../packages/ui/src/locales/aliases.json"
        ))
        .expect("checked shared locale aliases");
        for (alias, canonical) in aliases {
            let value = dictionary
                .get(&canonical)
                .expect("shared alias has a canonical translation")
                .clone();
            chinese_dictionary.insert(
                alias.clone(),
                chinese_dictionary
                    .get(&canonical)
                    .expect("Chinese alias")
                    .clone(),
            );
            dictionary.insert(alias, value);
        }
        Self(dictionary, chinese, chinese_dictionary)
    }

    pub fn is_chinese(&self) -> bool {
        SELECTED.with(|value| value.get()).unwrap_or(self.1)
    }

    pub fn language(&self) -> aipass_agent_protocol::UiLocale {
        if self.is_chinese() {
            aipass_agent_protocol::UiLocale::ZhCn
        } else {
            aipass_agent_protocol::UiLocale::En
        }
    }

    pub fn text(&self, key: &str) -> String {
        self.text_in(self.language(), key)
    }

    pub fn text_in(&self, language: aipass_agent_protocol::UiLocale, key: &str) -> String {
        (if language == aipass_agent_protocol::UiLocale::ZhCn {
            &self.2
        } else {
            &self.0
        })
        .get(key)
        .cloned()
        .unwrap_or_else(|| key.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_translation_keys_exist_in_both_locales() {
        for chinese in [false, true] {
            let locale = Locale::new(chinese);
            for source in [
                include_str!("settings_ui/about.rs"),
                include_str!("settings_ui/backup.rs"),
                include_str!("settings_ui/choices.rs"),
                include_str!("settings_ui/copying.rs"),
                include_str!("settings_ui/general.rs"),
                include_str!("settings_ui/motion.rs"),
                include_str!("settings_ui/panel.rs"),
                include_str!("settings_ui/panel_files.rs"),
                include_str!("settings_ui/security.rs"),
                include_str!("settings_ui/server.rs"),
                include_str!("settings_ui/sync.rs"),
                include_str!("settings_ui/sync_records.rs"),
                include_str!("settings_ui/view.rs"),
                include_str!("proxy_ui/context_menu.rs"),
                include_str!("proxy_ui/copying.rs"),
                include_str!("proxy_ui/credential_picker.rs"),
                include_str!("proxy_ui/dialogs.rs"),
                include_str!("proxy_ui/header.rs"),
                include_str!("proxy_ui/bind.rs"),
                include_str!("proxy_ui/integration.rs"),
                include_str!("proxy_ui/integration_card.rs"),
                include_str!("proxy_ui/logs.rs"),
                include_str!("proxy_ui/members.rs"),
                include_str!("proxy_ui/route.rs"),
                include_str!("proxy_ui/search.rs"),
                include_str!("proxy_ui/usage.rs"),
                include_str!("proxy_ui/view.rs"),
                include_str!("provider_tools_ui.rs"),
                include_str!("provider_tools_ui/fields.rs"),
                include_str!("provider_tools_ui/view.rs"),
                include_str!("provider_tools_ui/results.rs"),
                include_str!("providers/form.rs"),
                include_str!("providers/form_fields.rs"),
                include_str!("providers/form_picker.rs"),
                include_str!("providers/optional_fields.rs"),
                include_str!("providers/edit.rs"),
                include_str!("providers/presentation.rs"),
                include_str!("workspace/providers.rs"),
                include_str!("catalog/draft.rs"),
                include_str!("credentials.rs"),
                include_str!("providers/credentials.rs"),
                include_str!("providers/credentials/view.rs"),
            ] {
                for key in source.split('"').skip(1).step_by(2).filter(|part| {
                    [
                        "settings.",
                        "panel.",
                        "server.",
                        "search.",
                        "routeGroup.",
                        "updates.",
                        "error.",
                        "common.",
                        "provider",
                        "interface.",
                        "authScheme.",
                        "sidebar.",
                        "credential.",
                        "notice.",
                    ]
                    .iter()
                    .any(|prefix| part.starts_with(prefix))
                        && part.contains('.')
                        && !part.contains(' ')
                        && !part.contains('{')
                }) {
                    assert!(
                        locale.0.contains_key(key) && locale.2.contains_key(key),
                        "missing native locale: {key}"
                    );
                }
            }
        }
    }

    #[test]
    fn shared_aliases_resolve_in_both_native_locales() {
        let aliases: HashMap<String, String> = serde_json::from_str(include_str!(
            "../../../packages/ui/src/locales/aliases.json"
        ))
        .unwrap();
        for chinese in [false, true] {
            let locale = Locale::new(chinese);
            for (alias, canonical) in &aliases {
                assert_eq!(locale.text(alias), locale.text(canonical), "{alias}");
                assert_ne!(locale.text(alias), *alias);
            }
            assert_eq!(
                locale.text("auth.reset.cancel"),
                if chinese { "取消" } else { "Cancel" }
            );
        }
    }
}
