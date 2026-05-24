use aipass_config_writers::{
    apply_plan_encrypted, endpoint_url, find_backup_by_operation, plan_claude_code, plan_codex,
    plan_gemini_cli, rollback_encrypted, ConfigPlan, ToolEntry, ToolId,
};
use aipass_crypto::SecretString;
use aipass_native_host::native_manifest;
use aipass_provider_registry::{
    match_provider_by_domain, provider_kind_for_id, AuthScheme, InterfaceType, ProviderEndpoint,
    QuotaInfo,
};
use aipass_sync::{sync_local_folder, sync_webdav, HttpWebDavClient};
use aipass_vault::{EncryptedVaultExport, ProviderEntryInput, ProviderEntryUpdateInput, Vault};
use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{generate, Shell};
use directories::ProjectDirs;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use std::time::Duration;
use uuid::Uuid;

#[derive(Parser)]
#[command(
    name = "aipass",
    version,
    about = "Local-first AI Provider credential manager"
)]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[arg(long, global = true, env = "AIPASS_VAULT_DIR")]
    vault: Option<PathBuf>,
    #[arg(long, global = true, env = "AIPASS_MASTER_PASSWORD")]
    password: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Doctor,
    Completions {
        #[arg(value_enum)]
        shell: Shell,
    },
    Vault {
        #[command(subcommand)]
        command: VaultCommand,
    },
    Secret {
        #[command(subcommand)]
        command: SecretCommand,
    },
    NativeHost {
        #[command(subcommand)]
        command: NativeHostCommand,
    },
    Login,
    Lock,
    Init {
        #[arg(long)]
        password: Option<String>,
    },
    Add {
        #[arg(long)]
        title: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        domain: Vec<String>,
        #[arg(long)]
        endpoint: Option<String>,
        #[arg(long)]
        favicon_url: Option<String>,
        #[arg(long, value_enum)]
        interface: InterfaceArg,
        #[arg(long, value_enum)]
        auth: AuthArg,
        #[arg(long, env = "AIPASS_INPUT_API_KEY")]
        api_key: String,
        #[arg(long)]
        default_model: Option<String>,
        #[arg(long)]
        header: Vec<String>,
        #[arg(long)]
        quota_label: Option<String>,
        #[arg(long)]
        quota_limit: Option<String>,
        #[arg(long)]
        quota_remaining: Option<String>,
        #[arg(long)]
        quota_reset_at: Option<String>,
        #[arg(long)]
        notes: Option<String>,
        #[arg(long, default_value = "personal")]
        environment: String,
        #[arg(long)]
        tag: Vec<String>,
    },
    List {
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        archived: bool,
        #[arg(long)]
        all: bool,
    },
    Update {
        id: Uuid,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        domain: Vec<String>,
        #[arg(long)]
        endpoint: Option<String>,
        #[arg(long)]
        favicon_url: Option<String>,
        #[arg(long, value_enum)]
        interface: Option<InterfaceArg>,
        #[arg(long, value_enum)]
        auth: Option<AuthArg>,
        #[arg(long, env = "AIPASS_INPUT_API_KEY")]
        api_key: Option<String>,
        #[arg(long)]
        default_model: Option<String>,
        #[arg(long)]
        header: Vec<String>,
        #[arg(long)]
        quota_label: Option<String>,
        #[arg(long)]
        quota_limit: Option<String>,
        #[arg(long)]
        quota_remaining: Option<String>,
        #[arg(long)]
        quota_reset_at: Option<String>,
        #[arg(long)]
        notes: Option<String>,
        #[arg(long)]
        environment: Option<String>,
        #[arg(long)]
        tag: Vec<String>,
    },
    Archive {
        id: Uuid,
    },
    Restore {
        id: Uuid,
    },
    Delete {
        id: Uuid,
        #[arg(long)]
        yes: bool,
    },
    Search {
        query: String,
    },
    Probe {
        id: Uuid,
        #[arg(long, default_value_t = 15)]
        timeout_seconds: u64,
    },
    Get {
        id: Uuid,
        #[arg(long)]
        field: Option<String>,
        #[arg(long)]
        reveal: bool,
    },
    Copy {
        id: Uuid,
        #[arg(long, default_value = "api_key")]
        field: String,
    },
    Env {
        id: Uuid,
        #[arg(long, value_enum, default_value = "shell")]
        format: EnvFormat,
    },
    Exec {
        id: Uuid,
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
    Configure {
        #[arg(value_enum)]
        tool: ToolArg,
        id: Uuid,
        #[arg(long, value_enum, default_value = "helper")]
        mode: ConfigureMode,
        #[arg(long)]
        yes: bool,
    },
    Rollback {
        operation_id: Uuid,
    },
    Sync {
        #[arg(long)]
        dir: Option<PathBuf>,
        #[arg(long, env = "AIPASS_WEBDAV_URL")]
        webdav_url: Option<String>,
        #[arg(long, env = "AIPASS_WEBDAV_USERNAME")]
        webdav_username: Option<String>,
        #[arg(long, env = "AIPASS_WEBDAV_PASSWORD")]
        webdav_password: Option<String>,
    },
}

