use super::*;
use std::collections::BTreeSet;

const PENDING: &str = "pending-sync.aipsnapshot";

/// One authenticated unit includes the password envelope, epoch keys, and all
/// encrypted records. A remote never receives the root key or plaintext data.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSyncSnapshot {
    pub format: String,
    pub version: u16,
    pub parents: Vec<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    pub header: VaultHeader,
    pub payload: Ciphertext,
}

impl VaultSyncSnapshot {
    pub fn parse(bytes: &[u8]) -> Result<Self, VaultError> {
        let snapshot: Self = serde_json::from_slice(bytes)?;
        if snapshot.format != "aipass-vault-sync" || snapshot.version != 1 {
            return Err(VaultError::UnsupportedVersion);
        }
        validate_header(&snapshot.header)?;
        validate_import_kdf(&snapshot.header.kdf)?;
        if snapshot.parents.iter().any(|id| {
            id.len() != 64
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        }) {
            return Err(VaultError::InvalidExport);
        }
        Ok(snapshot)
    }

    fn aad(&self) -> Result<Vec<u8>, VaultError> {
        Ok(serde_json::to_vec(&(
            &self.format,
            self.version,
            &self.parents,
            self.created_at,
            &self.header,
        ))?)
    }

    /// Bootstrap only an absent vault. Authentication and materialization wait
    /// for the user's original password/recovery key in open_with_root_key.
    pub fn bootstrap(&self, root: &Path) -> Result<(), VaultError> {
        if root.join("manifest.aipmanifest").exists() {
            return Err(VaultError::AlreadyExists);
        }
        atomic_write_bytes(root.join(PENDING), &serde_json::to_vec(self)?)?;
        write_json(root.join("manifest.aipmanifest"), &self.header)?;
        Ok(())
    }
}

impl Vault {
    fn sync_files(&self) -> Result<VaultExportPayload, VaultError> {
        let files = exportable_files(&self.root)?
            .into_iter()
            .filter(|path| sync_path_allowed(path))
            .map(|relative_path| {
                let bytes = fs::read(self.root.join(&relative_path))?;
                Ok(VaultExportFile {
                    relative_path,
                    bytes_b64: STANDARD_NO_PAD.encode(bytes),
                })
            })
            .collect::<Result<Vec<_>, VaultError>>()?;
        Ok(VaultExportPayload { files })
    }

    pub fn sync_revision(&self) -> Result<String, VaultError> {
        revision(&self.header, &self.sync_files()?)
    }

    pub fn export_sync_snapshot(&self) -> Result<Vec<u8>, VaultError> {
        self.export_sync_snapshot_with_parents(Vec::new())
    }

    pub fn export_sync_snapshot_with_parents(
        &self,
        parents: Vec<String>,
    ) -> Result<Vec<u8>, VaultError> {
        let files = self.sync_files()?;
        self.encode_sync_snapshot(files, parents)
    }

    fn encode_sync_snapshot(
        &self,
        files: VaultExportPayload,
        parents: Vec<String>,
    ) -> Result<Vec<u8>, VaultError> {
        // Also authenticate tombstones as part of the complete snapshot. The
        // legacy per-object format cannot authenticate deletion markers alone.
        let payload = serde_json::to_vec(&files)?;
        let created_at = OffsetDateTime::now_utc();
        let aad = serde_json::to_vec(&(
            "aipass-vault-sync",
            1u16,
            &parents,
            created_at,
            &self.header,
        ))?;
        let snapshot = VaultSyncSnapshot {
            format: "aipass-vault-sync".into(),
            version: 1,
            parents,
            created_at,
            header: self.header.clone(),
            payload: encrypt_bytes(self.root_key.as_bytes(), &aad, &payload)?,
        };
        Ok(serde_json::to_vec(&snapshot)?)
    }

    /// Merge independent encrypted-file changes against a common ancestor.
    /// Conflicting edits and divergent key envelopes require an explicit choice.
    pub fn merge_sync_snapshot(
        &self,
        remote: &[u8],
        base: &[u8],
    ) -> Result<Option<Vec<u8>>, VaultError> {
        let (remote_header, remote) = self.decode_snapshot(remote)?;
        let (base_header, base) = self.decode_snapshot(base)?;
        if serde_json::to_vec(&remote_header.header)? != serde_json::to_vec(&self.header)?
            || serde_json::to_vec(&base_header.header)? != serde_json::to_vec(&self.header)?
        {
            return Ok(None);
        }
        let to_map = |payload: VaultExportPayload| {
            payload
                .files
                .into_iter()
                .map(|file| (file.relative_path, file.bytes_b64))
                .collect::<BTreeMap<_, _>>()
        };
        let local = to_map(self.sync_files()?);
        let remote = to_map(remote);
        let base = to_map(base);
        let paths = local
            .keys()
            .chain(remote.keys())
            .chain(base.keys())
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut files = Vec::new();
        for path in paths {
            let local = local.get(&path);
            let remote = remote.get(&path);
            let base = base.get(&path);
            let chosen = if local == remote || remote == base {
                local
            } else if local == base {
                remote
            } else {
                return Ok(None);
            };
            if let Some(bytes_b64) = chosen {
                files.push(VaultExportFile {
                    relative_path: path,
                    bytes_b64: bytes_b64.clone(),
                });
            }
        }
        self.encode_sync_snapshot(VaultExportPayload { files }, Vec::new())
            .map(Some)
    }

