use crate::webdav::WebDavClient;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;
use uuid::Uuid;

const SYNC_DIRS: &[(&str, &str)] = &[
    ("objects", "aipobj"),
    ("grants", "aipgrant"),
    ("devices", "aipdevice"),
    ("audit", "aipaudit"),
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SyncObject {
    pub object_id: Option<Uuid>,
    pub object_type: String,
    pub lamport: u64,
    pub hash_hex: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    pub relative_path: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConflictRecord {
    pub origin: String,
    pub conflict_path: PathBuf,
    pub target_path: PathBuf,
    pub object: SyncObject,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SyncStatus {
    Idle,
    Syncing,
    Conflict,
    Offline,
    AuthFailed,
    ServerError,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub uploaded: usize,
    pub downloaded: usize,
    pub conflicts: usize,
    pub quarantined: usize,
    pub status: SyncStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncCheckpoint {
    pub format: String,
    pub version: u16,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    pub objects: BTreeMap<String, String>,
}

pub fn sync_local_folder(vault_root: &Path, sync_root: &Path) -> Result<SyncReport> {
    let mut uploaded = 0;
    let mut downloaded = 0;
    let mut conflicts = 0;
    let mut quarantined = 0;
    let mut conflicted_paths = BTreeSet::new();
    fs::create_dir_all(sync_root)?;

    for (dir, _ext) in SYNC_DIRS {
        fs::create_dir_all(vault_root.join(dir))?;
        fs::create_dir_all(sync_root.join(dir))?;
    }
    fs::create_dir_all(vault_root.join("conflicts"))?;
    fs::create_dir_all(sync_root.join("conflicts"))?;

    for local in list_sync_files(vault_root)? {
        let remote = sync_root.join(&local.relative_path);
        match compare_and_copy(&vault_root.join(&local.relative_path), &remote)? {
            CopyOutcome::Copied => uploaded += 1,
            CopyOutcome::Conflict => {
                conflicts += 1;
                quarantined += 1;
                conflicted_paths.insert(local.relative_path.clone());
                quarantine_source_conflict(
                    sync_root,
                    "local",
                    &local.relative_path,
                    &vault_root.join(&local.relative_path),
                )?;
            }
            CopyOutcome::Skipped => {}
        }
    }

    for remote in list_sync_files(sync_root)? {
        if conflicted_paths.contains(&remote.relative_path) {
            continue;
        }
        let local = vault_root.join(&remote.relative_path);
        match compare_and_copy(&sync_root.join(&remote.relative_path), &local)? {
            CopyOutcome::Copied => downloaded += 1,
            CopyOutcome::Conflict => {
                conflicts += 1;
                quarantined += 1;
                quarantine_source_conflict(
                    vault_root,
                    "remote",
                    &remote.relative_path,
                    &sync_root.join(&remote.relative_path),
                )?;
            }
            CopyOutcome::Skipped => {}
        }
    }

    write_checkpoint(vault_root)?;
    write_checkpoint(sync_root)?;

    Ok(SyncReport {
        uploaded,
        downloaded,
        conflicts,
        quarantined,
        status: if conflicts > 0 {
            SyncStatus::Conflict
        } else {
            SyncStatus::Idle
        },
    })
}

pub fn sync_webdav(vault_root: &Path, client: &impl WebDavClient) -> Result<SyncReport> {
    let mut uploaded = 0;
    let mut downloaded = 0;
    let mut conflicts = 0;
    let mut quarantined = 0;

    for (dir, _ext) in SYNC_DIRS {
        fs::create_dir_all(vault_root.join(dir))?;
    }
    fs::create_dir_all(vault_root.join("conflicts"))?;

    let local_objects = list_sync_files(vault_root)?
        .into_iter()
        .map(|object| (normalize_remote_path(&object.relative_path), object))
        .collect::<BTreeMap<_, _>>();
    let remote_objects = list_webdav_sync_files(client)?;
    let all_paths = local_objects
        .keys()
        .chain(remote_objects.keys())
        .cloned()
        .collect::<BTreeSet<_>>();

    for path in all_paths {
        match (local_objects.get(&path), remote_objects.get(&path)) {
            (Some(local), None) => {
                let bytes = fs::read(vault_root.join(&local.relative_path))?;
                client.put(&path, &bytes, None)?;
                uploaded += 1;
            }
            (None, Some(remote)) => {
                let bytes = client.get(&path)?;
                let local_path = vault_root.join(&remote.relative_path);
                if let Some(parent) = local_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(local_path, bytes)?;
                downloaded += 1;
            }
            (Some(local), Some(remote)) if local.hash_hex == remote.hash_hex => {}
            (Some(local), Some(remote))
                if local.object_id == remote.object_id && local.lamport == remote.lamport =>
            {
                let bytes = client.get(&path)?;
                quarantine_remote_conflict(vault_root, &path, &bytes)?;
                conflicts += 1;
                quarantined += 1;
            }
            (Some(local), Some(remote)) if local.lamport > remote.lamport => {
                let bytes = fs::read(vault_root.join(&local.relative_path))?;
                client.put(&path, &bytes, remote.etag.as_deref())?;
                uploaded += 1;
            }
            (Some(local), Some(_remote)) => {
                let bytes = client.get(&path)?;
                fs::write(vault_root.join(&local.relative_path), bytes)?;
                downloaded += 1;
            }
            (None, None) => {}
        }
    }

    write_checkpoint(vault_root)?;

    Ok(SyncReport {
        uploaded,
        downloaded,
        conflicts,
        quarantined,
        status: if conflicts > 0 {
            SyncStatus::Conflict
        } else {
            SyncStatus::Idle
        },
    })
}

pub fn list_conflicts(root: &Path) -> Result<Vec<ConflictRecord>> {
    let mut records = Vec::new();
    for path in conflict_record_paths(root)? {
        let record: ConflictRecord = read_json(&path)?;
        checked_relative_path(&record.conflict_path)?;
        checked_relative_path(&record.target_path)?;
        records.push(record);
    }
    records.sort_by(|left, right| {
        left.object
            .updated_at
            .cmp(&right.object.updated_at)
            .then(left.target_path.cmp(&right.target_path))
            .then(left.conflict_path.cmp(&right.conflict_path))
    });
    Ok(records)
}

pub fn accept_conflict(root: &Path, conflict_path: &Path) -> Result<()> {
    let record_path = conflict_record_path(root, conflict_path)?;
    let record = get_conflict(root, conflict_path)?;
    let payload_path = root.join(checked_relative_path(&record.conflict_path)?);
    let target_path = root.join(checked_relative_path(&record.target_path)?);
    if !payload_path.exists() {
        bail!("conflict payload not found: {}", payload_path.display());
    }
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(&payload_path, &target_path)?;
    remove_conflict_record(&record_path)?;
    Ok(())
}

pub fn discard_conflict(root: &Path, conflict_path: &Path) -> Result<()> {
    let record_path = conflict_record_path(root, conflict_path)?;
    remove_conflict_record(&record_path)?;
    Ok(())
}

pub fn get_conflict(root: &Path, conflict_path: &Path) -> Result<ConflictRecord> {
    let record_path = conflict_record_path(root, conflict_path)?;
    let record: ConflictRecord = read_json(&record_path)?;
    checked_relative_path(&record.conflict_path)?;
    checked_relative_path(&record.target_path)?;
    Ok(record)
}

pub fn list_sync_files(root: &Path) -> Result<Vec<SyncObject>> {
    let mut objects = Vec::new();
    for (dir, ext) in SYNC_DIRS {
        let base = root.join(dir);
        if !base.exists() {
            continue;
        }
        for entry in fs::read_dir(&base)? {
            let path = entry?.path();
            if path.extension().and_then(|value| value.to_str()) != Some(*ext) {
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .context("sync path outside root")?
                .to_path_buf();
            objects.push(read_sync_object(root, relative)?);
        }
    }
    objects.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(objects)
}

pub fn list_webdav_sync_files(client: &impl WebDavClient) -> Result<BTreeMap<String, SyncObject>> {
    let mut objects = BTreeMap::new();
    for (dir, ext) in SYNC_DIRS {
        for entry in client.list(dir)? {
            if !entry.path.ends_with(&format!(".{ext}")) {
                continue;
            }
            let bytes = client.get(&entry.path)?;
            let mut object = sync_object_from_bytes(PathBuf::from(&entry.path), &bytes)?;
            object.etag = entry.etag;
            objects.insert(entry.path, object);
        }
    }
    Ok(objects)
}

pub fn hash_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path)?;
    Ok(hash_bytes(&bytes))
}

pub fn sync_server_visibility_scan(sync_root: &Path, forbidden: &[&str]) -> Result<Vec<PathBuf>> {
    let mut matches = Vec::new();
    scan(sync_root, forbidden, &mut matches)?;
    Ok(matches)
}

fn read_sync_object(root: &Path, relative_path: PathBuf) -> Result<SyncObject> {
    let path = root.join(&relative_path);
    let bytes = fs::read(&path)?;
    sync_object_from_bytes(relative_path, &bytes)
}

fn sync_object_from_bytes(relative_path: PathBuf, bytes: &[u8]) -> Result<SyncObject> {
    let value: serde_json::Value = serde_json::from_slice(bytes).unwrap_or_default();
    Ok(SyncObject {
        object_id: value
            .get("objectId")
            .or_else(|| value.get("object_id"))
            .and_then(|value| value.as_str())
            .and_then(|value| Uuid::parse_str(value).ok()),
        object_type: value
            .get("objectType")
            .or_else(|| value.get("object_type"))
            .and_then(|value| value.as_str())
            .unwrap_or("encrypted_object")
            .to_string(),
        lamport: value
            .get("lamport")
            .and_then(|value| value.as_u64())
            .unwrap_or_default(),
        updated_at: value
            .get("updatedAt")
            .or_else(|| value.get("updated_at"))
            .and_then(|value| value.as_str())
            .and_then(|value| {
                OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).ok()
            })
            .unwrap_or_else(OffsetDateTime::now_utc),
        hash_hex: hash_bytes(bytes),
        etag: None,
        relative_path,
    })
}

enum CopyOutcome {
    Copied,
    Conflict,
    Skipped,
}

fn compare_and_copy(source: &Path, target: &Path) -> Result<CopyOutcome> {
    if !target.exists() {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, target)?;
        return Ok(CopyOutcome::Copied);
    }
    let source_hash = hash_file(source)?;
    let target_hash = hash_file(target)?;
    if source_hash == target_hash {
        return Ok(CopyOutcome::Skipped);
    }
    let source_meta = read_object_meta(source)?;
    let target_meta = read_object_meta(target)?;
    if source_meta.object_id == target_meta.object_id && source_meta.lamport == target_meta.lamport
    {
        return Ok(CopyOutcome::Conflict);
    }
    if source_meta.lamport >= target_meta.lamport {
        fs::copy(source, target)?;
        Ok(CopyOutcome::Copied)
    } else {
        Ok(CopyOutcome::Skipped)
    }
}

fn read_object_meta(path: &Path) -> Result<SyncObject> {
    let root = path
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new("."));
    let relative = path.strip_prefix(root).unwrap_or(path).to_path_buf();
    read_sync_object(root, relative)
}