#[derive(Subcommand)]
enum NativeHostCommand {
    Manifest {
        #[arg(long)]
        host_path: Option<PathBuf>,
        #[arg(
            long = "extension-id",
            env = "AIPASS_EXTENSION_ID",
            value_delimiter = ',',
            required = true
        )]
        extension_id: Vec<String>,
    },
    Install {
        #[arg(long)]
        host_path: Option<PathBuf>,
        #[arg(
            long = "extension-id",
            env = "AIPASS_EXTENSION_ID",
            value_delimiter = ',',
            required = true
        )]
        extension_id: Vec<String>,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long, value_enum, default_value = "chrome")]
        browser: BrowserArg,
    },
}

#[derive(Subcommand)]
enum VaultCommand {
    Status,
    ChangePassword {
        #[arg(long)]
        new_password: String,
    },
    Rotate {
        #[arg(long, default_value = "manual.rotate")]
        reason: String,
    },
    Devices,
    RevokeDevice {
        id: Uuid,
    },
    Export {
        #[arg(long)]
        output: PathBuf,
        #[arg(long, env = "AIPASS_EXPORT_PASSWORD")]
        export_password: String,
    },
    Import {
        #[arg(long)]
        input: PathBuf,
        #[arg(long, env = "AIPASS_EXPORT_PASSWORD")]
        export_password: String,
    },
}

#[derive(Subcommand)]
enum SecretCommand {
    List {
        id: Uuid,
    },
    Add {
        id: Uuid,
        #[arg(long)]
        label: String,
        #[arg(long, env = "AIPASS_INPUT_API_KEY")]
        api_key: String,
    },
    Remove {
        id: Uuid,
        #[arg(long)]
        label: String,
    },
}

#[derive(Clone, ValueEnum)]
enum InterfaceArg {
    OpenaiCompatible,
    AnthropicMessages,
    Gemini,
    AzureOpenai,
    Bedrock,
    CustomHttp,
}

#[derive(Clone, ValueEnum)]
enum AuthArg {
    Bearer,
    XApiKey,
    GoogleApiKey,
    AzureApiKey,
    AwsProfile,
    CustomHeader,
}

#[derive(Clone, ValueEnum)]
enum ToolArg {
    Codex,
    ClaudeCode,
    GeminiCli,
}

#[derive(Clone, ValueEnum)]
enum EnvFormat {
    Shell,
    Json,
}

#[derive(Clone, ValueEnum)]
enum ConfigureMode {
    Helper,
    Env,
}

