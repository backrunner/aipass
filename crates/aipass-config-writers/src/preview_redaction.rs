//! Redact structured values before computing a diff, including multiline TOML.
use crate::native_auth::PrivateJson;
use serde_json::Value;
use std::sync::LazyLock;
use toml_edit::{DocumentMut, Item, Table, Value as TomlValue};

fn sensitive(key: &str) -> bool {
    let key = key.to_ascii_lowercase().replace(['_', '-'], "");
    [
        "apikey",
        "apitoken",
        "aipasskey",
        "accesstoken",
        "refreshtoken",
        "idtoken",
        "authtoken",
        "bearertoken",
        "authorization",
        "secret",
        "password",
        "cookie",
    ]
    .iter()
    .any(|part| key.contains(part))
        || key.ends_with("token")
        || matches!(key.as_str(), "auth" | "key")
}
fn helper(key: &str, value: &str) -> bool {
    static COMMAND: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"^aipass get [0-9a-f-]{36}(?: --secret-id '[0-9a-f-]{36}')? --reveal$")
            .unwrap()
    });
    key == "apiKeyHelper" && COMMAND.is_match(value)
}
fn json(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if sensitive(key) && !value.as_str().is_some_and(|v| helper(key, v)) {
                    let _old =
                        PrivateJson(std::mem::replace(value, Value::String("[redacted]".into())));
                } else {
                    json(value);
                }
            }
        }
        Value::Array(values) => values.iter_mut().for_each(json),
        _ => {}
    }
}
fn table(table: &mut Table) {
    for (key, item) in table.iter_mut() {
        if sensitive(key.get()) {
            *item = toml_edit::value("[redacted]");
        } else {
            redact_item(item);
        }
    }
}
fn redact_item(item: &mut Item) {
    match item {
        Item::Table(t) => table(t),
        Item::ArrayOfTables(tables) => tables.iter_mut().for_each(table),
        Item::Value(value) => redact_value(value),
        Item::None => {}
    }
}
fn redact_value(value: &mut TomlValue) {
    match value {
        TomlValue::InlineTable(t) => {
            for (key, value) in t.iter_mut() {
                if sensitive(key.get()) {
                    *value = TomlValue::from("[redacted]");
                } else {
                    redact_value(value);
                }
            }
        }
        TomlValue::Array(values) => values.iter_mut().for_each(redact_value),
        _ => {}
    }
}
pub fn redact_config(content: &str) -> String {
    if let Ok(value) = json5::from_str::<Value>(content) {
        let mut value = PrivateJson(value);
        json(&mut value.0);
        return serde_json::to_string_pretty(&value.0).unwrap_or_else(|_| "[redacted]".into());
    }
    if let Ok(mut doc) = content.parse::<DocumentMut>() {
        table(doc.as_table_mut());
        return doc.to_string();
    }
    // Shell env helpers and legacy line diffs are not structured documents.
    static SECRET: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
        r#"(?i)((?:[\w.-]*["']?)(?:api[_-]?key|aipass[_-]?key|token|authorization|auth|secret|password|cookie)[\w.-]*["']?\s*[:=]\s*)("""[\s\S]*?"""|'''[\s\S]*?'''|"(?:\\.|[^"\\])*"|'[^']*'|[^\r\n]+)"#
    ).unwrap()
    });
    SECRET.replace_all(content, "$1\"[redacted]\"").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn structured_tokens_are_redacted_regardless_of_literal_shape() {
        for raw in [
            "experimental_bearer_token = '''\nfake-private-token\n'''\nmodel = 'safe-model'\n",
            "[model_providers.aipass.http_headers]\nAuthorization = [\"fake-private-token\"]\n",
            r#"{"nested":[{"accessToken":"fake-private-token","refreshToken":"fake-private-token","apiKeyHelper":"echo fake-private-token"}]}"#,
            "[model_providers.custom.auth]\ncommand = 'echo'\nargs = ['fake-private-token']\n",
            r#"{"auth":{"command":"echo","args":["fake-private-token"]},"api_token":"fake-private-token"}"#,
            "export AIPASS_KEY_FIXTURE='fake-private-token'\nexport AIPASS_PROXY_TOKEN='fake-private-token'\n",
            r#"{"env":{"AIPASS_KEY_FIXTURE":"fake-private-token","AIPASS_PROXY_TOKEN":"fake-private-token"}}"#,
        ] {
            assert!(!redact_config(raw).contains("fake-private-token"));
        }
        assert!(redact_config("model = 'safe-model'\n").contains("safe-model"));
    }
}
