use crate::models::{ApplyResult, ConfigPlan, EncryptedBackup, PlannedWrite};
use crate::utils::{backup_aad, read_json, resolve_codex_dir, write_json};
use aipass_crypto::{decrypt_bytes, encrypt_bytes, KEY_LEN};
use aipass_storage::atomic_write_bytes;
use anyhow::{Context, Result};
use std::fs;
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
    apply_prepared_writes(plan.operation_id, &prepared)?;
    Ok(apply_result(plan))
}

pub fn apply_plan_with_plain_backup(plan: &ConfigPlan, content: &str) -> Result<ApplyResult> {
    let prepared = prepare_writes(plan, content)?;
    write_plain_backups(&prepared)?;
    apply_prepared_writes(plan.operation_id, &prepared)?;
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
    let prefix = operation_id.to_string();
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
        for entry in fs::read_dir(root)? {
            let path = entry?.path();
            if path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.starts_with(&prefix) && name.ends_with(".aipbackup"))
            {
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

fn backup_group_paths(backup_path: &Path, operation_id: Uuid) -> Result<Vec<PathBuf>> {
    let Some(parent) = backup_path.parent() else {
        return Ok(vec![backup_path.to_path_buf()]);
    };
    let prefix = operation_id.to_string();
    let mut paths = Vec::new();
    for entry in fs::read_dir(parent)? {
        let path = entry?.path();
        if path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.starts_with(&prefix) && name.ends_with(".aipbackup"))
        {
            paths.push(path);
        }
    }
    paths.sort();
    if paths.is_empty() {
        paths.push(backup_path.to_path_buf());
    }
    Ok(paths)
}

fn apply_result(plan: &ConfigPlan) -> ApplyResult {
    ApplyResult {
        operation_id: plan.operation_id,
        target_path: plan.target_path.clone(),
        backup_path: plan.backup_path.clone(),
    }
}