fn quarantine_source_conflict(
    root: &Path,
    origin: &str,
    target_relative_path: &Path,
    source_path: &Path,
) -> Result<ConflictRecord> {
    let bytes = fs::read(source_path)?;
    quarantine_conflict_bytes(root, origin, target_relative_path, &bytes)
}

fn quarantine_remote_conflict(root: &Path, target_path: &str, bytes: &[u8]) -> Result<()> {
    quarantine_conflict_bytes(root, "remote", Path::new(target_path), bytes).map(|_| ())
}

fn quarantine_conflict_bytes(
    root: &Path,
    origin: &str,
    target_relative_path: &Path,
    bytes: &[u8],
) -> Result<ConflictRecord> {
    let target_relative = checked_relative_path(target_relative_path)?;
    let name = target_relative
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("object");
    let record_dir = root.join("conflicts").join(origin).join(format!(
        "{}-{}",
        OffsetDateTime::now_utc().unix_timestamp_nanos(),
        slug_text(&format!(
            "{}-{}",
            normalize_remote_path(&target_relative),
            name
        ))
    ));
    fs::create_dir_all(&record_dir)?;
    let payload_path = record_dir.join(name);
    fs::write(&payload_path, bytes)?;
    let payload_relative = payload_path
        .strip_prefix(root)
        .context("conflict payload outside root")?
        .to_path_buf();
    let object = sync_object_from_bytes(payload_relative.clone(), bytes)?;
    let record = ConflictRecord {
        origin: origin.to_string(),
        conflict_path: payload_relative.clone(),
        target_path: target_relative,
        object,
    };
    write_json(payload_path.with_extension("aipconflict"), &record)?;
    Ok(record)
}

