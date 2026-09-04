use super::*;
use crate::paths::cloud_sync_dir;
use aipass_agent_protocol::{
    endpoint_url, CloudSyncProvider, OAuthAccountSummary, OAuthDeviceStart, OAuthLoginPoll,
    OAuthLoginStatus,
};
use aipass_provider_registry::{primary_secret_ref, OAuthProvider};
use aipass_vault::ManagedOAuthAccount;

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
        AgentRequest::ServerPricingRemoteSync {
            id,
            timeout_seconds,
        } => {
            let (entry, credentials) = with_vault(state, true, |vault| {
                let entry = vault.get_provider_summary(id).map_err(map_vault_error)?;
                let credentials = entry
                    .secret_refs
                    .iter()
                    .map(|secret| {
                        // Group metadata was moved from the entry onto each
                        // credential. Keep the legacy entry-level value as a
                        // fallback so existing New API keys still select the
                        // remote prices for their actual group.
                        let group = secret.group.clone().or_else(|| {
                            entry
                                .gateway
                                .as_ref()
                                .and_then(|gateway| gateway.group.clone())
                        });
                        Ok((
                            secret.id.clone(),
                            group,
                            vault
                                .reveal_secret_field(id, &secret.id)
                                .map_err(map_vault_error)?,
                        ))
                    })
                    .collect::<ServiceResult<Vec<_>>>()?;
                Ok((entry, credentials))
            })?;
            let endpoint = endpoint_url(&entry.endpoints);
            let pricing_kind = {
                let provider_id = entry
                    .provider_id
                    .as_deref()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .replace('-', "_");
                match provider_id.as_str() {
                    "new_api" | "one_api" => Some("new_api"),
                    "sub2api" => Some("sub_api"),
                    _ => {
                        let hint = format!(
                            "{} {}",
                            entry.title,
                            endpoint.as_deref().unwrap_or_default()
                        )
                        .to_ascii_lowercase()
                        .replace('-', "_");
                        if hint.contains("newapi")
                            || hint.contains("new_api")
                            || hint.contains("oneapi")
                            || hint.contains("one_api")
                        {
                            Some("new_api")
                        } else if hint.contains("subapi")
                            || hint.contains("sub_api")
                            || hint.contains("sub2api")
                        {
                            Some("sub_api")
                        } else {
                            None
                        }
                    }
                }
            };
            let result = if pricing_kind == Some("new_api") {
                if let Some(endpoint) = endpoint.as_deref() {
                    let mut result = with_vault(state, true, |vault| {
                        crate::pricing::load_pricing_config(&state.vault_dir, vault)
                    })?;
                    for (secret_id, group, secret) in &credentials {
                        let Some(remote_pricing) = crate::pricing::fetch_newapi_pricing(
                            endpoint,
                            secret,
                            timeout_seconds.max(1),
                        ) else {
                            continue;
                        };
                        // `/api/pricing` can be user-scoped on protected
                        // instances. Sync one payload with only the credential
                        // that produced it so another key cannot inherit its
                        // visible groups or clear a managed assignment.
                        let secret_groups = [(secret_id.clone(), group.clone())];
                        result = with_vault(state, true, |vault| {
                            crate::pricing::sync_newapi_pricing(
                                &state.vault_dir,
                                vault,
                                id,
                                endpoint,
                                &secret_groups,
                                &remote_pricing,
                            )
                        })?;
                    }
                    result
                } else {
                    with_vault(state, true, |vault| {
                        crate::pricing::load_pricing_config(&state.vault_dir, vault)
                    })?
                }
            } else if pricing_kind == Some("sub_api") {
                let mut result = with_vault(state, true, |vault| {
                    crate::pricing::load_pricing_config(&state.vault_dir, vault)
                })?;
                for (secret_id, _, secret) in &credentials {
                    let Some(endpoint) = endpoint.as_deref() else {
                        continue;
                    };
                    let Some(payload) = crate::pricing::fetch_subapi_billing(
                        endpoint,
                        secret,
                        timeout_seconds.max(1),
                    ) else {
                        continue;
                    };
                    result = with_vault(state, true, |vault| {
                        crate::pricing::sync_subapi_pricing(
                            &state.vault_dir,
                            vault,
                            id,
                            secret_id,
                            endpoint,
                            &payload,
                        )
                    })?;
                }
                result
            } else {
                with_vault(state, true, |vault| {
                    crate::pricing::load_pricing_config(&state.vault_dir, vault)
                })?
            };
            Ok(AgentResponse::success(result))
        }
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
            // The proxy runtime snapshot was resolved from the pre-import
            // vault. Stop it before locking so stale credentials cannot serve
            // while locked; the next unlock rebuilds it from the imported
            // vault (start_if_enabled only short-circuits on a live handle).
            if let Ok(mut proxy) = state.proxy.lock() {
                let _ = proxy.stop();
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
            vault.archive_provider(id).map_err(map_vault_error)?;
            refresh_proxy_provider_credentials(state, vault, id)?;
            Ok(())
        })
        .map(|_| AgentResponse::empty()),
        AgentRequest::ProviderRestore { id } => with_vault(state, false, |vault| {
            vault.restore_provider(id).map_err(map_vault_error)?;
            refresh_proxy_provider_credentials(state, vault, id)?;
            Ok(())
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
            let results = crate::ccswitch::import_ccswitch_providers(vault)
                .map_err(ServiceError::internal)?;
            // The import can add or refresh many credentials at once; rebuild
            // the running proxy snapshot rather than refreshing entry by entry.
            reload_running_proxy(state, vault)?;
            Ok(results)
        })
        .map(AgentResponse::success),
        AgentRequest::OAuthLoginStart { provider } => {
            let challenge = crate::oauth::oauth_manager()
                .start(provider)
                .map_err(|err| ServiceError::new(AgentErrorCode::Internal, err.to_string()))?;
            Ok(AgentResponse::success(OAuthDeviceStart {
                device_code: challenge.device_code,
                user_code: challenge.user_code,
                verification_uri: challenge.verification_uri,
                verification_uri_complete: challenge.verification_uri_complete,
                expires_in: challenge.expires_in,
                interval: challenge.interval,
            }))
        }
        AgentRequest::OAuthLoginPoll {
            provider,
            device_code,
        } => match crate::oauth::oauth_manager().poll(provider, &device_code) {
            Ok(outcome) => match outcome.bundle {
                None => Ok(AgentResponse::success(OAuthLoginPoll {
                    status: OAuthLoginStatus::Pending,
                    account: None,
                    message: None,
                    interval_secs: Some(outcome.interval_secs),
                })),
                Some(bundle) => {
                    let account = complete_oauth_login(state, provider, bundle)?;
                    // Consume the device entry only after persistence succeeds,
                    // so a failed persist does not lose the token bundle.
                    crate::oauth::oauth_manager().consume(&device_code);
                    Ok(AgentResponse::success(OAuthLoginPoll {
                        status: OAuthLoginStatus::Authorized,
                        account: Some(account),
                        message: None,
                        interval_secs: None,
                    }))
                }
            },
            Err(crate::oauth::OAuthError::ExpiredDeviceCode) => {
                Ok(AgentResponse::success(OAuthLoginPoll {
                    status: OAuthLoginStatus::Expired,
                    account: None,
                    message: None,
                    interval_secs: None,
                }))
            }
            // Transient failures (network, 5xx) must not kill the login: report
            // pending with a sanitized warning and let the client keep polling.
            Err(err) if err.is_retryable() => Ok(AgentResponse::success(OAuthLoginPoll {
                status: OAuthLoginStatus::Pending,
                account: None,
                message: Some(err.to_string()),
                interval_secs: crate::oauth::oauth_manager().current_interval(&device_code),
            })),
            Err(err) => Ok(AgentResponse::success(OAuthLoginPoll {
                status: OAuthLoginStatus::Error,
                account: None,
                message: Some(err.to_string()),
                interval_secs: None,
            })),
        },
        AgentRequest::OAuthLoginCancel { device_code, .. } => Ok(AgentResponse::success(
            crate::oauth::oauth_manager().cancel(&device_code),
        )),
        AgentRequest::OAuthAccountsList { provider } => with_vault(state, false, |vault| {
            let accounts = vault
                .list_oauth_accounts(provider)
                .map_err(map_vault_error)?;
            Ok(accounts
                .iter()
                .map(oauth_account_summary)
                .collect::<Vec<_>>())
        })
        .map(AgentResponse::success),
        AgentRequest::OAuthAccountsRemove {
            provider,
            account_id,
        } => with_vault(state, false, |vault| {
            let account = vault
                .get_oauth_account(account_id)
                .map_err(map_vault_error)?;
            // Same convention as set_default_oauth_account: refuse to act on an
            // account belonging to a different provider than the request names.
            if account.provider != provider {
                return Err(ServiceError::new(
                    AgentErrorCode::ValidationFailed,
                    "oauth account does not belong to the requested provider",
                ));
            }
            let entry_id = account.entry_id;
            vault
                .remove_oauth_account(account_id)
                .map_err(map_vault_error)?;
            if let Some(entry_id) = entry_id {
                // Two logins of the same identity can share one entry; only
                // retire it when no other managed account still references it.
                let still_referenced = vault
                    .list_oauth_accounts(None)
                    .map_err(map_vault_error)?
                    .iter()
                    .any(|account| account.entry_id == Some(entry_id));
                if !still_referenced {
                    vault.trash_provider(entry_id).map_err(map_vault_error)?;
                    cleanup_proxy_provider_references(state, vault, entry_id, None);
                }
            }
            Ok(())
        })
        .map(|_| AgentResponse::empty()),
        AgentRequest::OAuthAccountsSetDefault {
            provider,
            account_id,
        } => with_vault(state, false, |vault| {
            vault
                .set_default_oauth_account(provider, account_id)
                .map_err(map_vault_error)
        })
        .map(|_| AgentResponse::empty()),
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
            let saved = with_vault(state, true, |vault| {
                save_sync_settings(&state.vault_dir, vault, &updated)
                    .map_err(ServiceError::internal)
            })?;
            // The sync folder may have moved; point the filesystem watcher at
            // the new target (or drop it when the backend is not folder-based).
            crate::sync_watch::restart_sync_watcher(state, &saved);
            Ok(AgentResponse::success(sync_settings_view(&saved)))
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
            with_vault(state, false, |vault| {
                let result = save_detected_secret(vault, fields)?;
                // Covers every write path inside save_detected_secret: new
                // entries, adopted keys, and metadata/group-only updates.
                refresh_proxy_provider_credentials(state, vault, result.entry_id)?;
                Ok(result)
            })
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

