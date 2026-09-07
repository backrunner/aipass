//! Immutable encrypted snapshots prevent iCloud's asynchronous file replacement
//! and concurrent WebDAV writers from losing either branch of a vault history.
use crate::WebDavClient;
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

pub const SNAPSHOT_DIR: &str = "snapshots";
pub trait SnapshotRemote {
    fn list(&self) -> Result<Vec<String>>;
    fn get(&self, id: &str) -> Result<Vec<u8>>;
    fn publish(&self, bytes: &[u8]) -> Result<String>;
    fn legacy_objects(&self) -> Result<Vec<(std::path::PathBuf, Vec<u8>)>> {
        Ok(Vec::new())
    }
}

pub fn snapshot_id(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn valid_snapshot_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn name(id: &str) -> Result<String> {
    if !valid_snapshot_id(id) {
        bail!("invalid snapshot id");
    }
    Ok(format!("{id}.aipsnapshot"))
}

fn id_from_name(name: &str) -> Option<String> {
    let id = name.strip_suffix(".aipsnapshot")?;
    valid_snapshot_id(id).then(|| id.to_string())
}

pub struct FolderSnapshotRemote<'a>(pub &'a Path);

impl SnapshotRemote for FolderSnapshotRemote<'_> {
    fn legacy_objects(&self) -> Result<Vec<(std::path::PathBuf, Vec<u8>)>> {
        crate::list_sync_files(self.0)?
            .into_iter()
            .map(|object| {
                let bytes = fs::read(self.0.join(&object.relative_path))?;
                Ok((object.relative_path, bytes))
            })
            .collect()
    }
    fn list(&self) -> Result<Vec<String>> {
        let dir = self.0.join(SNAPSHOT_DIR);
        request_cloud_download(&dir);
        if !dir.exists() {
            return Ok(Vec::new());
        }
        ensure_directory(&dir)?;
        let mut ids = Vec::new();
        let mut pending = false;
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let filename = entry.file_name().to_string_lossy().into_owned();
            if let Some(name) = filename
                .strip_prefix('.')
                .and_then(|name| name.strip_suffix(".icloud"))
            {
                if id_from_name(name).is_some() {
                    request_cloud_download(&dir.join(name));
                    pending = true;
                }
            } else if entry.file_type()?.is_file() {
                if let Some(id) = id_from_name(&filename) {
                    ids.push(id);
                }
            }
        }
        if pending {
            bail!("iCloud is downloading existing vault files; retry when the download completes");
        }
        Ok(ids)
    }

    fn get(&self, id: &str) -> Result<Vec<u8>> {
        let path = self.0.join(SNAPSHOT_DIR).join(name(id)?);
        request_cloud_download(&path);
        if !fs::symlink_metadata(&path)?.file_type().is_file() {
            bail!("snapshot is not a regular file");
        }
        let bytes = fs::read(path)?;
        if snapshot_id(&bytes) != id {
            bail!("snapshot content hash mismatch");
        }
        Ok(bytes)
    }

    fn publish(&self, bytes: &[u8]) -> Result<String> {
        fs::create_dir_all(self.0)?;
        ensure_directory(self.0)?;
        let dir = self.0.join(SNAPSHOT_DIR);
        fs::create_dir_all(&dir)?;
        ensure_directory(&dir)?;
        let id = snapshot_id(bytes);
        aipass_storage::atomic_write_bytes(dir.join(name(&id)?), bytes)?;
        Ok(id)
    }
}

#[cfg(target_os = "macos")]
fn request_cloud_download(path: &Path) {
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    let url = NSURL::fileURLWithPath_isDirectory(
        &NSString::from_str(&path.to_string_lossy()),
        path.is_dir(),
    );
    let manager = NSFileManager::defaultManager();
    if manager.isUbiquitousItemAtURL(&url) {
        let _ = manager.startDownloadingUbiquitousItemAtURL_error(&url);
    }
}

#[cfg(not(target_os = "macos"))]
fn request_cloud_download(_path: &Path) {}

fn ensure_directory(path: &Path) -> Result<()> {
    if !fs::symlink_metadata(path)?.file_type().is_dir() {
        bail!("sync target is not a regular directory");
    }
    Ok(())
}

pub struct WebDavSnapshotRemote<'a, C: WebDavClient>(pub &'a C);

impl<C: WebDavClient> SnapshotRemote for WebDavSnapshotRemote<'_, C> {
    fn legacy_objects(&self) -> Result<Vec<(std::path::PathBuf, Vec<u8>)>> {
        crate::list_webdav_sync_files(self.0)?
            .into_iter()
            .map(|(path, object)| Ok((object.relative_path, self.0.get(&path)?)))
            .collect()
    }
    fn list(&self) -> Result<Vec<String>> {
        Ok(self
            .0
            .list(SNAPSHOT_DIR)?
            .into_iter()
            .filter_map(|entry| id_from_name(entry.path.strip_prefix("snapshots/")?))
            .collect())
    }
    fn get(&self, id: &str) -> Result<Vec<u8>> {
        let bytes = self.0.get(&format!("{SNAPSHOT_DIR}/{}", name(id)?))?;
        if snapshot_id(&bytes) != id {
            bail!("snapshot content hash mismatch");
        }
        Ok(bytes)
    }
    fn publish(&self, bytes: &[u8]) -> Result<String> {
        let id = snapshot_id(bytes);
        let path = format!("{SNAPSHOT_DIR}/{}", name(&id)?);
        if let Err(err) = self.0.put_if_absent(&path, bytes) {
            // A timed-out successful PUT is safe to retry: the immutable name
            // authenticates the exact bytes, never an unrelated replacement.
            if self.0.get(&path).context("snapshot publication failed")? != bytes {
                return Err(err);
            }
        }
        Ok(id)
    }
}
