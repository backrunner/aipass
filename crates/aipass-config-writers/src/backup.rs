use crate::models::{ApplyResult, ConfigPlan, EncryptedBackup, PlannedWrite};
use crate::utils::{backup_aad, read_json, resolve_codex_dir, write_json};
use aipass_crypto::{decrypt_bytes, encrypt_bytes, KEY_LEN};
use aipass_storage::atomic_write_bytes;
use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::collections::HashSet;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone, Debug)]
struct PreparedWrite {
    target_path: PathBuf,
    backup_path: PathBuf,
    content: Vec<u8>,
    original: Vec<u8>,
    target_existed: bool,
}

pub fn apply_plan(plan: &ConfigPlan, content: &str) -> Result<ApplyResult> {
    apply_plan_with_plain_backup(plan, content)
}

pub fn apply_plan_encrypted(
    plan: &ConfigPlan,
    content: &str,
    backup_key: &[u8; KEY_LEN],
) -> Result<ApplyResult> {
    let prepared = prepare_writes(plan, content)?;
    write_encrypted_backups(plan.operation_id, &prepared, backup_key)?;
    let sqlite_backups = prepare_codex_sqlite_backups(plan, Some(backup_key))?;
    if let Err(err) = apply_codex_sqlite_migration(plan) {
        let _ = restore_codex_sqlite_backups(&sqlite_backups, Some(backup_key));
        return Err(err);
    }
    if let Err(err) = apply_prepared_writes(plan.operation_id, &prepared) {
        let _ = restore_codex_sqlite_backups(&sqlite_backups, Some(backup_key));
        return Err(err);
    }
    Ok(apply_result(plan))
}

pub fn apply_plan_with_plain_backup(plan: &ConfigPlan, content: &str) -> Result<ApplyResult> {
    let prepared = prepare_writes(plan, content)?;
    write_plain_backups(&prepared)?;
    let sqlite_backups = prepare_codex_sqlite_backups(plan, None)?;
    if let Err(err) = apply_codex_sqlite_migration(plan) {
        let _ = restore_codex_sqlite_backups(&sqlite_backups, None);
        return Err(err);
    }
    if let Err(err) = apply_prepared_writes(plan.operation_id, &prepared) {
        let _ = restore_codex_sqlite_backups(&sqlite_backups, None);
        return Err(err);
    }
    Ok(apply_result(plan))
}

pub fn rollback(plan: &ConfigPlan) -> Result<()> {
    rollback_plain(plan)
}

pub fn rollback_encrypted(backup_path: &Path, backup_key: &[u8; KEY_LEN]) -> Result<ApplyResult> {
    let primary_backup: EncryptedBackup = read_json(backup_path)?;
    let backup_paths = backup_group_paths(backup_path, primary_backup.operation_id)?;
    for path in backup_paths {
        restore_encrypted_backup_file(&path, backup_key)?;
    }
    Ok(ApplyResult {
        operation_id: primary_backup.operation_id,
        target_path: primary_backup.target_path,
        backup_path: backup_path.to_path_buf(),
    })
}

pub fn find_backup_by_operation(home: &Path, operation_id: Uuid) -> Result<PathBuf> {
    let codex_dir = resolve_codex_dir(home);
    for root in [
        codex_dir.join(".aipass-backups"),
        home.join(".claude").join(".aipass-backups"),
        home.join(".gemini").join(".aipass-backups"),
        home.join(".config")
            .join("opencode")
            .join(".aipass-backups"),
        home.join(".aipass").join("tools").join(".aipass-backups"),
    ] {
        if !root.exists() {
            continue;
        }
        let mut paths = fs::read_dir(root)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        paths.sort();
        for path in paths {
            if backup_matches_operation(&path, operation_id) {
                return Ok(path);
            }
        }
    }
    Err(anyhow::anyhow!("backup operation {operation_id} not found"))
}

pub fn rollback_plain(plan: &ConfigPlan) -> Result<()> {
    restore_plain_backup_file(&plan.target_path, &plan.backup_path)?;
    for write in &plan.extra_writes {
        restore_plain_backup_file(&write.target_path, &write.backup_path)?;
    }
    restore_codex_sqlite_backups(&codex_sqlite_backup_paths(plan), None)?;
    Ok(())
}

