//! Detection and vault import for providers configured in CC Switch.
//!
//! CC Switch keeps providers in `~/.cc-switch/cc-switch.db`, or legacy
//! `config.json`, grouped by target app (including OpenCode).
//! The agent is the only component allowed to read them; keys are persisted into the vault and
//! never returned over IPC.

use aipass_agent_protocol::{endpoint_url, CcSwitchDetection, OfficialAccountRefreshResult};
use aipass_provider_registry::{
    AuthScheme, CredentialKind, InterfaceType, ProviderEndpoint, ProviderKind,
};
use aipass_vault::{ProviderEntryInput, Vault};
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::official_accounts::{home, refresh_account_secret};
use std::collections::HashSet;

#[path = "ccswitch_database.rs"]
mod database;

const CCSWITCH_APPS: [&str; 4] = ["claude", "codex", "gemini", "opencode"];

#[derive(Clone, Debug)]
pub(crate) struct CcswitchProvider {
    /// CC Switch app group this provider belongs to.
    app_id: String,
    name: String,
    /// Missing or blank keys are reported as "skipped" at import time.
    key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    website_host: Option<String>,
    category: Option<String>,
    notes: Option<String>,
    is_current: bool,
    interface_type: InterfaceType,
}

fn config_path() -> PathBuf {
    config_path_at(&home().join(".cc-switch"))
}

fn config_path_at(dir: &Path) -> PathBuf {
    let database = dir.join("cc-switch.db");
    if database.is_file() {
        database
    } else {
        dir.join("config.json")
    }
}

pub(crate) fn detect_ccswitch() -> CcSwitchDetection {
    let path = config_path();
    let config_exists = path.is_file();
    let app_installed = if cfg!(target_os = "macos") {
        Path::new("/Applications/CC Switch.app").exists()
            || home().join("Applications").join("CC Switch.app").exists()
    } else {
        // There is no reliable install marker on other platforms; the config
        // file is the best signal that the app was ever run.
        config_exists
    };
    CcSwitchDetection {
        config_exists,
        app_installed,
        config_path: config_exists.then(|| path.display().to_string()),
    }
}

/// Parse CC Switch's config into importable providers. A missing or
/// badly-formed file yields an empty list rather than an error, and one
/// malformed provider entry never aborts the rest.
pub(crate) fn discover_ccswitch_providers() -> Vec<CcswitchProvider> {
    discover_ccswitch_providers_at(&config_path())
}

fn discover_ccswitch_providers_at(path: &Path) -> Vec<CcswitchProvider> {
    if path.extension().is_some_and(|extension| extension == "db") {
        return database::discover(path).unwrap_or_else(|error| {
            crate::logging::write_component_log(
                crate::logging::AGENT_LOG,
                "WARN",
                &format!("cc-switch database unreadable: {error}"),
            );
            Vec::new()
        });
    }
    let value = match std::fs::read(path)
        .map_err(anyhow::Error::from)
        .and_then(|bytes| Ok(serde_json::from_slice::<Value>(&bytes)?))
    {
        Ok(value) => value,
        Err(error) => {
            crate::logging::write_component_log(
                crate::logging::AGENT_LOG,
                "WARN",
                &format!("cc-switch config at {} unreadable: {error}", path.display()),
            );
            return Vec::new();
        }
    };
    let Some(groups) = value.get("providers").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut providers = Vec::new();
    for app_id in CCSWITCH_APPS {
        let Some(group) = groups.get(app_id).and_then(Value::as_object) else {
            continue;
        };
        let current = group.get("current").and_then(Value::as_str);
        let Some(items) = group.get("providers").and_then(Value::as_object) else {
            continue;
        };
        for (id, item) in items {
            if let Some(provider) = parse_provider(app_id, id, item, current == Some(id.as_str())) {
                providers.push(provider);
            }
        }
    }
    // Import the currently selected provider of each app first, so dedupe
    // attributes a shared key to it rather than to a stale sibling.
    providers.sort_by_key(|provider| !provider.is_current);
    providers
}

