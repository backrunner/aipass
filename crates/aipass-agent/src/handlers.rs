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
        AgentRequest::ServerStatus => {
            let proxy = state
                .proxy
                .lock()
                .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "proxy lock poisoned"))?;
            Ok(AgentResponse::success(proxy.status()))
        }
        AgentRequest::ServerLogs => {
            let proxy = state
                .proxy
                .lock()
                .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "proxy lock poisoned"))?;
            Ok(AgentResponse::success(proxy.logs()?))
        }
        AgentRequest::ServerStart => with_vault(state, false, |vault| {
            let mut proxy = state
                .proxy
                .lock()
                .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "proxy lock poisoned"))?;
            proxy.start(vault)
        })
        .map(AgentResponse::success),
        AgentRequest::ServerStop => {
            let session = state.session.lock().map_err(|_| {
                ServiceError::new(AgentErrorCode::Internal, "session lock poisoned")
            })?;
            let mut proxy = state
                .proxy
                .lock()
                .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "proxy lock poisoned"))?;
            let status = match &*session {
                crate::session::SessionState::Locked => proxy.stop_while_locked(),
                crate::session::SessionState::Unlocked(info) => proxy.stop_and_save(&info.vault),
            }?;
            Ok(AgentResponse::success(status))
        }
        AgentRequest::ServerRouteSelect { route_id } => with_vault(state, false, |vault| {
            let mut proxy = state
                .proxy
                .lock()
                .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "proxy lock poisoned"))?;
            proxy.select_route(vault, route_id)
        })
        .map(AgentResponse::success),
        AgentRequest::ServerRouteSetEnabled { route_id, enabled } => {
            with_vault(state, false, |vault| {
                let mut proxy = state.proxy.lock().map_err(|_| {
                    ServiceError::new(AgentErrorCode::Internal, "proxy lock poisoned")
                })?;
                proxy.set_route_enabled(vault, route_id, enabled)
            })
            .map(AgentResponse::success)
        }
        AgentRequest::ServerConfigGet => with_vault(state, true, |vault| {
            let mut proxy = state
                .proxy
                .lock()
                .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "proxy lock poisoned"))?;
            proxy.client_config(vault)
        })
        .map(AgentResponse::success),
        AgentRequest::ServerConfigSet { config } => with_vault(state, false, |vault| {
            let mut proxy = state
                .proxy
                .lock()
                .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "proxy lock poisoned"))?;
            proxy.set_config(vault, config)
        })
        .map(AgentResponse::success),
        AgentRequest::ServerTokenRotate { route_id } => with_vault(state, false, |vault| {
            let mut proxy = state
                .proxy
                .lock()
                .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "proxy lock poisoned"))?;
            proxy.rotate_token(vault, route_id)
        })
        .map(AgentResponse::success),
        AgentRequest::ServerUsageSummary => with_vault(state, true, |vault| {
            let mut proxy = state
                .proxy
                .lock()
                .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "proxy lock poisoned"))?;
            proxy.load_config(vault)?;
            let pricing = proxy.pricing_config(vault)?;
            let list_prices = crate::pricing::load_list_prices(&state.vault_dir);
            proxy.usage_summary(&pricing, &list_prices)
        })
        .map(AgentResponse::success),
        AgentRequest::ServerUsageClear => with_vault(state, false, |_vault| {
            let proxy = state
                .proxy
                .lock()
                .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "proxy lock poisoned"))?;
            proxy.clear_usage()
        })
        .map(AgentResponse::success),
        AgentRequest::ServerUsageTimeseries {
            days,
            timezone_offset_minutes,
        } => with_vault(state, true, |vault| {
            let mut proxy = state
                .proxy
                .lock()
                .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "proxy lock poisoned"))?;
            proxy.load_config(vault)?;
            let pricing = proxy.pricing_config(vault)?;
            let list_prices = crate::pricing::load_list_prices(&state.vault_dir);
            proxy.usage_timeseries(days, timezone_offset_minutes, &pricing, &list_prices)
        })
        .map(AgentResponse::success),
        AgentRequest::ServerPricingConfigGet => with_vault(state, true, |vault| {
            let proxy = state
                .proxy
                .lock()
                .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "proxy lock poisoned"))?;
            proxy.pricing_config(vault)
        })
        .map(AgentResponse::success),
        AgentRequest::ServerPricingAssignmentSet {
            entry_id,
            secret_id,
            group_id,
            multiplier,
        } => with_vault(state, false, |vault| {
            let proxy = state
                .proxy
                .lock()
                .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "proxy lock poisoned"))?;
            proxy.set_pricing_assignment(vault, entry_id, secret_id, group_id, multiplier)
        })
        .map(AgentResponse::success),
        AgentRequest::ServerPricingGroupUpsert { group, apply_scope } => {
            with_vault(state, false, |vault| {
                let proxy = state.proxy.lock().map_err(|_| {
                    ServiceError::new(AgentErrorCode::Internal, "proxy lock poisoned")
                })?;
                proxy.upsert_pricing_group(vault, group, apply_scope)
            })
            .map(AgentResponse::success)
        }
        AgentRequest::ServerPricingGroupDelete { group_id } => with_vault(state, false, |vault| {
            let proxy = state
                .proxy
                .lock()
                .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "proxy lock poisoned"))?;
            proxy.delete_pricing_group(vault, group_id)
        })
        .map(AgentResponse::success),
        AgentRequest::ServerPricingGroupVersionDelete {
            group_id,
            effective_from,
        } => with_vault(state, false, |vault| {
            let proxy = state
                .proxy
                .lock()
                .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "proxy lock poisoned"))?;
            proxy.delete_pricing_group_version(vault, group_id, effective_from)
        })
        .map(AgentResponse::success),
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
            vault.update_provider(id, input).map_err(map_vault_error)?;
            refresh_proxy_provider_credentials(state, vault, id)?;
            Ok(())
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
            vault.trash_provider(id).map_err(map_vault_error)?;
            cleanup_proxy_provider_references(state, vault, id, None);
            Ok(())
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
                .map_err(map_vault_error)?;
            cleanup_proxy_provider_references(state, vault, id, None);
            Ok(())
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
                cleanup_proxy_provider_references(state, vault, summary.id, None);
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
            let secret_id = vault
                .add_secret(id, label, secret.into_inner())
                .map_err(map_vault_error)?;
            refresh_proxy_provider_credentials(state, vault, id)?;
            Ok(secret_id)
        })
        .map(AgentResponse::success),
        AgentRequest::SecretUpdate {
            id,
            secret_id,
            label,
            secret,
        } => with_vault(state, false, |vault| {
            let updated = vault
                .update_secret(
                    id,
                    &secret_id,
                    &label,
                    secret.map(SensitiveString::into_inner),
                )
                .map_err(map_vault_error)?;
            if updated {
                refresh_proxy_provider_credentials(state, vault, id)?;
            }
            Ok(updated)
        })
        .map(|updated| AgentResponse::success(json!({ "updated": updated }))),
        AgentRequest::SecretMetadataSet {
            id,
            secret_id,
            metadata,
        } => with_vault(state, false, |vault| {
            let updated = vault
                .set_secret_metadata(id, &secret_id, &metadata)
                .map_err(map_vault_error)?;
            if updated {
                refresh_proxy_provider_credentials(state, vault, id)?;
            }
            Ok(updated)
        })
        .map(|updated| AgentResponse::success(json!({ "updated": updated }))),
        AgentRequest::SecretRemove { id, label } => with_vault(state, false, |vault| {
            let secret_id = vault.remove_secret(id, &label).map_err(map_vault_error)?;
            cleanup_proxy_provider_references(state, vault, id, Some(&secret_id));
            Ok(())
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
        AgentRequest::OfficialAccountsRefresh { provider_ids } => {
            // Cheap unlocked pre-check: discovery spawns subprocesses and does
            // blocking network I/O, so a locked vault must fail before any of
            // that work (or any keychain/network access) starts.
            if session_status(state)?.locked {
                return Err(ServiceError::new(AgentErrorCode::Locked, "vault is locked"));
            }
            // Discovery and usage refresh spawn subprocesses and do blocking
            // network I/O; run them before taking the session lock so other
            // requests are not stalled for the duration.
            let collected = crate::official_accounts::collect_official_accounts(&provider_ids);
            with_vault(state, false, |vault| {
                let persisted =
                    crate::official_accounts::persist_official_accounts(vault, collected)
                        .map_err(ServiceError::internal)?;
                // Like the other credential-mutating handlers, reload the
                // rotated secrets into a running proxy after the vault writes.
                for (result, entry_id) in &persisted {
                    if let Some(entry_id) = entry_id {
                        if matches!(result.status.as_str(), "imported" | "refreshed") {
                            refresh_proxy_provider_credentials(state, vault, *entry_id)?;
                        }
                    }
                }
                Ok(persisted
                    .into_iter()
                    .map(|(result, _)| result)
                    .collect::<Vec<_>>())
            })
            .map(AgentResponse::success)
        }
        AgentRequest::CcSwitchDetect => {
            Ok(AgentResponse::success(crate::ccswitch::detect_ccswitch()))
        }
        AgentRequest::CcSwitchImport => with_vault(state, false, |vault| {
            crate::ccswitch::import_ccswitch_providers(vault).map_err(ServiceError::internal)
        })
        .map(AgentResponse::success),
        AgentRequest::ProviderFaviconBackfill { request } => {
            backfill_provider_favicons(state, request).map(AgentResponse::success)
        }
        AgentRequest::ToolConfigPreview { request } => with_vault(state, true, |vault| {
            let (entry, plan, content) = build_tool_config_plan(vault, &request)?;
            let files = tool_config_preview_files(&plan, &content);
            let preview = combined_tool_config_preview(&files);
            Ok(ToolConfigPreviewResponse {
                tool: request.tool,
                mode: request.mode,
                entry_id: entry.id,
                entry_title: entry.title,
                target_path: plan.target_path.display().to_string(),
                summary: plan.summary,
                preview,
                files,
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
        AgentRequest::ToolConfigProxyPreview { request } => with_vault(state, true, |vault| {
            let (entry, plan, content) = build_tool_config_proxy_plan(vault, state, &request)?;
            let files = tool_config_preview_files(&plan, &content);
            let preview = combined_tool_config_preview(&files);
            Ok(ToolConfigPreviewResponse {
                tool: tool_config_tool_for(&request.tool),
                mode: ToolConfigMode::Plaintext,
                entry_id: entry.id,
                entry_title: entry.title,
                target_path: plan.target_path.display().to_string(),
                summary: plan.summary,
                preview,
                files,
            })
        })
        .map(AgentResponse::success),
        AgentRequest::ToolConfigProxyApply { request } => with_vault(state, false, |vault| {
            let (entry, plan, content) = build_tool_config_proxy_plan(vault, state, &request)?;
            let result = apply_plan_encrypted(&plan, &content, &vault.config_backup_key())
                .map_err(ServiceError::internal)?;
            Ok(ToolConfigApplyResponse {
                tool: tool_config_tool_for(&request.tool),
                mode: ToolConfigMode::Plaintext,
                entry_id: entry.id,
                entry_title: entry.title,
                operation_id: result.operation_id,
                target_path: result.target_path.display().to_string(),
                backup_path: result.backup_path.display().to_string(),
                summary: plan.summary,
            })
        })
        .map(AgentResponse::success),
        AgentRequest::SyncLocal { dir } => run_sync_local(state, &dir).map(AgentResponse::success),
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
                    run_sync_local(state, &dir).map(AgentResponse::success)
                }
                SyncMode::ICloud => {
                    let dir = cloud_sync_dir(CloudSyncProvider::ICloud)
                        .map_err(ServiceError::internal)?;
                    run_sync_local(state, &dir).map(AgentResponse::success)
                }
                SyncMode::OneDrive => {
                    let dir = cloud_sync_dir(CloudSyncProvider::OneDrive)
                        .map_err(ServiceError::internal)?;
                    run_sync_local(state, &dir).map(AgentResponse::success)
                }
                SyncMode::WebDav => {
                    let url = settings.webdav_url.clone().ok_or_else(|| {
                        ServiceError::new(
                            AgentErrorCode::ValidationFailed,
                            "webdav sync target url is not configured",
                        )
                    })?;
                    let password = with_vault(state, false, |vault| {
                        sync_settings_password(&settings, vault).map_err(ServiceError::internal)
                    })?;
                    let client = HttpWebDavClient::new(
                        &url,
                        settings.webdav_username.clone(),
                        password.map(|value| value.into_inner()),
                    )
                    .map_err(ServiceError::internal)?;
                    Ok(AgentResponse::success(run_sync_webdav(state, &client)))
                }
            }
        }
        AgentRequest::SyncCloud { provider } => {
            let dir = cloud_sync_dir(provider).map_err(ServiceError::internal)?;
            run_sync_local(state, &dir).map(AgentResponse::success)
        }
        AgentRequest::SyncWebDav {
            url,
            username,
            password,
        } => {
            let client =
                HttpWebDavClient::new(&url, username, password.map(|value| value.into_inner()))
                    .map_err(ServiceError::internal)?;
            Ok(AgentResponse::success(run_sync_webdav(state, &client)))
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
        AgentRequest::SyncAcceptConflict { request } => with_vault(state, true, |vault| {
            let root = conflict_root(&state.vault_dir, &request)?;
            accept_conflict_with_validator(&root, &request.conflict_path, &|bytes| {
                vault.validate_sync_object_bytes(bytes).map_err(Into::into)
            })
            .map_err(ServiceError::internal)?;
            Ok(AgentResponse::empty())
        }),
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
        AgentRequest::BrowserSaveDetected { fields } => {
            with_vault(state, false, |vault| save_detected_secret(vault, fields))
                .map(AgentResponse::success)
        }
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

fn cleanup_proxy_provider_references(
    state: &Arc<AgentState>,
    vault: &Vault,
    entry_id: Uuid,
    secret_id: Option<&str>,
) {
    let mut proxy = match state.proxy.lock() {
        Ok(proxy) => proxy,
        Err(poisoned) => {
            write_component_log(
                AGENT_LOG,
                "WARN",
                "recovering poisoned proxy lock while removing provider references",
            );
            poisoned.into_inner()
        }
    };
    let result = proxy.remove_provider_references(vault, entry_id, secret_id);
    match result {
        Ok(true) => write_component_log(
            AGENT_LOG,
            "INFO",
            &format!("removed proxy route references for provider {entry_id}"),
        ),
        Ok(false) => {}
        Err(err) => {
            // A deleted vault credential must not remain usable from the old
            // runtime snapshot even when config cleanup cannot be persisted.
            let _ = proxy.stop();
            let _ = proxy.save_config(vault);
            write_component_log(
                AGENT_LOG,
                "WARN",
                &format!(
                    "failed to remove proxy route references for provider {entry_id}; stopped proxy: {}",
                    err.message
                ),
            );
        }
    }
}

fn refresh_proxy_provider_credentials(
    state: &Arc<AgentState>,
    vault: &Vault,
    entry_id: Uuid,
) -> ServiceResult<()> {
    let mut proxy = match state.proxy.lock() {
        Ok(proxy) => proxy,
        Err(poisoned) => {
            write_component_log(
                AGENT_LOG,
                "WARN",
                "recovering poisoned proxy lock while refreshing provider credentials",
            );
            poisoned.into_inner()
        }
    };
    proxy.refresh_provider_credentials(vault, entry_id)?;
    Ok(())
}

/// One grant per stored key, so a relay entry holding a key per gateway group
/// can be filled with any of them rather than only the first.
fn create_browser_fill_grants(
    vault: &Vault,
    entries: &[EntrySummary],
    origin: &str,
) -> ServiceResult<Vec<TtlGrantSummary>> {
    let mut grants = Vec::new();
    for entry in entries {
        let issued = vault
            .create_secret_grants_for_entry(
                entry.id,
                "chrome.fill",
                120,
                Some(origin.to_string()),
                BROWSER_FILL_GRANT_LIMIT,
            )
            .map_err(map_vault_error)?;
        if issued.is_empty() {
            grants.push(
                vault
                    .create_secret_grant(entry.id, "chrome.fill", 120, Some(origin.to_string()))
                    .map_err(map_vault_error)?,
            );
            continue;
        }
        grants.extend(issued);
    }
    Ok(grants)
}
