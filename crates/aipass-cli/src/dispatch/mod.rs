mod accounts;
mod credential;
mod misc;
mod proxy;
mod secret;
mod vault;

use crate::*;

pub(crate) fn run(cli: Cli) -> Result<()> {
    let json = cli.json;
    let vault = cli.vault.clone();
    let cli_password = cli.password.clone();

    match cli.command {
        Command::Doctor => misc::handle_doctor(json, vault, cli_password),
        Command::Completions { shell } => misc::handle_completions(shell),
        Command::Vault { command } => {
            vault::handle_vault_command(json, vault, cli_password, command)
        }
        Command::Secret { command } => {
            secret::handle_secret_command(json, vault, cli_password, command)
        }
        Command::Accounts { command } => {
            accounts::handle_accounts_command(json, vault, cli_password, command)
        }
        Command::NativeHost { command } => misc::handle_native_host_command(json, command),
        Command::Agent { command } => {
            misc::handle_agent_command(json, vault, cli_password, command)
        }
        Command::Proxy { command } => {
            proxy::handle_proxy_command(json, vault, cli_password, command)
        }
        Command::Unlock => misc::handle_unlock(json, vault, cli_password),
        Command::Lock => misc::handle_lock(json, vault, cli_password),
        Command::Init { password } => misc::handle_init(json, vault, cli_password, password),
        Command::Add { .. }
        | Command::List { .. }
        | Command::Update { .. }
        | Command::Archive { .. }
        | Command::Restore { .. }
        | Command::Delete { .. }
        | Command::Search { .. }
        | Command::Probe { .. }
        | Command::Get { .. }
        | Command::Copy { .. }
        | Command::Env { .. }
        | Command::Exec { .. }
        | Command::Inject { .. }
        | Command::Configure { .. }
        | Command::Switch { .. }
        | Command::Rollback { .. }
        | Command::Sync { .. } => {
            credential::handle_credential_command(json, vault, cli_password, cli.command)
        }
    }
}