#[derive(Clone, ValueEnum)]
enum BrowserArg {
    Chrome,
    Chromium,
    Edge,
    Brave,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Doctor => output(
            cli.json,
            serde_json::json!({
                "ok": true,
                "vaultDir": vault_dir(cli.vault)?.display().to_string(),
                "authSource": if cli.password.is_some() { "env_or_flag" } else { "missing" },
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
                let dir = vault_dir(cli.vault)?;
                output(
                    cli.json,
                    serde_json::json!({
                        "exists": dir.join("manifest.aipmanifest").exists(),
                        "vaultDir": dir,
                    }),
                    if dir.join("manifest.aipmanifest").exists() {
                        "Vault exists"
                    } else {
                        "Vault not initialized"
                    },
                )
            }
            VaultCommand::ChangePassword { new_password } => {
                let mut vault = open_vault(cli.vault, cli.password)?;
                vault.change_master_password(&SecretString::new(new_password))?;
                output(
                    cli.json,
                    serde_json::json!({ "ok": true, "epoch": vault.current_epoch() }),
                    "Master password changed",
                )
            }
            VaultCommand::Rotate { reason } => {
                let mut vault = open_vault(cli.vault, cli.password)?;
                let epoch = vault.advance_epoch_and_rewrap(&reason)?;
                output(
                    cli.json,
                    serde_json::json!({ "ok": true, "epoch": epoch }),
                    "Vault epoch rotated",
                )
            }
            VaultCommand::Devices => {
                let vault = open_vault(cli.vault, cli.password)?;
                let devices = vault.list_devices()?;
                output(
                    cli.json,
                    serde_json::to_value(&devices)?,
                    &format!("{} devices", devices.len()),
                )
            }
            VaultCommand::RevokeDevice { id } => {
                let mut vault = open_vault(cli.vault, cli.password)?;
                vault.revoke_device(id)?;
                output(
                    cli.json,
                    serde_json::json!({ "ok": true, "revokedDeviceId": id, "epoch": vault.current_epoch() }),
                    "Device revoked and vault epoch rotated",
                )
            }
            VaultCommand::Export {
                output: export_path,
                export_password,
            } => {
                let vault = open_vault(cli.vault, cli.password)?;
                let export = vault.export_encrypted(&SecretString::new(export_password))?;
                if let Some(parent) = export_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&export_path, serde_json::to_vec_pretty(&export)?)?;
                output(
                    cli.json,
                    serde_json::json!({ "ok": true, "output": export_path, "vaultId": export.vault_id }),
                    "Encrypted vault export written",
                )
            }
            VaultCommand::Import {
                input,
                export_password,
            } => {
                let export: EncryptedVaultExport = serde_json::from_slice(&fs::read(&input)?)?;
                let dir = vault_dir(cli.vault)?;
                Vault::import_encrypted(&dir, &SecretString::new(export_password), &export)?;
                output(
                    cli.json,
                    serde_json::json!({ "ok": true, "vaultDir": dir, "vaultId": export.vault_id }),
                    "Encrypted vault import restored",
                )
            }
        },
        Command::Secret { command } => match command {
            SecretCommand::List { id } => {
                let vault = open_vault(cli.vault, cli.password)?;
                let entry = find_entry(&vault, id)?;
                output(
                    cli.json,
                    serde_json::to_value(&entry.secret_refs)?,
                    &format!("{} secrets", entry.secret_refs.len()),
                )
            }
            SecretCommand::Add { id, label, api_key } => {
                let vault = open_vault(cli.vault, cli.password)?;
                let secret_id = vault.add_secret(id, label, api_key)?;
                output(
                    cli.json,
                    serde_json::json!({ "ok": true, "id": id, "secretId": secret_id }),
                    "Secret added",
                )
            }
            SecretCommand::Remove { id, label } => {
                let vault = open_vault(cli.vault, cli.password)?;
                vault.remove_secret(id, &label)?;
                output(
                    cli.json,
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
                fs::write(&install_path, serde_json::to_vec_pretty(&manifest)?)?;
                install_native_manifest_reference(&browser, &install_path)?;
                output(
                    cli.json,
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
        Command::Login => {
            let vault = open_vault(cli.vault, cli.password)?;
            output(
                cli.json,
                serde_json::json!({ "ok": true, "vaultId": vault.vault_id(), "epoch": vault.current_epoch() }),
                "Vault unlocked for this command",
            )
        }
        Command::Lock => output(
            cli.json,
            serde_json::json!({ "ok": true, "locked": true }),
            "No persistent CLI session is active",
        ),
        Command::Init { password } => {
            let password = password
                .or(cli.password)
                .context("provide --password or AIPASS_MASTER_PASSWORD")?;
            let dir = vault_dir(cli.vault)?;
            let creation = Vault::create(&dir, &SecretString::new(password))?;
            let recovery_key = creation.recovery_kit.recovery_key;
            let text = format!(
                "Vault created\nRecovery key (shown once): {recovery_key}\nStore this key offline; it cannot be shown again."
            );
            output(
                cli.json,
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
            let vault = open_vault(cli.vault, cli.password)?;
            let provider_guess = provider.or_else(|| {
                domain.first().and_then(|domain| {
                    match_provider_by_domain(domain).map(|provider| provider.id.to_string())
                })
            });
            let endpoints = endpoint
                .map(ProviderEndpoint::api)
                .into_iter()
                .collect::<Vec<_>>();
            let id = vault.add_provider(ProviderEntryInput {
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
                quota: quota_from_parts(quota_label, quota_limit, quota_remaining, quota_reset_at),
                tags: tag,
                environment,
                notes,
            })?;
            output(
                cli.json,
                serde_json::json!({ "id": id }),
                &format!("Added provider {id}"),
            )
        }
        Command::List {
            provider,
            archived,
            all,
        } => {
            let vault = open_vault(cli.vault, cli.password)?;
            let mut items = if all {
                vault
                    .list_provider_summaries()?
                    .into_iter()
                    .chain(vault.list_archived_provider_summaries()?)
                    .collect::<Vec<_>>()
            } else if archived {
                vault.list_archived_provider_summaries()?
            } else {
                vault.list_provider_summaries()?
            };
            if let Some(provider) = provider {
                items.retain(|item| item.provider_id.as_deref() == Some(provider.as_str()));
            }
            let len = items.len();
            output(
                cli.json,
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
            let vault = open_vault(cli.vault, cli.password)?;
            let existing = find_entry(&vault, id)?;
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
            vault.update_provider(id, input)?;
            output(
                cli.json,
                serde_json::json!({ "ok": true, "id": id }),
                "Provider updated",
            )
        }
        Command::Archive { id } => {
            let vault = open_vault(cli.vault, cli.password)?;
            vault.archive_provider(id)?;
            output(
                cli.json,
                serde_json::json!({ "ok": true, "id": id, "archived": true }),
                "Provider archived",
            )
        }
        Command::Restore { id } => {
            let vault = open_vault(cli.vault, cli.password)?;
            vault.restore_provider(id)?;
            output(
                cli.json,
                serde_json::json!({ "ok": true, "id": id, "archived": false }),
                "Provider restored",
            )
        }
        Command::Delete { id, yes } => {
            if !yes {
                anyhow::bail!("permanent delete requires --yes");
            }
            let vault = open_vault(cli.vault, cli.password)?;
            vault.delete_provider_permanently(id)?;
            output(
                cli.json,
                serde_json::json!({ "ok": true, "id": id, "deleted": true }),
                "Provider permanently deleted",
            )
        }
        Command::Search { query } => {
            let vault = open_vault(cli.vault, cli.password)?;
            let items = vault.search(&query)?;
            let len = items.len();
            output(
                cli.json,
                serde_json::to_value(&items)?,
                &format!("{len} matches"),
            )
        }
        Command::Probe {
            id,
            timeout_seconds,
        } => {
            let vault = open_vault(cli.vault, cli.password)?;
            let item = find_entry(&vault, id)?;
            let secret = vault.reveal_secret(id)?;
            let result = probe_entry(&item, &secret, timeout_seconds)?;
            output(
                cli.json,
                serde_json::to_value(&result)?,
                if result.ok {
                    "Probe succeeded"
                } else {
                    "Probe failed"
                },
            )
        }
        Command::Get { id, reveal, field } => {
            let vault = open_vault(cli.vault, cli.password)?;
            let field = field.unwrap_or_else(|| "api_key".to_string());
            if reveal && is_secret_field(&field) {
                let secret = vault.reveal_secret_field(id, secret_label_for_field(&field))?;
                output(
                    cli.json,
                    serde_json::json!({ "id": id, "field": field, "secret": secret }),
                    &secret,
                )
            } else {
                let item = find_entry(&vault, id)?;
                let value = field_value(&item, &field)?;
                output(
                    cli.json,
                    serde_json::json!({ "id": id, "field": field, "value": value }),
                    &value,
                )
            }
        }
        Command::Copy { id, field } => {
            let vault = open_vault(cli.vault, cli.password)?;
            let value = if is_secret_field(&field) {
                vault.reveal_secret_field(id, secret_label_for_field(&field))?
            } else {
                let item = find_entry(&vault, id)?;
                field_value(&item, &field)?
            };
            copy_to_clipboard(&value)?;
            output(
                cli.json,
                serde_json::json!({ "ok": true, "id": id, "field": field }),
                "Value copied to clipboard",
            )
        }
        Command::Env { id, format } => {
            let vault = open_vault(cli.vault, cli.password)?;
            let item = find_entry(&vault, id)?;
            let secret = vault.reveal_secret(id)?;
            let key = env_key_for_entry(&item);
            match format {
                EnvFormat::Json => output(
                    cli.json || matches!(format, EnvFormat::Json),
                    serde_json::json!({ key.clone(): secret }),
                    "",
                ),
                EnvFormat::Shell => {
                    let text = format!("export {}={}", key, shell_quote(&secret));
                    output(cli.json, serde_json::json!({ "env": text }), &text)
                }
            }
        }
        Command::Exec { id, command } => {
            let vault = open_vault(cli.vault, cli.password)?;
            let item = find_entry(&vault, id)?;
            let secret = vault.reveal_secret(id)?;
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
            let vault = open_vault(cli.vault, cli.password)?;
            let item = find_entry(&vault, id)?;
            let home = std::env::var("HOME")
                .map(PathBuf::from)
                .context("HOME missing")?;
            let entry = ToolEntry {
                id,
                title: item.title,
                provider_id: item.provider_id,
                endpoint: endpoint_url(&item.endpoints),
                interface_type: item.interface_type,
                auth_scheme: item.auth_scheme,
                env_key: env_key_for(tool.clone()),
            };
            let (plan, content) = match mode {
                ConfigureMode::Helper => match tool {
                    ToolArg::Codex => plan_codex(&home, &entry)?,
                    ToolArg::ClaudeCode => plan_claude_code(&home, &entry)?,
                    ToolArg::GeminiCli => plan_gemini_cli(&home, &entry)?,
                },
                ConfigureMode::Env => plan_tool_env_helper(&home, tool.clone(), &entry)?,
            };
            if !yes {
                return output(cli.json, serde_json::to_value(&plan)?, &plan.preview);
            }
            let result = apply_plan_encrypted(&plan, &content, &vault.config_backup_key())?;
            output(
                cli.json,
                serde_json::to_value(&result)?,
                "Configuration applied",
            )
        }
        Command::Rollback { operation_id } => {
            let vault = open_vault(cli.vault, cli.password)?;
            let home = std::env::var("HOME")
                .map(PathBuf::from)
                .context("HOME missing")?;
            let backup = find_backup_by_operation(&home, operation_id)?;
            let result = rollback_encrypted(&backup, &vault.config_backup_key())?;
            output(cli.json, serde_json::to_value(&result)?, "Rollback applied")
        }
        Command::Sync {
            dir,
            webdav_url,
            webdav_username,
            webdav_password,
        } => {
            let vault_root = vault_dir(cli.vault)?;
            let report = if let Some(url) = webdav_url {
                let client = HttpWebDavClient::new(&url, webdav_username, webdav_password)?;
                sync_webdav(&vault_root, &client)?
            } else {
                let dir = dir.context("provide --dir for local/iCloud sync or --webdav-url")?;
                sync_local_folder(&vault_root, &dir)?
            };
            output(cli.json, serde_json::to_value(&report)?, "Sync complete")
        }
    }
}

fn vault_dir(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    let dirs =
        ProjectDirs::from("dev", "aipass", "AIPass").context("cannot determine project dir")?;
    Ok(dirs.data_dir().join("vault"))
}

fn native_host_binary_path(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return absolute_path(path);
    }
    let exe = std::env::current_exe().context("cannot determine current executable")?;
    let host_name = if cfg!(target_os = "windows") {
        "aipass-native-host.exe"
    } else {
        "aipass-native-host"
    };
    let sibling = exe.with_file_name(host_name);
    if sibling.exists() {
        return absolute_path(sibling);
    }
    absolute_path(PathBuf::from(host_name))
}

fn absolute_path(path: PathBuf) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path);
    }
    Ok(std::env::current_dir()?.join(path))
}

fn allowed_origins(extension_ids: &[String]) -> Result<Vec<String>> {
    extension_ids
        .iter()
        .map(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                anyhow::bail!("empty extension id");
            }
            if trimmed.starts_with("chrome-extension://") {
                return Ok(if trimmed.ends_with('/') {
                    trimmed.to_string()
                } else {
                    format!("{trimmed}/")
                });
            }
            Ok(format!("chrome-extension://{trimmed}/"))
        })
        .collect()
}

