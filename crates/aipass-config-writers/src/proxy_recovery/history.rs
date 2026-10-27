//! A direct configuration retires its proxy ownership, but its ordinary backup
//! can later restore that proxy. Keep the matching recovery chain encrypted.
use super::*;

#[derive(Serialize, Deserialize)]
pub(super) struct Retired {
    owner: PathBuf,
    operation_id: Uuid,
    steps: Vec<Step>,
}

fn select(step: &Step, resources: &[Resource]) -> Step {
    Step {
        owner: step.owner.clone(),
        vault: step.vault.clone(),
        tool: step.tool.clone(),
        legacy: step.legacy,
        markers: step.markers.clone(),
        sources: step.sources.clone(),
        changes: step
            .changes
            .iter()
            .filter(|c| resources.contains(&c.resource))
            .map(|c| Change {
                resource: c.resource.clone(),
                before: c.before.clone(),
                after: c.after.clone(),
            })
            .collect(),
    }
}

impl Recovery {
    pub(super) fn discard_retired(&mut self, owner: &Path, changes: &[Change]) {
        for backup in &mut self.receipt.retired {
            if backup.owner == owner {
                for step in &mut backup.steps {
                    step.changes
                        .retain(|c| !changes.iter().any(|w| w.resource == c.resource));
                }
                backup.steps.retain(|s| !s.changes.is_empty());
            }
        }
        self.receipt.retired.retain(|b| !b.steps.is_empty());
    }

    pub fn retire(&mut self, owner: &Path, tool: &ToolId, resources: &[Resource]) -> Result<()> {
        self.retire_inner(owner, tool, resources, None, &[])
    }

    pub fn retire_for_operation(
        &mut self,
        owner: &Path,
        tool: &ToolId,
        resources: &[Resource],
        operation_id: Uuid,
        credentials: &[Resource],
    ) -> Result<()> {
        self.retire_inner(owner, tool, resources, Some(operation_id), credentials)
    }

    fn retire_inner(
        &mut self,
        owner: &Path,
        tool: &ToolId,
        resources: &[Resource],
        operation_id: Option<Uuid>,
        credentials: &[Resource],
    ) -> Result<()> {
        self.ensure_writable()?;
        let mut retired = Vec::new();
        for step in &self.receipt.steps {
            if step.owner == owner && &step.tool == tool {
                let selected = select(step, resources);
                if !selected.changes.is_empty() {
                    retired.push(selected);
                }
            }
        }
        // Direct writers can preserve dormant proxy entries in shared JSON or
        // TOML files. Retire only the fields intentionally changed by the direct
        // write; keep reversal ownership for those unchanged proxy entries.
        let mut remaining: Vec<Change> = Vec::new();
        let mut unresolved_legacy = Vec::new();
        for step in retired.iter().filter(|s| s.legacy) {
            for change in &step.changes {
                if change
                    .resource
                    .read()?
                    .as_deref()
                    .is_some_and(|raw| step.markers.contains_bytes(raw))
                {
                    unresolved_legacy.push(change.resource.clone());
                }
            }
        }
        for step in retired.iter().rev().filter(|s| !s.legacy) {
            for change in &step.changes {
                if credentials.contains(&change.resource) {
                    continue;
                }
                let index = match remaining.iter().position(|c| c.resource == change.resource) {
                    Some(index) => index,
                    None => {
                        remaining.push(Change::unchanged(change.resource.clone())?);
                        remaining.len() - 1
                    }
                };
                let current = &mut remaining[index];
                let path = match &change.resource {
                    Resource::File(path) => Some(path.as_path()),
                    _ => None,
                };
                let restored = merge::retire(
                    path,
                    change.before.as_deref(),
                    change.after.as_deref(),
                    current.after.as_deref(),
                    &step.markers,
                )?;
                current.after.zeroize();
                current.after = restored.map(|v| v.to_vec());
            }
        }
        remaining.retain(|c| c.before != c.after);
        for change in &mut remaining {
            std::mem::swap(&mut change.before, &mut change.after);
        }
        let remaining = retired
            .first()
            .filter(|_| !remaining.is_empty())
            .map(|step| Step {
                owner: owner.to_owned(),
                vault: step.vault.clone(),
                tool: tool.clone(),
                changes: remaining,
                sources: Vec::new(),
                legacy: false,
                markers: {
                    let mut markers = Markers::default();
                    for step in &retired {
                        markers.extend(&step.markers);
                    }
                    markers
                },
            });
        let writes: Vec<_> = resources
            .iter()
            .cloned()
            .map(|resource| Change {
                resource,
                before: None,
                after: None,
            })
            .collect();
        // Config backups retain only the latest operation for each target.
        self.discard_retired(owner, &writes);
        if let Some(operation_id) = operation_id {
            if !retired.is_empty() {
                self.receipt.retired.push(Retired {
                    owner: owner.to_owned(),
                    operation_id,
                    steps: retired,
                });
            }
        }
        for step in &mut self.receipt.steps {
            if step.owner == owner && &step.tool == tool {
                step.changes.retain(|c| {
                    !resources.contains(&c.resource)
                        || (step.legacy && unresolved_legacy.contains(&c.resource))
                });
            }
        }
        self.receipt.steps.retain(|s| !s.changes.is_empty());
        if let Some(step) = remaining {
            self.receipt.steps.push(step);
        }
        self.save()?;
        ensure!(
            unresolved_legacy.is_empty(),
            "legacy proxy entries remain; remove them before uninstalling"
        );
        Ok(())
    }

    /// Persist ownership before rollback touches a config. Interrupted rollback
    /// then remains recoverable or fails closed, even with the vault locked.
    pub fn reactivate(&mut self, owner: &Path, operation_id: Uuid) -> Result<()> {
        self.ensure_writable()?;
        self.ensure_attached(owner)?;
        let Some(index) = self
            .receipt
            .retired
            .iter()
            .position(|b| b.owner == owner && b.operation_id == operation_id)
        else {
            return Ok(());
        };
        for step in &self.receipt.retired[index].steps {
            ensure!(
                !self.receipt.steps.iter().any(|active| active.owner != owner
                    && active
                        .changes
                        .iter()
                        .any(|a| step.changes.iter().any(|b| a.resource == b.resource))),
                "another AIPass installation owns the rollback configuration"
            );
        }
        let backup = self.receipt.retired.remove(index);
        for active in &mut self.receipt.steps {
            if active.owner == owner {
                active.changes.retain(|a| {
                    !backup
                        .steps
                        .iter()
                        .any(|s| s.changes.iter().any(|b| a.resource == b.resource))
                });
            }
        }
        self.receipt.steps.retain(|s| !s.changes.is_empty());
        self.receipt.steps.extend(backup.steps);
        self.save()
    }
}