fn prepare_writes(plan: &ConfigPlan, primary_content: &str) -> Result<Vec<PreparedWrite>> {
    collected_writes(plan, primary_content)
        .into_iter()
        .map(|write| {
            if let Some(parent) = write.target_path.parent() {
                fs::create_dir_all(parent)?;
            }
            if let Some(parent) = write.backup_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let target_existed = write.target_path.exists();
            let original = if target_existed {
                fs::read(&write.target_path)
                    .with_context(|| format!("backup {}", write.target_path.display()))?
            } else {
                Vec::new()
            };
            Ok(PreparedWrite {
                target_path: write.target_path,
                backup_path: write.backup_path,
                content: write.content.into_bytes(),
                original,
                target_existed,
            })
        })
        .collect()
}

fn collected_writes(plan: &ConfigPlan, primary_content: &str) -> Vec<PlannedWrite> {
    let mut writes = Vec::with_capacity(1 + plan.extra_writes.len());
    writes.push(PlannedWrite {
        target_path: plan.target_path.clone(),
        backup_path: plan.backup_path.clone(),
        content: primary_content.to_string(),
    });
    writes.extend(plan.extra_writes.iter().cloned());
    writes
}

fn write_encrypted_backups(
    operation_id: Uuid,
    writes: &[PreparedWrite],
    backup_key: &[u8; KEY_LEN],
) -> Result<()> {
    for write in writes {
        let aad = backup_aad(operation_id, &write.target_path);
        let backup = EncryptedBackup {
            format: "aipass-config-backup".to_string(),
            version: 1,
            operation_id,
            target_path: write.target_path.clone(),
            target_existed: write.target_existed,
            created_at: OffsetDateTime::now_utc(),
            ciphertext: encrypt_bytes(backup_key, aad.as_bytes(), &write.original)?,
        };
        write_json(&write.backup_path, &backup)?;
    }
    prune_replaced_encrypted_backups(writes)?;
    Ok(())
}

fn write_plain_backups(writes: &[PreparedWrite]) -> Result<()> {
    for write in writes {
        if write.target_existed {
            atomic_write_bytes(&write.backup_path, &write.original)
                .with_context(|| format!("backup {}", write.target_path.display()))?;
        } else {
            atomic_write_bytes(&write.backup_path, b"")?;
        }
    }
    prune_replaced_encrypted_backups(writes)?;
    Ok(())
}

fn apply_prepared_writes(operation_id: Uuid, writes: &[PreparedWrite]) -> Result<()> {
    let mut applied = Vec::new();
    for write in writes {
        if let Err(err) = atomic_write_bytes(&write.target_path, &write.content) {
            let _ = restore_applied_writes(&applied);
            return Err(err).with_context(|| {
                format!(
                    "apply config write for operation {operation_id} to {}",
                    write.target_path.display()
                )
            });
        }
        applied.push(write.clone());
    }
    Ok(())
}

fn restore_applied_writes(writes: &[PreparedWrite]) -> Result<()> {
    for write in writes.iter().rev() {
        restore_original_bytes(write)?;
    }
    Ok(())
}

fn restore_original_bytes(write: &PreparedWrite) -> Result<()> {
    if let Some(parent) = write.target_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if write.target_existed {
        atomic_write_bytes(&write.target_path, &write.original)?;
    } else if write.target_path.exists() {
        fs::remove_file(&write.target_path)?;
    }
    Ok(())
}

fn restore_encrypted_backup_file(backup_path: &Path, backup_key: &[u8; KEY_LEN]) -> Result<()> {
    let backup: EncryptedBackup = read_json(backup_path)?;
    let aad = backup_aad(backup.operation_id, &backup.target_path);
    let original = decrypt_bytes(backup_key, aad.as_bytes(), &backup.ciphertext)?;
    if let Some(parent) = backup.target_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if backup.target_existed {
        atomic_write_bytes(&backup.target_path, &original)?;
    } else if backup.target_path.exists() {
        fs::remove_file(&backup.target_path)?;
    }
    Ok(())
}