fn default_native_manifest_path(browser: &BrowserArg) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        let vendor_dir = match browser {
            BrowserArg::Chrome => "Google/Chrome",
            BrowserArg::Chromium => "Chromium",
            BrowserArg::Edge => "Microsoft Edge",
            BrowserArg::Brave => "BraveSoftware/Brave-Browser",
        };
        Some(
            home.join("Library")
                .join("Application Support")
                .join(vendor_dir)
                .join("NativeMessagingHosts")
                .join("dev.aipass.native.json"),
        )
    }

    #[cfg(target_os = "linux")]
    {
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        let vendor_dir = match browser {
            BrowserArg::Chrome => "google-chrome",
            BrowserArg::Chromium => "chromium",
            BrowserArg::Edge => "microsoft-edge",
            BrowserArg::Brave => "BraveSoftware/Brave-Browser",
        };
        Some(
            home.join(".config")
                .join(vendor_dir)
                .join("NativeMessagingHosts")
                .join("dev.aipass.native.json"),
        )
    }

    #[cfg(target_os = "windows")]
    {
        let app_data = std::env::var_os("APPDATA").map(PathBuf::from)?;
        Some(
            app_data
                .join("AIPass")
                .join("NativeMessagingHosts")
                .join("dev.aipass.native.json"),
        )
    }
}

