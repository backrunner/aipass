use crate::*;

pub(crate) fn handle_credential_command(
    json: bool,
    vault: Option<PathBuf>,
    cli_password: Option<String>,
    command: Command,
) -> Result<()> {
    let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;

    match command {
        Command::Add {
            title,
            provider,
            domain,
            endpoint,
            console_url,
            favicon_url,
            credential_kind,
            account_identity,
            interface,
            auth,
            api_key,
            secret_label,
            default_model,
            model_alias,
            header,
            quota_label,
            quota_limit,
            quota_remaining,
            quota_reset_at,
            group,
            billing_rate,
            billing_currency,
            billing_unit_price,
            notes,
            tag,
        } => {
            let provider_guess = provider.or_else(|| {
                domain.first().and_then(|domain| {
                    match_provider_by_domain(domain).map(|provider| provider.id.to_string())
                })
            });
            let interface_type: InterfaceType = interface.into();
            let endpoints = endpoints_from_cli(endpoint, console_url)?;
            let id: Uuid = agent.request(AgentRequest::ProviderAdd {
                input: ProviderEntryInput {
                    max_concurrent_requests: None,
                    supports_websockets: None,
                    title,
                    provider_kind: provider_kind_for_id(provider_guess.as_deref()),
                    provider_id: provider_guess,
                    credential_kind: credential_kind.into(),
                    account_identity,
                    domains: domain,
                    favicon_url,
                    endpoints,
                    interface_type: interface_type.clone(),
                    auth_scheme: auth.into(),
                    api_key,
                    secret_label,
                    default_model,
                    model_aliases: parse_model_aliases(&model_alias)?,
                    headers: parse_headers(&header)?,
                    quota: quota_from_parts(
                        quota_label,
                        quota_limit,
                        None,
                        quota_remaining,
                        quota_reset_at,
                    ),
                    subscription: None,
                    gateway: None,
                    tags: tag,
                    notes,
                    secret_metadata: secret_metadata_from_cli(
                        group,
                        Some(interface_type),
                        billing_rate,
                        billing_currency,
                        billing_unit_price,
                    ),
                },
            })?;
            output(
                json,
                serde_json::json!({ "id": id }),
                &format!("Added provider {id}"),
            )
        }
        Command::List {
            provider,
            archived,
            all,
        } => {
            let mut items = if all {
                agent
                    .request::<Vec<aipass_vault::EntrySummary>>(AgentRequest::EntriesList {
                        archived: false,
                    })?
                    .into_iter()
                    .chain(agent.request::<Vec<aipass_vault::EntrySummary>>(
                        AgentRequest::EntriesList { archived: true },
                    )?)
                    .collect::<Vec<_>>()
            } else if archived {
                agent.request(AgentRequest::EntriesList { archived: true })?
            } else {
                agent.request(AgentRequest::EntriesList { archived: false })?
            };
            if let Some(provider) = provider {
                items.retain(|item| item.provider_id.as_deref() == Some(provider.as_str()));
            }
            let len = items.len();
            output(
                json,
                serde_json::to_value(&items)?,
                &format!("{len} providers"),
            )
        }
        Command::Update {
            id,
            title,
            provider,
            domain,
            endpoint,
            console_url,
            favicon_url,
            credential_kind,
            account_identity,
            interface,
            auth,
            api_key,
            secret_label,
            default_model,
            model_alias,
            header,
            quota_label,
            quota_limit,
            quota_remaining,
            quota_reset_at,
            group,
            billing_rate,
            billing_currency,
            billing_unit_price,
            notes,
            tag,
            max_concurrent_requests,
            supports_websockets,
        } => {
            let existing: aipass_vault::EntrySummary =
                agent.request(AgentRequest::ProviderGet { id })?;
            let domains = if domain.is_empty() {
                existing.domains.clone()
            } else {
                domain
            };
            let provider_guess = provider.or(existing.provider_id.clone()).or_else(|| {
                domains.first().and_then(|domain| {
                    match_provider_by_domain(domain).map(|provider| provider.id.to_string())
                })
            });
            let interface_changed = interface.is_some();
            let interface_type = interface
                .map(InterfaceType::from)
                .unwrap_or(existing.interface_type.clone());
            let secret_metadata = secret_metadata_from_cli(
                group,
                interface_changed.then_some(interface_type.clone()),
                billing_rate,
                billing_currency,
                billing_unit_price,
            );
            let input = ProviderEntryUpdateInput {
                max_concurrent_requests,
                supports_websockets,
                title: title.unwrap_or(existing.title),
                provider_kind: provider_kind_for_id(provider_guess.as_deref()),
                provider_id: provider_guess,
                credential_kind: credential_kind.map(Into::into),
                account_identity,
                domains,
                favicon_url: favicon_url.or(existing.favicon_url),
                endpoints: update_endpoints_from_cli(&existing.endpoints, endpoint, console_url)?,
                interface_type: interface_type.clone(),
                auth_scheme: auth.map(AuthScheme::from).unwrap_or(existing.auth_scheme),
                api_key,
                secret_label,
                default_model: default_model.or(existing.default_model),
                model_aliases: if model_alias.is_empty() {
                    existing.model_aliases
                } else {
                    parse_model_aliases(&model_alias)?
                },
                headers: if header.is_empty() {
                    None
                } else {
                    Some(parse_headers(&header)?)
                },
                quota: quota_from_parts(
                    quota_label,
                    quota_limit,
                    None,
                    quota_remaining,
                    quota_reset_at,
                )
                .or(existing.quota),
                subscription: None,
                gateway: existing.gateway,
                tags: if tag.is_empty() { existing.tags } else { tag },
                notes: notes.or(existing.notes),
                secret_metadata,
            };
            let _: serde_json::Value = agent.request(AgentRequest::ProviderUpdate { id, input })?;
            output(
                json,
                serde_json::json!({ "ok": true, "id": id }),
                "Provider updated",
            )
        }
        Command::Archive { id } => {
            let _: serde_json::Value = agent.request(AgentRequest::ProviderArchive { id })?;
            output(
                json,
                serde_json::json!({ "ok": true, "id": id, "archived": true }),
                "Provider archived",
            )
        }
        Command::Restore { id } => {
            let _: serde_json::Value = agent.request(AgentRequest::ProviderRestore { id })?;
            output(
                json,
                serde_json::json!({ "ok": true, "id": id, "archived": false }),
                "Provider restored",
            )
        }
        Command::Delete { id, yes } => {
            if !yes {
                anyhow::bail!("permanent delete requires --yes");
            }
            let _: serde_json::Value = agent.request(AgentRequest::ProviderDelete { id })?;
            output(
                json,
                serde_json::json!({ "ok": true, "id": id, "deleted": true }),
                "Provider permanently deleted",
            )
        }
        Command::Search { query } => {
            let items: Vec<aipass_vault::EntrySummary> =
                agent.request(AgentRequest::EntriesSearch { query })?;
            let len = items.len();
            output(
                json,
                serde_json::to_value(&items)?,
                &format!("{len} matches"),
            )
        }
        Command::Probe {
            id,
            timeout_seconds,
        } => {
            let result: ProbeResult = agent.request(AgentRequest::ProviderProbe {
                id,
                timeout_seconds,
            })?;
            output(
                json,
                serde_json::to_value(&result)?,
                if result.ok {
                    "Probe succeeded"
                } else {
                    "Probe failed"
                },
            )
        }
        Command::Get { id, reveal, field } => {
            let field = field.unwrap_or_else(|| "api_key".to_string());
            if reveal && is_secret_field(&field) {
                let secret: SecretValue = agent.request(AgentRequest::SecretRevealField {
                    id,
                    field: secret_label_for_field(&field).to_string(),
                })?;
                let secret = secret.secret.into_inner();
                output(
                    json,
                    serde_json::json!({ "id": id, "field": field, "secret": secret }),
                    &secret,
                )
            } else {
                let item: aipass_vault::EntrySummary =
                    agent.request(AgentRequest::ProviderGet { id })?;
                let value = field_value(&item, &field)?;
                output(
                    json,
                    serde_json::json!({ "id": id, "field": field, "value": value }),
                    &value,
                )
            }
        }
        Command::Copy { id, field } => {
            let value = if is_secret_field(&field) {
                agent
                    .request::<SecretValue>(AgentRequest::SecretRevealField {
                        id,
                        field: secret_label_for_field(&field).to_string(),
                    })?
                    .secret
                    .into_inner()
            } else {
                let item: aipass_vault::EntrySummary =
                    agent.request(AgentRequest::ProviderGet { id })?;
                field_value(&item, &field)?
            };
            copy_to_clipboard(&value)?;
            output(
                json,
                serde_json::json!({ "ok": true, "id": id, "field": field }),
                "Value copied to clipboard",
            )
        }
        Command::Env { id, format } => {
            let item: aipass_vault::EntrySummary =
                agent.request(AgentRequest::ProviderGet { id })?;
            let secret: SecretValue = agent.request(AgentRequest::SecretRevealField {
                id,
                field: "primary".to_string(),
            })?;
            let secret = secret.secret.into_inner();
            let key = env_key_for_entry(&item);
            match format {
                EnvFormat::Json => output(
                    json || matches!(format, EnvFormat::Json),
                    serde_json::json!({ key.clone(): secret }),
                    "",
                ),
                EnvFormat::Shell => {
                    let text = format!("export {}={}", key, shell_quote(&secret));
                    output(json, serde_json::json!({ "env": text }), &text)
                }
            }
        }
        Command::Exec { id, command } | Command::Inject { id, command } => {
            let item: aipass_vault::EntrySummary =
                agent.request(AgentRequest::ProviderGet { id })?;
            let secret: SecretValue = agent.request(AgentRequest::SecretRevealField {
                id,
                field: "primary".to_string(),
            })?;
            let secret = secret.secret.into_inner();
            let key = env_key_for_entry(&item);
            let (program, args) = command
                .split_first()
                .context("provide a command after --")?;
            let status = ProcessCommand::new(program)
                .args(args)
                .env(key, secret)
                .status()
                .context("failed to run child process")?;
            std::process::exit(status.code().unwrap_or(1));
        }
        Command::Configure {
            tool,
            id,
            mode,
            codex_api_key_mode,
            yes,
        }
        | Command::Switch {
            tool,
            id,
            mode,
            codex_api_key_mode,
            yes,
        } => {
            let id = resolve_entry_id(&agent, &id)?;
            let request = ToolConfigRequest {
                tool: tool.into(),
                id,
                mode: mode.into(),
                codex_api_key_mode: codex_api_key_mode.map(Into::into),
            };
            if !yes {
                let plan: ToolConfigPreviewResponse =
                    agent.request(AgentRequest::ToolConfigPreview { request })?;
                return output(json, serde_json::to_value(&plan)?, &plan.preview);
            }
            let result: ToolConfigApplyResponse =
                agent.request(AgentRequest::ToolConfigApply { request })?;
            output(
                json,
                serde_json::to_value(&result)?,
                "Configuration applied",
            )
        }
        Command::Rollback { operation_id } => {
            let result: serde_json::Value =
                agent.request(AgentRequest::ToolConfigRollback { operation_id })?;
            output(json, serde_json::to_value(&result)?, "Rollback applied")
        }
        Command::Sync {
            dir,
            icloud,
            onedrive,
            webdav_url,
            webdav_username,
            webdav_password,
        } => {
            let selection_count = usize::from(dir.is_some())
                + usize::from(icloud)
                + usize::from(onedrive)
                + usize::from(webdav_url.is_some());
            if selection_count > 1 {
                anyhow::bail!(
                    "choose exactly one sync target: --dir, --icloud, --onedrive, or --webdav-url"
                );
            }
            let report: aipass_sync::SyncReport = if let Some(url) = webdav_url {
                agent.request_no_unlock(AgentRequest::SyncWebDav {
                    url,
                    username: webdav_username,
                    password: webdav_password.map(Into::into),
                })?
            } else if icloud {
                agent.request_no_unlock(AgentRequest::SyncCloud {
                    provider: CloudSyncProvider::ICloud,
                })?
            } else if onedrive {
                agent.request_no_unlock(AgentRequest::SyncCloud {
                    provider: CloudSyncProvider::OneDrive,
                })?
            } else {
                let dir =
                    dir.context("provide one of --dir, --icloud, --onedrive, or --webdav-url")?;
                agent.request_no_unlock(AgentRequest::SyncLocal { dir })?
            };
            output(json, serde_json::to_value(&report)?, "Sync complete")
        }
        _ => unreachable!("credential command should only handle credential-related commands"),
    }
}