fn parse_provider(
    app_id: &str,
    id: &str,
    item: &Value,
    is_current: bool,
) -> Option<CcswitchProvider> {
    let object = item.as_object()?;
    let settings = object.get("settingsConfig").cloned().unwrap_or_default();
    let env = settings.get("env").cloned().unwrap_or_default();
    let package = str_field(&settings, "npm")
        .or_else(|| str_field(&settings, "package"))
        .map(|package| {
            package
                .strip_prefix("aisdk:")
                .unwrap_or(&package)
                .to_string()
        });
    let interface_type = if app_id == "claude"
        || (app_id == "opencode" && package.as_deref() == Some("@ai-sdk/anthropic"))
    {
        InterfaceType::AnthropicMessages
    } else if app_id == "opencode" && package.as_deref() == Some("@ai-sdk/google") {
        InterfaceType::Gemini
    } else {
        InterfaceType::OpenAiCompatible
    };
    let (key, base_url, model) = match app_id {
        "claude" => (
            str_field(&env, "ANTHROPIC_AUTH_TOKEN")
                .or_else(|| str_field(&env, "ANTHROPIC_API_KEY")),
            str_field(&env, "ANTHROPIC_BASE_URL"),
            str_field(&env, "ANTHROPIC_MODEL"),
        ),
        "codex" => {
            let key = settings
                .get("auth")
                .and_then(|auth| str_field(auth, "OPENAI_API_KEY"));
            let (base_url, model) = settings
                .get("config")
                .and_then(Value::as_str)
                .map(parse_codex_config)
                .unwrap_or_default();
            (key, base_url, model)
        }
        "gemini" => (
            str_field(&env, "GEMINI_API_KEY"),
            str_field(&env, "GOOGLE_GEMINI_BASE_URL"),
            str_field(&env, "GEMINI_MODEL"),
        ),
        "opencode" => {
            let options = settings
                .get("options")
                .or_else(|| settings.get("settings"))?;
            // SDK adapters define the wire protocol; do not silently import an
            // unsupported adapter as an OpenAI-compatible endpoint.
            if !matches!(
                package.as_deref(),
                Some(
                    "@ai-sdk/openai"
                        | "@ai-sdk/openai-compatible"
                        | "@ai-sdk/anthropic"
                        | "@ai-sdk/google"
                )
            ) {
                return None;
            }
            let key = str_field(options, "apiKey")
                .filter(|key| !key.starts_with("{env:") && !key.starts_with("{file:"));
            let base_url = str_field(options, "baseURL").or_else(|| match interface_type {
                InterfaceType::AnthropicMessages => Some(default_endpoint("claude").to_string()),
                InterfaceType::Gemini => Some(default_endpoint("gemini").to_string()),
                _ => None,
            });
            (key, base_url, None)
        }
        _ => (None, None, None),
    };
    Some(CcswitchProvider {
        app_id: app_id.to_string(),
        name: str_field(item, "name").unwrap_or_else(|| id.to_string()),
        key,
        base_url,
        model,
        website_host: str_field(item, "websiteUrl").and_then(|url| url_host(&url)),
        category: str_field(item, "category"),
        notes: str_field(item, "notes"),
        is_current,
        interface_type,
    })
}

/// Extract `(base_url, model)` from the codex `config.toml` text CC Switch
/// stores as a string. Unparseable TOML simply yields no values.
fn parse_codex_config(config: &str) -> (Option<String>, Option<String>) {
    // `str::parse::<toml::Value>` parses a single TOML *value*, not a whole
    // document, so go through `toml::from_str` instead.
    let Ok(value) = toml::from_str::<toml::Value>(config) else {
        return (None, None);
    };
    let model = value
        .get("model")
        .and_then(|model| model.as_str())
        .map(str::to_string);
    let base_url = value
        .get("model_providers")
        .and_then(|providers| providers.as_table())
        .and_then(|providers| {
            providers
                .values()
                .find_map(|provider| provider.get("base_url").and_then(|url| url.as_str()))
        })
        .map(str::to_string);
    (base_url, model)
}

fn str_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn url_host(url: &str) -> Option<String> {
    let rest = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .map(str::trim)
        .filter(|host| !host.is_empty())?;
    Some(host.to_string())
}