    fn decode_snapshot(
        &self,
        bytes: &[u8],
    ) -> Result<(VaultSyncSnapshot, VaultExportPayload), VaultError> {
        let snapshot = VaultSyncSnapshot::parse(bytes)?;
        if snapshot.header.vault_id != self.header.vault_id {
            return Err(VaultError::UnlockFailed);
        }
        let decoded = decrypt_bytes(
            self.root_key.as_bytes(),
            &snapshot.aad()?,
            &snapshot.payload,
        )?;
        let payload: VaultExportPayload = serde_json::from_slice(&decoded)?;
        let (epoch, mut index) = unwrap_epoch_and_index_keys(
            &self.root_key,
            &snapshot.header,
            VaultError::UnlockFailed,
        )?;
        index.zeroize();
        let mut paths = BTreeSet::new();
        for file in &payload.files {
            if !sync_path_allowed(&file.relative_path) || !paths.insert(&file.relative_path) {
                return Err(VaultError::InvalidExport);
            }
            let bytes = STANDARD_NO_PAD
                .decode(&file.bytes_b64)
                .map_err(|_| VaultError::InvalidExport)?;
            {
                let envelope: ObjectEnvelope = serde_json::from_slice(&bytes)?;
                if envelope.vault_id != snapshot.header.vault_id
                    || envelope.format != "aipass-object"
                    || envelope.version != 1
                {
                    return Err(VaultError::InvalidExport);
                }
                if file
                    .relative_path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    != Some(envelope.object_id.to_string().as_str())
                {
                    return Err(VaultError::InvalidExport);
                }
                if !envelope.tombstone {
                    let mut plaintext = decrypt_envelope_with_epoch(&envelope, &epoch)?;
                    plaintext.zeroize();
                }
            }
        }
        Ok((snapshot, payload))
    }

    pub fn sync_snapshot_revision(&self, bytes: &[u8]) -> Result<String, VaultError> {
        let (snapshot, payload) = self.decode_snapshot(bytes)?;
        revision(&snapshot.header, &payload)
    }

    pub fn sync_snapshot_summary(&self, bytes: &[u8]) -> Result<VaultSnapshotSummary, VaultError> {
        #[derive(Deserialize)]
        struct EntryOnly {
            entry: ProviderEntry,
        }
        let (snapshot, payload) = self.decode_snapshot(bytes)?;
        let (epoch, mut index) = unwrap_epoch_and_index_keys(
            &self.root_key,
            &snapshot.header,
            VaultError::UnlockFailed,
        )?;
        index.zeroize();
        let mut titles = Vec::new();
        for file in payload.files {
            if file.relative_path.parent() != Some(Path::new("objects")) {
                continue;
            }
            let bytes = STANDARD_NO_PAD
                .decode(file.bytes_b64)
                .map_err(|_| VaultError::InvalidExport)?;
            let envelope: ObjectEnvelope = serde_json::from_slice(&bytes)?;
            if envelope.tombstone || envelope.object_type != "provider_entry" {
                continue;
            }
            let plaintext =
                zeroize::Zeroizing::new(decrypt_envelope_with_epoch(&envelope, &epoch)?);
            let entry: EntryOnly = serde_json::from_slice(&plaintext)?;
            titles.push(entry.entry.title);
        }
        titles.sort();
        let provider_count = titles.len();
        titles.truncate(8);
        Ok(VaultSnapshotSummary {
            provider_count,
            titles,
        })
    }

    /// Journal the authenticated snapshot before replacing any files. On a
    /// crash, the next unlock replays the complete transaction before reads.
    pub fn apply_sync_snapshot(&mut self, bytes: &[u8]) -> Result<(), VaultError> {
        self.decode_snapshot(bytes)?;
        self.ensure_sync_ready()?;
        let original = self.export_sync_snapshot()?;
        atomic_write_bytes(self.root.join(PENDING), bytes)?;
        if let Err(error) = self.finish_pending_sync() {
            // Roll back with the same authenticated journal mechanism. Keep
            // the previous unlocked session usable on an ordinary IO failure.
            // If the disk also refuses rollback, leave the journal for recovery
            // and reject further vault access until it can be replayed.
            atomic_write_bytes(self.root.join(PENDING), &original)?;
            self.finish_pending_sync()?;
            return Err(error);
        }
        Ok(())
    }

