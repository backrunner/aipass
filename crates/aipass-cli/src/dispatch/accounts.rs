use crate::*;

pub(crate) fn handle_accounts_command(
    json: bool,
    vault: Option<PathBuf>,
    cli_password: Option<String>,
    command: AccountsCommand,
) -> Result<()> {
    match command {
        AccountsCommand::Refresh { provider_ids } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let results: Vec<OfficialAccountRefreshResult> =
                agent.request(AgentRequest::OfficialAccountsRefresh { provider_ids })?;
            let imported = results
                .iter()
                .filter(|result| result.status == "imported")
                .count();
            let refreshed = results
                .iter()
                .filter(|result| result.status == "refreshed")
                .count();
            output(
                json,
                serde_json::to_value(&results)?,
                &format!("{imported} imported, {refreshed} refreshed"),
            )
        }
    }
}
