use crate::models::ConfigPlan;
use aipass_provider_registry::ProviderEndpoint;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;
use toml_edit::{DocumentMut, Item, Table};
use uuid::Uuid;

pub fn endpoint_url(endpoints: &[ProviderEndpoint]) -> Option<String> {
    endpoints.iter().find_map(|endpoint| endpoint.url.clone())
}

pub(crate) fn new_plan(
    tool: crate::models::ToolId,
    target_path: PathBuf,
    summary: String,
    preview: String,
) -> ConfigPlan {
    let operation_id = Uuid::new_v4();
    let backup_path = target_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(".aipass-backups")
        .join(format!(
            "{}-{}.aipbackup",
            operation_id,
            OffsetDateTime::now_utc().unix_timestamp()
        ));
    ConfigPlan {
        operation_id,
        tool,
        target_path,
        backup_path,
        summary,
        preview,
    }
}

pub(crate) fn write_json(path: impl AsRef<Path>, value: &impl Serialize) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
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
    Ok(read_json_value(path)?
        .as_object()
        .cloned()
        .unwrap_or_default())
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
        *value = Value::Object(Map::new());
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
    content
        .lines()
        .map(|line| format!("+ {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn redacted_diff_preview(content: &str, redactions: &[&str]) -> String {
    let mut preview = diff_preview(content);
    for value in redactions {
        if !value.is_empty() {
            preview = preview.replace(value, "[redacted]");
        }
    }
    preview
}

pub(crate) fn dotenv_quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("\"{escaped}\"")
}