    pub fn ensure_sync_ready(&self) -> Result<(), VaultError> {
        if self.root.join(PENDING).exists() {
            return Err(VaultError::Io(std::io::Error::other(
                "vault synchronization needs disk recovery",
            )));
        }
        Ok(())
    }

    /// Retry an interrupted disk transaction using the current unlocked key.
    pub fn recover_pending_sync(&mut self) -> Result<(), VaultError> {
        self.finish_pending_sync()
    }

    pub(super) fn finish_pending_sync(&mut self) -> Result<(), VaultError> {
        let pending = self.root.join(PENDING);
        if !pending.exists() {
            return Ok(());
        }
        let bytes = fs::read(&pending)?;
        let (snapshot, payload) = self.decode_snapshot(&bytes)?;
        let old_paths = exportable_files(&self.root)?;
        let mut retained = BTreeSet::new();
        for file in payload.files {
            retained.insert(file.relative_path.clone());
            let bytes = STANDARD_NO_PAD
                .decode(&file.bytes_b64)
                .map_err(|_| VaultError::InvalidExport)?;
            let path = self.root.join(file.relative_path);
            if fs::read(&path).ok().as_ref() != Some(&bytes) {
                #[cfg(test)]
                tests::fail_snapshot_write()?;
                atomic_write_bytes(path, &bytes)?;
            }
        }
        for path in old_paths {
            if sync_path_allowed(&path) && !retained.contains(&path) {
                fs::remove_file(self.root.join(path))?;
            }
        }
        write_json(self.root.join("manifest.aipmanifest"), &snapshot.header)?;
        self.reload_from_disk()?;
        fs::remove_file(pending)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSnapshotSummary {
    pub provider_count: usize,
    pub titles: Vec<String>,
}

fn revision(header: &VaultHeader, payload: &VaultExportPayload) -> Result<String, VaultError> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(header, payload))?)
    ))
}

pub(super) fn sync_path_allowed(path: &Path) -> bool {
    let parts = path.components().collect::<Vec<_>>();
    if parts.len() != 2
        || parts
            .iter()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return false;
    }
    let Some(dir) = parts[0].as_os_str().to_str() else {
        return false;
    };
    let ext = match dir {
        "objects" => "aipobj",
        "grants" => "aipgrant",
        "devices" => "aipdevice",
        "audit" => "aipaudit",
        _ => return false,
    };
    path.extension().and_then(|value| value.to_str()) == Some(ext)
}

