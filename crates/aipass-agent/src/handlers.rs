#[path = "handlers/tools.rs"]
mod tools;
use super::*;
use crate::paths::cloud_sync_dir;
use aipass_agent_protocol::{endpoint_url, OAuthAccountSummary};
use aipass_vault::ManagedOAuthAccount;

#[path = "handlers/subscriptions.rs"]
mod subscriptions;

const BROWSER_FILL_GRANT_LIMIT: usize = 5;

pub(crate) fn handle_request(state: &Arc<AgentState>, request: AgentRequest) -> AgentResponse {
    let operation = crate::operation_log::OperationLog::start(&request);
    if let Err(err) = if matches!(request, AgentRequest::CloudKitExchange { .. }) {
        // Native transport housekeeping neither unlocks nor drives session policy.
        Ok(())
    } else {
        lock_if_idle(state).map(|_| ())
    } {
        let response = err.response();
        operation.finish(&response);
        return response;
    }
    let response = match dispatch_request(state, request) {
        Ok(response) => response,
        Err(err) => err.response(),
    };
    operation.finish(&response);
    response
}

fn dispatch_request(
    state: &Arc<AgentState>,
    request: AgentRequest,
) -> ServiceResult<AgentResponse> {
    match request {
        AgentRequest::SubscriptionImportStart { input } => state
            .subscription_imports
            .start(state, input)
            .map(AgentResponse::success),
        AgentRequest::SubscriptionImportPoll { ticket } => state
            .subscription_imports
            .poll(state, ticket)
            .map(AgentResponse::success),
        AgentRequest::SubscriptionImportCancel { ticket } => state
            .subscription_imports
            .cancel(state, ticket)
            .map(AgentResponse::success),
        request @ (AgentRequest::SubscriptionCliStatus { .. }
        | AgentRequest::ClaudeCliStatus
        | AgentRequest::ClaudeLoginStart
        | AgentRequest::ClaudeLoginPoll { .. }
        | AgentRequest::ClaudeLoginCode { .. }
        | AgentRequest::ClaudeLoginCancel { .. }
        | AgentRequest::CommunityRefresh { .. }
        | AgentRequest::ClaudeNativeRead { .. }
        | AgentRequest::ClaudeNativeWrite { .. }
        | AgentRequest::OAuthLoginStart { .. }
        | AgentRequest::OAuthLoginPoll { .. }
        | AgentRequest::OAuthLoginCancel { .. }) => subscriptions::handle(state, request),
        AgentRequest::CommunityAccountRead { entry_id, marker } => {
            with_vault(state, false, |vault| {
                crate::community::read(vault, entry_id, marker.expose())
            })
            .map(AgentResponse::success)
        }
        AgentRequest::CommunityAccountWrite {
            entry_id,
            marker,
            revision,
            bundle,
        } => with_vault(state, false, |vault| {
            crate::community::write(vault, entry_id, marker.expose(), revision, bundle.expose())
        })
        .map(|()| AgentResponse::empty()),
        request @ (AgentRequest::CommunityCatalog
        | AgentRequest::CommunityLoginStart { .. }
        | AgentRequest::CommunityLoginPoll { .. }
        | AgentRequest::CommunityLoginCode { .. }
        | AgentRequest::CommunityLoginCancel { .. }) => {
            let (bridge, proxy) = with_vault(state, false, |vault| {
                let mut proxy = state
                    .proxy
                    .lock()
                    .map_err(|_| ServiceError::internal(anyhow::anyhow!("proxy unavailable")))?;
                Ok((
                    proxy.community_bridge(),
                    proxy.load_config(vault)?.upstream_proxy,
                ))
            })?;
            let error = |e| ServiceError::new(AgentErrorCode::ValidationFailed, e);
            match request {
                AgentRequest::CommunityCatalog => {
                    bridge.catalog().map(AgentResponse::success).map_err(error)
                }
                AgentRequest::CommunityLoginStart { input } => bridge
                    .login_start(state, input, proxy)
                    .map(AgentResponse::success)
                    .map_err(error),
                AgentRequest::CommunityLoginPoll { ticket } => bridge
                    .login_poll(ticket)
                    .map(AgentResponse::success)
                    .map_err(error),
                AgentRequest::CommunityLoginCode { ticket, code } => bridge
                    .login_code(ticket, code.expose())
                    .map(|()| AgentResponse::empty())
                    .map_err(error),
                AgentRequest::CommunityLoginCancel { ticket } => bridge
                    .login_cancel(ticket)
                    .map(|()| AgentResponse::empty())
                    .map_err(error),
                _ => unreachable!(),
            }
        }
        AgentRequest::ClaudeBridgeMcp {
            capability,
            request,
        } => {
            if session_status(state)?.locked {
                return Err(ServiceError::new(AgentErrorCode::Locked, "vault is locked"));
            }
            let bridge = state
                .proxy
                .lock()
                .map_err(|_| ServiceError::internal(anyhow::anyhow!("proxy lock poisoned")))?
                .claude_bridge();
            bridge
                .mcp(capability.expose(), request)
                .map(AgentResponse::success)
                .map_err(|message| ServiceError::new(AgentErrorCode::PermissionDenied, message))
        }
        AgentRequest::ControlPanelStatus => {
            state.control_panel.status().map(AgentResponse::success)
        }
        AgentRequest::ControlPanelConfigure {
            settings,
            certificate,
            regenerate_certificate,
        } => with_vault(state, false, |vault| {
            crate::control_panel::ControlPanel::configure(
                state,
                vault.vault_id(),
                settings,
                certificate,
                regenerate_certificate,
            )
        })
        .map(AgentResponse::success),
        AgentRequest::ControlPanelStop => {
            crate::control_panel::ControlPanel::stop(state).map(AgentResponse::success)
        }
        AgentRequest::ControlPanelDisableRemoteUnlock => {
            crate::control_panel::ControlPanel::disable_remote_unlock(state)
                .map(AgentResponse::success)
        }
        AgentRequest::ControlPanelRotateAccessCode {
            allow_remote_unlock,
        } => with_vault(state, false, |vault| {
            crate::control_panel::ControlPanel::rotate_access_code(
                state,
                vault,
                allow_remote_unlock,
            )
        })
        .map(AgentResponse::success),
        AgentRequest::CloudKitExchange {
            completion,
            changed,
        } => {
            if changed {
                state.sync_wake.fetch_add(1, Ordering::Relaxed);
            }
            state
                .cloudkit
                .exchange(completion)
                .map(AgentResponse::success)
                .map_err(ServiceError::internal)
        }
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
        AgentRequest::LanguageSettingsGet { legacy_locale } => Ok(AgentResponse::success(
            crate::language::load(&state.vault_dir, legacy_locale)?,
        )),
        AgentRequest::LanguageSettingsSet { locale } => Ok(AgentResponse::success(
            crate::language::save(&state.vault_dir, locale)?,
        )),
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
        AgentRequest::ServerUsageSummary {
            days,
            timezone_offset_minutes,
            granularity,
        } => with_vault(state, true, |vault| {
            let mut proxy = state
                .proxy
                .lock()
                .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "proxy lock poisoned"))?;
            proxy.load_config(vault)?;
            let pricing = proxy.pricing_config(vault)?;
            let list_prices = crate::pricing::load_list_prices(&state.vault_dir);
            proxy.usage_summary(
                days.map(|days| {
                    aipass_proxy::usage_window_start(days, timezone_offset_minutes, granularity)
                }),
                &pricing,
                &list_prices,
            )
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
            granularity,
        } => with_vault(state, true, |vault| {
            let mut proxy = state
                .proxy
                .lock()
                .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "proxy lock poisoned"))?;
            proxy.load_config(vault)?;
            let pricing = proxy.pricing_config(vault)?;
            let list_prices = crate::pricing::load_list_prices(&state.vault_dir);
            proxy.usage_timeseries(
                days,
                timezone_offset_minutes,
                granularity,
                &pricing,
                &list_prices,
            )
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
                            secret
                                .endpoint
                                .clone()
                                .or_else(|| endpoint_url(&entry.endpoints)),
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
                if credentials
                    .iter()
                    .any(|(_, _, endpoint, _)| endpoint.is_some())
                {
                    let mut result = with_vault(state, true, |vault| {
                        crate::pricing::load_pricing_config(&state.vault_dir, vault)
                    })?;
                    for (secret_id, group, key_endpoint, secret) in &credentials {
                        let Some(endpoint) = key_endpoint.as_deref() else {
                            continue;
                        };
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
                for (secret_id, _, key_endpoint, secret) in &credentials {
                    let Some(endpoint) = key_endpoint.as_deref() else {
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
        AgentRequest::VaultCreate {
            password,
            local_only,
        } => {
            let response = create_vault(state, password.into_inner(), local_only)?;
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
            crate::vault_sync::import_file(state, &input, export_password)?;
            Ok(AgentResponse::success(json!({ "imported": true })))
        }
        AgentRequest::VaultImportSync { settings, password } => {
            crate::vault_sync::import_sync(state, settings, password)?;
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
        AgentRequest::ProviderRuntimeGet { id } => with_vault(state, true, |vault| {
            Ok(AgentResponse::success(crate::provider_runtime::view(
                crate::provider_runtime::load(vault, id)?,
            )))
        }),
        AgentRequest::ProviderRuntimeSet { id, options } => with_vault(state, true, |vault| {
            let view = crate::provider_runtime::save(vault, id, options)?;
            state
                .proxy
                .lock()
                .map_err(|_| ServiceError::internal(anyhow::anyhow!("proxy unavailable")))?
                .refresh_provider_credentials(vault, id)?;
            Ok(AgentResponse::success(view))
        }),
        AgentRequest::ProviderRuntimeProbe { id } => {
            crate::provider_runtime::probe(state, id).map(AgentResponse::success)
        }
        AgentRequest::ProviderWebhookTest { id, webhook_id } => {
            crate::provider_runtime::test_webhook(state, id, webhook_id)
                .map(|_| AgentResponse::empty())
        }
        AgentRequest::ProviderGet { id } => with_vault(state, true, |vault| {
            vault.get_provider_summary(id).map_err(map_vault_error)
        })
        .map(AgentResponse::success),
        AgentRequest::ProviderAdd { input } => with_vault(state, false, |vault| {
            vault.add_provider(input).map_err(map_vault_error)
        })
        .map(AgentResponse::success),
        AgentRequest::ProviderUpdate { id, input } => {
            // Snapshot under the vault lock, probe outside it, then compare before
            // committing. Forms cannot bypass recovery by sending true directly.
            let recovery = with_vault(state, false, |vault| {
                let mut entry = vault.get_provider_summary(id).map_err(map_vault_error)?;
                if input.supports_websockets != Some(true)
                    || entry.supports_websockets != Some(false)
                {
                    return Ok(None);
                }
                let revision = entry.updated_at;
                let outbound = state
                    .proxy
                    .lock()
                    .map_err(|_| ServiceError::internal(anyhow::anyhow!("proxy lock poisoned")))?
                    .config(vault)?
                    .upstream_proxy;
                let secret = zeroize::Zeroizing::new(match input.api_key.as_ref() {
                    Some(secret) => secret.clone(),
                    None => vault.reveal_secret(id).map_err(map_vault_error)?,
                });
                let headers = zeroize::Zeroizing::new(match &input.headers {
                    Some(headers) => headers.clone(),
                    None => vault.reveal_provider_headers(id).map_err(map_vault_error)?,
                });
                entry.endpoints.clone_from(&input.endpoints);
                entry.interface_type = input.interface_type.clone();
                if let Some(primary) = entry.secret_refs.first_mut() {
                    input.secret_metadata.apply_to(primary);
                }
                entry.auth_scheme = input.auth_scheme.clone();
                entry.default_model.clone_from(&input.default_model);
                entry.provider_id.clone_from(&input.provider_id);
                entry.provider_kind = input.provider_kind.clone();
                if let Some(kind) = &input.credential_kind {
                    entry.credential_kind = *kind;
                }
                Ok(Some((revision, entry, secret, headers, outbound)))
            })?;
            let verified =
                if let Some((revision, entry, mut secret, mut headers, outbound)) = recovery {
                    let result = probe_entry(
                        entry,
                        std::mem::take(&mut *secret),
                        15,
                        std::mem::take(&mut *headers),
                        outbound.clone(),
                        Some(state),
                    );
                    if result.websocket.as_ref().and_then(|ws| ws.supported) != Some(true) {
                        return Err(ServiceError::new(
                            AgentErrorCode::ValidationFailed,
                            format!(
                                "websocket_probe_unconfirmed: status={}; {}",
                                result
                                    .websocket
                                    .as_ref()
                                    .and_then(|ws| ws.status)
                                    .map(|status| status.to_string())
                                    .unwrap_or_else(|| "unknown".into()),
                                result
                                    .websocket
                                    .as_ref()
                                    .and_then(|ws| ws.error.clone())
                                    .or(result.error)
                                    .unwrap_or_else(|| {
                                        "Responses WS support could not be confirmed".into()
                                    }),
                            ),
                        ));
                    }
                    Some((revision, outbound))
                } else {
                    None
                };
            with_vault(state, false, |vault| {
                if let Some((revision, outbound)) = verified {
                    let current = vault.get_provider_summary(id).map_err(map_vault_error)?;
                    let current_outbound = state
                        .proxy
                        .lock()
                        .map_err(|_| {
                            ServiceError::internal(anyhow::anyhow!("proxy lock poisoned"))
                        })?
                        .config(vault)?
                        .upstream_proxy;
                    if current.updated_at != revision || current_outbound != outbound {
                        return Err(ServiceError::new(
                            AgentErrorCode::Conflict,
                            "provider configuration changed during WS probe; retry saving",
                        ));
                    }
                } else if input.supports_websockets == Some(true)
                    && vault
                        .get_provider_summary(id)
                        .map_err(map_vault_error)?
                        .supports_websockets
                        == Some(false)
                {
                    return Err(ServiceError::new(
                        AgentErrorCode::Conflict,
                        "provider WS preference changed; reload before enabling",
                    ));
                }
                vault.update_provider(id, input).map_err(map_vault_error)?;
                refresh_proxy_provider_credentials(state, vault, id)?;
                Ok(())
            })?;
            state.sync_revision.fetch_add(1, Ordering::Relaxed);
            state.sync_wake.fetch_add(1, Ordering::Relaxed);
            Ok(AgentResponse::empty())
        }
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
        AgentRequest::SecretRevealId { id, secret_id } => with_vault(state, true, |vault| {
            vault
                .reveal_secret_by_id(id, &secret_id)
                .map_err(map_vault_error)
        })
        .map(|secret| {
            AgentResponse::success(SecretValue {
                secret: secret.into(),
            })
        }),
        AgentRequest::SecretRevealHeaders { id } => with_vault(state, true, |vault| {
            vault.reveal_provider_headers(id).map_err(map_vault_error)
        })
        .map(|headers| {
            AgentResponse::success(ProviderHeaderValues {
                headers: headers
                    .into_iter()
                    .map(|(name, value)| (name, SensitiveString::from(value)))
                    .collect(),
            })
        }),
        AgentRequest::SecretAdd {
            id,
            label,
            secret,
            metadata,
        } => with_vault(state, false, |vault| {
            let secret_id = vault
                .add_secret_with_metadata(
                    id,
                    label,
                    secret.into_inner(),
                    &metadata.unwrap_or_default(),
                )
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
            metadata,
        } => with_vault(state, false, |vault| {
            let mut updated = vault
                .update_secret(
                    id,
                    &secret_id,
                    &label,
                    secret.map(SensitiveString::into_inner),
                )
                .map_err(map_vault_error)?;
            if let Some(metadata) = metadata {
                updated |= vault
                    .set_secret_metadata(id, &secret_id, &metadata)
                    .map_err(map_vault_error)?;
            }
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
            let (entry, secret, headers, outbound) = with_vault(state, true, |vault| {
                let outbound = state
                    .proxy
                    .lock()
                    .map_err(|_| {
                        ServiceError::new(AgentErrorCode::Internal, "proxy lock poisoned")
                    })?
                    .config(vault)?
                    .upstream_proxy;
                Ok((
                    vault.get_provider_summary(id).map_err(map_vault_error)?,
                    vault.reveal_secret(id).map_err(map_vault_error)?,
                    vault.reveal_provider_headers(id).map_err(map_vault_error)?,
                    outbound,
                ))
            })?;
            let checked_secret = entry.secret_refs.first().cloned();
            let api = entry.credential_kind == aipass_provider_registry::CredentialKind::Api;
            let result = probe_entry(
                entry,
                secret,
                timeout_seconds.max(1),
                headers,
                outbound,
                Some(state),
            );
            if api {
                crate::tool_switch::remember_probe(
                    state,
                    id,
                    checked_secret.as_ref(),
                    result.ok,
                    result.status,
                );
            }
            Ok(AgentResponse::success(result))
        }
        AgentRequest::ProviderUsageProbe {
            id,
            secret_id,
            mode,
            timeout_seconds,
            base_url,
            access_token,
            user_id,
        } => {
            let (entry, secret) = with_vault(state, true, |vault| {
                let selector = secret_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                let secret = match selector {
                    Some(selector) => vault
                        .reveal_secret_field(id, selector)
                        .map_err(map_vault_error)?,
                    None => vault.reveal_secret(id).map_err(map_vault_error)?,
                };
                Ok((
                    vault.get_provider_summary(id).map_err(map_vault_error)?,
                    secret,
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
        AgentRequest::ProviderUsageApply {
            id,
            quota,
            gateway,
            source,
            subscription,
        } => {
            let usage_source = source.and_then(|source| {
                serde_json::to_value(source)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_string))
            });
            with_vault(state, false, |vault| {
                vault
                    .update_provider_usage(id, quota, gateway, usage_source.as_deref())
                    .map_err(map_vault_error)?;
                // A probe-sourced apply replaces the subscription snapshot
                // wholesale: an upstream that reports no subscription window
                // (e.g. SubAPI wallet mode) must clear a previously stored one.
                if subscription.is_some() || usage_source.is_some() {
                    vault
                        .update_provider_subscription(id, subscription)
                        .map_err(map_vault_error)?;
                }
                Ok(())
            })
            .map(|_| AgentResponse::empty())
        }
        AgentRequest::OfficialAccountsRefresh { provider_ids } => {
            crate::subscription_import::refresh_compat(state, provider_ids)
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
        request @ (AgentRequest::ToolConfigStatus { .. }
        | AgentRequest::ToolConfigLoginStart { .. }
        | AgentRequest::ToolConfigLoginPoll { .. }
        | AgentRequest::ToolConfigLoginCode { .. }
        | AgentRequest::ToolConfigLoginCancel { .. }
        | AgentRequest::ToolConfigPreview { .. }
        | AgentRequest::ToolConfigApply { .. }
        | AgentRequest::ToolConfigRollback { .. }
        | AgentRequest::ToolConfigProxyPreview { .. }
        | AgentRequest::ToolConfigProxyApply { .. }) => tools::handle(state, request),
        AgentRequest::SyncLocal { dir } => run_sync_local(state, &dir).map(AgentResponse::success),
        AgentRequest::SyncSettingsGet => load_sync_settings(&state.vault_dir)
            .map(|settings| AgentResponse::success(sync_settings_view(&settings)))
            .map_err(ServiceError::internal),
        AgentRequest::SyncSettingsSet { settings } => {
            let _sync = state
                .sync_lock
                .lock()
                .map_err(|_| ServiceError::internal(anyhow::anyhow!("sync lock poisoned")))?;
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
        AgentRequest::SyncConfigured => run_sync_configured(state).map(AgentResponse::success),
        AgentRequest::SyncCloud { provider } => {
            if provider == aipass_agent_protocol::CloudSyncProvider::ICloud {
                crate::vault_sync::run_cloudkit(state).map(AgentResponse::success)
            } else {
                let dir = cloud_sync_dir(provider).map_err(ServiceError::internal)?;
                run_sync_local(state, &dir).map(AgentResponse::success)
            }
        }
        AgentRequest::SyncWebDav {
            url,
            username,
            password,
        } => {
            let client =
                HttpWebDavClient::new(&url, username, password.map(|value| value.into_inner()))
                    .map_err(ServiceError::internal)?;
            Ok(AgentResponse::success(run_sync_webdav_target(
                state,
                &client,
                &format!("webdav:{url}"),
            )))
        }
        AgentRequest::SyncConflicts { dir, provider } => with_vault(state, true, |vault| {
            let mut conflicts = conflict_responses(ConflictScope::Vault, &state.vault_dir, vault)?;
            for id in
                crate::vault_sync::conflicts(&state.vault_dir).map_err(ServiceError::internal)?
            {
                let bytes = fs::read(
                    crate::vault_sync::cache_path(&state.vault_dir, &id)
                        .map_err(ServiceError::internal)?,
                )
                .map_err(ServiceError::internal)?;
                let snapshot =
                    aipass_vault::VaultSyncSnapshot::parse(&bytes).map_err(map_vault_error)?;
                conflicts.push(SyncConflictResponse {
                    scope: ConflictScope::Vault,
                    origin: "snapshot".into(),
                    conflict_path: PathBuf::from(format!("sync-cache/{id}.aipsnapshot")),
                    target_path: PathBuf::from("manifest.aipmanifest"),
                    object: aipass_sync::SyncObject {
                        object_id: Some(snapshot.header.vault_id),
                        object_type: "vault_snapshot".into(),
                        lamport: 0,
                        hash_hex: id,
                        etag: None,
                        updated_at: snapshot.created_at,
                        relative_path: PathBuf::from("manifest.aipmanifest"),
                    },
                    conflict_summary: None,
                    target_summary: None,
                    snapshot_summary: Some(
                        vault
                            .sync_snapshot_summary(&bytes)
                            .map_err(map_vault_error)?,
                    ),
                });
            }
            for id in
                crate::vault_sync::quarantined(&state.vault_dir).map_err(ServiceError::internal)?
            {
                conflicts.push(SyncConflictResponse {
                    scope: ConflictScope::Vault,
                    origin: "quarantine".into(),
                    conflict_path: PathBuf::from(format!("sync-quarantine/{id}.aipsnapshot")),
                    target_path: PathBuf::from("manifest.aipmanifest"),
                    object: aipass_sync::SyncObject {
                        object_id: None,
                        object_type: "invalid_snapshot".into(),
                        lamport: 0,
                        hash_hex: id,
                        etag: None,
                        updated_at: OffsetDateTime::now_utc(),
                        relative_path: PathBuf::from("manifest.aipmanifest"),
                    },
                    conflict_summary: None,
                    target_summary: None,
                    snapshot_summary: None,
                });
            }
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
        AgentRequest::SyncAcceptConflict { request }
            if request
                .conflict_path
                .extension()
                .and_then(|value| value.to_str())
                == Some("aipsnapshot") =>
        {
            let id = request
                .conflict_path
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            crate::vault_sync::resolve(state, id, true)?;
            Ok(AgentResponse::empty())
        }
        AgentRequest::SyncDiscardConflict { request }
            if request
                .conflict_path
                .extension()
                .and_then(|value| value.to_str())
                == Some("aipsnapshot") =>
        {
            let id = request
                .conflict_path
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            crate::vault_sync::resolve(state, id, false)?;
            Ok(AgentResponse::empty())
        }
        AgentRequest::SyncAcceptConflict { request } => with_vault_mut(state, true, |vault| {
            let root = conflict_root(&state.vault_dir, &request)?;
            accept_conflict_with_validator(&root, &request.conflict_path, &|bytes| {
                vault.validate_sync_object_bytes(bytes).map_err(Into::into)
            })
            .map_err(ServiceError::internal)?;
            vault.reload_from_disk().map_err(map_vault_error)?;
            reload_running_proxy(state, vault)?;
            state.sync_revision.fetch_add(1, Ordering::Relaxed);
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
                    "failed to remove proxy route references for provider {entry_id}; stopped proxy code={:?}",
                    err.code
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
        let formatted = time::OffsetDateTime::from_unix_timestamp(account.expires_at_ms / 1000)
            .ok()
            .and_then(|t| {
                t.format(&time::format_description::well_known::Rfc3339)
                    .ok()
            })
            .unwrap_or_default();
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