fn conflict_record_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let conflicts_root = root.join("conflicts");
    if !conflicts_root.exists() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    collect_conflict_records(&conflicts_root, &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn collect_conflict_records(path: &Path, matches: &mut Vec<PathBuf>) -> Result<()> {
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            collect_conflict_records(&entry?.path(), matches)?;
        }
    } else if path.extension().and_then(|value| value.to_str()) == Some("aipconflict") {
        matches.push(path.to_path_buf());
    }
    Ok(())
}

fn conflict_record_path(root: &Path, conflict_path: &Path) -> Result<PathBuf> {
    let conflict_path = root.join(checked_relative_path(conflict_path)?);
    let record_path = conflict_path.with_extension("aipconflict");
    if !record_path.exists() {
        bail!("conflict record not found: {}", conflict_path.display());
    }
    Ok(record_path)
}

fn remove_conflict_record(record_path: &Path) -> Result<()> {
    if let Some(parent) = record_path.parent() {
        fs::remove_dir_all(parent)?;
    }
    Ok(())
}

fn write_checkpoint(root: &Path) -> Result<()> {
    let mut objects = BTreeMap::new();
    for object in list_sync_files(root)? {
        objects.insert(object.relative_path.display().to_string(), object.hash_hex);
    }
    let checkpoint = SyncCheckpoint {
        format: "aipass-sync-checkpoint".to_string(),
        version: 1,
        updated_at: OffsetDateTime::now_utc(),
        objects,
    };
    fs::write(
        root.join("sync-checkpoint.aipcheckpoint"),
        serde_json::to_vec_pretty(&checkpoint)?,
    )?;
    Ok(())
}

fn write_json(path: impl AsRef<Path>, value: &impl Serialize) -> Result<()> {
    if let Some(parent) = path.as_ref().parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: impl AsRef<Path>) -> Result<T> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

pub(crate) fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn checked_relative_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        bail!("sync path must be relative: {}", path.display());
    }
    Ok(path.to_path_buf())
}

fn normalize_remote_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .filter(|part| !part.is_empty() && part != ".")
        .collect::<Vec<_>>()
        .join("/")
}

fn slug_text(value: &str) -> String {
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

fn scan(path: &Path, forbidden: &[&str], matches: &mut Vec<PathBuf>) -> Result<()> {
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            scan(&entry?.path(), forbidden, matches)?;
        }
    } else {
        let text = String::from_utf8_lossy(&fs::read(path)?).into_owned();
        if forbidden
            .iter()
            .filter(|needle| !needle.is_empty())
            .any(|needle| text.contains(needle))
        {
            matches.push(path.to_path_buf());
        }
    }
    Ok(())
}
