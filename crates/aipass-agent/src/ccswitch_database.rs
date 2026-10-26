//! Read-only adapter for CC Switch's SQLite provider store.

use super::{parse_provider, CcswitchProvider};
use serde_json::Value;
use std::path::Path;

pub(super) fn discover(path: &Path) -> rusqlite::Result<Vec<CcswitchProvider>> {
    let db =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.busy_timeout(std::time::Duration::from_secs(2))?;
    let mut statement = db.prepare(
        "SELECT id, app_type, name, settings_config, website_url, category, notes, is_current
         FROM providers WHERE app_type IN ('claude', 'codex', 'gemini', 'opencode')
         ORDER BY is_current DESC, app_type, id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            serde_json::json!({
                "name": row.get::<_, String>(2)?,
                "settingsConfig": serde_json::from_str::<Value>(&row.get::<_, String>(3)?)
                    .unwrap_or(Value::Null),
                "websiteUrl": row.get::<_, Option<String>>(4)?,
                "category": row.get::<_, Option<String>>(5)?,
                "notes": row.get::<_, Option<String>>(6)?,
            }),
            row.get::<_, bool>(7)?,
        ))
    })?;
    let mut providers = Vec::new();
    for row in rows {
        let Ok((id, app, item, current)) = row else {
            continue;
        };
        if !item["settingsConfig"].is_object() {
            continue;
        }
        if let Some(provider) = parse_provider(&app, &id, &item, current) {
            providers.push(provider);
        }
    }
    Ok(providers)
}
