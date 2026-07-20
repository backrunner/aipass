use crate::models::ConfigPlan;
use aipass_provider_registry::ProviderEndpoint;
use aipass_storage::atomic_write_bytes;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, Table};
use uuid::Uuid;

pub fn endpoint_url(endpoints: &[ProviderEndpoint]) -> Option<String> {
    endpoints.iter().find_map(|endpoint| endpoint.url.clone())
}

pub(crate) fn resolve_codex_dir(home: &Path) -> PathBuf {
    if let Some(dir) = std::env::var_os("CODEX_HOME") {
        let path = PathBuf::from(dir);
        if !path.as_os_str().is_empty()
            && !path.to_string_lossy().trim().is_empty()
            && path.is_dir()
        {
            return path;
        }
    }
    home.join(".codex")
}

pub(crate) fn new_plan(
    tool: crate::models::ToolId,
    target_path: PathBuf,
    summary: String,
    preview: String,
) -> ConfigPlan {
    let operation_id = Uuid::new_v4();
    let backup_path = config_backup_path(&target_path);
    ConfigPlan {
        operation_id,
        tool,
        target_path,
        backup_path,
        summary,
        preview,
        extra_writes: Vec::new(),
        codex_provider_migration: None,
    }
}

pub fn config_backup_path(target_path: &Path) -> PathBuf {
    let file_name = target_path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("config");
    target_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(".aipass-backups")
        .join(format!("{file_name}.aipbackup"))
}

pub(crate) fn write_json(path: impl AsRef<Path>, value: &impl Serialize) -> Result<()> {
    atomic_write_bytes(path, &serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

pub(crate) fn read_json<T: for<'de> Deserialize<'de>>(path: impl AsRef<Path>) -> Result<T> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

pub(crate) fn backup_aad(operation_id: Uuid, target_path: &Path) -> String {
    format!(
        "aipass-config-backup;operation={operation_id};target={}",
        target_path.display()
    )
}

pub(crate) fn read_toml(path: &Path) -> Result<DocumentMut> {
    if path.exists() {
        Ok(fs::read_to_string(path)?.parse::<DocumentMut>()?)
    } else {
        Ok(DocumentMut::new())
    }
}

pub(crate) fn ensure_table<'a>(doc: &'a mut DocumentMut, key: &str) -> Result<&'a mut Table> {
    if !doc.contains_key(key) {
        doc[key] = Item::Table(Table::new());
    }
    doc[key]
        .as_table_mut()
        .with_context(|| format!("{key} is not a TOML table"))
}

pub(crate) fn read_json_object(path: &Path) -> Result<serde_json::Map<String, serde_json::Value>> {
    read_json_value(path)?
        .as_object()
        .cloned()
        .with_context(|| format!("{} must contain a JSON object", path.display()))
}

pub(crate) fn read_json_value(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(Value::Object(Map::new()));
    }
    let content = fs::read_to_string(path)?;
    Ok(json5::from_str(&content)?)
}

pub(crate) fn ensure_json_object<'a>(
    object: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>> {
    let value = object
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !value.is_object() {
        anyhow::bail!("{key} must be a JSON object");
    }
    value
        .as_object_mut()
        .with_context(|| format!("{key} is not a JSON object"))
}

pub(crate) fn slug(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

pub(crate) fn diff_preview(content: &str) -> String {
    diff_preview_from("", content)
}

pub fn diff_preview_for_path(path: &Path, content: &str) -> String {
    let before = fs::read_to_string(path).unwrap_or_default();
    diff_preview_from(&before, content)
}

/// Produce a small, line-oriented diff without exposing an entire file as a
/// replacement. This is intentionally dependency-free because plans are also
/// used by the native agent before any config is written.
pub(crate) fn diff_preview_from(before: &str, after: &str) -> String {
    if before == after {
        return "(no changes)".to_string();
    }

    let before_lines = before.lines().collect::<Vec<_>>();
    let after_lines = after.lines().collect::<Vec<_>>();
    let mut prefix = 0;
    while prefix < before_lines.len()
        && prefix < after_lines.len()
        && before_lines[prefix] == after_lines[prefix]
    {
        prefix += 1;
    }

    let mut before_end = before_lines.len();
    let mut after_end = after_lines.len();
    while before_end > prefix
        && after_end > prefix
        && before_lines[before_end - 1] == after_lines[after_end - 1]
    {
        before_end -= 1;
        after_end -= 1;
    }

    let mut lines = Vec::new();
    if prefix > 0 {
        lines.push(format!("  {}", before_lines[prefix - 1]));
    }
    lines.extend(
        before_lines[prefix..before_end]
            .iter()
            .map(|line| format!("- {line}")),
    );
    lines.extend(
        after_lines[prefix..after_end]
            .iter()
            .map(|line| format!("+ {line}")),
    );
    if before_end < before_lines.len() && after_end < after_lines.len() {
        lines.push(format!("  {}", after_lines[after_end]));
    }
    lines.join("\n")
}

pub fn redacted_diff_preview(content: &str, redactions: &[&str]) -> String {
    let mut preview = if content
        .lines()
        .any(|line| line.starts_with("+ ") || line.starts_with("- "))
    {
        content.to_string()
    } else {
        diff_preview(content)
    };
    for value in redactions {
        if !value.is_empty() {
            preview = preview.replace(value, "[redacted]");
        }
    }
    preview = preview
        .lines()
        .map(|line| {
            if (line.starts_with("- ") || line.starts_with("+ ") || line.starts_with("  "))
                && looks_like_secret_line(line)
            {
                format!("{}[redacted]", &line[..2])
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    preview
}

fn looks_like_secret_line(line: &str) -> bool {
    let body = line
        .get(2..)
        .unwrap_or(line)
        .trim()
        .strip_prefix("export ")
        .unwrap_or_else(|| line.get(2..).unwrap_or(line).trim());
    let key = body
        .split(['=', ':'])
        .next()
        .unwrap_or_default()
        .trim_matches(|character: char| {
            character.is_whitespace() || matches!(character, '"' | '\'' | '{' | ',')
        })
        .to_ascii_lowercase();
    key.contains("api_key")
        || key.contains("apikey")
        || key.contains("api-token")
        || key.contains("access_token")
        || key.contains("auth_token")
        || key.contains("bearer_token")
        || key.contains("secret")
}

pub(crate) fn dotenv_quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("\"{escaped}\"")
}
