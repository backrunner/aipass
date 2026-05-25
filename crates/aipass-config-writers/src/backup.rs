use crate::models::{ApplyResult, ConfigPlan, EncryptedBackup};
use crate::utils::{backup_aad, read_json, write_json};
use aipass_crypto::{decrypt_bytes, encrypt_bytes, KEY_LEN};
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;
use uuid::Uuid;

pub fn apply_plan(plan: &ConfigPlan, content: &str) -> Result<ApplyResult> {
    apply_plan_with_plain_backup(plan, content)
}

pub fn apply_plan_encrypted(
    plan: &ConfigPlan,
    content: &str,
    backup_key: &[u8; KEY_LEN],
) -> Result<ApplyResult> {
    if let Some(parent) = plan.target_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = plan.backup_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let target_existed = plan.target_path.exists();
    let original = if target_existed {
        fs::read(&plan.target_path)
            .with_context(|| format!("backup {}", plan.target_path.display()))?
    } else {
        Vec::new()
    };
    let aad = backup_aad(plan.operation_id, &plan.target_path);
    let backup = EncryptedBackup {
        format: "aipass-config-backup".to_string(),
        version: 1,
        operation_id: plan.operation_id,
        target_path: plan.target_path.clone(),
        target_existed,
        created_at: OffsetDateTime::now_utc(),
        ciphertext: encrypt_bytes(backup_key, aad.as_bytes(), &original)?,
    };
    write_json(&plan.backup_path, &backup)?;
    fs::write(&plan.target_path, content)?;
    Ok(ApplyResult {
        operation_id: plan.operation_id,
        target_path: plan.target_path.clone(),
        backup_path: plan.backup_path.clone(),
    })
}

pub fn apply_plan_with_plain_backup(plan: &ConfigPlan, content: &str) -> Result<ApplyResult> {
    if let Some(parent) = plan.target_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = plan.backup_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if plan.target_path.exists() {
        fs::copy(&plan.target_path, &plan.backup_path)
            .with_context(|| format!("backup {}", plan.target_path.display()))?;
    } else {
        fs::write(&plan.backup_path, b"")?;
    }
    fs::write(&plan.target_path, content)?;
    Ok(ApplyResult {
        operation_id: plan.operation_id,
        target_path: plan.target_path.clone(),
        backup_path: plan.backup_path.clone(),
    })
}

pub fn rollback(plan: &ConfigPlan) -> Result<()> {
    rollback_plain(plan)
}

pub fn rollback_encrypted(backup_path: &Path, backup_key: &[u8; KEY_LEN]) -> Result<ApplyResult> {
    let backup: EncryptedBackup = read_json(backup_path)?;
    let aad = backup_aad(backup.operation_id, &backup.target_path);
    let original = decrypt_bytes(backup_key, aad.as_bytes(), &backup.ciphertext)?;
    if let Some(parent) = backup.target_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if backup.target_existed {
        fs::write(&backup.target_path, original)?;
    } else if backup.target_path.exists() {
        fs::remove_file(&backup.target_path)?;
    }
    Ok(ApplyResult {
        operation_id: backup.operation_id,
        target_path: backup.target_path,
        backup_path: backup_path.to_path_buf(),
    })
}

pub fn find_backup_by_operation(home: &Path, operation_id: Uuid) -> Result<PathBuf> {
    let prefix = operation_id.to_string();
    for root in [
        home.join(".codex").join(".aipass-backups"),
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
    if plan.backup_path.exists() {
        if fs::metadata(&plan.backup_path)?.len() == 0 {
            let _ = fs::remove_file(&plan.target_path);
        } else {
            fs::copy(&plan.backup_path, &plan.target_path)?;
        }
    }
    Ok(())
}