pub(super) fn validate_import_kdf(kdf: &KdfParams) -> Result<(), VaultError> {
    if kdf.algorithm != "argon2id"
        || kdf.memory_kib > 1024 * 1024
        || kdf.iterations > 10
        || kdf.parallelism > 16
    {
        return Err(VaultError::InvalidExport);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    thread_local! { static FAIL_AFTER: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) }; }
    pub(super) fn fail_snapshot_write() -> Result<(), VaultError> {
        FAIL_AFTER.with(|counter| match counter.get() {
            Some(0) => {
                counter.set(None);
                Err(VaultError::Io(std::io::Error::other(
                    "injected write failure",
                )))
            }
            Some(n) => {
                counter.set(Some(n - 1));
                Ok(())
            }
            None => Ok(()),
        })
    }

    #[test]
    fn failed_snapshot_apply_rolls_back_without_discarding_the_unlocked_vault() {
        let dir = tempdir().unwrap();
        let mut original = vault(dir.path());
        for _ in 0..3 {
            let id = Uuid::new_v4();
            original
                .write_envelope(
                    original.record_path(id),
                    id,
                    "test_record",
                    1,
                    b"private-data",
                )
                .unwrap();
        }
        let before = original.export_sync_snapshot().unwrap();
        let revision = original.sync_revision().unwrap();
        original.advance_epoch_and_rewrap("test").unwrap();
        let after = original.export_sync_snapshot().unwrap();
        original.apply_sync_snapshot(&before).unwrap();
        FAIL_AFTER.with(|counter| counter.set(Some(1)));
        assert!(original.apply_sync_snapshot(&after).is_err());
        assert_eq!(original.sync_revision().unwrap(), revision);
        original.ensure_sync_ready().unwrap();
        let id = Uuid::new_v4();
        original
            .write_envelope(
                original.record_path(id),
                id,
                "test_record",
                1,
                b"still unlocked",
            )
            .unwrap();
        assert!(Vault::open(dir.path(), &SecretString::new("master")).is_ok());
    }

    fn vault(root: &Path) -> Vault {
        Vault::create_with_device_and_kdf(
            root,
            &SecretString::new("master"),
            "device",
            KdfParams::with_random_salt(1024, 1, 1),
        )
        .unwrap()
        .vault
    }

    #[test]
    fn snapshot_bootstrap_authenticates_before_materializing_and_survives_epoch_change() {
        let source = tempdir().unwrap();
        let dest = tempdir().unwrap();
        let mut original = vault(source.path());
        let id = Uuid::new_v4();
        original
            .write_envelope(
                original.record_path(id),
                id,
                "test_record",
                1,
                b"fake-secret-and-private-endpoint",
            )
            .unwrap();
        let bytes = original.export_sync_snapshot().unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("fake-secret"));
        VaultSyncSnapshot::parse(&bytes)
            .unwrap()
            .bootstrap(dest.path())
            .unwrap();
        assert!(Vault::open(dest.path(), &SecretString::new("wrong")).is_err());
        assert!(!dest
            .path()
            .join("objects")
            .join(format!("{id}.aipobj"))
            .exists());
        let mut restored = Vault::open(dest.path(), &SecretString::new("master")).unwrap();
        assert_eq!(
            original.sync_revision().unwrap(),
            restored.sync_revision().unwrap()
        );
        original.advance_epoch_and_rewrap("test").unwrap();
        original
            .change_master_password_with_kdf(
                &SecretString::new("new-master"),
                KdfParams::with_random_salt(1024, 1, 1),
            )
            .unwrap();
        restored
            .apply_sync_snapshot(&original.export_sync_snapshot().unwrap())
            .unwrap();
        assert_eq!(original.current_epoch(), restored.current_epoch());
        assert!(Vault::open(dest.path(), &SecretString::new("master")).is_err());
        assert!(Vault::open(dest.path(), &SecretString::new("new-master")).is_ok());
    }

    #[test]
    fn snapshot_authenticates_header_parent_links_and_deletions() {
        let dir = tempdir().unwrap();
        let mut original = vault(dir.path());
        let bytes = original.export_sync_snapshot().unwrap();
        let revision = original.sync_revision().unwrap();
        let mut snapshot = VaultSyncSnapshot::parse(&bytes).unwrap();
        snapshot.parents.push("a".repeat(64));
        assert!(original
            .apply_sync_snapshot(&serde_json::to_vec(&snapshot).unwrap())
            .is_err());
        snapshot = VaultSyncSnapshot::parse(&bytes).unwrap();
        snapshot.header.updated_at += Duration::seconds(1);
        assert!(original
            .apply_sync_snapshot(&serde_json::to_vec(&snapshot).unwrap())
            .is_err());
        assert_eq!(revision, original.sync_revision().unwrap());
        assert!(!dir.path().join(PENDING).exists());
    }

    #[test]
    fn pending_snapshot_replays_complete_transaction_after_interrupted_write() {
        let dir = tempdir().unwrap();
        let mut original = vault(dir.path());
        let initial = original.export_sync_snapshot().unwrap();
        original.advance_epoch_and_rewrap("test").unwrap();
        let updated = original.export_sync_snapshot().unwrap();
        let revision = original.sync_revision().unwrap();
        original.apply_sync_snapshot(&initial).unwrap();
        atomic_write_bytes(dir.path().join(PENDING), &updated).unwrap();
        let recovered = Vault::open(dir.path(), &SecretString::new("master")).unwrap();
        assert_eq!(recovered.sync_revision().unwrap(), revision);
        assert!(!dir.path().join(PENDING).exists());
    }

    #[test]
    fn backup_rejects_unknown_paths_before_writing_any_file() {
        let dir = tempdir().unwrap();
        let source = vault(dir.path());
        let password = SecretString::new("export");
        let mut export = source
            .export_encrypted_with_kdf(&password, KdfParams::with_random_salt(1024, 1, 1))
            .unwrap();
        let key = derive_master_key(&password, &export.kdf).unwrap();
        let payload = VaultExportPayload {
            files: vec![VaultExportFile {
                relative_path: "../outside".into(),
                bytes_b64: STANDARD_NO_PAD.encode(b"bad"),
            }],
        };
        export.payload = encrypt_bytes(
            key.as_bytes(),
            export_aad(export.vault_id, export.created_at).as_bytes(),
            &serde_json::to_vec(&payload).unwrap(),
        )
        .unwrap();
        let target = tempdir().unwrap();
        assert!(Vault::import_encrypted(target.path(), &password, &export).is_err());
        assert_eq!(fs::read_dir(target.path()).unwrap().count(), 0);
    }
}
