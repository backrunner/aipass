use aipass_agent::{
    agent_service_name, default_vault_dir,
    logging::{init_component_logging, install_panic_logger, write_component_log, AGENT_LOG},
    run_server, ServerOptions,
};
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
    #[arg(long, default_value_t = false, hide = true)]
    install_autostart: bool,
    #[arg(long, default_value_t = false, hide = true)]
    uninstall_autostart: bool,
}

fn main() -> Result<()> {
    let log_path = init_component_logging(AGENT_LOG).ok();
    install_panic_logger(AGENT_LOG);
    write_component_log(
        AGENT_LOG,
        "INFO",
        &format!(
            "agent process starting pid={} log_path={}",
            std::process::id(),
            log_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "unavailable".to_string())
        ),
    );
    let cli = Cli::parse();
    let vault_dir = cli.vault.unwrap_or(default_vault_dir()?);
    let service_name = cli.service_name.unwrap_or(agent_service_name(&vault_dir)?);
    if cli.install_autostart {
        let agent_binary = std::env::current_exe()?;
        let status = aipass_agent::install_agent_autostart(&agent_binary, &vault_dir)?;
        println!("AIPass agent autostart installed: {}", status.service_name);
        return Ok(());
    }
    if cli.uninstall_autostart {
        let status = aipass_agent::uninstall_agent_autostart(&vault_dir)?;
        println!(
            "AIPass agent autostart uninstalled: {}",
            status.service_name
        );
        return Ok(());
    }
    let options = ServerOptions::for_current_process(vault_dir);
    #[cfg(target_os = "windows")]
    {
        if cli.service {
            return aipass_agent::run_windows_service_dispatcher(service_name, options);
        }
    }
    let _ = service_name;
    run_server(options)
}