fn restore_plain_backup_file(target_path: &Path, backup_path: &Path) -> Result<()> {
    if !backup_path.exists() {
        return Ok(());
    }
    if fs::metadata(backup_path)?.len() == 0 {
        let _ = fs::remove_file(target_path);
    } else {
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let original = fs::read(backup_path)?;
        atomic_write_bytes(target_path, &original)?;
    }
    Ok(())
}

type SqliteBackup = (PathBuf, PathBuf);

fn codex_sqlite_database_paths(plan: &ConfigPlan) -> Vec<PathBuf> {
    let Some(codex_dir) = plan.target_path.parent() else {
        return Vec::new();
    };
    [
        codex_dir.join("state_5.sqlite"),
        codex_dir.join("sqlite").join("state_5.sqlite"),
        codex_dir.join("codex-dev.db"),
        codex_dir.join("sqlite").join("codex-dev.db"),
    ]
    .into_iter()
    .filter(|path| path.exists())
    .collect()
}

fn codex_sqlite_backup_paths(plan: &ConfigPlan) -> Vec<SqliteBackup> {
    let Some(codex_dir) = plan.target_path.parent() else {
        return Vec::new();
    };
    codex_sqlite_database_paths(plan)
        .into_iter()
        .map(|target| {
            (
                target.clone(),
                codex_dir.join(".aipass-backups").join(format!(
                    "sqlite-{}-{}.aipbackup",
                    path_hash(&target),
                    plan.operation_id
                )),
            )
        })
        .collect()
}

fn prepare_codex_sqlite_backups(
    plan: &ConfigPlan,
    backup_key: Option<&[u8; KEY_LEN]>,
) -> Result<Vec<SqliteBackup>> {
    if plan.codex_provider_migration.is_none() {
        return Ok(Vec::new());
    }
    let all_paths = codex_sqlite_backup_paths(plan);
    if all_paths.is_empty() {
        return Ok(all_paths);
    }
    let mut paths = Vec::new();
    for (target, backup) in all_paths {
        if !sqlite_provider_tables(&target)?.is_empty() {
            paths.push((target, backup));
        }
    }
    if paths.is_empty() {
        return Ok(paths);
    }

    for (target, backup) in &paths {
        checkpoint_sqlite(target)?;
        let original = fs::read(target)?;
        if let Some(key) = backup_key {
            let encrypted = EncryptedBackup {
                format: "aipass-config-backup".to_string(),
                version: 1,
                operation_id: plan.operation_id,
                target_path: target.clone(),
                target_existed: true,
                created_at: OffsetDateTime::now_utc(),
                ciphertext: encrypt_bytes(
                    key,
                    backup_aad(plan.operation_id, target).as_bytes(),
                    &original,
                )?,
            };
            write_json(backup, &encrypted)?;
        } else {
            if let Some(parent) = backup.parent() {
                fs::create_dir_all(parent)?;
            }
            atomic_write_bytes(backup, &original)?;
        }
    }
    Ok(paths)
}

fn apply_codex_sqlite_migration(plan: &ConfigPlan) -> Result<()> {
    let Some(migration) = plan.codex_provider_migration.as_ref() else {
        return Ok(());
    };
    for database in codex_sqlite_database_paths(plan) {
        let tables = sqlite_provider_tables(&database)?;
        if tables.is_empty() {
            continue;
        }
        let mut connection = Connection::open(&database)?;
        let transaction = connection.transaction()?;
        for table in tables {
            transaction.execute(
                &format!(
                    "UPDATE {} SET model_provider = ?1 WHERE model_provider = ?2",
                    sqlite_identifier(&table)
                ),
                params![migration.to_provider, migration.from_provider],
            )?;
        }
        transaction.commit()?;
        drop(connection);
        checkpoint_sqlite(&database)?;
    }
    Ok(())
}

