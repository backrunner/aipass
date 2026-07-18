use super::*;
use crate::paths::cloud_sync_dir;
use aipass_agent_protocol::CloudSyncProvider;

const BROWSER_FILL_GRANT_LIMIT: usize = 5;

pub(crate) fn handle_request(state: &Arc<AgentState>, request: AgentRequest) -> AgentResponse {
    if let Err(err) = lock_if_idle(state) {
        return err.response();
    }
    match dispatch_request(state, request) {
        Ok(response) => response,
        Err(err) => err.response(),
    }
}

fn dispatch_request(
    state: &Arc<AgentState>,
    request: AgentRequest,
) -> ServiceResult<AgentResponse> {
    match request {
        AgentRequest::SessionStatus | AgentRequest::VaultStatus => {
            Ok(AgentResponse::success(session_status(state)?))
        }
        AgentRequest::SessionUnlock { mode } => match mode {
            SessionUnlockMode::Password { password } => {
                let result = unlock_with_password(state, password.into_inner())?;
                Ok(AgentResponse::success(result))
            }
            SessionUnlockMode::NativeWindow => {
                open_desktop_window("unlock", &state.vault_dir)?;
                Ok(AgentResponse::success(session_status(state)?))
            }
            SessionUnlockMode::NativeWindowWait { timeout_ms } => {
                open_desktop_window("unlock", &state.vault_dir)?;
                let timeout = std::time::Duration::from_millis(timeout_ms.clamp(1_000, 120_000));
                Ok(AgentResponse::success(wait_for_unlock(state, timeout)?))
            }
        },
        AgentRequest::SessionLock { reason } => {
            lock_session(state, reason);
            Ok(AgentResponse::success(session_status(state)?))
        }
        AgentRequest::SessionTouch => {
            touch_session(state);
            Ok(AgentResponse::success(session_status(state)?))
        }
        AgentRequest::SessionPolicyGet => Ok(AgentResponse::success(current_policy(state)?)),
        AgentRequest::SessionPolicySet { policy } => {
            let policy = clamp_policy(policy);
            save_policy(&state.vault_dir, &policy)?;
            *state.policy.lock().map_err(|_| {
                ServiceError::new(AgentErrorCode::Internal, "policy lock poisoned")
            })? = policy.clone();
            Ok(AgentResponse::success(policy))
        }
        AgentRequest::VaultCreate { password } => {
            let response = create_vault(state, password.into_inner())?;
            Ok(AgentResponse::success(response))
        }
        AgentRequest::VaultRecover {
            recovery_key,
            new_password,
        } => {
            let response =
                recover_vault(state, recovery_key.into_inner(), new_password.into_inner())?;
            Ok(AgentResponse::success(response))
        }
        AgentRequest::VaultReset => Ok(AgentResponse::success(reset_vault(state)?)),
        AgentRequest::VaultChangePassword { new_password } => {
            let mut new_password = new_password.into_inner();
            let result = with_vault_mut(state, false, |vault| {
                let secret = SecretString::new(new_password.as_str());
                vault
                    .change_master_password(&secret)
                    .map_err(map_vault_error)?;
                Ok(serde_json::json!({ "ok": true, "epoch": vault.current_epoch() }))
            });
            new_password.zeroize();
            result.map(AgentResponse::success)
        }
        AgentRequest::VaultRotate { reason } => with_vault_mut(state, false, |vault| {
            let epoch = vault
                .advance_epoch_and_rewrap(&reason)
                .map_err(map_vault_error)?;
            Ok(json!({ "ok": true, "epoch": epoch }))
        })
        .map(AgentResponse::success),
        AgentRequest::VaultExport {
            output,
            export_password,
        } => with_vault(state, false, |vault| {
            let export_password = SecretString::new(export_password.into_inner());
            let export = vault
                .export_encrypted(&export_password)
                .map_err(map_vault_error)?;
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent).map_err(ServiceError::internal)?;
            }
            atomic_write_bytes(
                &output,
                &serde_json::to_vec_pretty(&export).map_err(ServiceError::internal)?,
            )
            .map_err(ServiceError::internal)?;
            Ok(json!({ "ok": true, "output": output, "vaultId": export.vault_id }))
        })
        .map(AgentResponse::success),
        AgentRequest::VaultImport {
            input,
            export_password,
        } => {
            let root = state.vault_dir.clone();
            let export: EncryptedVaultExport =
                serde_json::from_slice(&fs::read(&input).map_err(ServiceError::internal)?)
                    .map_err(ServiceError::internal)?;
            let backup = if root.exists() {
                let backup = root.with_file_name(format!(
                    "vault-import-backup-{}",
                    OffsetDateTime::now_utc().unix_timestamp()
                ));
                fs::rename(&root, &backup).map_err(ServiceError::internal)?;
                Some(backup)
            } else {
                None
            };
            let export_password = SecretString::new(export_password.into_inner());
            if let Err(err) = Vault::import_encrypted(&root, &export_password, &export) {
                if let Some(backup) = backup {
                    let _ = fs::remove_dir_all(&root);
                    let _ = fs::rename(backup, &root);
                }
                return Err(map_vault_error(err));
            }
            lock_session(state, LockReason::Import);
            Ok(AgentResponse::success(json!({ "imported": true })))
        }
        AgentRequest::EntriesList { archived } => with_vault(state, true, |vault| {
            if archived {
                vault
                    .list_archived_provider_summaries()
                    .map_err(map_vault_error)
            } else {
                vault.list_provider_summaries().map_err(map_vault_error)
            }
        })
        .map(AgentResponse::success),
        AgentRequest::EntriesTrash => with_vault(state, true, |vault| {
            vault
                .list_trash_provider_summaries()
                .map_err(map_vault_error)
        })
        .map(AgentResponse::success),
        AgentRequest::EntriesFavorites => with_vault(state, true, |vault| {
            vault
                .list_favorite_provider_summaries()
                .map_err(map_vault_error)
        })
        .map(AgentResponse::success),
        AgentRequest::EntriesSearch { query } => with_vault(state, true, |vault| {
            vault.search(&query).map_err(map_vault_error)
        })
        .map(AgentResponse::success),
        AgentRequest::ProviderGet { id } => with_vault(state, true, |vault| {
            vault.get_provider_summary(id).map_err(map_vault_error)
        })
        .map(AgentResponse::success),
        AgentRequest::ProviderAdd { input } => with_vault(state, false, |vault| {
            vault.add_provider(input).map_err(map_vault_error)
        })
        .map(AgentResponse::success),
        AgentRequest::ProviderUpdate { id, input } => with_vault(state, false, |vault| {
            vault.update_provider(id, input).map_err(map_vault_error)
        })
        .map(|_| AgentResponse::empty()),
        AgentRequest::ProviderArchive { id } => with_vault(state, false, |vault| {
            vault.archive_provider(id).map_err(map_vault_error)
        })
        .map(|_| AgentResponse::empty()),
        AgentRequest::ProviderRestore { id } => with_vault(state, false, |vault| {
            vault.restore_provider(id).map_err(map_vault_error)
        })
        .map(|_| AgentResponse::empty()),
        AgentRequest::ProviderTrash { id } => with_vault(state, false, |vault| {
            vault.trash_provider(id).map_err(map_vault_error)
        })
        .map(|_| AgentResponse::empty()),
        AgentRequest::ProviderFavorite { id, favorite } => with_vault(state, false, |vault| {
            vault
                .set_provider_favorite(id, favorite)
                .map_err(map_vault_error)
        })
        .map(|_| AgentResponse::empty()),
        AgentRequest::ProviderDelete { id } => with_vault(state, false, |vault| {
            vault
                .delete_provider_permanently(id)
                .map_err(map_vault_error)
        })
        .map(|_| AgentResponse::empty()),
        AgentRequest::TrashPurgeExpired => with_vault(state, false, |vault| {
            vault
                .purge_expired_trash(time::Duration::days(30))
                .map_err(map_vault_error)
        })
        .map(|count| AgentResponse::success(json!({ "purged": count }))),
        AgentRequest::TrashEmpty => with_vault(state, false, |vault| {
            let trashed = vault
                .list_trash_provider_summaries()
                .map_err(map_vault_error)?;
            for summary in &trashed {
                vault
                    .delete_provider_permanently(summary.id)
                    .map_err(map_vault_error)?;
            }
            Ok(trashed.len())
        })
        .map(|count| AgentResponse::success(json!({ "purged": count }))),
        AgentRequest::SecretRevealField { id, field } => with_vault(state, true, |vault| {
            vault
                .reveal_secret_field(id, &field)
                .map_err(map_vault_error)
        })
        .map(|secret| {
            AgentResponse::success(SecretValue {
                secret: secret.into(),
            })
        }),
        AgentRequest::SecretAdd { id, label, secret } => with_vault(state, false, |vault| {
            vault
                .add_secret(id, label, secret.into_inner())
                .map_err(map_vault_error)
        })
        .map(AgentResponse::success),
        AgentRequest::SecretRemove { id, label } => with_vault(state, false, |vault| {
            vault.remove_secret(id, &label).map_err(map_vault_error)
        })
        .map(|_| AgentResponse::empty()),
        AgentRequest::DevicesList => with_vault(state, true, |vault| {
            vault.list_devices().map_err(map_vault_error)
        })
        .map(AgentResponse::success),
        AgentRequest::DeviceRevoke { id } => with_vault_mut(state, false, |vault| {
            vault.revoke_device(id).map_err(map_vault_error)
        })
        .map(|_| AgentResponse::empty()),
        AgentRequest::ProviderProbe {
            id,
            timeout_seconds,
        } => {
            let (entry, secret) = with_vault(state, true, |vault| {
                Ok((
                    vault.get_provider_summary(id).map_err(map_vault_error)?,
                    vault.reveal_secret(id).map_err(map_vault_error)?,
                ))
            })?;
            Ok(AgentResponse::success(probe_entry(
                entry,
                secret,
                timeout_seconds.max(1),
            )))
        }
        AgentRequest::ProviderUsageProbe {
            id,
            mode,
            timeout_seconds,
            base_url,
            access_token,
            user_id,
        } => {
            let (entry, secret) = with_vault(state, true, |vault| {
                Ok((
                    vault.get_provider_summary(id).map_err(map_vault_error)?,
                    vault.reveal_secret(id).map_err(map_vault_error)?,
                ))
            })?;
            Ok(AgentResponse::success(
                crate::usage_probe::probe_provider_usage(
                    entry,
                    secret,
                    crate::usage_probe::UsageProbeOptions {
                        mode,
                        timeout_seconds: timeout_seconds.max(1),
                        base_url,
                        access_token,
                        user_id,
                    },
                ),
            ))
        }
        AgentRequest::ProviderUsageApply { id, quota, gateway } => {
            with_vault(state, false, |vault| {
                vault
                    .update_provider_usage(id, quota, gateway)
                    .map_err(map_vault_error)
            })
            .map(|_| AgentResponse::empty())
        }
        AgentRequest::ProviderFaviconBackfill { request } => {
            backfill_provider_favicons(state, request).map(AgentResponse::success)
        }
        AgentRequest::ToolConfigPreview { request } => with_vault(state, true, |vault| {
            let (entry, plan, _) = build_tool_config_plan(vault, &request)?;
            Ok(ToolConfigPreviewResponse {
                tool: request.tool,
                mode: request.mode,
                entry_id: entry.id,
                entry_title: entry.title,
                target_path: plan.target_path.display().to_string(),
                summary: plan.summary,
                preview: plan.preview,
            })
        })
        .map(AgentResponse::success),
        AgentRequest::ToolConfigApply { request } => with_vault(state, false, |vault| {
            let (entry, plan, content) = build_tool_config_plan(vault, &request)?;
            let result = apply_plan_encrypted(&plan, &content, &vault.config_backup_key())
                .map_err(ServiceError::internal)?;
            Ok(tool_apply_response(request, entry, plan, result))
        })
        .map(AgentResponse::success),
        AgentRequest::ToolConfigRollback { operation_id } => with_vault(state, false, |vault| {
            let home = home_dir()?;
            let backup = aipass_config_writers::find_backup_by_operation(&home, operation_id)
                .map_err(ServiceError::internal)?;
            rollback_encrypted(&backup, &vault.config_backup_key()).map_err(ServiceError::internal)
        })
        .map(AgentResponse::success),
        AgentRequest::SyncLocal { dir } => sync_local_folder(&state.vault_dir, &dir)
            .map(AgentResponse::success)
            .map_err(ServiceError::internal),
        AgentRequest::SyncSettingsGet => load_sync_settings(&state.vault_dir)
            .map(|settings| AgentResponse::success(sync_settings_view(&settings)))
            .map_err(ServiceError::internal),
        AgentRequest::SyncSettingsSet { settings } => {
            let current = load_sync_settings(&state.vault_dir).map_err(ServiceError::internal)?;
            let updated = apply_sync_settings_update(current, settings);
            with_vault(state, true, |vault| {
                let saved = save_sync_settings(&state.vault_dir, vault, &updated)
                    .map_err(ServiceError::internal)?;
                Ok(sync_settings_view(&saved))
            })
            .map(AgentResponse::success)
        }
        AgentRequest::SyncConfigured => {
            let settings = load_sync_settings(&state.vault_dir).map_err(ServiceError::internal)?;
            match settings.mode {
                SyncMode::Local => {
                    let dir = settings.sync_folder.ok_or_else(|| {
                        ServiceError::new(
                            AgentErrorCode::ValidationFailed,
                            "local sync target is not configured",
                        )
                    })?;
                    sync_local_folder(&state.vault_dir, &dir)
                        .map(AgentResponse::success)
                        .map_err(ServiceError::internal)
                }
                SyncMode::ICloud => {
                    let dir = cloud_sync_dir(CloudSyncProvider::ICloud)
                        .map_err(ServiceError::internal)?;
                    sync_local_folder(&state.vault_dir, &dir)
                        .map(AgentResponse::success)
                        .map_err(ServiceError::internal)
                }
                SyncMode::OneDrive => {
                    let dir = cloud_sync_dir(CloudSyncProvider::OneDrive)
                        .map_err(ServiceError::internal)?;
                    sync_local_folder(&state.vault_dir, &dir)
                        .map(AgentResponse::success)
                        .map_err(ServiceError::internal)
                }
                SyncMode::WebDav => {
                    let url = settings.webdav_url.clone().ok_or_else(|| {
                        ServiceError::new(
                            AgentErrorCode::ValidationFailed,
                            "webdav sync target url is not configured",
                        )
                    })?;
                    with_vault(state, false, |vault| {
                        let password = sync_settings_password(&settings, vault)
                            .map_err(ServiceError::internal)?;
                        let client = HttpWebDavClient::new(
                            &url,
                            settings.webdav_username.clone(),
                            password.map(|value| value.into_inner()),
                        )
                        .map_err(ServiceError::internal)?;
                        Ok(sync_webdav_report(&state.vault_dir, &client))
                    })
                    .map(AgentResponse::success)
                }
            }
        }
        AgentRequest::SyncCloud { provider } => {
            let dir = cloud_sync_dir(provider).map_err(ServiceError::internal)?;
            sync_local_folder(&state.vault_dir, &dir)
                .map(AgentResponse::success)
                .map_err(ServiceError::internal)
        }
        AgentRequest::SyncWebDav {
            url,
            username,
            password,
        } => {
            let client =
                HttpWebDavClient::new(&url, username, password.map(|value| value.into_inner()))
                    .map_err(ServiceError::internal)?;
            Ok(AgentResponse::success(sync_webdav_report(
                &state.vault_dir,
                &client,
            )))
        }
        AgentRequest::SyncConflicts { dir, provider } => with_vault(state, true, |vault| {
            let mut conflicts = conflict_responses(ConflictScope::Vault, &state.vault_dir, vault)?;
            if let Some(dir) = dir {
                conflicts.extend(conflict_responses(ConflictScope::Sync, &dir, vault)?);
            }
            if let Some(provider) = provider {
                let dir = cloud_sync_dir(provider).map_err(ServiceError::internal)?;
                conflicts.extend(conflict_responses(ConflictScope::Sync, &dir, vault)?);
            }
            Ok(conflicts)
        })
        .map(AgentResponse::success),
        AgentRequest::SyncAcceptConflict { request } => {
            let root = conflict_root(&state.vault_dir, &request)?;
            accept_conflict(&root, &request.conflict_path).map_err(ServiceError::internal)?;
            Ok(AgentResponse::empty())
        }
        AgentRequest::SyncDiscardConflict { request } => {
            let root = conflict_root(&state.vault_dir, &request)?;
            discard_conflict(&root, &request.conflict_path).map_err(ServiceError::internal)?;
            Ok(AgentResponse::empty())
        }
        AgentRequest::BrowserContextLookup { origin, url } => with_vault(state, true, |vault| {
            let mut entries = vault.lookup_by_origin(&origin).map_err(map_vault_error)?;
            if entries.is_empty() {
                entries = vault.lookup_by_origin(&url).map_err(map_vault_error)?;
            }
            entries.truncate(BROWSER_FILL_GRANT_LIMIT);
            let grants = create_browser_fill_grants(vault, &entries, &origin)?;
            Ok(BrowserContextLookupData { entries, grants })
        })
        .map(AgentResponse::success),
        AgentRequest::BrowserEntriesSearch { origin, query } => with_vault(state, true, |vault| {
            let mut entries = vault.search(&query).map_err(map_vault_error)?;
            entries.truncate(BROWSER_FILL_GRANT_LIMIT);
            let grants = create_browser_fill_grants(vault, &entries, &origin)?;
            Ok(BrowserContextLookupData { entries, grants })
        })
        .map(AgentResponse::success),
        AgentRequest::BrowserSecretFill { entry_id, grant_id } => {
            with_vault(state, true, |vault| {
                let secret = vault
                    .consume_secret_grant(grant_id)
                    .map_err(map_vault_error)?;
                Ok(BrowserFillResult {
                    entry_id: entry_id.unwrap_or(grant_id),
                    field: "api_key".to_string(),
                    secret: secret.into(),
                })
            })
            .map(AgentResponse::success)
        }
        AgentRequest::BrowserPreviewDetected { fields } => with_vault(state, true, |vault| {
            Ok(detected_secret_preview(vault, &fields))
        })
        .map(AgentResponse::success),
        AgentRequest::BrowserSaveDetected { fields } => with_vault(state, false, |vault| {
            let entry_id = save_detected_secret(vault, fields)?;
            Ok(SaveDetectedResult { entry_id })
        })
        .map(AgentResponse::success),
        AgentRequest::BrowserIgnoreOrigin { origin } => {
            let ignored_origins = ignore_origin(&state.vault_dir, &origin)?;
            Ok(AgentResponse::success(BrowserIgnoreOriginResult {
                ignored_origins,
            }))
        }
        AgentRequest::BrowserIsOriginIgnored { origin } => {
            Ok(AgentResponse::success(BrowserIgnoredStatus {
                ignored: is_origin_ignored(&state.vault_dir, &origin)?,
            }))
        }
        AgentRequest::UiOpenMain => {
            open_desktop_window("main", &state.vault_dir)?;
            Ok(AgentResponse::empty())
        }
        AgentRequest::UiOpenUnlock => {
            open_desktop_window("unlock", &state.vault_dir)?;
            Ok(AgentResponse::empty())
        }
        AgentRequest::UiOpenQuickAccess => {
            open_desktop_window("quick-access", &state.vault_dir)?;
            Ok(AgentResponse::empty())
        }
        AgentRequest::AgentShutdown => {
            lock_session(state, LockReason::AppQuit);
            state.shutdown.store(true, Ordering::SeqCst);
            Ok(AgentResponse::empty())
        }
    }
}

fn create_browser_fill_grants(
    vault: &Vault,
    entries: &[EntrySummary],
    origin: &str,
) -> ServiceResult<Vec<TtlGrantSummary>> {
    entries
        .iter()
        .map(|entry| {
            vault
                .create_secret_grant(entry.id, "chrome.fill", 120, Some(origin.to_string()))
                .map_err(map_vault_error)
        })
        .collect()
}