fn default_endpoint(app_id: &str) -> &'static str {
    match app_id {
        "claude" => "https://api.anthropic.com",
        "gemini" => "https://generativelanguage.googleapis.com",
        _ => "https://api.openai.com/v1",
    }
}

fn normalize_endpoint(url: &str) -> &str {
    url.trim_end_matches('/')
}

/// Imported base URLs become proxy upstreams bearing credentials, so reject
/// anything that is not a plain http(s) URL with a real host and no userinfo.
/// Plain `http://` stays allowed: local relays legitimately use it.
fn validate_base_url(url: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| format!("invalid base URL: {url}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!("base URL must use http(s): {url}"));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(format!("base URL must not contain userinfo: {url}"));
    }
    if parsed.host_str().is_none_or(str::is_empty) {
        return Err(format!("base URL must have a host: {url}"));
    }
    Ok(())
}

/// Import discovered providers into the vault, deduping against existing
/// entries and within the batch. A failure on one provider is reported in its
/// result and never aborts the remaining providers.
pub(crate) fn import_ccswitch_providers(
    vault: &Vault,
) -> anyhow::Result<Vec<OfficialAccountRefreshResult>> {
    import_providers(vault, discover_ccswitch_providers())
}

fn import_providers(
    vault: &Vault,
    providers: Vec<CcswitchProvider>,
) -> anyhow::Result<Vec<OfficialAccountRefreshResult>> {
    // Archived entries still belong to the user and must dedupe/refresh in
    // place; only trashed entries are forgotten. Both listing variants
    // already skip trash.
    let mut existing = vault.list_provider_summaries()?;
    existing.extend(vault.list_archived_provider_summaries()?);
    let mut seen_in_batch = HashSet::new();
    let mut results = Vec::new();
    for provider in providers {
        match import_provider(vault, &mut existing, &mut seen_in_batch, &provider) {
            Ok(result) => results.push(result),
            Err(error) => results.push(import_result(&provider, "error", Some(error.to_string()))),
        }
    }
    Ok(results)
}

fn import_provider(
    vault: &Vault,
    existing: &mut Vec<aipass_vault::EntrySummary>,
    seen_in_batch: &mut HashSet<(String, String)>,
    provider: &CcswitchProvider,
) -> anyhow::Result<OfficialAccountRefreshResult> {
    let Some(key) = provider.key.clone().filter(|key| !key.trim().is_empty()) else {
        return Ok(import_result(provider, "skipped", None));
    };
    let fingerprint = vault.fingerprint_secret(&key);
    let base_url = provider
        .base_url
        .clone()
        .unwrap_or_else(|| default_endpoint(&provider.app_id).to_string());
    if let Err(error) = validate_base_url(&base_url) {
        return Ok(import_result(provider, "error", Some(error)));
    }
    // Exact duplicates inside one import batch are skipped rather than
    // refreshed twice.
    if !seen_in_batch.insert((
        normalize_endpoint(&base_url).to_string(),
        fingerprint.clone(),
    )) {
        return Ok(import_result(provider, "skipped", None));
    }

    // The same key stored anywhere in the vault identifies the provider,
    // regardless of how the entry was originally created.
    if let Some(id) = existing
        .iter()
        .find(|entry| entry.fingerprint == fingerprint)
        .map(|entry| entry.id)
    {
        refresh_account_secret(vault, id, &key)?;
        return Ok(import_result(provider, "refreshed", None));
    }

    // Otherwise a matching endpoint plus title points at the same provider
    // configured under a rotated key.
    let endpoint_match = existing
        .iter()
        .find(|entry| {
            entry.title == provider.name
                && endpoint_url(&entry.endpoints)
                    .map(|url| normalize_endpoint(&url) == normalize_endpoint(&base_url))
                    .unwrap_or(false)
        })
        .map(|entry| entry.id);
    if let Some(id) = endpoint_match {
        refresh_account_secret(vault, id, &key)?;
        // The secret rotated, so refresh the cached summary for later dedupe.
        if let Ok(summary) = vault.get_provider_summary(id) {
            if let Some(slot) = existing.iter_mut().find(|entry| entry.id == id) {
                *slot = summary;
            }
        }
        return Ok(import_result(provider, "refreshed", None));
    }

    let new_id = vault.add_provider(new_entry_input(provider, &key, &base_url))?;
    // Make the freshly imported entry visible to later providers in this
    // batch so a duplicate matches instead of importing twice.
    if let Ok(summary) = vault.get_provider_summary(new_id) {
        existing.push(summary);
    }
    Ok(import_result(provider, "imported", None))
}

