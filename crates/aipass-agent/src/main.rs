use aipass_agent::{agent_service_name, default_vault_dir, run_server, ServerOptions};
use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about = "AIPass local background agent")]
struct Cli {
    #[arg(long)]
    vault: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    service: bool,
    #[arg(long)]
    service_name: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let vault_dir = cli.vault.unwrap_or(default_vault_dir()?);
    let service_name = cli.service_name.unwrap_or(agent_service_name(&vault_dir)?);
    let options = ServerOptions { vault_dir };
    #[cfg(target_os = "windows")]
    {
        if cli.service {
            return aipass_agent::run_windows_service_dispatcher(service_name, options);
        }
    }
    let _ = service_name;
    run_server(options)
}
