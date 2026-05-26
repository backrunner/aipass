use super::*;

pub(crate) fn run(cli: Cli) -> Result<()> {
    let json = cli.json;
    let vault = cli.vault.clone();
    let cli_password = cli.password.clone();
    match cli.command {
        Command::Doctor => output(
            json,
            serde_json::json!({
                "ok": true,
                "vaultDir": vault_dir(vault.clone())?.display().to_string(),
                "authSource": if cli_password.is_some() { "env_or_flag" } else { "missing" },
            }),
            "AIPass doctor ok",
        ),
        Command::Completions { shell } => {
            let mut command = Cli::command();
            generate(shell, &mut command, "aipass", &mut io::stdout());
            Ok(())
        }
        Command::Vault { command } => match command {
            VaultCommand::Status => {
                let dir = vault_dir(vault.clone())?;
                let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
                let status = agent
                    .request_no_unlock::<SessionStatus>(AgentRequest::SessionStatus)
                    .unwrap_or(SessionStatus {
                        exists: dir.join("manifest.aipmanifest").exists(),
                        locked: true,
                        policy: Default::default(),
                        last_lock_reason: Some(LockReason::AgentRestart),
                        vault_namespace: None,
                    });
                output(
                    json,
                    serde_json::json!({
                        "exists": status.exists,
                        "locked": status.locked,
                        "policy": status.policy,
                        "vaultDir": dir,
                    }),
                    if status.exists {
                        if status.locked {
                            "Vault exists (locked)"
                        } else {
                            "Vault exists (unlocked)"
                        }
                    } else {
                        "Vault not initialized"
                    },
                )
            }
            VaultCommand::ChangePassword { new_password } => {
                let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
                let result: serde_json::Value =
                    agent.request(AgentRequest::VaultChangePassword {
                        new_password: new_password.into(),
                    })?;
                output(json, result, "Master password changed")
            }
            VaultCommand::Rotate { reason } => {
                let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
                let result: serde_json::Value =
                    agent.request(AgentRequest::VaultRotate { reason })?;
                output(json, result, "Vault epoch rotated")
            }
            VaultCommand::Devices => {
                let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
                let devices: Vec<aipass_vault::DeviceRecord> =
                    agent.request(AgentRequest::DevicesList)?;
                output(
                    json,
                    serde_json::to_value(&devices)?,
                    &format!("{} devices", devices.len()),
                )
            }
            VaultCommand::RevokeDevice { id } => {
                let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
                let result: serde_json::Value = agent.request(AgentRequest::DeviceRevoke { id })?;
                output(json, result, "Device revoked and vault epoch rotated")
            }
            VaultCommand::Export {
                output: export_path,
                export_password,
            } => {
                let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
                let result: serde_json::Value = agent.request(AgentRequest::VaultExport {
                    output: export_path.clone(),
                    export_password: export_password.into(),
                })?;
                output(json, result, "Encrypted vault export written")
            }
            VaultCommand::Import {
                input,
                export_password,
            } => {
                let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
                let dir = vault_dir(vault.clone())?;
                let export: serde_json::Value = agent.request(AgentRequest::VaultImport {
                    input,
                    export_password: export_password.into(),
                })?;
                output(
                    json,
                    serde_json::json!({ "ok": true, "vaultDir": dir, "result": export }),
                    "Encrypted vault import restored",
                )
            }
        },
        Command::Secret { command } => match command {
            SecretCommand::List { id } => {
                let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
                let entry: aipass_vault::EntrySummary =
                    agent.request(AgentRequest::ProviderGet { id })?;
                output(
                    json,
                    serde_json::to_value(&entry.secret_refs)?,
                    &format!("{} secrets", entry.secret_refs.len()),
                )
            }
            SecretCommand::Add { id, label, api_key } => {
                let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
                let secret_id: String = agent.request(AgentRequest::SecretAdd {
                    id,
                    label,
                    secret: api_key.into(),
                })?;
                output(
                    json,
                    serde_json::json!({ "ok": true, "id": id, "secretId": secret_id }),
                    "Secret added",
                )
            }
            SecretCommand::Remove { id, label } => {
                let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
                let _: serde_json::Value = agent.request(AgentRequest::SecretRemove {
                    id,
                    label: label.clone(),
                })?;
                output(
                    json,
                    serde_json::json!({ "ok": true, "id": id, "removed": label }),
                    "Secret removed",
                )
            }
        },
        Command::NativeHost { command } => match command {
            NativeHostCommand::Manifest {
                host_path,
                extension_id,
            } => {
                let host_path = native_host_binary_path(host_path)?;
                let origins = allowed_origins(&extension_id)?;
                let manifest = native_manifest(&host_path, &origins);
                println!("{}", serde_json::to_string_pretty(&manifest)?);
                Ok(())
            }
            NativeHostCommand::Install {
                host_path,
                extension_id,
                output: manifest_output,
                browser,
            } => {
                let host_path = native_host_binary_path(host_path)?;
                let origins = allowed_origins(&extension_id)?;
                let install_path = manifest_output.unwrap_or_else(|| {
                    default_native_manifest_path(&browser).expect("manifest path")
                });
                if let Some(parent) = install_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let manifest = native_manifest(&host_path, &origins);
                atomic_write_bytes(&install_path, &serde_json::to_vec_pretty(&manifest)?)?;
                install_native_manifest_reference(&browser, &install_path)?;
                output(
                    json,
                    serde_json::json!({
                        "ok": true,
                        "browser": browser_name(&browser),
                        "hostPath": host_path,
                        "manifestPath": install_path,
                        "allowedOrigins": origins,
                    }),
                    "Native messaging host installed",
                )
            }
        },
        Command::Agent { command } => match command {
            AgentSubcommand::Install => install_agent_service(json, vault.clone()),
            AgentSubcommand::Status => {
                #[cfg(target_os = "windows")]
                {
                    let dir = vault_dir(vault.clone())?;
                    let service = aipass_agent::query_windows_service(&dir)?;
                    let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
                    let status: SessionStatus = agent
                        .request_no_unlock(AgentRequest::SessionStatus)
                        .unwrap_or(SessionStatus {
                            exists: manifest_exists(vault.clone())?,
                            locked: true,
                            policy: Default::default(),
                            last_lock_reason: Some(LockReason::AgentRestart),
                            vault_namespace: None,
                        });
                    return output(
                        json,
                        serde_json::json!({
                            "registered": service.registered,
                            "running": service.running,
                            "serviceName": service.service_name,
                            "serviceState": service.state,
                            "session": status,
                        }),
                        if service.running {
                            if status.locked {
                                "Agent service running (locked)"
                            } else {
                                "Agent service running (unlocked)"
                            }
                        } else if service.registered {
                            "Agent service registered (stopped)"
                        } else {
                            "Agent service not installed"
                        },
                    );
                }

                let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
                let status: SessionStatus = agent
                    .request_no_unlock(AgentRequest::SessionStatus)
                    .unwrap_or(SessionStatus {
                        exists: manifest_exists(vault.clone())?,
                        locked: true,
                        policy: Default::default(),
                        last_lock_reason: Some(LockReason::AgentRestart),
                        vault_namespace: None,
                    });
                output(
                    json,
                    serde_json::to_value(&status)?,
                    if status.locked {
                        "Agent locked"
                    } else {
                        "Agent unlocked"
                    },
                )
            }
            AgentSubcommand::Start => {
                let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
                agent.ensure_running()?;
                output(json, serde_json::json!({ "ok": true }), "Agent started")
            }
            AgentSubcommand::Stop => {
                #[cfg(target_os = "windows")]
                {
                    let dir = vault_dir(vault.clone())?;
                    aipass_agent::stop_windows_service(&dir)?;
                    return output(
                        json,
                        serde_json::json!({ "ok": true }),
                        "Agent service stopped",
                    );
                }
                let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
                agent.client.shutdown()?;
                output(json, serde_json::json!({ "ok": true }), "Agent stopped")
            }
        },
        Command::Login => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            agent.ensure_running()?;
            let status = agent.unlock_for_request()?;
            output(json, serde_json::to_value(&status)?, "Vault unlocked")
        }
        Command::Lock => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let status: SessionStatus = agent.request_no_unlock(AgentRequest::SessionLock {
                reason: LockReason::Manual,
            })?;
            output(json, serde_json::to_value(&status)?, "Vault locked")
        }
        Command::Init { password } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            agent.ensure_running()?;
            let password = password
                .or(cli_password.clone())
                .context("provide --password or AIPASS_MASTER_PASSWORD")?;
            let dir = vault_dir(vault.clone())?;
            let creation: VaultCreateResponse =
                agent.request_no_unlock(AgentRequest::VaultCreate {
                    password: password.into(),
                })?;
            let recovery_key = creation.recovery_kit.recovery_key;
            let text = format!(
                "Vault created\nRecovery key (shown once): {recovery_key}\nStore this key offline; it cannot be shown again."
            );
            output(
                json,
                serde_json::json!({ "ok": true, "vaultDir": dir, "recoveryKey": recovery_key }),
                &text,
            )
        }
        Command::Add {
            title,
            provider,
            domain,
            endpoint,
            favicon_url,
            interface,
            auth,
            api_key,
            default_model,
            header,
            quota_label,
            quota_limit,
            quota_remaining,
            quota_reset_at,
            notes,
            environment,
            tag,
        } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let provider_guess = provider.or_else(|| {
                domain.first().and_then(|domain| {
                    match_provider_by_domain(domain).map(|provider| provider.id.to_string())
                })
            });
            let endpoints = endpoint
                .map(ProviderEndpoint::api)
                .into_iter()
                .collect::<Vec<_>>();
            let id: Uuid = agent.request(AgentRequest::ProviderAdd {
                input: ProviderEntryInput {
                    title,
                    provider_kind: provider_kind_for_id(provider_guess.as_deref()),
                    provider_id: provider_guess,
                    domains: domain,
                    favicon_url,
                    endpoints,
                    interface_type: interface.into(),
                    auth_scheme: auth.into(),
                    api_key,
                    default_model,
                    headers: parse_headers(&header)?,
                    quota: quota_from_parts(
                        quota_label,
                        quota_limit,
                        quota_remaining,
                        quota_reset_at,
                    ),
                    tags: tag,
                    environment,
                    notes,
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
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
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
            favicon_url,
            interface,
            auth,
            api_key,
            default_model,
            header,
            quota_label,
            quota_limit,
            quota_remaining,
            quota_reset_at,
            notes,
            environment,
            tag,
        } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
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
            let input = ProviderEntryUpdateInput {
                title: title.unwrap_or(existing.title),
                provider_kind: provider_kind_for_id(provider_guess.as_deref()),
                provider_id: provider_guess,
                domains,
                favicon_url: favicon_url.or(existing.favicon_url),
                endpoints: endpoint
                    .map(ProviderEndpoint::api)
                    .map(|endpoint| vec![endpoint])
                    .unwrap_or(existing.endpoints),
                interface_type: interface
                    .map(InterfaceType::from)
                    .unwrap_or(existing.interface_type),
                auth_scheme: auth.map(AuthScheme::from).unwrap_or(existing.auth_scheme),
                api_key,
                default_model: default_model.or(existing.default_model),
                headers: if header.is_empty() {
                    None
                } else {
                    Some(parse_headers(&header)?)
                },
                quota: quota_from_parts(quota_label, quota_limit, quota_remaining, quota_reset_at)
                    .or(existing.quota),
                tags: if tag.is_empty() { existing.tags } else { tag },
                environment: environment.unwrap_or(existing.environment),
                notes: notes.or(existing.notes),
            };
            let _: serde_json::Value = agent.request(AgentRequest::ProviderUpdate { id, input })?;
            output(
                json,
                serde_json::json!({ "ok": true, "id": id }),
                "Provider updated",
            )
        }
        Command::Archive { id } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let _: serde_json::Value = agent.request(AgentRequest::ProviderArchive { id })?;
            output(
                json,
                serde_json::json!({ "ok": true, "id": id, "archived": true }),
                "Provider archived",
            )
        }
        Command::Restore { id } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
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
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let _: serde_json::Value = agent.request(AgentRequest::ProviderDelete { id })?;
            output(
                json,
                serde_json::json!({ "ok": true, "id": id, "deleted": true }),
                "Provider permanently deleted",
            )
        }
        Command::Search { query } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
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
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
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
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
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
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
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
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
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
        Command::Exec { id, command } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
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
            yes,
        } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let request = ToolConfigRequest {
                tool: tool.into(),
                id,
                mode: mode.into(),
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
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
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
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
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
    }
}