fn reload_running_proxy(state: &Arc<AgentState>, vault: &Vault) -> ServiceResult<()> {
    let mut proxy = match state.proxy.lock() {
        Ok(proxy) => proxy,
        Err(poisoned) => {
            write_component_log(
                AGENT_LOG,
                "WARN",
                "recovering poisoned proxy lock while reloading the runtime",
            );
            poisoned.into_inner()
        }
    };
    proxy.reload_if_running(vault)
}

fn oauth_account_summary(account: &ManagedOAuthAccount) -> OAuthAccountSummary {
    let credential_expires_at = if account.expires_at_ms > 0 {
        let formatted = crate::oauth::native_write::ms_to_rfc3339(account.expires_at_ms);
        if formatted.is_empty() {
            None
        } else {
            Some(formatted)
        }
    } else {
        None
    };
    OAuthAccountSummary {
        id: account.id,
        provider: account.provider,
        account_identity: account.account_identity.clone(),
        chatgpt_account_id: account.chatgpt_account_id.clone(),
        entry_id: account.entry_id,
        is_default: account.is_default,
        authenticated_at: (account.authenticated_at.unix_timestamp_nanos() / 1_000_000) as i64,
        credential_expires_at,
        requires_reauth: account.requires_reauth,
    }
}

/// Persist a freshly authorized device-code login: create/refresh the provider
/// entry, store the refreshable token bundle, write it back to the native CLI
/// credential file, and push the access token into the running proxy.
fn complete_oauth_login(
    state: &Arc<AgentState>,
    provider: OAuthProvider,
    bundle: crate::oauth::OAuthTokenBundle,
) -> ServiceResult<OAuthAccountSummary> {
    use crate::oauth::native_write::{self, NativeSyncOutcome};
    let now = crate::oauth::now_ms();
    let expires_at_ms =
        now.saturating_add(crate::oauth::clamp_expires_in(bundle.expires_in).saturating_mul(1000));
    let credential_expires_at = native_write::ms_to_rfc3339(expires_at_ms);
    with_vault(state, true, |vault| {
        let entry_id = crate::official_accounts::persist_login_account(
            vault,
            provider.provider_id(),
            bundle.account_identity.clone(),
            bundle.chatgpt_account_id.clone(),
            bundle.access_token.clone(),
            Some(credential_expires_at),
        )
        .map_err(ServiceError::internal)?;
        let provider_accounts = vault
            .list_oauth_accounts(Some(provider))
            .map_err(map_vault_error)?;
        // Re-authenticating the same identity reuses the same provider entry, so
        // update the existing managed account instead of creating a duplicate.
        let existing = provider_accounts
            .iter()
            .find(|account| account.entry_id == Some(entry_id));
        let is_update = existing.is_some();
        let is_default = match existing {
            Some(account) => account.is_default,
            None => provider_accounts.is_empty(),
        };
        let account_id = existing
            .map(|account| account.id)
            .unwrap_or_else(Uuid::new_v4);
        let mut account = ManagedOAuthAccount {
            id: account_id,
            provider,
            account_identity: bundle.account_identity.clone(),
            chatgpt_account_id: bundle.chatgpt_account_id.clone(),
            access_token: bundle.access_token.clone(),
            refresh_token: bundle.refresh_token.clone(),
            id_token: bundle.id_token.clone(),
            expires_at_ms,
            last_refresh_ms: now,
            entry_id: Some(entry_id),
            is_default,
            requires_reauth: false,
            authenticated_at: OffsetDateTime::now_utc(),
        };
        let outcome = match provider {
            OAuthProvider::Codex => native_write::sync_codex_auth_json(
                &bundle.access_token,
                &bundle.refresh_token,
                bundle.id_token.as_deref(),
                bundle.chatgpt_account_id.as_deref().unwrap_or_default(),
                None,
                now,
            ),
            OAuthProvider::Grok => native_write::sync_grok_auth_json(
                &bundle.access_token,
                &bundle.refresh_token,
                None,
                expires_at_ms,
                bundle.account_identity.as_deref(),
            ),
        }
        .map_err(ServiceError::internal)?;
        match outcome {
            NativeSyncOutcome::Adopted {
                access_token,
                refresh_token,
                id_token,
                last_refresh_ms,
                expires_at_ms: adopted_expires_at_ms,
            } => {
                // The CLI already had a newer generation; keep it and mirror the
                // adopted access token into the entry secret the proxy reads.
                account.refresh_token = refresh_token;
                if id_token.is_some() {
                    account.id_token = id_token;
                }
                account.last_refresh_ms = last_refresh_ms;
                // Keep the stored expiry describing the stored (adopted) token.
                if let Some(adopted_expires_at_ms) = adopted_expires_at_ms {
                    account.expires_at_ms = adopted_expires_at_ms;
                }
                if access_token != account.access_token {
                    if let Ok(summary) = vault.get_provider_summary(entry_id) {
                        if let Some(secret) = primary_secret_ref(&summary.secret_refs) {
                            vault
                                .update_secret(
                                    entry_id,
                                    &secret.id,
                                    &secret.label,
                                    Some(access_token.clone()),
                                )
                                .map_err(map_vault_error)?;
                        }
                    }
                    account.access_token = access_token;
                }
            }
            NativeSyncOutcome::Skipped(reason) => {
                write_component_log(
                    AGENT_LOG,
                    "WARN",
                    &format!("oauth native write-back skipped: {reason}"),
                );
            }
            NativeSyncOutcome::Written => {}
        }
        if is_update {
            vault
                .update_oauth_account(account.clone())
                .map_err(map_vault_error)?;
        } else {
            vault
                .add_oauth_account(account.clone())
                .map_err(map_vault_error)?;
        }
        refresh_proxy_provider_credentials(state, vault, entry_id)?;
        Ok(oauth_account_summary(&account))
    })
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
