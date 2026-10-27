//! Only remove launchd registration belonging to the uninstalling executable.
use super::*;
use aipass_agent_protocol::{AgentRequest, AGENT_PROTOCOL_VERSION};
use anyhow::{ensure, Context};
use std::time::{Duration, Instant};

pub(super) fn run(agent: &Path, vault: &Path) -> Result<()> {
    let status = query_autostart(vault)?;
    let supervisor = status
        .supervisor_path
        .context("Agent supervisor path unavailable")?;
    let expected = format!("AGENT={}", shell_quote(&agent.display().to_string()));
    match fs::read_to_string(supervisor) {
        Ok(script) => ensure!(
            script.lines().any(|line| line == expected),
            "another installation owns this Agent autostart; uninstall stopped"
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    }
    // Stop supervision before authenticated shutdown, then wait for the socket
    // to disappear. Do not treat an older protocol as an absent service.
    suspend_autostart(vault)?;
    let client = crate::AgentClient::for_vault(vault.to_owned())?;
    if let Ok(response) = client.request_raw(&AgentRequest::SessionStatus) {
        if response.protocol_version < AGENT_PROTOCOL_VERSION {
            client.shutdown_older_agent(response.protocol_version)?;
        } else {
            client.shutdown()?;
        }
        let deadline = Instant::now() + Duration::from_secs(8);
        while client.request_raw(&AgentRequest::SessionStatus).is_ok() {
            ensure!(
                Instant::now() < deadline,
                "Agent did not stop; uninstall stopped"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    uninstall_autostart(vault)?;
    Ok(())
}
