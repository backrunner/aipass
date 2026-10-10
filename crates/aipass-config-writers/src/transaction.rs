//! Recoverable encrypted transactions spanning files and native credential stores.
use crate::native_auth::Resource;
use aipass_crypto::{decrypt_bytes, encrypt_bytes, Ciphertext};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

#[derive(Serialize, Deserialize)]
pub struct Change {
    pub resource: Resource,
    pub before: Option<Vec<u8>>,
    pub after: Option<Vec<u8>>,
}
impl Drop for Change {
    fn drop(&mut self) {
        self.before.zeroize();
        self.after.zeroize();
    }
}
impl Change {
    pub fn new(resource: Resource, after: Option<Vec<u8>>) -> Result<Self> {
        let before = resource.read()?.map(|v| v.to_vec());
        Ok(Self {
            resource,
            before,
            after,
        })
    }
    pub fn unchanged(resource: Resource) -> Result<Self> {
        let before = resource.read()?.map(|v| v.to_vec());
        let after = before.clone();
        Ok(Self {
            resource,
            before,
            after,
        })
    }
}
#[derive(Serialize, Deserialize)]
pub struct Transaction {
    pub operation_id: Uuid,
    pub committed: bool,
    pub changes: Vec<Change>,
    pub metadata: Value,
}
#[derive(Serialize, Deserialize)]
struct Envelope {
    version: u16,
    operation_id: Uuid,
    ciphertext: Ciphertext,
}