fn install_native_manifest_reference(browser: &BrowserArg, manifest_path: &PathBuf) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        let key = match browser {
            BrowserArg::Chrome => {
                r"HKCU\Software\Google\Chrome\NativeMessagingHosts\dev.aipass.native"
            }
            BrowserArg::Chromium => {
                r"HKCU\Software\Chromium\NativeMessagingHosts\dev.aipass.native"
            }
            BrowserArg::Edge => {
                r"HKCU\Software\Microsoft\Edge\NativeMessagingHosts\dev.aipass.native"
            }
            BrowserArg::Brave => {
                r"HKCU\Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\dev.aipass.native"
            }
        };
        let status = ProcessCommand::new("reg")
            .args([
                "add",
                key,
                "/ve",
                "/t",
                "REG_SZ",
                "/d",
                &manifest_path.display().to_string(),
                "/f",
            ])
            .status()
            .context("failed to register native host")?;
        if !status.success() {
            anyhow::bail!("native host registry update failed");
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (browser, manifest_path);
    }

    Ok(())
}

fn browser_name(browser: &BrowserArg) -> &'static str {
    match browser {
        BrowserArg::Chrome => "chrome",
        BrowserArg::Chromium => "chromium",
        BrowserArg::Edge => "edge",
        BrowserArg::Brave => "brave",
    }
}

