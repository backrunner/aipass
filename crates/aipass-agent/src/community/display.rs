//! Human-readable record names, independent of the account's ownership key.
use super::*;

pub(super) fn account_title(provider: &str, identity: &str, auth: &Value) -> String {
    if crate::provider_plugins::is_plugin(provider) {
        return auth["pluginName"].as_str().unwrap_or(provider).to_owned();
    }
    let catalog: Value =
        serde_json::from_str(include_str!("../subscriptions/catalog.json")).unwrap_or(Value::Null);
    let brand = if provider == "commandcode-plan" {
        "Command Code"
    } else {
        catalog
            .as_array()
            .and_then(|items| items.iter().find(|p| p["id"] == provider))
            .and_then(|p| p["name"].as_str())
            .unwrap_or(provider)
    };
    let friendly = [
        "/metadata/email",
        "/email",
        "/metadata/displayName",
        "/metadata/name",
    ]
    .iter()
    .find_map(|path| {
        auth.pointer(path)
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
    });
    let key_label = identity
        .strip_prefix("key-")
        .or_else(|| identity.rsplit_once(":key-").map(|(_, key)| key))
        .filter(|s| s.len() == 64)
        .map(|s| format!("API key {}", &s[s.len() - 8..]));
    let name = friendly.or(key_label.as_deref()).unwrap_or(identity).trim();
    let name = if name.contains('@') {
        name.split(':').next().unwrap_or(name)
    } else {
        name
    };
    let name = if name.len() >= 20 && name.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        &name[name.len() - 8..]
    } else {
        name
    };
    if name.is_empty() {
        brand.to_owned()
    } else {
        format!("{brand} · {name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_do_not_change_ownership_keys() {
        let id = "f055738b-a707-4b5b-8304-c37dd2525035";
        assert_eq!(
            account_title(
                "commandcode-plan",
                id,
                &json!({"metadata":{"email":"alice@example.test"}})
            ),
            "Command Code · alice@example.test"
        );
        assert_eq!(
            account_title("commandcode-plan", id, &json!({})),
            "Command Code · d2525035"
        );
        assert_eq!(
            account_title("codex", "alice@example.test:workspace-a", &json!({})),
            "Codex · alice@example.test"
        );
    }
}
