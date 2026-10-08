//! Native account validation and bounded compatibility helpers.
use super::*;

pub(crate) fn validate_account(
    vault: &aipass_vault::Vault,
    id: Uuid,
    token: &str,
) -> crate::session::ServiceResult<()> {
    use crate::session::{map_vault_error, ServiceError};
    let entry = vault.get_provider_summary(id).map_err(map_vault_error)?;
    let secret =
        aipass_provider_registry::primary_secret_ref(&entry.secret_refs).ok_or_else(|| {
            ServiceError::new(
                aipass_agent_protocol::AgentErrorCode::NotFound,
                "Claude credential missing",
            )
        })?;
    let current = vault
        .runtime_provider_credentials(id, &secret.id)
        .map_err(map_vault_error)?;
    if entry.provider_kind != aipass_provider_registry::ProviderKind::Official
        || entry.provider_id.as_deref() != Some("anthropic")
        || entry.credential_kind != aipass_provider_registry::CredentialKind::OAuth
        || vault.fingerprint_secret(current.secret.expose()) != vault.fingerprint_secret(token)
    {
        return Err(ServiceError::new(
            aipass_agent_protocol::AgentErrorCode::Conflict,
            "Claude account changed",
        ));
    }
    Ok(())
}
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|_| "could not prepare native Claude configuration".into())
}
#[cfg(target_os = "macos")]
pub(crate) fn native_service(path: &Path) -> String {
    format!(
        "Claude Code-credentials-{}",
        &format!("{:x}", Sha256::digest(path.to_string_lossy().as_bytes()))[..8]
    )
}