fn restore_codex_sqlite_backups(
    backups: &[SqliteBackup],
    backup_key: Option<&[u8; KEY_LEN]>,
) -> Result<()> {
    for (target, backup) in backups.iter().rev() {
        if !backup.exists() {
            continue;
        }
        if let Some(key) = backup_key {
            restore_encrypted_backup_file(backup, key)
        } else {
            restore_plain_backup_file(target, backup)
        }?;
        remove_sqlite_sidecars(target);
    }
    Ok(())
}

fn sqlite_provider_tables(database: &Path) -> Result<Vec<String>> {
    let connection = Connection::open(database)?;
    let mut table_query = connection.prepare(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name IN ('threads', 'local_thread_catalog')",
    )?;
    let tables = table_query
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(table_query);
    let mut provider_tables = Vec::new();
    for table in tables {
        let mut column_query =
            connection.prepare(&format!("PRAGMA table_info({})", sqlite_identifier(&table)))?;
        let has_provider = column_query
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .iter()
            .any(|column| column == "model_provider");
        if has_provider {
            provider_tables.push(table.to_string());
        }
    }
    Ok(provider_tables)
}

fn checkpoint_sqlite(database: &Path) -> Result<()> {
    let connection = Connection::open(database)?;
    connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    Ok(())
}

fn remove_sqlite_sidecars(database: &Path) {
    for suffix in ["-wal", "-shm"] {
        let sidecar = database.with_extension(format!(
            "{}{}",
            database
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("db"),
            suffix
        ));
        let _ = fs::remove_file(sidecar);
    }
}

fn sqlite_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn path_hash(path: &Path) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.to_string_lossy().hash(&mut hasher);
    hasher.finish()
}

fn backup_group_paths(backup_path: &Path, operation_id: Uuid) -> Result<Vec<PathBuf>> {
    let Some(parent) = backup_path.parent() else {
        return Ok(vec![backup_path.to_path_buf()]);
    };
    let mut paths = Vec::new();
    for entry in fs::read_dir(parent)? {
        let path = entry?.path();
        if backup_matches_operation(&path, operation_id) {
            paths.push(path);
        }
    }
    paths.sort();
    if paths.is_empty() {
        paths.push(backup_path.to_path_buf());
    }
    Ok(paths)
}

fn prune_replaced_encrypted_backups(writes: &[PreparedWrite]) -> Result<()> {
    let keep_paths = writes
        .iter()
        .map(|write| write.backup_path.clone())
        .collect::<HashSet<_>>();
    for write in writes {
        prune_replaced_encrypted_backups_for_target(
            &write.backup_path,
            &write.target_path,
            &keep_paths,
        )?;
    }
    Ok(())
}

fn prune_replaced_encrypted_backups_for_target(
    backup_path: &Path,
    target_path: &Path,
    keep_paths: &HashSet<PathBuf>,
) -> Result<()> {
    let Some(parent) = backup_path.parent() else {
        return Ok(());
    };
    if !parent.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(parent)? {
        let path = entry?.path();
        if keep_paths.contains(&path) || !is_aipbackup_file(&path) {
            continue;
        }
        if encrypted_backup_matches_target(&path, target_path) {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

fn encrypted_backup_matches_target(path: &Path, target_path: &Path) -> bool {
    read_json::<EncryptedBackup>(path)
        .map(|backup| backup.target_path == target_path)
        .unwrap_or(false)
}

fn backup_matches_operation(path: &Path, operation_id: Uuid) -> bool {
    backup_name_matches_operation(path, operation_id)
        || read_json::<EncryptedBackup>(path)
            .map(|backup| backup.operation_id == operation_id)
            .unwrap_or(false)
}

fn backup_name_matches_operation(path: &Path, operation_id: Uuid) -> bool {
    let prefix = operation_id.to_string();
    path.file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|name| name.starts_with(&prefix) && name.ends_with(".aipbackup"))
}

fn is_aipbackup_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|name| name.ends_with(".aipbackup"))
}

fn apply_result(plan: &ConfigPlan) -> ApplyResult {
    ApplyResult {
        operation_id: plan.operation_id,
        target_path: plan.target_path.clone(),
        backup_path: plan.backup_path.clone(),
    }
}