pub fn fingerprint(resources: &[Resource], key: &[u8; 32], context: &[u8]) -> Result<String> {
    let mut hash = Sha256::new();
    hash.update(key);
    hash.update(context);
    for resource in resources {
        hash.update(serde_json::to_vec(resource)?);
        match resource.read()? {
            Some(v) => {
                hash.update([1]);
                hash.update((v.len() as u64).to_le_bytes());
                hash.update(&v);
            }
            None => hash.update([0]),
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}
impl Transaction {
    pub fn path(root: &Path, id: Uuid) -> PathBuf {
        root.join(format!("{id}.aiptransaction"))
    }
    pub fn save(&self, root: &Path, key: &[u8; 32]) -> Result<PathBuf> {
        let path = Self::path(root, self.operation_id);
        let raw = Zeroizing::new(serde_json::to_vec(self)?);
        let aad = format!(
            "aipass-tool-transaction;v=1;operation={}",
            self.operation_id
        );
        let envelope = Envelope {
            version: 1,
            operation_id: self.operation_id,
            ciphertext: encrypt_bytes(key, aad.as_bytes(), &raw)?,
        };
        Resource::File(path.clone()).write(Some(&serde_json::to_vec(&envelope)?))?;
        Ok(path)
    }
    pub fn load(path: &Path, key: &[u8; 32]) -> Result<Self> {
        let envelope: Envelope = serde_json::from_slice(&fs::read(path)?)?;
        if envelope.version != 1
            || path.file_stem().and_then(|v| v.to_str()) != Some(&envelope.operation_id.to_string())
        {
            bail!("invalid tool transaction");
        }
        let aad = format!(
            "aipass-tool-transaction;v=1;operation={}",
            envelope.operation_id
        );
        let raw = Zeroizing::new(decrypt_bytes(key, aad.as_bytes(), &envelope.ciphertext)?);
        let value: Self = serde_json::from_slice(&raw)?;
        if value.operation_id != envelope.operation_id {
            bail!("invalid tool transaction");
        }
        Ok(value)
    }
    pub fn apply(&self) -> Result<()> {
        self.apply_with(|resource, bytes| resource.write(bytes))
    }
    fn apply_with(
        &self,
        mut write: impl FnMut(&Resource, Option<&[u8]>) -> Result<()>,
    ) -> Result<()> {
        // Validate the entire preimage before changing any resource.
        for c in &self.changes {
            if c.resource.read()?.as_deref().map(|v| v.as_slice()) != c.before.as_deref() {
                bail!("tool credentials changed externally");
            }
        }
        for c in &self.changes {
            if c.before == c.after {
                continue;
            }
            if c.resource.read()?.as_deref().map(|v| v.as_slice()) != c.before.as_deref() {
                bail!("tool credentials changed externally");
            }
            if let Err(e) = write(&c.resource, c.after.as_deref()) {
                self.restore()
                    .context("tool rollback incomplete; encrypted recovery retained")?;
                return Err(e);
            }
        }
        Ok(())
    }
    pub fn restore(&self) -> Result<()> {
        self.restore_with(|resource, bytes| resource.write(bytes))
    }
    fn restore_with(
        &self,
        mut write: impl FnMut(&Resource, Option<&[u8]>) -> Result<()>,
    ) -> Result<()> {
        // An external writer must never be silently overwritten, including recovery.
        for c in &self.changes {
            let current = c.resource.read()?;
            let current = current.as_deref().map(|v| v.as_slice());
            if current != c.before.as_deref() && current != c.after.as_deref() {
                bail!("tool credentials changed externally; recovery requires retry");
            }
        }
        for c in self.changes.iter().rev() {
            if c.before != c.after {
                let current = c.resource.read()?;
                let current = current.as_deref().map(|v| v.as_slice());
                if current != c.before.as_deref() && current != c.after.as_deref() {
                    bail!("tool credentials changed externally; recovery requires retry");
                }
                if current != c.before.as_deref() {
                    write(&c.resource, c.before.as_deref())?;
                }
            }
        }
        Ok(())
    }
    pub fn reverse(&self) -> Result<Vec<Change>> {
        self.changes
            .iter()
            .map(|c| Change::new(c.resource.clone(), c.before.clone()))
            .collect()
    }
}
pub fn pending(root: &Path, key: &[u8; 32]) -> Result<Vec<Transaction>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.extension().and_then(|v| v.to_str()) == Some("aiptransaction") {
            let tx = Transaction::load(&path, key)?;
            if !tx.committed {
                result.push(tx);
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encrypted_transaction_restores_absence_and_refuses_external_writes() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("auth.json");
        let tx = Transaction {
            operation_id: Uuid::new_v4(),
            committed: false,
            changes: vec![Change::new(
                Resource::File(file.clone()),
                Some(b"fake-refresh-token".to_vec()),
            )
            .unwrap()],
            metadata: Value::Null,
        };
        let backup = tx.save(dir.path(), &[7; 32]).unwrap();
        assert!(
            !String::from_utf8_lossy(&fs::read(&backup).unwrap()).contains("fake-refresh-token")
        );
        tx.apply().unwrap();
        let recovered = Transaction::load(&backup, &[7; 32]).unwrap();
        recovered.restore().unwrap();
        assert!(!file.exists());
        tx.apply().unwrap();
        fs::write(&file, "external-token").unwrap();
        assert!(recovered.restore().is_err());
        assert_eq!(fs::read_to_string(file).unwrap(), "external-token");
        assert!(Transaction::load(&backup, &[8; 32]).is_err());
    }
    #[test]
    fn partial_file_or_keychain_write_failure_restores_all_preimages() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("config");
        let b = dir.path().join("credential");
        fs::write(&a, "original").unwrap();
        let tx = Transaction {
            operation_id: Uuid::new_v4(),
            committed: false,
            metadata: Value::Null,
            changes: vec![
                Change::new(Resource::File(a.clone()), Some(b"new-config".to_vec())).unwrap(),
                Change::new(Resource::File(b.clone()), Some(b"fake-token".to_vec())).unwrap(),
            ],
        };
        tx.save(dir.path(), &[7; 32]).unwrap();
        let error = tx
            .apply_with(|r, bytes| {
                if *r == Resource::File(b.clone()) {
                    bail!("system credential write denied")
                } else {
                    r.write(bytes)
                }
            })
            .unwrap_err();
        assert!(error.to_string().contains("denied"));
        assert_eq!(fs::read(a).unwrap(), b"original");
        assert!(!b.exists());
    }
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "opt-in macOS Keychain fixture; writes and removes a uniquely named test item"]
    fn isolated_macos_keychain_transaction_roundtrip() {
        let directory = tempfile::tempdir().unwrap();
        let item = Resource::Keychain {
            service: format!("AIPass-switch-fixture-{}", Uuid::new_v4()),
            account: "fixture".into(),
        };
        struct Cleanup(Resource);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = self.0.write(None);
            }
        }
        let _cleanup = Cleanup(item.clone());
        assert!(item.read().unwrap().is_none());
        let tx = Transaction {
            operation_id: Uuid::new_v4(),
            committed: false,
            metadata: Value::Null,
            changes: vec![
                Change::new(
                    Resource::File(directory.path().join("config")),
                    Some(b"selected".to_vec()),
                )
                .unwrap(),
                Change::new(item.clone(), Some(b"fake-keychain-grant".to_vec())).unwrap(),
            ],
        };
        let backup = tx.save(directory.path(), &[9; 32]).unwrap();
        assert!(
            !String::from_utf8_lossy(&fs::read(&backup).unwrap()).contains("fake-keychain-grant")
        );
        tx.apply().unwrap();
        assert_eq!(
            item.read().unwrap().unwrap().as_slice(),
            b"fake-keychain-grant"
        );
        Transaction::load(&backup, &[9; 32])
            .unwrap()
            .restore()
            .unwrap();
        assert!(item.read().unwrap().is_none());
        assert!(!directory.path().join("config").exists());
    }
    #[test]
    fn preimage_conflict_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        fs::write(&a, b"old").unwrap();
        let tx = Transaction {
            operation_id: Uuid::new_v4(),
            committed: false,
            metadata: Value::Null,
            changes: vec![
                Change::new(Resource::File(a.clone()), Some(b"new".to_vec())).unwrap(),
                Change::new(Resource::File(b.clone()), Some(b"new".to_vec())).unwrap(),
            ],
        };
        fs::write(b, b"external").unwrap();
        assert!(tx.apply().is_err());
        assert_eq!(fs::read(a).unwrap(), b"old");
    }

    #[test]
    fn recovery_rechecks_each_resource_after_an_external_write() {
        let dir = tempfile::tempdir().unwrap();
        let a = Resource::File(dir.path().join("a"));
        let b = Resource::File(dir.path().join("b"));
        a.write(Some(b"old-a")).unwrap();
        b.write(Some(b"old-b")).unwrap();
        let tx = Transaction {
            operation_id: Uuid::new_v4(),
            committed: false,
            metadata: Value::Null,
            changes: vec![
                Change::new(a.clone(), Some(b"new-a".to_vec())).unwrap(),
                Change::new(b.clone(), Some(b"new-b".to_vec())).unwrap(),
            ],
        };
        tx.apply().unwrap();
        assert!(tx
            .restore_with(|resource, bytes| {
                resource.write(bytes)?;
                a.write(Some(b"external-login"))?;
                Ok(())
            })
            .is_err());
        assert_eq!(a.read().unwrap().unwrap().as_slice(), b"external-login");
        assert_eq!(b.read().unwrap().unwrap().as_slice(), b"old-b");
    }
}
