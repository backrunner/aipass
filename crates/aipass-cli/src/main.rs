use aipass_agent::{default_vault_dir, AgentClient, AgentClientConfig, AgentCommandError};
use aipass_agent_protocol::{
    AgentRequest, CloudSyncProvider, LockReason, ProbeResult, SecretValue, SessionStatus,
    ToolConfigApplyResponse, ToolConfigPreviewResponse, ToolConfigRequest, VaultCreateResponse,
};
use aipass_config_writers::endpoint_url;
use aipass_native_host::native_manifest;
use aipass_provider_registry::{
    match_provider_by_domain, provider_kind_for_id, AuthScheme, InterfaceType, ProviderEndpoint,
    QuotaInfo,
};
use aipass_storage::atomic_write_bytes;
use aipass_vault::{ProviderEntryInput, ProviderEntryUpdateInput};
use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{generate, Shell};
use rpassword::prompt_password;
use std::fs;
use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use uuid::Uuid;

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

fn manifest_exists(explicit_vault: Option<PathBuf>) -> Result<bool> {
    Ok(vault_dir(explicit_vault)?
        .join("manifest.aipmanifest")
        .exists())
}

fn install_agent_service(json: bool, explicit_vault: Option<PathBuf>) -> Result<()> {
    let vault_dir = vault_dir(explicit_vault)?;
    let agent_binary = agent_binary_path()?;
    let namespace = aipass_agent::namespace_for_vault_dir(&vault_dir)?;
    #[cfg(target_os = "linux")]
    let launch_command = format!(
        "\"{}\" --vault \"{}\"",
        agent_binary.display(),
        vault_dir.display()
    );

    #[cfg(target_os = "macos")]
    let install_path = {
        let home = std::env::var_os("HOME").context("HOME is not set")?;
        let path = PathBuf::from(home)
            .join("Library")
            .join("LaunchAgents")
            .join(format!("dev.aipass.agent.{namespace}.plist"));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>dev.aipass.agent.{namespace}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
    <string>--vault</string>
    <string>{}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
</dict>
</plist>
"#,
            xml_escape(&agent_binary.display().to_string()),
            xml_escape(&vault_dir.display().to_string()),
        );
        atomic_write_bytes(&path, plist.as_bytes())?;
        let _ = ProcessCommand::new("launchctl")
            .args(["unload", path.to_string_lossy().as_ref()])
            .status();
        let status = ProcessCommand::new("launchctl")
            .args(["load", path.to_string_lossy().as_ref()])
            .status()
            .context("failed to load LaunchAgent")?;
        if !status.success() {
            anyhow::bail!("launchctl load failed");
        }
        path
    };

    #[cfg(target_os = "linux")]
    let install_path = {
        let home = std::env::var_os("HOME").context("HOME is not set")?;
        let path = PathBuf::from(home)
            .join(".config")
            .join("systemd")
            .join("user")
            .join(format!("aipass-agent-{namespace}.service"));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let unit = format!(
            r#"[Unit]
Description=AIPass Agent ({namespace})

[Service]
ExecStart={launch_command}
Restart=on-failure
RestartSec=2

[Install]
WantedBy=default.target
"#
        );
        atomic_write_bytes(&path, unit.as_bytes())?;
        let reload = ProcessCommand::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status();
        if let Ok(status) = reload {
            if !status.success() {
                anyhow::bail!("systemctl --user daemon-reload failed");
            }
        }
        let enable = ProcessCommand::new("systemctl")
            .args([
                "--user",
                "enable",
                "--now",
                path.file_name()
                    .and_then(|value| value.to_str())
                    .context("invalid service name")?,
            ])
            .status();
        if let Ok(status) = enable {
            if !status.success() {
                anyhow::bail!("systemctl --user enable --now failed");
            }
        }
        path
    };

    #[cfg(target_os = "windows")]
    let install_path = {
        let status = aipass_agent::install_windows_service(&agent_binary, &vault_dir)
            .context("failed to register Windows agent service")?;
        PathBuf::from(format!(r"SCM\{}", status.service_name))
    };

    output(
        json,
        serde_json::json!({
            "ok": true,
            "vaultDir": vault_dir,
            "agentBinary": agent_binary,
            "installPath": install_path,
            "namespace": namespace,
        }),
        "Agent service installed",
    )
}

fn agent_binary_path() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("cannot determine current executable")?;
    let agent_name = if cfg!(target_os = "windows") {
        "aipass-agent.exe"
    } else {
        "aipass-agent"
    };
    let sibling = exe.with_file_name(agent_name);
    if sibling.exists() {
        return absolute_path(sibling);
    }
    absolute_path(PathBuf::from(agent_name))
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
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