fn open_vault(path: Option<PathBuf>, password: Option<String>) -> Result<Vault> {
    let password = password.context("provide AIPASS_MASTER_PASSWORD or --password")?;
    Vault::open(vault_dir(path)?, &SecretString::new(password)).map_err(Into::into)
}

fn output(json: bool, value: serde_json::Value, text: &str) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!("{text}");
    }
    Ok(())
}

fn parse_headers(values: &[String]) -> Result<Vec<(String, String)>> {
    values
        .iter()
        .map(|value| {
            let (name, header_value) = value
                .split_once('=')
                .context("headers must use name=value format")?;
            let name = name.trim();
            if name.is_empty() {
                anyhow::bail!("header name cannot be empty");
            }
            Ok((name.to_string(), header_value.trim().to_string()))
        })
        .collect()
}

fn quota_from_parts(
    label: Option<String>,
    limit: Option<String>,
    remaining: Option<String>,
    reset_at: Option<String>,
) -> Option<QuotaInfo> {
    if label.is_none() && limit.is_none() && remaining.is_none() && reset_at.is_none() {
        return None;
    }
    Some(QuotaInfo {
        label,
        limit,
        remaining,
        reset_at,
    })
}

fn field_value(item: &aipass_vault::EntrySummary, field: &str) -> Result<String> {
    match field {
        "api_key" | "secret" => Ok(item.masked_secret.clone()),
        "title" => Ok(item.title.clone()),
        "provider" | "provider_id" => Ok(item.provider_id.clone().unwrap_or_default()),
        "provider_kind" => Ok(format!("{:?}", item.provider_kind)),
        "domain" | "domains" => Ok(item.domains.join(",")),
        "endpoint" | "base_url" => Ok(endpoint_url(&item.endpoints).unwrap_or_default()),
        "interface" => Ok(format!("{:?}", item.interface_type)),
        "auth" => Ok(format!("{:?}", item.auth_scheme)),
        "default_model" => Ok(item.default_model.clone().unwrap_or_default()),
        "environment" => Ok(item.environment.clone()),
        "tags" => Ok(item.tags.join(",")),
        "notes" => Ok(item.notes.clone().unwrap_or_default()),
        "fingerprint" => Ok(item.fingerprint.clone()),
        other => anyhow::bail!("unsupported field: {other}"),
    }
}

fn is_secret_field(field: &str) -> bool {
    matches!(field, "api_key" | "secret") || item_label(field).is_some()
}

fn secret_label_for_field(field: &str) -> &str {
    item_label(field).unwrap_or("primary")
}

fn item_label(field: &str) -> Option<&str> {
    field
        .strip_prefix("secret:")
        .or_else(|| field.strip_prefix("key:"))
        .filter(|label| !label.is_empty())
}

