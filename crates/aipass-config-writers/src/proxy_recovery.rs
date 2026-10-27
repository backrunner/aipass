//! Device-local, encrypted uninstall receipts. No vault key is stored here.
//! The caller holds one OS file lock across preparation and the actual write.
#[cfg(target_os = "macos")]
pub mod macos_key;
mod merge;
use crate::{
    native_auth::Resource,
    transaction::{Change, Transaction},
    ConfigPlan, ToolId,
};
use aipass_crypto::{decrypt_bytes, encrypt_bytes, Ciphertext};
use anyhow::{bail, ensure, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    fs::File,
    path::{Path, PathBuf},
};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

const AAD: &[u8] = b"aipass-proxy-uninstall;v=1";
#[derive(Serialize, Deserialize)]
struct Step {
    owner: PathBuf,
    vault: PathBuf,
    tool: ToolId,
    changes: Vec<Change>,
    #[serde(default)]
    sources: Vec<NativeSource>,
    #[serde(default)]
    legacy: bool,
}
#[derive(Serialize, Deserialize)]
pub struct NativeSource {
    pub active: crate::native_auth::NativeStore,
    pub archived: crate::native_auth::NativeStore,
    pub identity: String,
}
#[derive(Default, Serialize, Deserialize)]
struct Receipt {
    steps: Vec<Step>,
    #[serde(default)]
    vaults: Vec<(PathBuf, PathBuf)>,
    #[serde(default)]
    detached: Vec<PathBuf>,
}

pub struct Recovery {
    root: PathBuf,
    key: Zeroizing<[u8; 32]>,
    receipt: Receipt,
    _lock: File,
}