fn new_entry_input(provider: &CcswitchProvider, key: &str, base_url: &str) -> ProviderEntryInput {
    let provider_kind = match provider.category.as_deref() {
        Some("official") | Some("cn_official") => ProviderKind::Official,
        _ => ProviderKind::ThirdParty,
    };
    ProviderEntryInput {
        max_concurrent_requests: None,
        supports_websockets: None,
        title: provider.name.clone(),
        provider_kind,
        provider_id: None,
        credential_kind: CredentialKind::Api,
        account_identity: Some(provider.name.clone()),
        domains: provider.website_host.clone().into_iter().collect(),
        favicon_url: None,
        endpoints: vec![ProviderEndpoint::api(base_url)],
        interface_type: provider.interface_type.clone(),
        auth_scheme: if provider.interface_type == InterfaceType::Gemini {
            AuthScheme::GoogleApiKey
        } else {
            AuthScheme::Bearer
        },
        api_key: key.to_string(),
        secret_label: None,
        default_model: provider.model.clone(),
        model_aliases: Vec::new(),
        headers: Vec::new(),
        quota: None,
        subscription: None,
        gateway: None,
        tags: vec!["ccswitch".to_string()],
        notes: Some(
            provider
                .notes
                .clone()
                .unwrap_or_else(|| "Imported from CC Switch".to_string()),
        ),
        secret_metadata: Default::default(),
    }
}

