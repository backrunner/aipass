use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Parser)]
#[command(
    name = "aipass",
    version,
    about = "Local-first AI Provider credential manager"
)]
pub struct Cli {
    #[arg(long, global = true)]
    pub json: bool,
    #[arg(long, global = true, env = "AIPASS_VAULT_DIR")]
    pub vault: Option<PathBuf>,
    #[arg(long, global = true, env = "AIPASS_MASTER_PASSWORD")]
    pub password: Option<String>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    Doctor,
    Completions {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    Vault {
        #[command(subcommand)]
        command: VaultCommand,
    },
    Secret {
        #[command(subcommand)]
        command: SecretCommand,
    },
    Accounts {
        #[command(subcommand)]
        command: AccountsCommand,
    },
    NativeHost {
        #[command(subcommand)]
        command: NativeHostCommand,
    },
    Agent {
        #[command(subcommand)]
        command: AgentSubcommand,
    },
    Proxy {
        #[command(subcommand)]
        command: ProxyCommand,
    },
    /// Unlock the vault with master password
    Unlock,
    Lock,
    Init {
        #[arg(long)]
        password: Option<String>,
    },
    /// Add a provider credential to the vault.
    #[command(visible_aliases = ["credential", "credentials"])]
    Add {
        #[arg(long)]
        title: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        domain: Vec<String>,
        #[arg(long)]
        endpoint: Vec<String>,
        #[arg(long = "console-url")]
        console_url: Vec<String>,
        #[arg(long)]
        favicon_url: Option<String>,
        #[arg(long, value_enum, default_value = "api")]
        credential_kind: CredentialKindArg,
        #[arg(long)]
        account_identity: Option<String>,
        #[arg(long, value_enum)]
        interface: InterfaceArg,
        #[arg(long, value_enum)]
        auth: AuthArg,
        #[arg(long, env = "AIPASS_INPUT_API_KEY")]
        api_key: String,
        #[arg(long)]
        secret_label: Option<String>,
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
        group: Option<String>,
        #[arg(long)]
        billing_rate: Option<String>,
        #[arg(long)]
        billing_currency: Option<String>,
        #[arg(long)]
        billing_unit_price: Option<String>,
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
        endpoint: Vec<String>,
        #[arg(long = "console-url")]
        console_url: Vec<String>,
        #[arg(long)]
        favicon_url: Option<String>,
        #[arg(long, value_enum)]
        credential_kind: Option<CredentialKindArg>,
        #[arg(long)]
        account_identity: Option<String>,
        #[arg(long, value_enum)]
        interface: Option<InterfaceArg>,
        #[arg(long, value_enum)]
        auth: Option<AuthArg>,
        #[arg(long, env = "AIPASS_INPUT_API_KEY")]
        api_key: Option<String>,
        #[arg(long)]
        secret_label: Option<String>,
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
        group: Option<String>,
        #[arg(long)]
        billing_rate: Option<String>,
        #[arg(long)]
        billing_currency: Option<String>,
        #[arg(long)]
        billing_unit_price: Option<String>,
        #[arg(long)]
        notes: Option<String>,
        #[arg(long)]
        tag: Vec<String>,
        #[arg(long)]
        max_concurrent_requests: Option<u32>,
        #[arg(long)]
        supports_websockets: Option<bool>,
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
        id: String,
        #[arg(long, value_enum, default_value = "helper")]
        mode: ConfigureMode,
        #[arg(long, value_enum)]
        codex_api_key_mode: Option<CodexApiKeyModeArg>,
        #[arg(long)]
        yes: bool,
    },
    Switch {
        #[arg(value_enum)]
        tool: ToolArg,
        id: String,
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
pub enum NativeHostCommand {
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
pub enum AgentSubcommand {
    Install,
    Uninstall,
    Status,
    Start,
    Stop,
}

#[derive(Subcommand)]
pub enum ProxyCommand {
    Status,
    Start,
    Stop,
    ConfigGet,
    ConfigSet {
        #[arg(long)]
        file: PathBuf,
    },
    RouteList,
    RouteCreate {
        #[arg(long)]
        name: String,
        #[arg(long)]
        provider_id: Uuid,
        #[arg(long)]
        secret_id: String,
        #[arg(long, value_enum, default_value = "open_ai_responses")]
        inbound_protocol: ProxyProtocolArg,
        #[arg(long, value_enum, default_value = "open_ai_responses")]
        upstream_protocol: ProxyProtocolArg,
        #[arg(long, default_value = "false")]
        conversion_enabled: bool,
    },
    RouteDelete {
        route_id: Uuid,
    },
    RouteSetEnabled {
        route_id: Uuid,
        #[arg(long)]
        enabled: bool,
    },
    RouteSelect {
        route_id: Uuid,
    },
    TokenRotate {
        route_id: Uuid,
    },
    Logs {
        #[arg(long, default_value = "100")]
        limit: usize,
    },
    Usage {
        #[arg(long)]
        days: Option<u32>,
        #[arg(long, default_value = "0")]
        timezone_offset_minutes: i32,
    },
    UsageClear,
    GroupList,
    GroupSwitch {
        route_id: Uuid,
        group: String,
    },
    GroupEnable {
        route_id: Uuid,
        group: String,
    },
    GroupDisable {
        route_id: Uuid,
        group: String,
    },
    TargetList {
        route_id: Uuid,
    },
    TargetAdd {
        route_id: Uuid,
        #[arg(long)]
        provider_id: Uuid,
        #[arg(long)]
        secret_id: String,
        #[arg(long)]
        group: Option<String>,
        #[arg(long, default_value = "100")]
        priority: u16,
        #[arg(long, default_value = "1")]
        weight: u32,
    },
    TargetRemove {
        route_id: Uuid,
        target_id: Uuid,
    },
    TargetEnable {
        route_id: Uuid,
        target_id: Uuid,
    },
    TargetDisable {
        route_id: Uuid,
        target_id: Uuid,
    },
    TargetSetPriority {
        route_id: Uuid,
        target_id: Uuid,
        #[arg(long)]
        priority: u16,
    },
    TargetSetWeight {
        route_id: Uuid,
        target_id: Uuid,
        #[arg(long)]
        weight: u32,
    },
    RouteUpdateTarget {
        route_id: Uuid,
        #[arg(long)]
        target_index: usize,
        #[arg(long)]
        weight: Option<u32>,
    },
    RouteApply {
        #[arg(value_enum)]
        tool: ToolArg,
        route_id: Uuid,
    },
    ProviderUpdate {
        provider_id: Uuid,
        #[arg(long)]
        prefer_websocket: Option<bool>,
        #[arg(long)]
        max_concurrent_requests: Option<u32>,
    },
}

#[derive(Clone, ValueEnum)]
pub enum ProxyProtocolArg {
    AnthropicMessages,
    OpenAiResponses,
    OpenAiChatCompletions,
}

#[derive(Subcommand)]
pub enum VaultCommand {
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
pub enum SecretCommand {
    List {
        id: Uuid,
    },
    Add {
        id: Uuid,
        #[arg(long)]
        label: String,
        #[arg(long, env = "AIPASS_INPUT_API_KEY")]
        api_key: String,
        /// Gateway group this key belongs to (relay gateways hold one key per group).
        #[arg(long)]
        group: Option<String>,
        /// Wire format this key speaks; falls back to the provider's interface.
        #[arg(long, value_enum)]
        interface: Option<InterfaceArg>,
    },
    Remove {
        id: Uuid,
        #[arg(long)]
        label: String,
    },
}

#[derive(Subcommand)]
pub enum AccountsCommand {
    Refresh {
        #[arg(long = "provider")]
        provider_ids: Vec<String>,
    },
}

#[derive(Clone, ValueEnum)]
pub enum InterfaceArg {
    OpenaiCompatible,
    AnthropicMessages,
    Gemini,
    AzureOpenai,
    Bedrock,
    CustomHttp,
}

#[derive(Clone, ValueEnum)]
pub enum CredentialKindArg {
    Api,
    Oauth,
}

#[derive(Clone, ValueEnum)]
pub enum AuthArg {
    Bearer,
    XApiKey,
    GoogleApiKey,
    AzureApiKey,
    AwsProfile,
    CustomHeader,
}

#[derive(Clone, ValueEnum)]
pub enum ToolArg {
    Codex,
    ClaudeCode,
    GeminiCli,
    #[value(name = "opencode")]
    OpenCode,
    Grok,
    Pi,
    Cursor,
}

#[derive(Clone, ValueEnum)]
pub enum EnvFormat {
    Shell,
    Json,
}

#[derive(Clone, ValueEnum)]
pub enum ConfigureMode {
    Official,
    Helper,
    Env,
    Plaintext,
}

#[derive(Clone, ValueEnum)]
pub enum CodexApiKeyModeArg {
    ExperimentalBearerToken,
    AuthJson,
}

#[derive(Clone, ValueEnum)]
pub enum BrowserArg {
    Chrome,
    Chromium,
    Edge,
    Brave,
}
