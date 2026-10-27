//! Conservative three-way undo. Only values changed by our writer are owned.
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::path::Path;
use zeroize::Zeroizing;

fn undo_value(
    before: Option<&Value>,
    after: Option<&Value>,
    current: Option<&Value>,
) -> Result<Option<Value>> {
    if before == after || current == before {
        return Ok(current.cloned());
    }
    if current == after {
        return Ok(before.cloned());
    }
    // Descend into objects even if we created the object: preserve unrelated
    // keys added since installation. Arrays are intentionally indivisible.
    if let Some(Value::Object(now)) = current {
        if before.is_none_or(Value::is_object) && after.is_none_or(Value::is_object) {
            let mut result = now.clone();
            let old = before.and_then(Value::as_object);
            let new = after.and_then(Value::as_object);
            let mut keys = std::collections::BTreeSet::new();
            keys.extend(old.into_iter().flat_map(|m| m.keys()));
            keys.extend(new.into_iter().flat_map(|m| m.keys()));
            for key in keys {
                let value = undo_value(
                    old.and_then(|m| m.get(key)),
                    new.and_then(|m| m.get(key)),
                    now.get(key),
                )?;
                match value {
                    Some(value) => {
                        result.insert(key.clone(), value);
                    }
                    None => {
                        result.remove(key);
                    }
                }
            }
            return Ok(if before.is_none() && result.is_empty() {
                None
            } else {
                Some(Value::Object(result))
            });
        }
    }
    bail!("proxy configuration conflict; no configuration was overwritten")
}

fn parse_json(bytes: Option<&[u8]>) -> Result<Option<Value>> {
    bytes
        .map(|b| {
            serde_json::from_slice(b).context(
                "commented or invalid JSON requires exact recovery; configuration was preserved",
            )
        })
        .transpose()
}

pub(super) fn undo(
    path: Option<&Path>,
    before: Option<&[u8]>,
    after: Option<&[u8]>,
    current: Option<&[u8]>,
) -> Result<Option<Zeroizing<Vec<u8>>>> {
    if before == after || current == before {
        return Ok(current.map(|v| Zeroizing::new(v.to_vec())));
    }
    if current == after {
        return Ok(before.map(|v| Zeroizing::new(v.to_vec())));
    }
    let extension = path
        .and_then(Path::extension)
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if matches!(extension, "json" | "jsonc" | "json5") {
        let old = crate::native_auth::PrivateJson(parse_json(before)?.unwrap_or(Value::Null));
        let new = crate::native_auth::PrivateJson(parse_json(after)?.unwrap_or(Value::Null));
        let now = crate::native_auth::PrivateJson(parse_json(current)?.unwrap_or(Value::Null));
        return undo_value(
            before.map(|_| &old.0),
            after.map(|_| &new.0),
            current.map(|_| &now.0),
        )?
        .map(|v| {
            let v = crate::native_auth::PrivateJson(v);
            Ok(Zeroizing::new(serde_json::to_vec_pretty(&v.0)?))
        })
        .transpose();
    }
    if extension == "toml" {
        fn doc(bytes: Option<&[u8]>) -> Result<toml_edit::DocumentMut> {
            Ok(std::str::from_utf8(bytes.unwrap_or(b""))?.parse()?)
        }
        let old = doc(before)?;
        let new = doc(after)?;
        let mut now = doc(current)?;
        undo_table(old.as_table(), new.as_table(), now.as_table_mut())?;
        return Ok(Some(Zeroizing::new(now.to_string().into_bytes())));
    }
    // Opaque launchers, credential stores and unsupported formats require an
    // exact pre/postimage. Never guess how to merge shell code or credentials.
    bail!("proxy configuration conflict; original recovery data retained")
}

fn undo_table(
    before: &toml_edit::Table,
    after: &toml_edit::Table,
    current: &mut toml_edit::Table,
) -> Result<()> {
    use toml_edit::{Item, Table};
    let keys: std::collections::BTreeSet<_> =
        before.iter().chain(after.iter()).map(|(k, _)| k).collect();
    for key in keys {
        let old = before.get(key);
        let new = after.get(key);
        let now = current.get(key);
        // Comparison via an isolated table includes each item's TOML meaning
        // and decorations. A formatting-only edit is conservatively a conflict.
        fn same(a: Option<&Item>, b: Option<&Item>) -> bool {
            match (a.and_then(Item::as_table), b.and_then(Item::as_table)) {
                (Some(a), Some(b)) => {
                    a.len() == b.len() && a.iter().all(|(key, value)| same(Some(value), b.get(key)))
                }
                (Some(_), None) | (None, Some(_)) => false,
                _ => a.map(ToString::to_string) == b.map(ToString::to_string),
            }
        }
        if same(old, new) || same(now, old) {
            continue;
        }
        if same(now, new) {
            if let Some(item) = old {
                current.insert(key, item.clone());
            } else {
                current.remove(key);
            }
        } else if new.is_some_and(Item::is_table)
            && old.is_none_or(Item::is_table)
            && now.is_some_and(Item::is_table)
        {
            let empty = Table::new();
            undo_table(
                old.and_then(Item::as_table).unwrap_or(&empty),
                new.and_then(Item::as_table).unwrap(),
                current.get_mut(key).and_then(Item::as_table_mut).unwrap(),
            )?;
            if old.is_none() && current[key].as_table().is_some_and(Table::is_empty) {
                current.remove(key);
            }
        } else {
            bail!("proxy configuration conflict; original recovery data retained");
        }
    }
    Ok(())
}
