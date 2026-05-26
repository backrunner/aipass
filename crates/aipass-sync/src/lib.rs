mod local;
mod webdav;

pub use local::{
    accept_conflict, discard_conflict, get_conflict, hash_file, list_conflicts, list_sync_files,
    list_webdav_sync_files, sync_local_folder, sync_server_visibility_scan, sync_webdav,
    ConflictRecord, SyncCheckpoint, SyncObject, SyncReport, SyncStatus,
};
pub use webdav::{classify_webdav_error, HttpWebDavClient, WebDavClient, WebDavEntry};

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use tempfile::tempdir;
    use time::OffsetDateTime;
    use uuid::Uuid;

    #[derive(Default)]
    struct MemoryWebDav {
        files: Mutex<std::collections::BTreeMap<String, (Vec<u8>, String)>>,
    }

    impl MemoryWebDav {
        fn insert(&self, path: &str, bytes: Vec<u8>) {
            let hash = crate::local::hash_bytes(&bytes);
            self.files
                .lock()
                .unwrap()
                .insert(path.to_string(), (bytes, format!("\"{hash}\"")));
        }

        fn contains_secret(&self, secret: &str) -> bool {
            self.files
                .lock()
                .unwrap()
                .values()
                .any(|(bytes, _)| String::from_utf8_lossy(bytes).contains(secret))
        }
    }

    impl WebDavClient for MemoryWebDav {
        fn list(&self, prefix: &str) -> anyhow::Result<Vec<WebDavEntry>> {
            let prefix = prefix.trim_matches('/');
            Ok(self
                .files
                .lock()
                .unwrap()
                .iter()
                .filter(|(path, _)| path.starts_with(&format!("{prefix}/")))
                .map(|(path, (bytes, etag))| WebDavEntry {
                    path: path.clone(),
                    etag: Some(etag.clone()),
                    len: bytes.len() as u64,
                })
                .collect())
        }

        fn get(&self, path: &str) -> anyhow::Result<Vec<u8>> {
            self.files
                .lock()
                .unwrap()
                .get(path)
                .map(|(bytes, _)| bytes.clone())
                .with_context(|| format!("{path} missing"))
        }

        fn put(
            &self,
            path: &str,
            bytes: &[u8],
            _etag: Option<&str>,
        ) -> anyhow::Result<Option<String>> {
            let etag = format!("\"{}\"", crate::local::hash_bytes(bytes));
            self.files
                .lock()
                .unwrap()
                .insert(path.to_string(), (bytes.to_vec(), etag.clone()));
            Ok(Some(etag))
        }

        fn delete(&self, path: &str, _etag: Option<&str>) -> anyhow::Result<()> {
            self.files.lock().unwrap().remove(path);
            Ok(())
        }
    }

    fn encrypted_object(object_id: Uuid, object_type: &str, lamport: u64, payload: &str) -> String {
        serde_json::json!({
            "format": "aipass-object",
            "version": 1,
            "vaultId": Uuid::new_v4(),
            "objectId": object_id,
            "objectType": object_type,
            "schemaVersion": 1,
            "cryptoVersion": 1,
            "deviceId": Uuid::new_v4(),
            "lamport": lamport,
            "updatedAt": OffsetDateTime::now_utc(),
            "wrappedDek": { "epoch": 1, "key_id": Uuid::new_v4(), "nonce_b64": "n", "ciphertext_b64": payload },
            "payload": { "aead": "xchacha20poly1305", "nonce_b64": "n", "ciphertext_b64": payload }
        })
        .to_string()
    }

    #[test]
    fn syncs_only_encrypted_object_families_and_visibility_scan_works() {
        let vault = tempdir().unwrap();
        let sync = tempdir().unwrap();
        let id = Uuid::new_v4();
        fs::create_dir_all(vault.path().join("objects")).unwrap();
        fs::create_dir_all(vault.path().join("grants")).unwrap();
        fs::write(
            vault.path().join("objects").join("a.aipobj"),
            encrypted_object(id, "provider_entry", 1, "ciphertext"),
        )
        .unwrap();
        fs::write(
            vault.path().join("grants").join("g.aipgrant"),
            encrypted_object(Uuid::new_v4(), "ttl_grant", 1, "grantcipher"),
        )
        .unwrap();
        fs::write(vault.path().join("objects").join("ignored.txt"), "secret").unwrap();
        let report = sync_local_folder(vault.path(), sync.path()).unwrap();
        assert_eq!(report.uploaded, 2);
        assert!(sync.path().join("objects").join("a.aipobj").exists());
        assert!(sync.path().join("grants").join("g.aipgrant").exists());
        let matches = sync_server_visibility_scan(sync.path(), &["secret"]).unwrap();
        assert!(matches.is_empty());
    }

    #[test]
    fn same_lamport_different_hash_becomes_conflict() {
        let vault = tempdir().unwrap();
        let sync = tempdir().unwrap();
        let id = Uuid::new_v4();
        fs::create_dir_all(vault.path().join("objects")).unwrap();
        fs::create_dir_all(sync.path().join("objects")).unwrap();
        fs::write(
            vault.path().join("objects").join("a.aipobj"),
            encrypted_object(id, "provider_entry", 7, "local"),
        )
        .unwrap();
        fs::write(
            sync.path().join("objects").join("a.aipobj"),
            encrypted_object(id, "provider_entry", 7, "remote"),
        )
        .unwrap();
        let report = sync_local_folder(vault.path(), sync.path()).unwrap();
        assert_eq!(report.status, SyncStatus::Conflict);
        assert_eq!(report.conflicts, 1);
        assert_eq!(report.quarantined, 1);
        let conflicts = list_conflicts(sync.path()).unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].origin, "local");
        assert_eq!(conflicts[0].target_path, PathBuf::from("objects/a.aipobj"));
        accept_conflict(sync.path(), &conflicts[0].conflict_path).unwrap();
        assert!(list_conflicts(sync.path()).unwrap().is_empty());
        assert!(
            fs::read_to_string(sync.path().join("objects").join("a.aipobj"))
                .unwrap()
                .contains("local")
        );
    }

    #[test]
    fn discard_conflict_keeps_target_version() {
        let vault = tempdir().unwrap();
        let sync = tempdir().unwrap();
        let id = Uuid::new_v4();
        fs::create_dir_all(vault.path().join("objects")).unwrap();
        fs::create_dir_all(sync.path().join("objects")).unwrap();
        fs::write(
            vault.path().join("objects").join("a.aipobj"),
            encrypted_object(id, "provider_entry", 7, "local"),
        )
        .unwrap();
        fs::write(
            sync.path().join("objects").join("a.aipobj"),
            encrypted_object(id, "provider_entry", 7, "remote"),
        )
        .unwrap();

        let report = sync_local_folder(vault.path(), sync.path()).unwrap();
        assert_eq!(report.status, SyncStatus::Conflict);
        let conflicts = list_conflicts(sync.path()).unwrap();
        assert_eq!(conflicts.len(), 1);
        discard_conflict(sync.path(), &conflicts[0].conflict_path).unwrap();
        assert!(list_conflicts(sync.path()).unwrap().is_empty());
        assert!(
            fs::read_to_string(sync.path().join("objects").join("a.aipobj"))
                .unwrap()
                .contains("remote")
        );
    }

    #[test]
    fn webdav_sync_uploads_encrypted_families_without_plaintext() {
        let vault = tempdir().unwrap();
        let client = MemoryWebDav::default();
        let id = Uuid::new_v4();
        fs::create_dir_all(vault.path().join("objects")).unwrap();
        fs::create_dir_all(vault.path().join("devices")).unwrap();
        fs::write(
            vault.path().join("objects").join("a.aipobj"),
            encrypted_object(id, "provider_entry", 1, "ciphertext-only"),
        )
        .unwrap();
        fs::write(
            vault.path().join("devices").join("d.aipdevice"),
            encrypted_object(Uuid::new_v4(), "device_record", 1, "device-ciphertext"),
        )
        .unwrap();
        fs::write(
            vault.path().join("objects").join("ignored.txt"),
            "sk-ant-api03-ignored-plaintext",
        )
        .unwrap();

        let report = sync_webdav(vault.path(), &client).unwrap();
        assert_eq!(report.uploaded, 2);
        assert!(!client.contains_secret("sk-ant-api03-ignored-plaintext"));
        assert!(client
            .files
            .lock()
            .unwrap()
            .contains_key("objects/a.aipobj"));
        assert!(client
            .files
            .lock()
            .unwrap()
            .contains_key("devices/d.aipdevice"));
    }

    #[test]
    fn webdav_same_lamport_different_hash_is_quarantined() {
        let vault = tempdir().unwrap();
        let client = MemoryWebDav::default();
        let id = Uuid::new_v4();
        fs::create_dir_all(vault.path().join("objects")).unwrap();
        fs::write(
            vault.path().join("objects").join("a.aipobj"),
            encrypted_object(id, "provider_entry", 9, "local"),
        )
        .unwrap();
        client.insert(
            "objects/a.aipobj",
            encrypted_object(id, "provider_entry", 9, "remote").into_bytes(),
        );

        let report = sync_webdav(vault.path(), &client).unwrap();
        assert_eq!(report.status, SyncStatus::Conflict);
        assert_eq!(report.quarantined, 1);
        let conflicts = list_conflicts(vault.path()).unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].origin, "remote");
        assert_eq!(conflicts[0].target_path, PathBuf::from("objects/a.aipobj"));
    }

    #[test]
    fn propfind_parser_normalizes_webdav_href_to_relative_paths() {
        let xml = r#"<?xml version="1.0"?>
<D:multistatus xmlns:D="DAV:">
  <D:response>
    <D:href>/remote/aipass/objects/</D:href>
    <D:propstat>
      <D:prop><D:getetag>"etag-1"</D:getetag><D:getcontentlength>1</D:getcontentlength></D:prop>
    </D:propstat>
  </D:response>
</D:multistatus>"#;
        let entries = crate::webdav::parse_propfind_response(xml, "remote/aipass").unwrap();
        assert_eq!(entries[0].path, "objects");
        assert_eq!(entries[0].etag.as_deref(), Some("\"etag-1\""));
    }
}