fn env_key_for(tool: ToolArg) -> String {
    match tool {
        ToolArg::Codex => "AIPASS_API_KEY".to_string(),
        ToolArg::ClaudeCode => "ANTHROPIC_API_KEY".to_string(),
        ToolArg::GeminiCli => "GEMINI_API_KEY".to_string(),
    }
}

fn env_key_for_entry(item: &aipass_vault::EntrySummary) -> String {
    match item.provider_id.as_deref() {
        Some("anthropic") => "ANTHROPIC_API_KEY".to_string(),
        Some("gemini") => "GEMINI_API_KEY".to_string(),
        Some("openrouter") => "OPENROUTER_API_KEY".to_string(),
        Some("deepseek") => "DEEPSEEK_API_KEY".to_string(),
        Some("moonshot") => "MOONSHOT_API_KEY".to_string(),
        Some("qwen") => "DASHSCOPE_API_KEY".to_string(),
        Some("zhipu") => "ZHIPUAI_API_KEY".to_string(),
        Some("volcengine") => "ARK_API_KEY".to_string(),
        Some("groq") => "GROQ_API_KEY".to_string(),
        Some("together") => "TOGETHER_API_KEY".to_string(),
        Some("fireworks") => "FIREWORKS_API_KEY".to_string(),
        _ => match item.auth_scheme {
            AuthScheme::GoogleApiKey => "GEMINI_API_KEY".to_string(),
            AuthScheme::AzureApiKey => "AZURE_OPENAI_API_KEY".to_string(),
            _ => "AIPASS_API_KEY".to_string(),
        },
    }
}

fn find_entry(vault: &Vault, id: Uuid) -> Result<aipass_vault::EntrySummary> {
    vault
        .list_provider_summaries()?
        .into_iter()
        .chain(vault.list_archived_provider_summaries()?)
        .find(|item| item.id == id)
        .context("entry not found")
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ProbeResult {
    ok: bool,
    provider_id: Option<String>,
    interface_type: InterfaceType,
    status: Option<u16>,
    endpoint: Option<String>,
    model_count: Option<usize>,
    error: Option<String>,
}

fn probe_entry(
    item: &aipass_vault::EntrySummary,
    secret: &str,
    timeout_seconds: u64,
) -> Result<ProbeResult> {
    let endpoint = endpoint_url(&item.endpoints);
    let Some(endpoint) = endpoint.clone() else {
        return Ok(ProbeResult {
            ok: false,
            provider_id: item.provider_id.clone(),
            interface_type: item.interface_type.clone(),
            status: None,
            endpoint,
            model_count: None,
            error: Some("endpoint missing".to_string()),
        });
    };
    if matches!(
        item.interface_type,
        InterfaceType::Bedrock | InterfaceType::CustomHttp
    ) {
        return Ok(ProbeResult {
            ok: false,
            provider_id: item.provider_id.clone(),
            interface_type: item.interface_type.clone(),
            status: None,
            endpoint: Some(endpoint),
            model_count: None,
            error: Some("probe not available for this interface".to_string()),
        });
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(timeout_seconds.max(1)))
        .user_agent("AIPass/1.0 provider-probe")
        .build()?;
    let (url, request) = match item.interface_type {
        InterfaceType::OpenAiCompatible | InterfaceType::AzureOpenAi => {
            let url = format!("{}/models", endpoint.trim_end_matches('/'));
            let mut request = client.get(&url);
            match item.auth_scheme {
                AuthScheme::AzureApiKey => request = request.header("api-key", secret),
                _ => request = request.bearer_auth(secret),
            }
            (url, request)
        }
        InterfaceType::AnthropicMessages => {
            let url = format!("{}/v1/models", endpoint.trim_end_matches('/'));
            let request = client
                .get(&url)
                .header("x-api-key", secret)
                .header("anthropic-version", "2023-06-01");
            (url, request)
        }
        InterfaceType::Gemini => {
            let url = format!(
                "{}/v1beta/models?key={}",
                endpoint.trim_end_matches('/'),
                secret
            );
            let safe_url = format!(
                "{}/v1beta/models?key=[redacted]",
                endpoint.trim_end_matches('/')
            );
            (safe_url, client.get(&url))
        }
        InterfaceType::Bedrock | InterfaceType::CustomHttp => unreachable!(),
    };
    match request.send() {
        Ok(response) => {
            let status = response.status().as_u16();
            let body = response.text().unwrap_or_default();
            Ok(ProbeResult {
                ok: (200..300).contains(&status),
                provider_id: item.provider_id.clone(),
                interface_type: item.interface_type.clone(),
                status: Some(status),
                endpoint: Some(url),
                model_count: count_models(&body),
                error: None,
            })
        }
        Err(err) => Ok(ProbeResult {
            ok: false,
            provider_id: item.provider_id.clone(),
            interface_type: item.interface_type.clone(),
            status: None,
            endpoint: Some(url),
            model_count: None,
            error: Some(redact_probe_error(&err.to_string())),
        }),
    }
}

fn count_models(body: &str) -> Option<usize> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    value
        .get("data")
        .or_else(|| value.get("models"))
        .and_then(|value| value.as_array())
        .map(Vec::len)
}

