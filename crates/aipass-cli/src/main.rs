use aipass_agent::{default_vault_dir, AgentClient, AgentClientConfig, AgentCommandError};
use aipass_agent_protocol::{
    AgentRequest, CloudSyncProvider, CodexApiKeyMode, LockReason, ProbeResult, SecretValue,
    SessionStatus, ToolConfigApplyResponse, ToolConfigPreviewResponse, ToolConfigRequest,
    VaultCreateResponse,
};
use aipass_config_writers::endpoint_url;
use aipass_native_host::native_manifest;
use aipass_provider_registry::{
    match_provider_by_domain, provider_kind_for_id, AuthScheme, EndpointKind, InterfaceType,
    ProviderEndpoint, QuotaInfo,
};
use aipass_storage::atomic_write_bytes;
use aipass_vault::{ProviderEntryInput, ProviderEntryUpdateInput};
use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{generate, Shell};
use rpassword::prompt_password;
use std::fs;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use uuid::Uuid;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

mod dispatch;

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
    Agent {
        #[command(subcommand)]
        command: AgentSubcommand,
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
        #[arg(long = "console-url")]
        console_url: Vec<String>,
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
        #[arg(long = "model-alias")]
        model_alias: Vec<String>,
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
        #[arg(long = "console-url")]
        console_url: Vec<String>,
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
        #[arg(long = "model-alias")]
        model_alias: Vec<String>,
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
    Inject {
        id: Uuid,
        #[arg(last = true, required = true)]
        command: Vec<String>,
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
        #[arg(long, value_enum)]
        codex_api_key_mode: Option<CodexApiKeyModeArg>,
        #[arg(long)]
        yes: bool,
    },
    Rollback {
        operation_id: Uuid,
    },
    Sync {
        #[arg(long)]
        dir: Option<PathBuf>,
        #[arg(long)]
        icloud: bool,
        #[arg(long)]
        onedrive: bool,
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
enum AgentSubcommand {
    Install,
    Uninstall,
    Status,
    Start,
    Stop,
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
    #[value(name = "opencode")]
    OpenCode,
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
    Plaintext,
}

#[derive(Clone, ValueEnum)]
enum CodexApiKeyModeArg {
    ExperimentalBearerToken,
    AuthJson,
}

#[derive(Clone, ValueEnum)]
enum BrowserArg {
    Chrome,
    Chromium,
    Edge,
    Brave,
}

fn main() -> Result<()> {
    dispatch::run(Cli::parse())
}

fn vault_dir(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    default_vault_dir()
}

struct CliAgent {
    client: AgentClient,
    password: Option<String>,
    interactive: bool,
}

impl CliAgent {
    fn from_parts(vault: Option<PathBuf>, password: Option<String>) -> Result<Self> {
        let config = AgentClientConfig::for_vault(vault_dir(vault)?)?;
        Ok(Self {
            client: AgentClient::new(config),
            password,
            interactive: std::io::stdin().is_terminal(),
        })
    }

    fn ensure_running(&self) -> Result<()> {
        self.client.ensure_running()
    }

    fn request<T: serde::de::DeserializeOwned>(&self, request: AgentRequest) -> Result<T> {
        self.ensure_running()?;
        match self.client.request::<T>(&request) {
            Ok(value) => Ok(value),
            Err(err) if err.is_locked() => {
                self.unlock_for_request()?;
                self.client
                    .request::<T>(&request)
                    .map_err(agent_error_to_anyhow)
            }
            Err(err) => Err(agent_error_to_anyhow(err)),
        }
    }

    fn request_no_unlock<T: serde::de::DeserializeOwned>(
        &self,
        request: AgentRequest,
    ) -> Result<T> {
        self.ensure_running()?;
        self.client
            .request::<T>(&request)
            .map_err(agent_error_to_anyhow)
    }

    fn unlock_for_request(&self) -> Result<SessionStatus> {
        let mut password = if let Some(password) = self.password.clone() {
            password
        } else if self.interactive {
            prompt_password("AIPass master password: ").context("failed to read master password")?
        } else {
            anyhow::bail!("vault is locked");
        };
        let response = self
            .client
            .request::<SessionStatus>(&AgentRequest::SessionUnlock {
                mode: aipass_agent_protocol::SessionUnlockMode::Password {
                    password: password.as_str().into(),
                },
            })
            .map_err(agent_error_to_anyhow);
        password.clear();
        response
    }
}

fn agent_error_to_anyhow(err: AgentCommandError) -> anyhow::Error {
    let message = match err.code {
        Some(code) => format!(
            "{}: {}",
            aipass_agent_protocol::error_code_name(&code),
            err.message
        ),
        None => err.message,
    };
    anyhow::anyhow!(message)
}

fn native_host_binary_path(explicit: Option<PathBuf>) -> Result<PathBuf> {
    let path = native_host_binary_candidate(explicit)?;
    ensure_native_host_binary_usable(&path)?;
    Ok(path)
}

fn native_host_binary_candidate(explicit: Option<PathBuf>) -> Result<PathBuf> {
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

#[derive(Clone, Debug)]
struct NativeHostBinaryStatus {
    exists: bool,
    usable: bool,
    error: Option<String>,
}

fn native_host_binary_status(path: &Path) -> NativeHostBinaryStatus {
    let Ok(metadata) = fs::metadata(path) else {
        return NativeHostBinaryStatus {
            exists: false,
            usable: false,
            error: Some("native host binary was not found".to_string()),
        };
    };
    if !metadata.is_file() {
        return NativeHostBinaryStatus {
            exists: true,
            usable: false,
            error: Some("native host path is not a file".to_string()),
        };
    }
    if metadata.len() == 0 {
        return NativeHostBinaryStatus {
            exists: true,
            usable: false,
            error: Some("native host binary is empty".to_string()),
        };
    }
    #[cfg(unix)]
    if metadata.permissions().mode() & 0o111 == 0 {
        return NativeHostBinaryStatus {
            exists: true,
            usable: false,
            error: Some("native host binary is not executable".to_string()),
        };
    }
    NativeHostBinaryStatus {
        exists: true,
        usable: true,
        error: None,
    }
}

fn ensure_native_host_binary_usable(path: &Path) -> Result<()> {
    let status = native_host_binary_status(path);
    if status.usable {
        Ok(())
    } else {
        anyhow::bail!(
            "native host binary is not usable at {}: {}",
            path.display(),
            status
                .error
                .unwrap_or_else(|| "unknown validation error".to_string())
        )
    }
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

fn manifest_exists(explicit_vault: Option<PathBuf>) -> Result<bool> {
    Ok(vault_dir(explicit_vault)?
        .join("manifest.aipmanifest")
        .exists())
}

fn install_agent_service(json: bool, explicit_vault: Option<PathBuf>) -> Result<()> {
    let vault_dir = vault_dir(explicit_vault)?;
    let agent_binary = aipass_agent::agent_binary_path()?;
    let namespace = aipass_agent::namespace_for_vault_dir(&vault_dir)?;
    let status = aipass_agent::install_agent_autostart(&agent_binary, &vault_dir)
        .context("failed to install AIPass agent autostart")?;
    output(
        json,
        serde_json::json!({
            "ok": true,
            "vaultDir": vault_dir,
            "agentBinary": agent_binary,
            "autostart": autostart_status_json(&status),
            "namespace": namespace,
        }),
        "Agent autostart installed",
    )
}

fn start_agent_service(json: bool, explicit_vault: Option<PathBuf>) -> Result<()> {
    let vault_dir = vault_dir(explicit_vault)?;
    let agent_binary = aipass_agent::agent_binary_path()?;
    let status = aipass_agent::install_agent_autostart(&agent_binary, &vault_dir)
        .context("failed to start AIPass agent autostart")?;
    let client = AgentClient::for_vault(vault_dir.clone())?;
    client.ensure_running()?;
    output(
        json,
        serde_json::json!({
            "ok": true,
            "vaultDir": vault_dir,
            "autostart": autostart_status_json(&status),
        }),
        "Agent autostart started",
    )
}

fn stop_agent_service(json: bool, explicit_vault: Option<PathBuf>) -> Result<()> {
    let vault_dir = vault_dir(explicit_vault)?;
    let status = aipass_agent::stop_agent_autostart(&vault_dir)
        .context("failed to stop AIPass agent autostart")?;
    output(
        json,
        serde_json::json!({
            "ok": true,
            "vaultDir": vault_dir,
            "autostart": autostart_status_json(&status),
        }),
        "Agent autostart stopped",
    )
}

fn uninstall_agent_service(json: bool, explicit_vault: Option<PathBuf>) -> Result<()> {
    let vault_dir = vault_dir(explicit_vault)?;
    let status = aipass_agent::uninstall_agent_autostart(&vault_dir)
        .context("failed to uninstall AIPass agent autostart")?;
    output(
        json,
        serde_json::json!({
            "ok": true,
            "vaultDir": vault_dir,
            "autostart": autostart_status_json(&status),
        }),
        "Agent autostart uninstalled",
    )
}

fn autostart_status_json(status: &aipass_agent::AgentAutostartStatus) -> serde_json::Value {
    serde_json::json!({
        "serviceName": &status.service_name,
        "registered": status.registered,
        "running": status.running,
        "installPath": &status.install_path,
        "supervisorPath": &status.supervisor_path,
        "agentBinary": &status.agent_binary,
    })
}

fn doctor_report(
    explicit_vault: Option<PathBuf>,
    auth_available: bool,
) -> Result<serde_json::Value> {
    let dir = vault_dir(explicit_vault)?;
    let manifest_path = dir.join("manifest.aipmanifest");
    let vault_exists = manifest_path.exists();
    let agent_status = AgentClientConfig::for_vault(dir.clone())
        .ok()
        .map(AgentClient::new)
        .and_then(|client| {
            client
                .request::<SessionStatus>(&AgentRequest::SessionStatus)
                .ok()
        });
    let native_host_binary = native_host_binary_candidate(None)?;
    let native_host_binary_status = native_host_binary_status(&native_host_binary);
    let native_host_binary_exists = native_host_binary_status.exists;
    let native_host_binary_usable = native_host_binary_status.usable;
    let native_hosts = native_host_browser_reports();
    let allowed_extension_ids = allowed_extension_ids_from_env();
    let configured_extension_ids =
        aipass_native_host::load_allowed_extension_ids().unwrap_or_default();
    let effective_extension_ids = if allowed_extension_ids.is_empty() {
        configured_extension_ids.clone()
    } else {
        allowed_extension_ids.clone()
    };
    let native_host_installed = native_hosts
        .as_array()
        .map(|items| {
            items.iter().any(|item| {
                item.get("manifestExists").and_then(|value| value.as_bool()) == Some(true)
            })
        })
        .unwrap_or(false);
    let checks = serde_json::json!([
        {
            "name": "vault_manifest",
            "ok": vault_exists,
            "message": if vault_exists { "vault manifest found" } else { "vault is not initialized" }
        },
        {
            "name": "agent",
            "ok": agent_status.is_some(),
            "message": if agent_status.is_some() { "agent responded" } else { "agent is not reachable" }
        },
        {
            "name": "native_host_binary",
            "ok": native_host_binary_usable,
            "message": if native_host_binary_usable {
                "native host binary is usable"
            } else {
                native_host_binary_status.error.as_deref().unwrap_or("native host binary is not usable")
            }
        },
        {
            "name": "native_host_manifest",
            "ok": native_host_installed,
            "message": if native_host_installed { "browser manifest installed" } else { "browser native host manifest is not installed" }
        },
        {
            "name": "extension_allowlist",
            "ok": !effective_extension_ids.is_empty(),
            "message": if effective_extension_ids.is_empty() { "extension id allowlist is empty" } else { "extension id allowlist configured" }
        }
    ]);
    Ok(serde_json::json!({
        "ok": checks
            .as_array()
            .map(|items| {
                items.iter().all(|item| {
                    item.get("ok").and_then(|value| value.as_bool()) == Some(true)
                })
            })
            .unwrap_or(false),
        "vaultDir": dir,
        "vaultManifest": manifest_path,
        "authSource": if auth_available { "env_or_flag" } else { "missing" },
        "agent": agent_status.map(|status| serde_json::json!({
            "reachable": true,
            "exists": status.exists,
            "locked": status.locked,
            "lastLockReason": status.last_lock_reason,
            "vaultNamespace": status.vault_namespace,
        })).unwrap_or_else(|| serde_json::json!({ "reachable": false })),
        "nativeHost": {
            "binaryPath": native_host_binary,
            "binaryExists": native_host_binary_exists,
            "binaryUsable": native_host_binary_usable,
            "binaryError": native_host_binary_status.error,
            "settingsPath": aipass_native_host::native_host_settings_path().ok(),
            "browsers": native_hosts,
        },
        "extensionAllowlist": {
            "env": allowed_extension_ids,
            "configured": configured_extension_ids,
            "effective": effective_extension_ids,
        },
        "checks": checks,
    }))
}

fn native_host_browser_reports() -> serde_json::Value {
    let browsers = [
        BrowserArg::Chrome,
        BrowserArg::Chromium,
        BrowserArg::Edge,
        BrowserArg::Brave,
    ];
    serde_json::Value::Array(
        browsers
            .into_iter()
            .filter_map(|browser| {
                let manifest_path = default_native_manifest_path(&browser)?;
                let manifest_exists = manifest_path.exists();
                let manifest = fs::read_to_string(&manifest_path)
                    .ok()
                    .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
                let allowed_origins = manifest
                    .as_ref()
                    .and_then(|value| value.get("allowed_origins"))
                    .and_then(|value| value.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.as_str().map(ToString::to_string))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                Some(serde_json::json!({
                    "browser": browser_name(&browser),
                    "manifestPath": manifest_path,
                    "manifestExists": manifest_exists,
                    "allowedOrigins": allowed_origins,
                }))
            })
            .collect(),
    )
}

fn allowed_extension_ids_from_env() -> Vec<String> {
    std::env::var("AIPASS_ALLOWED_EXTENSION_IDS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn doctor_text(report: &serde_json::Value, ok: bool) -> String {
    let vault = report
        .get("vaultDir")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    let agent = report
        .get("agent")
        .and_then(|value| value.get("reachable"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let native_host_count = report
        .get("nativeHost")
        .and_then(|value| value.get("browsers"))
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter(|item| {
                    item.get("manifestExists").and_then(|value| value.as_bool()) == Some(true)
                })
                .count()
        })
        .unwrap_or(0);
    let allowlist_count = report
        .get("extensionAllowlist")
        .and_then(|value| value.get("effective"))
        .and_then(|value| value.as_array())
        .map(Vec::len)
        .unwrap_or(0);
    format!(
        "AIPass doctor: {}\nVault: {vault}\nAgent: {}\nNative host manifests: {native_host_count}\nExtension allowlist ids: {allowlist_count}",
        if ok { "ok" } else { "issues found" },
        if agent { "reachable" } else { "not reachable" }
    )
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

fn endpoints_from_cli(
    endpoint: Option<String>,
    console_urls: Vec<String>,
) -> Vec<ProviderEndpoint> {
    endpoint
        .map(ProviderEndpoint::api)
        .into_iter()
        .chain(console_urls.into_iter().map(ProviderEndpoint::console))
        .collect()
}

fn update_endpoints_from_cli(
    existing: &[ProviderEndpoint],
    endpoint: Option<String>,
    console_urls: Vec<String>,
) -> Vec<ProviderEndpoint> {
    let mut endpoints = existing
        .iter()
        .filter(|item| {
            (endpoint.is_none() || item.kind != EndpointKind::Api)
                && (console_urls.is_empty() || item.kind != EndpointKind::Console)
        })
        .cloned()
        .collect::<Vec<_>>();
    if let Some(endpoint) = endpoint {
        endpoints.push(ProviderEndpoint::api(endpoint));
    }
    endpoints.extend(console_urls.into_iter().map(ProviderEndpoint::console));
    endpoints
}

fn parse_model_aliases(values: &[String]) -> Result<Vec<(String, String)>> {
    values
        .iter()
        .map(|value| {
            let (alias, model) = value
                .split_once('=')
                .context("model aliases must use alias=model format")?;
            let alias = alias.trim();
            let model = model.trim();
            if alias.is_empty() || model.is_empty() {
                anyhow::bail!("model alias and model cannot be empty");
            }
            Ok((alias.to_string(), model.to_string()))
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
        "console_url" | "console" => Ok(console_url(&item.endpoints).unwrap_or_default()),
        "interface" => Ok(format!("{:?}", item.interface_type)),
        "auth" => Ok(format!("{:?}", item.auth_scheme)),
        "default_model" => Ok(item.default_model.clone().unwrap_or_default()),
        "curl" | "curl_snippet" => Ok(curl_snippet_for_entry(item)),
        "env" | "env_export" => Ok(env_export_for_entry(item)),
        "config" | "config_snippet" => config_snippet_for_entry(item),
        "tags" => Ok(item.tags.join(",")),
        "notes" => Ok(item.notes.clone().unwrap_or_default()),
        "fingerprint" => Ok(item.fingerprint.clone()),
        other => anyhow::bail!("unsupported field: {other}"),
    }
}

fn console_url(endpoints: &[ProviderEndpoint]) -> Option<String> {
    endpoints
        .iter()
        .find(|endpoint| endpoint.kind == aipass_provider_registry::EndpointKind::Console)
        .and_then(|endpoint| endpoint.url.clone())
}

fn curl_snippet_for_entry(item: &aipass_vault::EntrySummary) -> String {
    let key = env_key_for_entry(item);
    let endpoint =
        endpoint_url(&item.endpoints).unwrap_or_else(|| "https://api.example.com".to_string());
    if matches!(item.interface_type, InterfaceType::Bedrock)
        || matches!(item.auth_scheme, AuthScheme::AwsProfile)
    {
        let region = item
            .endpoints
            .iter()
            .find_map(|endpoint| endpoint.region.as_deref())
            .unwrap_or("${AWS_REGION:-us-east-1}");
        return format!(
            "AWS_PROFILE=${{{key}:-default}} aws bedrock list-foundation-models --region {region}"
        );
    }
    match item.interface_type {
        InterfaceType::AnthropicMessages => format!(
            "curl -sS {}/v1/models -H 'x-api-key: ${}' -H 'anthropic-version: 2023-06-01'",
            endpoint.trim_end_matches('/'),
            key
        ),
        InterfaceType::Gemini => format!(
            "curl -sS '{}/v1beta/models?key=${}'",
            endpoint.trim_end_matches('/'),
            key
        ),
        InterfaceType::AzureOpenAi => format!(
            "curl -sS {}/models -H 'api-key: ${}'",
            endpoint.trim_end_matches('/'),
            key
        ),
        InterfaceType::OpenAiCompatible | InterfaceType::CustomHttp | InterfaceType::Bedrock => {
            let auth = auth_header_snippet(&item.auth_scheme, &key);
            format!("curl -sS {}/models {auth}", endpoint.trim_end_matches('/'))
        }
    }
}

fn auth_header_snippet(auth_scheme: &AuthScheme, key: &str) -> String {
    match auth_scheme {
        AuthScheme::Bearer => format!("-H 'Authorization: Bearer ${key}'"),
        AuthScheme::XApiKey => format!("-H 'x-api-key: ${key}'"),
        AuthScheme::GoogleApiKey | AuthScheme::AwsProfile => String::new(),
        AuthScheme::AzureApiKey => format!("-H 'api-key: ${key}'"),
        AuthScheme::CustomHeader => format!("-H 'Authorization: ${key}'"),
    }
}

fn env_export_for_entry(item: &aipass_vault::EntrySummary) -> String {
    let mut lines = vec![format!(
        "export {}=\"$(aipass get {} --field api_key --reveal)\"",
        env_key_for_entry(item),
        item.id
    )];
    if let Some(endpoint) = endpoint_url(&item.endpoints) {
        lines.push(format!("export AIPASS_BASE_URL={}", shell_quote(&endpoint)));
    }
    if let Some(model) = &item.default_model {
        lines.push(format!("export AIPASS_MODEL={}", shell_quote(model)));
    }
    lines.join("\n")
}

fn config_snippet_for_entry(item: &aipass_vault::EntrySummary) -> Result<String> {
    Ok(serde_json::to_string_pretty(&serde_json::json!({
        "provider": item.provider_id,
        "title": item.title,
        "interfaceType": item.interface_type,
        "authScheme": item.auth_scheme,
        "baseUrl": endpoint_url(&item.endpoints),
        "consoleUrl": console_url(&item.endpoints),
        "envKey": env_key_for_entry(item),
        "defaultModel": item.default_model,
        "modelAliases": item.model_aliases,
    }))?)
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
        Some("replicate") => "REPLICATE_API_TOKEN".to_string(),
        Some("together") => "TOGETHER_API_KEY".to_string(),
        Some("fireworks") => "FIREWORKS_API_KEY".to_string(),
        Some("bedrock") => "AWS_PROFILE".to_string(),
        _ => match item.auth_scheme {
            AuthScheme::GoogleApiKey => "GEMINI_API_KEY".to_string(),
            AuthScheme::AzureApiKey => "AZURE_OPENAI_API_KEY".to_string(),
            AuthScheme::AwsProfile => "AWS_PROFILE".to_string(),
            _ => "AIPASS_API_KEY".to_string(),
        },
    }
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

impl From<ToolArg> for aipass_agent_protocol::ToolConfigTool {
    fn from(value: ToolArg) -> Self {
        match value {
            ToolArg::Codex => Self::Codex,
            ToolArg::ClaudeCode => Self::ClaudeCode,
            ToolArg::GeminiCli => Self::GeminiCli,
            ToolArg::OpenCode => Self::OpenCode,
        }
    }
}

impl From<ConfigureMode> for aipass_agent_protocol::ToolConfigMode {
    fn from(value: ConfigureMode) -> Self {
        match value {
            ConfigureMode::Helper => Self::Helper,
            ConfigureMode::Env => Self::Env,
            ConfigureMode::Plaintext => Self::Plaintext,
        }
    }
}

impl From<CodexApiKeyModeArg> for CodexApiKeyMode {
    fn from(value: CodexApiKeyModeArg) -> Self {
        match value {
            CodexApiKeyModeArg::ExperimentalBearerToken => Self::ExperimentalBearerToken,
            CodexApiKeyModeArg::AuthJson => Self::AuthJson,
        }
    }
}
