use crate::*;

pub(crate) fn handle_accounts_command(
    json: bool,
    vault: Option<PathBuf>,
    cli_password: Option<String>,
    command: AccountsCommand,
) -> Result<()> {
    match command {
        AccountsCommand::Import {
            provider_ids,
            directory,
        } => {
            if directory.is_some() && provider_ids.len() != 1 {
                anyhow::bail!("--directory requires exactly one --provider");
            }
            let sources = directory
                .map(|root| aipass_agent_protocol::SubscriptionImportSource {
                    provider: provider_ids[0].clone(),
                    root,
                    selector: String::new(),
                })
                .into_iter()
                .collect();
            let agent = CliAgent::from_parts(vault, cli_password)?;
            let task: aipass_agent_protocol::SubscriptionImportTask =
                agent.request(AgentRequest::SubscriptionImportStart {
                    input: aipass_agent_protocol::SubscriptionImportInput {
                        provider_ids,
                        sources,
                        retry: None,
                    },
                })?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            runtime.block_on(async {
                let interrupt = tokio::signal::ctrl_c();
                tokio::pin!(interrupt);
                let mut cancelling = false;
                loop {
                    // Poll/cancel must never reopen a vault locked after import began.
                    let status: aipass_agent_protocol::SubscriptionImportTask = agent.request_no_unlock(AgentRequest::SubscriptionImportPoll { ticket: task.ticket })?;
                    if matches!(status.phase.as_str(), "complete" | "cancelled") {
                        let imported = status.results.iter().filter(|r| r.status == aipass_agent_protocol::SubscriptionImportStatus::Imported).count();
                        if !json {
                            for item in &status.results {
                                println!("{} · {} · {} · {}{}", item.source.provider,
                                    item.account_identity.as_deref().unwrap_or("—"),
                                    serde_json::to_value(item.status)?.as_str().unwrap_or("failed"),
                                    item.source.root.display(),
                                    item.error_code.as_ref().map(|code| format!(" · {code}")).unwrap_or_default());
                            }
                        }
                        output(json, serde_json::to_value(&status)?, &format!("{imported} imported · {} sources checked", status.completed))?;
                        return Ok::<(), anyhow::Error>(());
                    }
                    tokio::select! {
                        _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {},
                        signal = &mut interrupt, if !cancelling => {
                            signal?;
                            let _: aipass_agent_protocol::SubscriptionImportTask = agent.request_no_unlock(AgentRequest::SubscriptionImportCancel { ticket: task.ticket })?;
                            cancelling = true;
                        }
                    }
                }
            })?;
            Ok(())
        }
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
