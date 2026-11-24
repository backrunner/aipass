//! Validate plugin-declared controls and the values received from any IPC client.
use super::{label, Result};
use aipass_agent_protocol::SensitiveString;
use aipass_provider_registry::plugin_ui::UiText;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Method {
    #[serde(rename = "type")]
    kind: String,
    label: UiText,
    #[serde(default)]
    prompts: Vec<Prompt>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Prompt {
    key: String,
    #[serde(rename = "type")]
    kind: String,
    message: UiText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    help: Option<UiText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    placeholder: Option<UiText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    min: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_length: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    when: Option<Condition>,
    #[serde(default)]
    required: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    options: Vec<Choice>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Choice {
    label: UiText,
    value: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Condition {
    field: String,
    equals: String,
}

impl Prompt {
    fn is_choice(&self) -> bool {
        matches!(self.kind.as_str(), "select" | "radio")
    }
    fn implicit_default(&self) -> Option<&str> {
        self.default
            .as_deref()
            .or_else(|| match self.kind.as_str() {
                "select" => self.options.first().map(|o| o.value.as_str()),
                "checkbox" => Some("false"),
                _ => None,
            })
    }
    fn accepts(&self, s: &str) -> bool {
        if s.encode_utf16().count() > self.max_length.unwrap_or(8192) {
            return false;
        }
        if s.is_empty() {
            return !self.required && !matches!(self.kind.as_str(), "checkbox" | "select");
        }
        match self.kind.as_str() {
            "select" | "radio" => self.options.iter().any(|o| o.value == s),
            "checkbox" => s == "true" || (!self.required && s == "false"),
            "integer" => s.parse::<i64>().is_ok_and(|n| {
                n.unsigned_abs() <= 9_007_199_254_740_991
                    && self.min.is_none_or(|m| n >= m)
                    && self.max.is_none_or(|m| n <= m)
            }),
            "url" => url::Url::parse(s).is_ok_and(|u| {
                matches!(u.scheme(), "http" | "https")
                    && u.has_host()
                    && u.username().is_empty()
                    && u.password().is_none()
            }),
            "text" | "secret" | "textarea" => !self.required || !s.trim().is_empty(),
            _ => false,
        }
    }
    fn valid(&self, preceding: &[Prompt]) -> bool {
        let plain = matches!(self.kind.as_str(), "text" | "secret" | "textarea" | "url");
        let numeric = self.kind == "integer";
        let mut values = HashSet::new();
        self.message.valid(256)
            && self.help.as_ref().is_none_or(|t| t.valid(512))
            && self
                .placeholder
                .as_ref()
                .is_none_or(|t| plain && t.valid(256))
            && (1..=48).contains(&self.key.len())
            && self
                .key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            && !matches!(
                self.key.as_str(),
                "__proto__"
                    | "constructor"
                    | "prototype"
                    | "toString"
                    | "valueOf"
                    | "hasOwnProperty"
            )
            && !preceding.iter().any(|p| p.key == self.key)
            && (plain || numeric || self.is_choice() || self.kind == "checkbox")
            && self
                .max_length
                .is_none_or(|n| plain && (1..=8192).contains(&n))
            && [self.min, self.max]
                .into_iter()
                .flatten()
                .all(|n| numeric && n.unsigned_abs() <= 9_007_199_254_740_991)
            && self.min.zip(self.max).is_none_or(|(a, b)| a <= b)
            && if self.is_choice() {
                (1..=64).contains(&self.options.len())
                    && self
                        .options
                        .iter()
                        .all(|o| o.label.valid(256) && label(&o.value) && values.insert(&o.value))
            } else {
                self.options.is_empty()
            }
            && self.default.as_deref().is_none_or(|s| {
                self.kind != "secret"
                    && (self.accepts(s) || self.kind == "checkbox" && s == "false")
            })
            && self.when.as_ref().is_none_or(|condition| {
                preceding.iter().any(|p| {
                    p.key == condition.field
                        && ((p.is_choice()
                            && p.options.iter().any(|o| o.value == condition.equals))
                            || (p.kind == "checkbox"
                                && matches!(condition.equals.as_str(), "true" | "false")))
                })
            })
    }
}
impl Method {
    pub(super) fn valid(&self) -> bool {
        matches!(self.kind.as_str(), "api" | "oauth")
            && self.label.valid(256)
            && self.prompts.len() <= 16
            && self
                .prompts
                .iter()
                .enumerate()
                .all(|(i, p)| p.valid(&self.prompts[..i]))
    }
    pub(super) fn catalog(&self, index: usize) -> Value {
        json!({"index":index,"type":self.kind,"label":self.label,"native":false,"prompts":self.prompts})
    }
    pub(crate) fn inputs(
        &self,
        mut raw: BTreeMap<String, SensitiveString>,
    ) -> Result<BTreeMap<String, SensitiveString>> {
        let mut active: BTreeMap<String, SensitiveString> = BTreeMap::new();
        for p in &self.prompts {
            let supplied = raw.remove(&p.key);
            if p.when
                .as_ref()
                .is_some_and(|c| active.get(&c.field).is_none_or(|v| v.expose() != c.equals))
            {
                continue;
            }
            let value = supplied.or_else(|| p.implicit_default().map(SensitiveString::new));
            if !p.accepts(value.as_ref().map_or("", SensitiveString::expose)) {
                return Err("Invalid plugin form input".into());
            }
            if let Some(value) = value {
                active.insert(p.key.clone(), value);
            }
        }
        if !raw.is_empty() {
            return Err("Unknown plugin form input".into());
        }
        Ok(active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn method(prompts: Value) -> Method {
        serde_json::from_value(
            json!({"type":"api","label":{"en":"Account","zh-CN":"账号"},"prompts":prompts}),
        )
        .unwrap()
    }

    #[test]
    fn only_visible_values_and_declared_defaults_reach_the_plugin() {
        let form = method(json!([
            {"type":"checkbox","key":"team","message":"Team"},
            {"type":"secret","key":"code","message":"Code","required":true,"when":{"field":"team","equals":"true"}},
            {"type":"select","key":"region","message":"Region","options":[{"label":"Local","value":"local"}]},
            {"type":"integer","key":"seats","message":"Seats","default":"4","min":1,"max":10}
        ]));
        assert!(form.valid());
        let values = form
            .inputs([("code".into(), SensitiveString::new("hidden"))].into())
            .unwrap();
        assert!(!values.contains_key("code"));
        assert_eq!(values["team"].expose(), "false");
        assert_eq!(values["region"].expose(), "local");
        assert_eq!(values["seats"].expose(), "4");
        for (key, value) in [
            ("team", "true"),
            ("team", "yes"),
            ("team", ""),
            ("region", ""),
            ("seats", "11"),
            ("seats", "1.5"),
            ("region", "absent"),
            ("unknown", "x"),
        ] {
            assert!(
                form.inputs([(key.into(), SensitiveString::new(value))].into())
                    .is_err(),
                "{key}"
            );
        }
    }

    #[test]
    fn rejects_invalid_definitions_and_urls() {
        for prompt in [
            json!({"type":"secret","default":"secret"}),
            json!({"type":"integer","min":2,"max":1}),
            json!({"type":"radio","options":[{"label":"A","value":"a"},{"label":"B","value":"a"}]}),
            json!({"type":"text","when":{"field":"missing","equals":"true"}}),
            json!({"type":"text","message":{"en":" "}}),
            json!({"type":"text","maxLength":9000}),
        ] {
            let mut field = json!({"key":"value","message":"Value"});
            field
                .as_object_mut()
                .unwrap()
                .extend(prompt.as_object().unwrap().clone());
            assert!(!method(json!([field])).valid());
        }
        let form = method(json!([{"type":"url","key":"url","message":"URL","required":true}]));
        for url in ["", "file:///tmp/data", "https://name:password@example.test"] {
            assert!(form
                .inputs([("url".into(), SensitiveString::new(url))].into())
                .is_err());
        }
        assert!(form
            .inputs([("url".into(), SensitiveString::new("https://example.test"))].into())
            .is_ok());
    }
}
