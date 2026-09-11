use crate::*;

pub(crate) fn handle_secret_command(
    json: bool,
    vault: Option<PathBuf>,
    cli_password: Option<String>,
    command: SecretCommand,
) -> Result<()> {
    match command {
        SecretCommand::List { id } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let entry: aipass_vault::EntrySummary =
                agent.request(AgentRequest::ProviderGet { id })?;
            output(
                json,
                serde_json::to_value(&entry.secret_refs)?,
                &format!("{} secrets", entry.secret_refs.len()),
            )
        }
        SecretCommand::Add {
            id,
            label,
            api_key,
            group,
            interface,
        } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let secret_id: String = agent.request(AgentRequest::SecretAdd {
                id,
                label,
                secret: api_key.into(),
                metadata: Some(SecretMetadataInput {
                    group,
                    interface_type: interface.map(InterfaceType::from),
                    billing: None,
                }),
            })?;
            output(
                json,
                serde_json::json!({ "ok": true, "id": id, "secretId": secret_id }),
                "Secret added",
            )
        }
        SecretCommand::Remove { id, label } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let _: serde_json::Value = agent.request(AgentRequest::SecretRemove {
                id,
                label: label.clone(),
            })?;
            output(
                json,
                serde_json::json!({ "ok": true, "id": id, "removed": label }),
                "Secret removed",
            )
        }
    }
}