fn redact_probe_error(value: &str) -> String {
    if value.contains("sk-")
        || value.contains("AIza")
        || value.contains("key=")
        || value.to_lowercase().contains("authorization")
        || value.to_lowercase().contains("api-key")
    {
        "[redacted]".to_string()
    } else {
        value.to_string()
    }
}

fn plan_tool_env_helper(
    home: &std::path::Path,
    tool: ToolArg,
    entry: &ToolEntry,
) -> Result<(ConfigPlan, String)> {
    let tool_id = match tool {
        ToolArg::Codex => ToolId::Codex,
        ToolArg::ClaudeCode => ToolId::ClaudeCode,
        ToolArg::GeminiCli => ToolId::GeminiCli,
    };
    let tool_name = match tool {
        ToolArg::Codex => "codex",
        ToolArg::ClaudeCode => "claude-code",
        ToolArg::GeminiCli => "gemini-cli",
    };
    let target = home
        .join(".aipass")
        .join("tools")
        .join(format!("{tool_name}.env"));
    let operation_id = Uuid::new_v4();
    let backup_path = target
        .parent()
        .unwrap_or(home)
        .join(".aipass-backups")
        .join(format!(
            "{}-{}.aipbackup",
            operation_id,
            time::OffsetDateTime::now_utc().unix_timestamp()
        ));
    let mut content = format!(
        "# Generated by AIPass. This file stores helper references, not plaintext secrets.\n{}=\"$(aipass get {} --field api_key --reveal)\"\n",
        entry.env_key, entry.id
    );
    if let Some(endpoint) = &entry.endpoint {
        content.push_str(&format!("AIPASS_BASE_URL={}\n", shell_quote(endpoint)));
    }
    let plan = ConfigPlan {
        operation_id,
        tool: tool_id,
        target_path: target,
        backup_path,
        summary: format!("Configure {tool_name} env helper for {}", entry.title),
        preview: content.clone(),
    };
    Ok((plan, content))
}

fn copy_to_clipboard(secret: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let mut child = ProcessCommand::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("pbcopy unavailable")?;

    #[cfg(target_os = "windows")]
    let mut child = ProcessCommand::new("clip")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("clip unavailable")?;

    #[cfg(all(unix, not(target_os = "macos")))]
    let mut child = ProcessCommand::new("sh")
        .arg("-c")
        .arg("command -v wl-copy >/dev/null && wl-copy || xclip -selection clipboard")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("wl-copy/xclip unavailable")?;

    if let Some(stdin) = child.stdin.as_mut() {
        use std::io::Write;
        stdin.write_all(secret.as_bytes())?;
    }
    let status = child.wait()?;
    if !status.success() {
        anyhow::bail!("clipboard command failed");
    }
    Ok(())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

impl From<InterfaceArg> for InterfaceType {
    fn from(value: InterfaceArg) -> Self {
        match value {
            InterfaceArg::OpenaiCompatible => InterfaceType::OpenAiCompatible,
            InterfaceArg::AnthropicMessages => InterfaceType::AnthropicMessages,
            InterfaceArg::Gemini => InterfaceType::Gemini,
            InterfaceArg::AzureOpenai => InterfaceType::AzureOpenAi,
            InterfaceArg::Bedrock => InterfaceType::Bedrock,
            InterfaceArg::CustomHttp => InterfaceType::CustomHttp,
        }
    }
}

impl From<AuthArg> for AuthScheme {
    fn from(value: AuthArg) -> Self {
        match value {
            AuthArg::Bearer => AuthScheme::Bearer,
            AuthArg::XApiKey => AuthScheme::XApiKey,
            AuthArg::GoogleApiKey => AuthScheme::GoogleApiKey,
            AuthArg::AzureApiKey => AuthScheme::AzureApiKey,
            AuthArg::AwsProfile => AuthScheme::AwsProfile,
            AuthArg::CustomHeader => AuthScheme::CustomHeader,
        }
    }
}