impl Recovery {
    /// Key loading is performed only after acquiring the global lock, so first
    /// writes from different vaults cannot generate competing recovery keys.
    pub fn open(
        root: &Path,
        key: impl FnOnce(bool) -> Result<Zeroizing<[u8; 32]>>,
    ) -> Result<Self> {
        let path = root.join("receipt.aiprecovery");
        crate::native_auth::ensure_private_parent(&path)?;
        let lock_path = root.join("recovery.lock");
        Resource::File(lock_path.clone()).read()?;
        let lock = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        lock.try_lock().map_err(|_| {
            anyhow::anyhow!("another configuration or uninstall operation is running")
        })?;
        let bytes = Resource::File(path).read()?;
        let key = key(bytes.is_some() || root.join("pending.aiptransaction").exists())?;
        let receipt = if let Some(bytes) = bytes {
            let envelope: Ciphertext = serde_json::from_slice(&bytes)?;
            let raw = Zeroizing::new(decrypt_bytes(&key, AAD, &envelope)?);
            serde_json::from_slice(&raw)?
        } else {
            Receipt::default()
        };
        Ok(Self {
            root: root.to_owned(),
            key,
            receipt,
            _lock: lock,
        })
    }
    fn save(&self) -> Result<()> {
        let raw = Zeroizing::new(serde_json::to_vec(&self.receipt)?);
        ensure!(
            raw.len() < 2 * 1024 * 1024,
            "proxy recovery receipt is full; restore before writing more configurations"
        );
        let envelope = encrypt_bytes(&self.key, AAD, &raw)?;
        Resource::File(self.root.join("receipt.aiprecovery"))
            .write(Some(&serde_json::to_vec(&envelope)?))
    }
    fn pending_path(&self) -> PathBuf {
        self.root.join("pending.aiptransaction")
    }
    pub fn ensure_writable(&self) -> Result<()> {
        ensure!(
            !self.pending_path().exists(),
            "unfinished uninstall recovery; retry uninstall first"
        );
        Ok(())
    }
    pub fn ensure_attached(&self, owner: &Path) -> Result<()> {
        ensure!(
            !self.receipt.detached.iter().any(|o| o == owner),
            "this installation has been detached for uninstall; configuration writes are disabled"
        );
        Ok(())
    }
    pub fn record(
        &mut self,
        owner: &Path,
        vault: &Path,
        tool: ToolId,
        changes: Vec<Change>,
    ) -> Result<()> {
        self.record_native(owner, vault, tool, changes, Vec::new())
    }
    pub fn record_native(
        &mut self,
        owner: &Path,
        vault: &Path,
        tool: ToolId,
        changes: Vec<Change>,
        sources: Vec<NativeSource>,
    ) -> Result<()> {
        self.ensure_attached(owner)?;
        ensure!(
            !self.pending_path().exists(),
            "unfinished uninstall recovery; retry uninstall first"
        );
        for step in &self.receipt.steps {
            ensure!(!(step.owner == owner && step.tool == tool && step.legacy && step.changes.iter().any(|a| changes.iter().any(|b| a.resource == b.resource))), "older proxy configuration has no verified original; switch this Agent to direct configuration before reapplying the proxy");
            if step.owner != owner
                && step
                    .changes
                    .iter()
                    .any(|a| changes.iter().any(|b| a.resource == b.resource))
            {
                bail!("another AIPass installation owns this proxy configuration");
            }
        }
        self.receipt.steps.push(Step {
            owner: owner.to_owned(),
            vault: vault.to_owned(),
            tool,
            changes,
            sources,
            legacy: false,
        });
        let registration = (owner.to_owned(), vault.to_owned());
        if !self.receipt.vaults.contains(&registration) {
            self.receipt.vaults.push(registration);
        }
        self.save()
    }
    /// Compact only a verified completed write. A prepared or interrupted write
    /// must retain its intermediate preimage for uninstall recovery.
    pub fn commit(&mut self) -> Result<()> {
        if self.receipt.steps.len() < 2 {
            return Ok(());
        }
        let last = self.receipt.steps.last().expect("recovery step");
        if !last.sources.is_empty() || last.legacy {
            return Ok(());
        }
        let previous = &self.receipt.steps[self.receipt.steps.len() - 2];
        let compact = previous.owner == last.owner
            && previous.vault == last.vault
            && previous.tool == last.tool
            && !previous.legacy
            && previous.changes.len() == last.changes.len()
            && last.changes.iter().all(|c| {
                previous
                    .changes
                    .iter()
                    .any(|old| old.resource == c.resource && old.after == c.before)
            });
        if !compact {
            return Ok(());
        }
        for change in &last.changes {
            if change.resource.read()?.as_deref().map(Vec::as_slice) != change.after.as_deref() {
                return Ok(());
            }
        }
        let mut last = self.receipt.steps.pop().expect("completed recovery step");
        let mut previous = self.receipt.steps.pop().expect("previous recovery step");
        for change in &mut last.changes {
            let old = previous
                .changes
                .iter()
                .find(|old| old.resource == change.resource)
                .expect("matching recovery resource");
            change.before.zeroize();
            change.before = old.before.clone();
        }
        last.sources = std::mem::take(&mut previous.sources);
        self.receipt.steps.push(last);
        self.save()
    }
    /// Called only after a successful intentional switch away from the proxy.
    pub fn retire(&mut self, owner: &Path, tool: &ToolId, resources: &[Resource]) -> Result<()> {
        ensure!(
            !self.pending_path().exists(),
            "unfinished uninstall recovery"
        );
        for step in &mut self.receipt.steps {
            if step.owner == owner && &step.tool == tool {
                step.changes.retain(|c| !resources.contains(&c.resource));
            }
        }
        self.receipt.steps.retain(|s| !s.changes.is_empty());
        self.save()
    }
    pub fn plan_changes(plan: &ConfigPlan, content: &str) -> Result<Vec<Change>> {
        std::iter::once((&plan.target_path, content))
            .chain(
                plan.extra_writes
                    .iter()
                    .map(|w| (&w.target_path, w.content.as_str())),
            )
            .map(|(p, c)| Change::new(Resource::File(p.clone()), Some(c.as_bytes().to_vec())))
            .collect()
    }
    pub fn vaults(&self, owner: &Path) -> Vec<PathBuf> {
        let mut result: Vec<_> = self
            .receipt
            .vaults
            .iter()
            .filter(|(o, _)| o == owner)
            .map(|(_, v)| v.clone())
            .collect();
        result.sort();
        result.dedup();
        result
    }
    pub fn tracked(&self, owner: &Path, tool: &ToolId, resources: &[Resource]) -> bool {
        self.receipt.steps.iter().any(|s| {
            s.owner == owner
                && &s.tool == tool
                && !s.legacy
                && s.changes.iter().any(|c| resources.contains(&c.resource))
        })
    }
    pub fn audit(
        &mut self,
        owner: &Path,
        vault: &Path,
        legacy: Vec<(ToolId, Vec<Resource>)>,
    ) -> Result<()> {
        self.ensure_writable()?;
        self.receipt.detached.retain(|o| o != owner);
        for (tool, resources) in legacy {
            if let Some(step) = self
                .receipt
                .steps
                .iter_mut()
                .find(|s| s.owner == owner && s.vault == vault && s.tool == tool && s.legacy)
            {
                for resource in resources {
                    if !step.changes.iter().any(|c| c.resource == resource) {
                        step.changes.push(Change {
                            resource,
                            before: None,
                            after: None,
                        });
                    }
                }
                continue;
            }
            self.receipt.steps.push(Step {
                owner: owner.to_owned(),
                vault: vault.to_owned(),
                tool,
                changes: resources
                    .into_iter()
                    .map(|resource| Change {
                        resource,
                        before: None,
                        after: None,
                    })
                    .collect(),
                sources: Vec::new(),
                legacy: true,
            });
        }
        let registration = (owner.to_owned(), vault.to_owned());
        if !self.receipt.vaults.contains(&registration) {
            self.receipt.vaults.push(registration);
        }
        self.save()
    }
    pub fn finish(&mut self, owner: &Path) -> Result<()> {
        self.receipt.vaults.retain(|(o, _)| o != owner);
        self.save()
    }
    /// Preflight the whole installation in memory before writing any file.
    /// A durable transaction makes interrupted uninstalls resumable.
    pub fn restore(&mut self, owner: &Path) -> Result<usize> {
        ensure!(!self.receipt.steps.iter().any(|s| s.owner == owner && s.legacy), "older proxy configuration has no verified original; open AIPass, switch affected Agents to direct configuration and retry uninstall");
        let pending_path = self.pending_path();
        let tx = if pending_path.exists() {
            let id: Uuid = serde_json::from_slice::<serde_json::Value>(&fs::read(&pending_path)?)?
                ["operation_id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("invalid uninstall recovery"))?
                .parse()?;
            // Transaction validates its UUID filename; retain the pending marker
            // separately from the authenticated transaction payload.
            let tx = Transaction::load(&Transaction::path(&self.root, id), &self.key)?;
            ensure!(
                tx.metadata["owner"] == serde_json::to_value(owner)?,
                "another installation has unfinished uninstall recovery"
            );
            tx
        } else {
            let mut result: Vec<Change> = Vec::new();
            for step in self.receipt.steps.iter().rev().filter(|s| s.owner == owner) {
                for c in &step.changes {
                    let index = match result.iter().position(|r| r.resource == c.resource) {
                        Some(i) => i,
                        None => {
                            result.push(Change::unchanged(c.resource.clone())?);
                            result.len() - 1
                        }
                    };
                    let r = &mut result[index];
                    let mut original = c.before.as_deref().map(|v| Zeroizing::new(v.to_vec()));
                    if r.after.as_deref() != c.before.as_deref() {
                        for source in &step.sources {
                            let resource = if c.resource == source.active.primary {
                                Some(&source.archived.primary)
                            } else if source.active.profile.as_ref() == Some(&c.resource) {
                                source.archived.profile.as_ref()
                            } else {
                                None
                            };
                            if let Some(resource) = resource {
                                let primary = source.archived.primary.read()?.ok_or_else(|| {
                                    anyhow::anyhow!("original native credential unavailable")
                                })?;
                                let profile = source
                                    .archived
                                    .profile
                                    .as_ref()
                                    .map(Resource::read)
                                    .transpose()?
                                    .flatten();
                                ensure!(
                                    source
                                        .archived
                                        .identity_from(
                                            &primary,
                                            profile.as_deref().map(Vec::as_slice)
                                        )?
                                        .as_deref()
                                        == Some(source.identity.as_str()),
                                    "original native account changed; uninstall recovery stopped"
                                );
                                original = if resource == &source.archived.primary {
                                    Some(primary)
                                } else {
                                    profile
                                };
                            }
                        }
                    }
                    let path = match &c.resource {
                        Resource::File(p) => Some(p.as_path()),
                        _ => None,
                    };
                    let restored = merge::undo(
                        path,
                        original.as_deref().map(Vec::as_slice),
                        c.after.as_deref(),
                        r.after.as_deref(),
                    )?
                    .map(|v| v.to_vec());
                    r.after.zeroize();
                    r.after = restored;
                }
            }
            // Internal native bindings describe the proxy's ownership. They
            // must be validated through the full chain, then removed rather
            // than resurrecting a stale binding for the restored vendor CLI.
            for change in &mut result {
                if matches!(&change.resource, Resource::File(p) if p.extension().is_some_and(|e| e == "aipbinding"))
                {
                    change.after = None;
                }
            }
            let tx = Transaction {
                operation_id: Uuid::new_v4(),
                committed: false,
                changes: result,
                metadata: serde_json::json!({ "owner": owner }),
            };
            tx.save(&self.root, &self.key)?;
            Resource::File(pending_path.clone()).write(Some(&serde_json::to_vec(
                &serde_json::json!({"operation_id": tx.operation_id}),
            )?))?;
            tx
        };
        // Resume either side of each write; a third value is an external edit.
        let mut changes = Vec::new();
        for c in &tx.changes {
            let current = c.resource.read()?;
            ensure!(
                current.as_deref().map(Vec::as_slice) == c.before.as_deref()
                    || current.as_deref().map(Vec::as_slice) == c.after.as_deref(),
                "configuration changed during uninstall; recovery retained"
            );
            if current.as_deref().map(Vec::as_slice) != c.after.as_deref() {
                changes.push(Change::new(c.resource.clone(), c.after.clone())?);
            }
        }
        let count = changes.len();
        Transaction {
            operation_id: Uuid::new_v4(),
            committed: false,
            changes,
            metadata: serde_json::Value::Null,
        }
        .apply()?;
        self.receipt.steps.retain(|s| s.owner != owner);
        if !self.receipt.detached.iter().any(|o| o == owner) {
            self.receipt.detached.push(owner.to_owned());
        }
        self.save()?;
        fs::remove_file(pending_path)?;
        fs::remove_file(Transaction::path(&self.root, tx.operation_id))?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests;
