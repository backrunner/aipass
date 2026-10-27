//! Commit native tool writes and the corresponding uninstall receipt.
use super::*;
pub(super) struct Prepared {
    pub(super) title: String,
    pub(super) identity: Option<String>,
    pub(super) selected: Option<String>,
    pub(super) route_id: Option<Uuid>,
    pub(super) markers: aipass_config_writers::proxy_recovery::Markers,
    pub(super) plan: ConfigPlan,
    pub(super) content: String,
}
pub(super) fn apply_prepared(
    state: &Arc<AgentState>,
    vault: &Vault,
    request: &ToolConfigRequest,
    prepared: Prepared,
) -> ServiceResult<ToolConfigApplyResponse> {
    let Prepared {
        title,
        identity,
        selected,
        route_id,
        markers,
        plan,
        content,
    } = prepared;
    let content = Zeroizing::new(content);
    let home = crate::server::home_dir(vault)?;
    let active = active_store(&home, &request.tool).map_err(safe_error)?;
    let key = Zeroizing::new(vault.config_backup_key());
    let mut recovery =
        crate::proxy_recovery::WriteGuard::begin(&home, route_id.is_some()).map_err(safe_error)?;
    let mut refs = Vec::new();
    let mut changes = Vec::new();
    archive_outgoing(vault, request, &active, &home, &mut changes, &mut refs)
        .map_err(safe_error)?;
    let source = if request.mode == ToolConfigMode::Official {
        Some(source_store(vault, request, &home).map_err(safe_error)?)
    } else {
        None
    };
    if let Some(source) = &source {
        let expected = identity
            .as_deref()
            .ok_or_else(|| safe_error(anyhow::anyhow!("native account identity unavailable")))?;
        if source.identity().map_err(safe_error)?.as_deref() != Some(expected) {
            return Err(ServiceError::new(
                AgentErrorCode::Conflict,
                "native account changed",
            ));
        }
        copy_store(source, &active, &mut changes).map_err(safe_error)?;
        let before = reference(vault, request.id, &request.tool).map_err(safe_error)?;
        let mut after = before.clone();
        after["nativeHome"] = json!(active.home);
        refs.push(RefUpdate {
            id: request.id,
            provider: if request.tool == ToolConfigTool::Codex {
                "codex"
            } else {
                "anthropic"
            }
            .into(),
            before,
            after,
        });
    } else {
        for resource in active.resources() {
            changes.push(Change::unchanged(resource).map_err(safe_error)?);
        }
    }
    changes.push(
        Change::new(
            Resource::File(plan.target_path.clone()),
            Some(content.as_bytes().to_vec()),
        )
        .map_err(safe_error)?,
    );
    for write in &plan.extra_writes {
        // For keyring Codex API auth, use its existing secure backend.
        let resource = if write.target_path == active.home.join("auth.json") {
            active.primary.clone()
        } else {
            Resource::File(write.target_path.clone())
        };
        replace_change(
            &mut changes,
            Change::new(resource, Some(write.content.as_bytes().to_vec())).map_err(safe_error)?,
        );
    }
    // Native/auto backend selection must stay consistent with actual writes.
    preserve_codex_backend(&active, &plan, &mut changes).map_err(safe_error)?;
    let mut config_resources = std::iter::once(Resource::File(plan.target_path.clone()))
        .chain(
            plan.extra_writes
                .iter()
                .filter(|w| w.target_path != active.home.join("auth.json"))
                .map(|w| Resource::File(w.target_path.clone())),
        )
        .collect::<Vec<_>>();
    if request.mode != ToolConfigMode::Official {
        config_resources.extend(active.resources());
        config_resources.sort_by_key(|r| r.label());
        config_resources.dedup();
    }
    // Save binding in the same encrypted transaction; calculate postimage fingerprint in memory.
    let mut saved_request = request.clone();
    saved_request.preview_id = None;
    if saved_request.secret_id.is_none() {
        saved_request.secret_id = selected;
    }
    let config_fingerprint = fingerprint_after(
        &config_resources,
        &changes,
        &key,
        &context(&saved_request).map_err(safe_error)?,
    )
    .map_err(safe_error)?;
    let binding = Binding {
        route_id,
        title: title.clone(),
        request: saved_request,
        operation_id: plan.operation_id,
        identity: if source.is_some() {
            identity.clone()
        } else {
            None
        },
        config_resources,
        config_fingerprint,
    };
    changes.push(
        Change::new(
            Resource::File(binding_path(state, &request.tool)),
            Some(encode_binding(&binding, &key).map_err(safe_error)?),
        )
        .map_err(safe_error)?,
    );
    dedup_changes(&mut changes).map_err(safe_error)?;
    if route_id.is_some() {
        if let Some(recovery) = &mut recovery {
            let owned: Vec<_> = std::iter::once(Resource::File(plan.target_path.clone()))
                .chain(
                    plan.extra_writes
                        .iter()
                        .map(|w| Resource::File(w.target_path.clone())),
                )
                .chain(active.resources())
                .collect();
            let mut writes: Vec<_> = changes
                .iter()
                .filter(|c| c.before != c.after && owned.contains(&c.resource))
                .map(|c| Change {
                    resource: c.resource.clone(),
                    before: c.before.clone(),
                    after: c.after.clone(),
                })
                .collect();
            // Internal bindings must disappear with the integration; restoring
            // an old binding would claim ownership of the user's restored CLI.
            if let Some(binding) = changes
                .iter()
                .find(|c| c.resource == Resource::File(binding_path(state, &request.tool)))
            {
                writes.push(Change {
                    resource: binding.resource.clone(),
                    before: binding.before.clone(),
                    after: binding.after.clone(),
                });
            }
            let mut sources = Vec::new();
            for update in &refs {
                if update.before["nativeHome"].as_str()
                    == Some(active.home.to_string_lossy().as_ref())
                {
                    if let Some(path) = update.after["nativeHome"]
                        .as_str()
                        .filter(|p| *p != active.home.to_string_lossy())
                    {
                        sources.push(aipass_config_writers::proxy_recovery::NativeSource {
                            active: active.clone(),
                            archived: archived_store(&active, Path::new(path))
                                .map_err(safe_error)?,
                            identity: active.identity().map_err(safe_error)?.ok_or_else(|| {
                                safe_error(anyhow::anyhow!("original account identity unavailable"))
                            })?,
                        });
                    }
                }
            }
            recovery
                .record(
                    &state.vault_dir,
                    plan.tool.clone(),
                    writes,
                    sources,
                    markers,
                )
                .map_err(safe_error)?;
        }
    }
    let mut tx = Transaction {
        operation_id: plan.operation_id,
        committed: false,
        changes,
        metadata: serde_json::to_value(&refs).map_err(safe_error)?,
    };
    let backup = tx.save(&root(state), &key).map_err(safe_error)?;
    if let Err(error) = tx
        .apply()
        .map_err(safe_error)
        .and_then(|_| set_references(vault, &refs, true))
    {
        tx.restore().map_err(safe_error)?;
        set_references(vault, &refs, false)?;
        std::fs::remove_file(&backup).map_err(safe_error)?;
        return Err(error);
    }
    tx.committed = true;
    if let Err(e) = tx.save(&root(state), &key) {
        tx.restore().map_err(safe_error)?;
        set_references(vault, &refs, false)?;
        return Err(safe_error(e));
    }
    if route_id.is_some() {
        if let Some(recovery) = &mut recovery {
            recovery.commit();
        }
    }
    let retirement_failed = route_id.is_none()
        && recovery.as_mut().is_some_and(|r| {
            r.retire(
                &plan,
                &active
                    .resources()
                    .into_iter()
                    .chain(std::iter::once(Resource::File(binding_path(
                        state,
                        &request.tool,
                    ))))
                    .collect::<Vec<_>>(),
            )
            .is_err()
        });
    if retirement_failed {
        crate::logging::write_component_log(
            crate::logging::AGENT_LOG,
            "WARN",
            "event=proxy.uninstall.retirement_pending",
        );
    }
    // A cache error cannot turn committed credentials into a replayable failure.
    if refresh_cache(state, vault, &refs).is_err() {
        crate::logging::write_component_log(
            crate::logging::AGENT_LOG,
            "WARN",
            "event=tool.switch.proxy_refresh_failed",
        );
    }
    crate::logging::write_component_log(
        crate::logging::AGENT_LOG,
        "INFO",
        &format!(
            "event=tool.switch.committed operation_id={} tool={} entry_id={}",
            tx.operation_id,
            tool_name(&request.tool),
            request.id
        ),
    );
    Ok(ToolConfigApplyResponse {
        tool: request.tool.clone(),
        mode: request.mode.clone(),
        entry_id: request.id,
        entry_title: title,
        operation_id: tx.operation_id,
        target_path: plan.target_path.display().to_string(),
        backup_path: backup.display().to_string(),
        summary: plan.summary,
        outcome: ToolConfigOutcome::Applied,
        message: Some(if retirement_failed { "Configuration applied; proxy restoration needs attention. Remove remaining proxy entries or retry this configuration before uninstalling." } else { "Restart the CLI to use the selected credentials" }.into()),
    })
}
