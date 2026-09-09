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
        self.encode_snapshot_with_header(self.header.clone(), files, parents)
    }

    fn encode_snapshot_with_header(
        &self,
        header: VaultHeader,
        files: VaultExportPayload,
        parents: Vec<String>,
    ) -> Result<Vec<u8>, VaultError> {
        // Also authenticate tombstones as part of the complete snapshot. The
        // legacy per-object format cannot authenticate deletion markers alone.
        let payload = serde_json::to_vec(&files)?;
        let created_at = OffsetDateTime::now_utc();
        let aad = serde_json::to_vec(&("aipass-vault-sync", 1u16, &parents, created_at, &header))?;
        let snapshot = VaultSyncSnapshot {
            format: "aipass-vault-sync".into(),
            version: 1,
            parents,
            created_at,
            header,
            payload: encrypt_bytes(self.root_key.as_bytes(), &aad, &payload)?,
        };
        Ok(serde_json::to_vec(&snapshot)?)
    }

    pub(super) fn commit_epoch_rotation(
        &mut self,
        header: VaultHeader,
        reason: &str,
        revoked_device: Option<Uuid>,
    ) -> Result<(), VaultError> {
        let snapshot = self.prepare_epoch_rotation(header, reason, revoked_device)?;
        self.apply_sync_snapshot(&snapshot)
    }

    /// Prepare the entire next epoch without changing disk or session keys.
    /// Password/recovery wrappers and device revocation share the same journal
    /// as the records, including the audit event, so failure can roll back all
    /// of them and unlock can replay an interrupted commit.
    fn prepare_epoch_rotation(
        &self,
        mut header: VaultHeader,
        reason: &str,
        revoked_device: Option<Uuid>,
    ) -> Result<Vec<u8>, VaultError> {
        self.ensure_sync_ready()?;
        let next_epoch = advance_epoch(&self.epoch_key)?;
        let mut files = self.sync_files()?;
        let event = AuditEvent {
            id: Uuid::new_v4(),
            at: OffsetDateTime::now_utc(),
            action: reason.into(),
            record_id: None,
            detail: Some("epoch advanced".into()),
        };
        let audit_path = PathBuf::from("audit").join(format!("{}.aipaudit", event.id));
        let audit = self.build_envelope(
            &self.root.join(&audit_path),
            event.id,
            "audit_event",
            1,
            &serde_json::to_vec(&event)?,
        )?;
        files.files.push(VaultExportFile {
            relative_path: audit_path,
            bytes_b64: STANDARD_NO_PAD.encode(serde_json::to_vec(&audit)?),
        });
        for file in &mut files.files {
            let bytes = STANDARD_NO_PAD
                .decode(&file.bytes_b64)
                .map_err(|_| VaultError::InvalidExport)?;
            let mut envelope: ObjectEnvelope = serde_json::from_slice(&bytes)?;
            if envelope.tombstone {
                continue;
            }
            let mut plaintext = zeroize::Zeroizing::new(self.decrypt_envelope_bytes(&envelope)?);
            if revoked_device
                .is_some_and(|id| self.device_path(id) == self.root.join(&file.relative_path))
            {
                let mut device: DeviceRecord = serde_json::from_slice(&plaintext)?;
                device.trusted = false;
                device.revoked_at = Some(event.at);
                device.last_epoch = next_epoch.epoch().epoch;
                *plaintext = serde_json::to_vec(&device)?;
            }
            envelope.device_id = self.device_id;
            envelope.updated_at = event.at;
            envelope.lamport = envelope.lamport.saturating_add(1);
            let envelope = encrypt_envelope(envelope, &plaintext, &next_epoch)?;
            file.bytes_b64 = STANDARD_NO_PAD.encode(serde_json::to_vec(&envelope)?);
        }
        header.wrapped_epoch_key = encrypt_bytes(
            self.root_key.as_bytes(),
            header_key_aad(header.vault_id, "epoch").as_bytes(),
            next_epoch.as_bytes(),
        )?;
        header.current_epoch = next_epoch.epoch().clone();
        header.updated_at = event.at;
        self.encode_snapshot_with_header(header, files, Vec::new())
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
        #[cfg(test)]
        tests::fail_snapshot_write()?;
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
    fn rotation_failures_preserve_records_password_recovery_and_device_trust() {
        for operation in ["rotate", "password", "recovery", "revoke"] {
            // A corrupt input fails preparation; IO faults fail after records
            // have been replaced, including just before manifest commit.
            for fail_after in [None, Some(1), Some(4)] {
                let dir = tempdir().unwrap();
                let creation = Vault::create_with_device_and_kdf(
                    dir.path(),
                    &SecretString::new("master"),
                    "device",
                    KdfParams::with_random_salt(1024, 1, 1),
                )
                .unwrap();
                let recovery = SecretString::new(&creation.recovery_kit.recovery_key);
                let mut original = creation.vault;
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
                if fail_after.is_none() {
                    fs::write(dir.path().join("audit/damaged.aipaudit"), b"invalid JSON").unwrap();
                }
                let before = original.sync_revision().unwrap();
                FAIL_AFTER.with(|counter| counter.set(fail_after));
                let result = match operation {
                    "rotate" => original.advance_epoch_and_rewrap("test").map(|_| ()),
                    "password" => original.change_master_password_with_kdf(
                        &SecretString::new("new-master"),
                        KdfParams::with_random_salt(1024, 1, 1),
                    ),
                    "recovery" => Vault::recover_master_password_with_kdf(
                        dir.path(),
                        &recovery,
                        &SecretString::new("new-master"),
                        KdfParams::with_random_salt(1024, 1, 1),
                    )
                    .map(|_| ()),
                    "revoke" => original.revoke_device(original.device_id),
                    _ => unreachable!(),
                };
                assert!(result.is_err(), "{operation}/{fail_after:?}");
                assert_eq!(
                    before,
                    original.sync_revision().unwrap(),
                    "{operation}/{fail_after:?}"
                );
                original.ensure_sync_ready().unwrap();
                let reopened = Vault::open(dir.path(), &SecretString::new("master")).unwrap();
                let envelope: ObjectEnvelope = read_json(reopened.record_path(id)).unwrap();
                assert_eq!(
                    reopened.decrypt_envelope_bytes(&envelope).unwrap(),
                    b"private-data"
                );
                assert!(reopened
                    .list_devices()
                    .unwrap()
                    .iter()
                    .all(|d| d.trusted && d.revoked_at.is_none()));
                assert!(Vault::open(dir.path(), &SecretString::new("new-master")).is_err());
                // A failed password attempt must not remain in memory and
                // become committed by the next unrelated epoch rotation.
                if fail_after.is_none() {
                    fs::remove_file(dir.path().join("audit/damaged.aipaudit")).unwrap();
                }
                original.advance_epoch_and_rewrap("retry").unwrap();
                assert!(Vault::open(dir.path(), &SecretString::new("master")).is_ok());
                assert!(Vault::recover_master_password_with_kdf(
                    dir.path(),
                    &recovery,
                    &SecretString::new("recovered"),
                    KdfParams::with_random_salt(1024, 1, 1)
                )
                .is_ok());
            }
        }
    }

    #[test]
    fn interrupted_password_rotation_recovers_records_and_new_password_together() {
        let dir = tempdir().unwrap();
        let mut original = vault(dir.path());
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
        let mut header = original.header.clone();
        original
            .rewrap_root_key_for_new_password(
                &mut header,
                &SecretString::new("new-master"),
                KdfParams::with_random_salt(1024, 1, 1),
            )
            .unwrap();
        let snapshot = original
            .prepare_epoch_rotation(header, "password", None)
            .unwrap();
        assert!(!String::from_utf8_lossy(&snapshot).contains("private-data"));
        atomic_write_bytes(dir.path().join(PENDING), &snapshot).unwrap();
        FAIL_AFTER.with(|counter| counter.set(Some(1)));
        assert!(original.finish_pending_sync().is_err());
        assert!(original.ensure_sync_ready().is_err());
        drop(original);
        let recovered = Vault::open(dir.path(), &SecretString::new("master")).unwrap();
        let envelope: ObjectEnvelope = read_json(recovered.record_path(id)).unwrap();
        assert_eq!(
            recovered.decrypt_envelope_bytes(&envelope).unwrap(),
            b"private-data"
        );
        recovered.ensure_sync_ready().unwrap();
        assert_eq!(recovered.current_epoch().epoch, 1);
        assert!(Vault::open(dir.path(), &SecretString::new("master")).is_err());
        assert!(Vault::open(dir.path(), &SecretString::new("new-master")).is_ok());
    }

    #[test]
    fn encrypted_backup_preserves_local_pricing_without_synchronizing_it() {
        let source = tempdir().unwrap();
        let target = tempdir().unwrap();
        let original = vault(source.path());
        let plaintext = br#"{"groups":[{"id":"custom","name":"Private prices"}],"assignments":[{"entryId":"provider","groupId":"custom","multiplier":1.5}]}"#;
        let payload = original
            .encrypt_local_state("proxy-pricing", plaintext)
            .unwrap();
        let bytes =
            serde_json::to_vec(&serde_json::json!({"version":1,"payload":payload})).unwrap();
        fs::write(source.path().join("pricing.aipstate"), &bytes).unwrap();
        let sync = original.export_sync_snapshot().unwrap();
        let (_, files) = original.decode_snapshot(&sync).unwrap();
        assert!(!files
            .files
            .iter()
            .any(|file| file.relative_path == Path::new("pricing.aipstate")));
        let password = SecretString::new("export");
        let backup = original
            .export_encrypted_with_kdf(&password, KdfParams::with_random_salt(1024, 1, 1))
            .unwrap();
        Vault::import_encrypted(target.path(), &password, &backup).unwrap();
        assert_eq!(
            fs::read(target.path().join("pricing.aipstate")).unwrap(),
            bytes
        );
        let restored = Vault::open(target.path(), &SecretString::new("master")).unwrap();
        assert_eq!(
            restored
                .decrypt_local_state("proxy-pricing", &payload)
                .unwrap(),
            plaintext
        );
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
