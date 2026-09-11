use aipass_agent::{default_vault_dir, AgentClient, AgentClientConfig, AgentCommandError};
use aipass_agent_protocol::{
    AgentRequest, CloudSyncProvider, LockReason, OfficialAccountRefreshResult, ProbeResult,
    ProxyStatus, SecretValue, ServerTokenResponse, ServerUsageSummary, SessionStatus,
    ToolConfigApplyResponse, ToolConfigPreviewResponse, ToolConfigRequest, VaultCreateResponse,
};
use aipass_config_writers::endpoint_url;
use aipass_native_host::native_manifest;
use aipass_provider_registry::{
    match_provider_by_domain, provider_kind_for_id, AuthScheme, EndpointKind, InterfaceType,
    ProviderEndpoint, QuotaInfo,
};
use aipass_proxy::{ProxyConfig, ProxyRouteConfig, ProxyTargetConfig, RouteStrategy};
use aipass_storage::atomic_write_bytes;
use aipass_vault::{ProviderEntryInput, ProviderEntryUpdateInput, SecretMetadataInput};
use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};
use rpassword::prompt_password;
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use uuid::Uuid;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

mod cli_types;
mod dispatch;
mod support;
mod type_conversions;

use cli_types::*;
use support::*;

fn main() -> Result<()> {
    dispatch::run(Cli::parse())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENTRY_ID: &str = "00000000-0000-0000-0000-000000000001";

    #[test]
    fn credential_alias_parses_as_add() {
        let cli = Cli::try_parse_from([
            "aipass",
            "credential",
            "--title",
            "Gateway",
            "--interface",
            "openai-compatible",
            "--auth",
            "bearer",
            "--api-key",
            "test-key",
        ])
        .expect("credential alias should parse");

        assert!(matches!(cli.command, Command::Add { .. }));
    }

    #[test]
    fn add_accepts_repeatable_endpoints() {
        let cli = Cli::try_parse_from([
            "aipass",
            "add",
            "--title",
            "Gateway",
            "--endpoint",
            "https://api.example.test/v1",
            "--endpoint",
            "https://api-2.example.test/v1",
            "--endpoint",
            "https://api-3.example.test/v1",
            "--console-url",
            "https://console.example.test",
            "--interface",
            "openai-compatible",
            "--auth",
            "bearer",
            "--api-key",
            "test-key",
        ])
        .expect("repeatable endpoints should parse");

        let Command::Add {
            endpoint,
            console_url,
            ..
        } = cli.command
        else {
            panic!("expected add command");
        };
        let endpoints = endpoints_from_cli(endpoint, console_url).expect("valid endpoints");
        assert_eq!(endpoints.len(), 4);
        assert_eq!(endpoints[0].id, "api");
        assert_eq!(endpoints[3].kind, EndpointKind::Console);
    }

    #[test]
    fn endpoint_query_commas_are_preserved() {
        let endpoints = endpoints_from_cli(
            vec!["https://api.example.test/v1?ids=a,b".to_string()],
            vec![],
        )
        .expect("query commas should be valid URL content");
        assert_eq!(endpoints.len(), 1);
        assert_eq!(
            endpoints[0].url.as_deref(),
            Some("https://api.example.test/v1?ids=a,b")
        );
    }

    #[test]
    fn endpoint_validation_rejects_non_http_urls() {
        let error = endpoints_from_cli(vec!["file:///tmp/provider".to_string()], vec![])
            .expect_err("non-http endpoint should fail");
        assert!(error.to_string().contains("absolute HTTP or HTTPS"));
    }

    #[test]
    fn switch_accepts_every_supported_agent_application() {
        for tool in [
            "codex",
            "claude-code",
            "gemini-cli",
            "opencode",
            "grok",
            "pi",
            "cursor",
        ] {
            let cli = Cli::try_parse_from(["aipass", "switch", tool, ENTRY_ID])
                .expect("supported agent application should parse");
            assert!(matches!(cli.command, Command::Switch { .. }));
        }
    }

    #[test]
    fn official_account_refresh_accepts_repeated_provider_filters() {
        let cli = Cli::try_parse_from([
            "aipass",
            "accounts",
            "refresh",
            "--provider",
            "openai",
            "--provider",
            "anthropic",
        ])
        .expect("account refresh should parse");

        let Command::Accounts {
            command: AccountsCommand::Refresh { provider_ids },
        } = cli.command
        else {
            panic!("expected accounts refresh command");
        };
        assert_eq!(provider_ids, ["openai", "anthropic"]);
    }
}
