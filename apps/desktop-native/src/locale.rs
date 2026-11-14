use std::collections::HashMap;

pub struct Locale(HashMap<String, String>, bool);

impl Locale {
    pub fn new(chinese: bool) -> Self {
        let mut dictionary: HashMap<String, String> =
            serde_json::from_str(include_str!("../../../packages/ui/src/locales/en.json"))
                .expect("checked shared English locale JSON");
        if chinese {
            dictionary.extend(
                serde_json::from_str::<HashMap<String, String>>(include_str!(
                    "../../../packages/ui/src/locales/zh-CN.json"
                ))
                .expect("checked shared Chinese locale JSON"),
            );
        }
        let aliases: HashMap<String, String> = serde_json::from_str(include_str!(
            "../../../packages/ui/src/locales/aliases.json"
        ))
        .expect("checked shared locale aliases");
        for (alias, canonical) in aliases {
            let value = dictionary
                .get(&canonical)
                .expect("shared alias has a canonical translation")
                .clone();
            dictionary.insert(alias, value);
        }
        Self(dictionary, chinese)
    }

    pub fn language(&self) -> aipass_agent_protocol::UiLocale {
        if self.1 {
            aipass_agent_protocol::UiLocale::ZhCn
        } else {
            aipass_agent_protocol::UiLocale::En
        }
    }

    pub fn text(&self, key: &str) -> String {
        self.0.get(key).cloned().unwrap_or_else(|| key.to_owned())
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
                }) {
                    assert!(locale.0.contains_key(key), "missing native locale: {key}");
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
