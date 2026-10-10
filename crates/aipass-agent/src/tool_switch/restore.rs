use super::*;
pub(crate) fn has_backup(state: &AgentState, id: Uuid) -> bool {
    Transaction::path(&root(state), id).exists()
}
pub(crate) fn rollback(
    state: &Arc<AgentState>,
    id: Uuid,
) -> ServiceResult<ToolConfigApplyResponse> {
    let _guard = SWITCH_LOCK
        .lock()
        .map_err(|_| safe_error(anyhow::anyhow!("tool coordinator unavailable")))?;
    let previous = with_vault(state, false, |vault| {
        let tx = Transaction::load(
            &Transaction::path(&root(state), id),
            &vault.config_backup_key(),
        )
        .map_err(safe_error)?;
        if !tx.committed {
            return Err(ServiceError::new(
                AgentErrorCode::Conflict,
                "pending tool recovery",
            ));
        }
        let binding=tx.changes.iter().find(|c|matches!(&c.resource,Resource::File(p) if p.extension().and_then(|e|e.to_str())==Some("aipbinding"))).ok_or_else(||safe_error(anyhow::anyhow!("not a tool switch backup")))?;
        if let Some(raw) = &binding.before {
            let envelope: Ciphertext = serde_json::from_slice(raw).map_err(safe_error)?;
            let raw = Zeroizing::new(
                decrypt_bytes(
                    &vault.config_backup_key(),
                    b"aipass-tool-binding;v=1",
                    &envelope,
                )
                .map_err(safe_error)?,
            );
            let binding: Binding = serde_json::from_slice(&raw).map_err(safe_error)?;
            Ok(Some((binding.request, binding.route_id)))
        } else {
            // An imported original login has already been archived with its newest
            // rotated grant. Restore that native account instead of historical bytes.
            let updates: Vec<RefUpdate> =
                serde_json::from_value(tx.metadata.clone()).map_err(safe_error)?;
            let home = crate::server::home_dir(vault)?;
            for update in updates {
                let tool = if update.provider == "codex" {
                    ToolConfigTool::Codex
                } else {
                    ToolConfigTool::ClaudeCode
                };
                let active = active_store(&home, &tool).map_err(safe_error)?;
                if update.before["nativeHome"].as_str()
                    == Some(active.home.to_string_lossy().as_ref())
                    && update.after["nativeHome"] != update.before["nativeHome"]
                {
                    return Ok(Some((
                        ToolConfigRequest {
                            tool,
                            id: update.id,
                            mode: ToolConfigMode::Official,
                            secret_id: None,
                            codex_api_key_mode: None,
                            preview_id: None,
                        },
                        None,
                    )));
                }
            }
            Ok(None)
        }
    })?;
    if let Some((mut request, route_id)) = previous {
        // Restore from the latest CLI-owned account, never replay old refresh tokens.
        request.preview_id = None;
        let current = with_vault(state, false, |vault| {
            let binding = load_binding(state, &request.tool, &vault.config_backup_key())
                .map_err(safe_error)?
                .ok_or_else(|| safe_error(anyhow::anyhow!("tool binding unavailable")))?;
            if binding.operation_id != id {
                return Err(ServiceError::new(
                    AgentErrorCode::Conflict,
                    "only the current switch can be restored",
                ));
            }
            let actual = fingerprint(
                &binding.config_resources,
                &vault.config_backup_key(),
                &context(&binding.request).map_err(safe_error)?,
            )
            .map_err(safe_error)?;
            if actual != binding.config_fingerprint {
                return Err(ServiceError::new(
                    AgentErrorCode::Conflict,
                    "tool configuration changed externally",
                ));
            }
            Ok(())
        });
        current?;
        if status_snapshot(state, request.tool.clone())?.state == "conflict" {
            return Err(ServiceError::new(
                AgentErrorCode::Conflict,
                "native credentials changed externally",
            ));
        }
        if let Some(route_id) = route_id {
            let tool = request.tool.clone();
            return proxy::apply_proxy_locked(
                state,
                ToolConfigProxyRequest {
                    tool: if request.tool == ToolConfigTool::Codex {
                        aipass_config_writers::ToolId::Codex
                    } else {
                        aipass_config_writers::ToolId::ClaudeCode
                    },
                    route_id,
                },
                tool,
            );
        }
        return apply_locked(state, request.clone()).or_else(|e| safety::apply_error(&request, e));
    }
    // The first switch must also be reversible, including an unmanaged original login.
    let locks = account_locks(state)?;
    let mut guards = Vec::new();
    for lock in &locks {
        guards.push(lock.try_lock().map_err(|_| {
            ServiceError::new(
                AgentErrorCode::Conflict,
                "account renewal is in progress; retry",
            )
        })?);
    }
    with_vault(state, false, |vault| {
        let key = Zeroizing::new(vault.config_backup_key());
        let tx =
            Transaction::load(&Transaction::path(&root(state), id), &key).map_err(safe_error)?;
        let binding_change=tx.changes.iter().find(|c|matches!(&c.resource,Resource::File(p) if p.extension().and_then(|e|e.to_str())==Some("aipbinding"))).ok_or_else(||safe_error(anyhow::anyhow!("tool binding unavailable")))?;
        let raw = binding_change
            .after
            .as_ref()
            .ok_or_else(|| safe_error(anyhow::anyhow!("tool binding unavailable")))?;
        let envelope: Ciphertext = serde_json::from_slice(raw).map_err(safe_error)?;
        let raw = Zeroizing::new(
            decrypt_bytes(&key, b"aipass-tool-binding;v=1", &envelope).map_err(safe_error)?,
        );
        let binding: Binding = serde_json::from_slice(&raw).map_err(safe_error)?;
        let current = load_binding(state, &binding.request.tool, &key)
            .map_err(safe_error)?
            .ok_or_else(|| safe_error(anyhow::anyhow!("tool binding unavailable")))?;
        if current.operation_id != id {
            return Err(ServiceError::new(
                AgentErrorCode::Conflict,
                "only the current switch can be restored",
            ));
        }
        let home = crate::server::home_dir(vault)?;
        let active = active_store(&home, &binding.request.tool).map_err(safe_error)?;
        let verified = historical::validate(state, vault, &active, &tx)?;
        let mut changes = Vec::new();
        let mut refs = Vec::new();
        let mut outgoing = binding.request.clone();
        outgoing.mode = ToolConfigMode::Plaintext;
        archive_outgoing(vault, &outgoing, &active, &home, &mut changes, &mut refs)
            .map_err(safe_error)?;
        // Check immutable configuration, while allowing normal vendor token rotation.
        if fingerprint(
            &binding.config_resources,
            &key,
            &context(&binding.request).map_err(safe_error)?,
        )
        .map_err(safe_error)?
            != binding.config_fingerprint
        {
            return Err(ServiceError::new(
                AgentErrorCode::Conflict,
                "tool configuration changed externally",
            ));
        }
        let native_resources = active.resources();
        for c in &tx.changes {
            if native_resources.contains(&c.resource)
                || binding.config_resources.contains(&c.resource)
                || c.resource == Resource::File(binding_path(state, &binding.request.tool))
            {
                // Do not overwrite a different external native login.
                if native_resources.contains(&c.resource)
                    && binding.identity.is_some()
                    && active.identity().map_err(safe_error)? != binding.identity
                {
                    return Err(ServiceError::new(
                        AgentErrorCode::Conflict,
                        "native account changed externally",
                    ));
                }
                replace_change(
                    &mut changes,
                    Change::new(c.resource.clone(), c.before.clone()).map_err(safe_error)?,
                );
            }
        }
        for (resource, bytes) in verified {
            replace_change(
                &mut changes,
                Change::new(resource, Some(bytes.to_vec())).map_err(safe_error)?,
            );
        }
        let historical: Vec<RefUpdate> =
            serde_json::from_value(tx.metadata.clone()).map_err(safe_error)?;
        for update in historical {
            if update.before["nativeHome"].as_str() == Some(active.home.to_string_lossy().as_ref())
            {
                let tool = if update.provider == "codex" {
                    ToolConfigTool::Codex
                } else {
                    ToolConfigTool::ClaudeCode
                };
                let before = reference(vault, update.id, &tool).map_err(safe_error)?;
                refs.push(RefUpdate {
                    id: update.id,
                    provider: update.provider,
                    before,
                    after: update.before,
                });
            }
        }
        dedup_changes(&mut changes).map_err(safe_error)?;
        let mut restore = Transaction {
            operation_id: Uuid::new_v4(),
            committed: false,
            changes,
            metadata: serde_json::to_value(&refs).map_err(safe_error)?,
        };
        let backup = restore.save(&root(state), &key).map_err(safe_error)?;
        if let Err(e) = restore
            .apply()
            .map_err(safe_error)
            .and_then(|_| set_references(vault, &refs, true))
        {
            restore.restore().map_err(safe_error)?;
            set_references(vault, &refs, false)?;
            return Err(e);
        }
        restore.committed = true;
        if let Err(e) = restore.save(&root(state), &key) {
            restore.restore().map_err(safe_error)?;
            set_references(vault, &refs, false)?;
            return Err(safe_error(e));
        }
        let _ = refresh_cache(state, vault, &refs);
        Ok(ToolConfigApplyResponse {
            tool: binding.request.tool,
            mode: binding.request.mode,
            entry_id: binding.request.id,
            entry_title: String::new(),
            operation_id: restore.operation_id,
            target_path: active.home.display().to_string(),
            backup_path: backup.display().to_string(),
            summary: "Restored previous configuration".into(),
            outcome: ToolConfigOutcome::Applied,
            message: Some(
                "Restart the CLI; an expired original login must be renewed in its official CLI"
                    .into(),
            ),
        })
    })
}