fn import_result(
    provider: &CcswitchProvider,
    status: &str,
    error: Option<String>,
) -> OfficialAccountRefreshResult {
    OfficialAccountRefreshResult {
        provider_id: provider.app_id.clone(),
        account_identity: Some(provider.name.clone()),
        credential_kind: CredentialKind::Api,
        snapshot: None,
        status: status.to_string(),
        error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aipass_crypto::SecretString;

    fn test_vault(temp: &tempfile::TempDir) -> Vault {
        Vault::create(
            temp.path(),
            &SecretString::new("correct horse battery staple"),
        )
        .expect("create vault")
        .vault
    }

    fn write_fixture(dir: &Path) -> PathBuf {
        let path = dir.join("config.json");
        let fixture = serde_json::json!({
            "providers": {
                "claude": {
                    "current": "a1",
                    "providers": {
                        "a1": {
                            "id": "a1",
                            "name": "Claude Relay",
                            "category": "aggregator",
                            "websiteUrl": "https://relay.example.com/pricing",
                            "notes": "team relay",
                            "settingsConfig": {
                                "env": {
                                    "ANTHROPIC_AUTH_TOKEN": "sk-ant-relay",
                                    "ANTHROPIC_BASE_URL": "https://relay.example.com/api",
                                    "ANTHROPIC_MODEL": "claude-opus-4-1"
                                }
                            }
                        },
                        "a2": {
                            "id": "a2",
                            "name": "Keyless",
                            "settingsConfig": {"env": {}}
                        },
                        "a3": "not-an-object"
                    }
                },
                "codex": {
                    "current": "c1",
                    "providers": {
                        "c1": {
                            "id": "c1",
                            "name": "OpenAI Direct",
                            "category": "official",
                            "settingsConfig": {
                                "auth": {"OPENAI_API_KEY": "sk-openai-direct"},
                                "config": "model = \"gpt-5.2-codex\"\n\n[model_providers.openai]\nbase_url = \"https://api.openai.com/v1\"\n"
                            }
                        }
                    }
                },
                "gemini": {
                    "providers": {
                        "g1": {
                            "id": "g1",
                            "name": "Gemini",
                            "settingsConfig": {
                                "env": {
                                    "GEMINI_API_KEY": "gm-key",
                                    "GOOGLE_GEMINI_BASE_URL": "https://gemini.example.com",
                                    "GEMINI_MODEL": "gemini-3-pro"
                                }
                            }
                        }
                    }
                },
                "opencode": {
                    "providers": {
                        "o1": {"id": "o1", "name": "Ignored", "settingsConfig": {"env": {}}}
                    }
                }
            }
        });
        std::fs::write(&path, serde_json::to_vec(&fixture).expect("serialize")).expect("write");
        path
    }

    #[test]
    fn imports_opencode_sdk_settings_and_options_without_resolving_references() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("cc-switch.db");
        let db = rusqlite::Connection::open(&path).expect("database");
        db.execute_batch("CREATE TABLE providers (id TEXT, app_type TEXT, name TEXT, settings_config TEXT, website_url TEXT, category TEXT, notes TEXT, is_current INTEGER);").expect("schema");
        for (id, settings) in [
            (
                "legacy",
                serde_json::json!({"package":"@ai-sdk/openai-compatible", "settings":{"apiKey":"test-legacy", "baseURL":"https://legacy.example/v1"}}),
            ),
            (
                "modern",
                serde_json::json!({"npm":"aisdk:@ai-sdk/anthropic", "options":{"apiKey":"test-modern"}}),
            ),
            (
                "google",
                serde_json::json!({"package":"aisdk:@ai-sdk/google", "settings":{"apiKey":"test-google"}}),
            ),
            (
                "reference",
                serde_json::json!({"npm":"@ai-sdk/openai", "options":{"apiKey":"{env:OPENAI_API_KEY}"}}),
            ),
            (
                "unsupported",
                serde_json::json!({"npm":"unknown-sdk", "options":{"apiKey":"test-unsupported"}}),
            ),
        ] {
            db.execute(
                "INSERT INTO providers VALUES (?1, 'opencode', ?1, ?2, NULL, NULL, NULL, 0)",
                rusqlite::params![id, settings.to_string()],
            )
            .expect("insert");
        }
        let providers = discover_ccswitch_providers_at(&path);
        assert_eq!(providers.len(), 4);
        let vault_dir = tempfile::tempdir().expect("vault dir");
        let vault = test_vault(&vault_dir);
        let results = import_providers(&vault, providers).expect("import");
        assert_eq!(
            results
                .iter()
                .filter(|result| result.status == "imported")
                .count(),
            3
        );
        assert_eq!(
            results
                .iter()
                .filter(|result| result.status == "skipped")
                .count(),
            1
        );
        let entries = vault.list_provider_summaries().expect("entries");
        let google = entries
            .iter()
            .find(|entry| entry.title == "google")
            .expect("google");
        assert_eq!(google.interface_type, InterfaceType::Gemini);
        assert_eq!(google.auth_scheme, AuthScheme::GoogleApiKey);
        let modern = entries
            .iter()
            .find(|entry| entry.title == "modern")
            .expect("anthropic");
        assert_eq!(modern.interface_type, InterfaceType::AnthropicMessages);
        assert_eq!(
            endpoint_url(&modern.endpoints),
            Some("https://api.anthropic.com".to_string())
        );
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.title == "legacy")
                .expect("openai")
                .interface_type,
            InterfaceType::OpenAiCompatible
        );
    }

    #[test]
    fn parses_supported_apps_and_tolerates_bad_entries() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = write_fixture(temp.path());
        let providers = discover_ccswitch_providers_at(&path);

        // The malformed "a3" entry and OpenCode config without an adapter are
        // dropped; the keyless provider is kept for a "skipped" result.
        assert_eq!(providers.len(), 4);

        let claude = providers
            .iter()
            .find(|provider| provider.name == "Claude Relay")
            .expect("claude provider");
        assert_eq!(claude.app_id, "claude");
        assert!(claude.is_current);
        assert_eq!(claude.key.as_deref(), Some("sk-ant-relay"));
        assert_eq!(
            claude.base_url.as_deref(),
            Some("https://relay.example.com/api")
        );
        assert_eq!(claude.model.as_deref(), Some("claude-opus-4-1"));
        assert_eq!(claude.website_host.as_deref(), Some("relay.example.com"));
        assert_eq!(claude.category.as_deref(), Some("aggregator"));
        assert_eq!(claude.notes.as_deref(), Some("team relay"));

        let codex = providers
            .iter()
            .find(|provider| provider.app_id == "codex")
            .expect("codex provider");
        assert_eq!(codex.key.as_deref(), Some("sk-openai-direct"));
        assert_eq!(codex.base_url.as_deref(), Some("https://api.openai.com/v1"));
        assert_eq!(codex.model.as_deref(), Some("gpt-5.2-codex"));

        let keyless = providers
            .iter()
            .find(|provider| provider.name == "Keyless")
            .expect("keyless provider");
        assert_eq!(keyless.key, None);

        let gemini = providers
            .iter()
            .find(|provider| provider.app_id == "gemini")
            .expect("gemini provider");
        assert_eq!(gemini.key.as_deref(), Some("gm-key"));
        assert_eq!(
            gemini.base_url.as_deref(),
            Some("https://gemini.example.com")
        );
        assert_eq!(gemini.model.as_deref(), Some("gemini-3-pro"));
    }

    #[test]
    fn prefers_database_over_legacy_config() {
        let temp = tempfile::tempdir().expect("tempdir");
        let legacy = temp.path().join("config.json");
        assert_eq!(config_path_at(temp.path()), legacy);
        std::fs::write(&legacy, b"{}").expect("legacy");
        assert_eq!(config_path_at(temp.path()), legacy);
        let database = temp.path().join("cc-switch.db");
        std::fs::write(&database, b"").expect("database");
        assert_eq!(config_path_at(temp.path()), database);
    }

    #[test]
    fn sqlite_providers_import_without_modifying_source() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("cc-switch.db");
        let db = rusqlite::Connection::open(&path).expect("fixture db");
        db.execute_batch(
            "CREATE TABLE providers (id TEXT, app_type TEXT, name TEXT,
             settings_config TEXT, website_url TEXT, category TEXT, notes TEXT,
             is_current INTEGER);
             INSERT INTO providers VALUES ('bad', 'claude', 'Bad', '{', NULL, NULL, NULL, 0);
             INSERT INTO providers VALUES ('ignored', 'opencode', 'Ignored', '{}', NULL, NULL, NULL, 0);",
        ).expect("schema");
        let settings = serde_json::json!({"env": {
            "ANTHROPIC_AUTH_TOKEN": "fixture-key",
            "ANTHROPIC_BASE_URL": "https://relay.example.com",
            "ANTHROPIC_MODEL": "fixture-model"
        }});
        db.execute(
            "INSERT INTO providers VALUES ('active', 'claude', 'Relay', ?1,
             'https://relay.example.com/docs', 'aggregator', 'fixture notes', 1)",
            [settings.to_string()],
        )
        .expect("provider");
        drop(db);
        let before = std::fs::read(&path).expect("source bytes");
        let providers = discover_ccswitch_providers_at(&path);
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].name, "Relay");
        assert!(providers[0].is_current);
        assert_eq!(providers[0].model.as_deref(), Some("fixture-model"));
        assert_eq!(
            providers[0].website_host.as_deref(),
            Some("relay.example.com")
        );
        assert_eq!(providers[0].notes.as_deref(), Some("fixture notes"));
        let vault_dir = tempfile::tempdir().expect("vault dir");
        let vault = test_vault(&vault_dir);
        let results = import_providers(&vault, providers).expect("import");
        assert_eq!(results[0].status, "imported");
        let entries = vault.list_provider_summaries().expect("summaries");
        assert_eq!(
            entries[0].fingerprint,
            vault.fingerprint_secret("fixture-key")
        );
        assert_eq!(before, std::fs::read(&path).expect("unchanged source"));
    }

    #[test]
    fn malformed_config_yields_no_providers() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("config.json");
        std::fs::write(&path, b"{ not json").expect("write");
        assert!(discover_ccswitch_providers_at(&path).is_empty());
        assert!(discover_ccswitch_providers_at(&temp.path().join("missing.json")).is_empty());
    }

    fn provider(
        app_id: &str,
        name: &str,
        key: Option<&str>,
        base_url: Option<&str>,
    ) -> CcswitchProvider {
        CcswitchProvider {
            app_id: app_id.to_string(),
            name: name.to_string(),
            key: key.map(str::to_string),
            base_url: base_url.map(str::to_string),
            model: None,
            website_host: None,
            category: None,
            notes: None,
            is_current: false,
            interface_type: if app_id == "claude" {
                InterfaceType::AnthropicMessages
            } else {
                InterfaceType::OpenAiCompatible
            },
        }
    }

    fn manual_entry(vault: &Vault, title: &str, key: &str, endpoint: &str) {
        vault
            .add_provider(ProviderEntryInput {
                max_concurrent_requests: None,
                supports_websockets: None,
                title: title.to_string(),
                provider_kind: ProviderKind::ThirdParty,
                provider_id: None,
                credential_kind: CredentialKind::Api,
                account_identity: None,
                domains: Vec::new(),
                favicon_url: None,
                endpoints: vec![ProviderEndpoint::api(endpoint)],
                interface_type: InterfaceType::AnthropicMessages,
                auth_scheme: AuthScheme::Bearer,
                api_key: key.to_string(),
                secret_label: None,
                default_model: None,
                model_aliases: Vec::new(),
                headers: Vec::new(),
                quota: None,
                subscription: None,
                gateway: None,
                tags: Vec::new(),
                notes: None,
                secret_metadata: Default::default(),
            })
            .expect("add manual entry");
    }

    #[test]
    fn key_already_in_vault_is_refreshed_not_duplicated() {
        let temp = tempfile::tempdir().expect("tempdir");
        let vault = test_vault(&temp);
        manual_entry(
            &vault,
            "Manual",
            "sk-ant-relay",
            "https://relay.example.com/api",
        );

        let results = import_providers(
            &vault,
            vec![provider(
                "claude",
                "Claude Relay",
                Some("sk-ant-relay"),
                Some("https://relay.example.com/api"),
            )],
        )
        .expect("import");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, "refreshed");
        assert_eq!(results[0].provider_id, "claude");
        assert_eq!(results[0].account_identity.as_deref(), Some("Claude Relay"));
        assert_eq!(results[0].credential_kind, CredentialKind::Api);
        assert!(results[0].snapshot.is_none());
        assert_eq!(vault.list_provider_summaries().expect("summaries").len(), 1);
    }

    #[test]
    fn same_endpoint_and_key_twice_in_a_batch_is_skipped() {
        let temp = tempfile::tempdir().expect("tempdir");
        let vault = test_vault(&temp);

        let results = import_providers(
            &vault,
            vec![
                provider(
                    "claude",
                    "Relay One",
                    Some("sk-ant-relay"),
                    Some("https://relay.example.com/api"),
                ),
                provider(
                    "claude",
                    "Relay Two",
                    Some("sk-ant-relay"),
                    Some("https://relay.example.com/api/"),
                ),
            ],
        )
        .expect("import");
        assert_eq!(results[0].status, "imported");
        assert_eq!(results[1].status, "skipped");
        assert_eq!(vault.list_provider_summaries().expect("summaries").len(), 1);
    }

    #[test]
    fn new_provider_is_imported_with_expected_mapping() {
        let temp = tempfile::tempdir().expect("tempdir");
        let vault = test_vault(&temp);
        let mut incoming = provider(
            "claude",
            "Claude Relay",
            Some("sk-ant-relay"),
            Some("https://relay.example.com/api"),
        );
        incoming.model = Some("claude-opus-4-1".to_string());
        incoming.category = Some("aggregator".to_string());
        incoming.website_host = Some("relay.example.com".to_string());

        let results = import_providers(&vault, vec![incoming]).expect("import");
        assert_eq!(results[0].status, "imported");

        let entries = vault.list_provider_summaries().expect("summaries");
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.title, "Claude Relay");
        assert_eq!(entry.provider_kind, ProviderKind::ThirdParty);
        assert_eq!(entry.credential_kind, CredentialKind::Api);
        assert_eq!(entry.interface_type, InterfaceType::AnthropicMessages);
        assert_eq!(entry.auth_scheme, AuthScheme::Bearer);
        assert_eq!(
            endpoint_url(&entry.endpoints).as_deref(),
            Some("https://relay.example.com/api")
        );
        assert_eq!(entry.default_model.as_deref(), Some("claude-opus-4-1"));
        assert_eq!(entry.tags, vec!["ccswitch".to_string()]);
        assert_eq!(entry.domains, vec!["relay.example.com".to_string()]);
        assert_eq!(entry.fingerprint, vault.fingerprint_secret("sk-ant-relay"));
        assert_eq!(entry.notes.as_deref(), Some("Imported from CC Switch"));
    }

    #[test]
    fn official_category_maps_to_official_kind() {
        let temp = tempfile::tempdir().expect("tempdir");
        let vault = test_vault(&temp);
        let mut incoming = provider("codex", "OpenAI", Some("sk-openai"), None);
        incoming.category = Some("cn_official".to_string());

        let results = import_providers(&vault, vec![incoming]).expect("import");
        assert_eq!(results[0].status, "imported");
        let entries = vault.list_provider_summaries().expect("summaries");
        assert_eq!(entries[0].provider_kind, ProviderKind::Official);
        // No base_url in the config falls back to the app's official endpoint.
        assert_eq!(
            endpoint_url(&entries[0].endpoints).as_deref(),
            Some("https://api.openai.com/v1")
        );
        assert_eq!(entries[0].interface_type, InterfaceType::OpenAiCompatible);
    }

    #[test]
    fn keyless_provider_is_reported_as_skipped() {
        let temp = tempfile::tempdir().expect("tempdir");
        let vault = test_vault(&temp);

        let results = import_providers(&vault, vec![provider("gemini", "Gemini", None, None)])
            .expect("import");
        assert_eq!(results[0].status, "skipped");
        assert!(results[0].error.is_none());
        assert!(vault
            .list_provider_summaries()
            .expect("summaries")
            .is_empty());
    }

    #[test]
    fn invalid_base_urls_are_rejected_but_http_is_allowed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let vault = test_vault(&temp);

        for (name, base_url) in [
            ("FTP", "ftp://relay.example.com/api"),
            ("Userinfo", "https://user:pass@relay.example.com/api"),
        ] {
            let results = import_providers(
                &vault,
                vec![provider("claude", name, Some("sk-key"), Some(base_url))],
            )
            .expect("import");
            assert_eq!(results[0].status, "error");
            assert!(results[0].error.is_some());
        }
        assert!(vault
            .list_provider_summaries()
            .expect("summaries")
            .is_empty());

        // Local relays legitimately use plain http.
        let results = import_providers(
            &vault,
            vec![provider(
                "claude",
                "Local Relay",
                Some("sk-key"),
                Some("http://127.0.0.1:8317/api"),
            )],
        )
        .expect("import");
        assert_eq!(results[0].status, "imported");
        let entries = vault.list_provider_summaries().expect("summaries");
        assert_eq!(entries.len(), 1);
        assert_eq!(
            endpoint_url(&entries[0].endpoints).as_deref(),
            Some("http://127.0.0.1:8317/api")
        );
    }

    #[test]
    fn archived_entry_is_refreshed_in_place_without_unarchiving() {
        let temp = tempfile::tempdir().expect("tempdir");
        let vault = test_vault(&temp);
        manual_entry(
            &vault,
            "Manual",
            "sk-ant-relay",
            "https://relay.example.com/api",
        );
        let id = vault.list_provider_summaries().expect("summaries")[0].id;
        vault.archive_provider(id).expect("archive");

        let results = import_providers(
            &vault,
            vec![provider(
                "claude",
                "Claude Relay",
                Some("sk-ant-relay"),
                Some("https://relay.example.com/api"),
            )],
        )
        .expect("import");
        assert_eq!(results[0].status, "refreshed");
        assert!(vault.list_provider_summaries().expect("active").is_empty());
        assert_eq!(
            vault
                .list_archived_provider_summaries()
                .expect("archived")
                .len(),
            1
        );
    }
}
