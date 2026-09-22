//! Opt-in LAN control surface. The listener is independent of the inference proxy.
mod api;
mod sessions;
mod tls;
mod transport;

use crate::session::{AgentState, ServiceError, ServiceResult, SessionState};
use aipass_agent_protocol::{
    AgentErrorCode, ControlPanelCertificate, ControlPanelSettings, ControlPanelStatus,
};
use aipass_storage::atomic_write_bytes;
use aipass_vault::{RemoteUnlockEnvelope, Vault};
use serde::{Deserialize, Serialize};
pub(crate) use sessions::{check_session, remote_request};
use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Default)]
pub(crate) struct ControlPanel {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    stored: Stored,
    listener: Option<transport::Listener>,
    error: Option<String>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Stored {
    settings: ControlPanelSettings,
    identity: Option<tls::Identity>,
    #[serde(default)]
    access_code_hash: Option<[u8; 32]>,
    #[serde(default)]
    access_code_vault: Option<uuid::Uuid>,
    #[serde(default)]
    remote_unlock: Option<RemoteUnlockEnvelope>,
}

fn settings_path(state: &AgentState) -> PathBuf {
    state
        .vault_dir
        .parent()
        .unwrap_or(&state.vault_dir)
        .join(format!("control-panel-{}.json", state.namespace))
}

pub(crate) fn invalid(message: &str) -> ServiceError {
    ServiceError::new(AgentErrorCode::ValidationFailed, message)
}

pub(crate) fn unavailable() -> ServiceError {
    ServiceError::new(AgentErrorCode::Internal, "control panel unavailable")
}

fn addresses() -> Vec<String> {
    let mut values: Vec<_> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .map(|interface| interface.ip())
        .filter(|ip| !ip.is_unspecified() && !ip.is_multicast())
        // Link-local IPv6 requires scope identifiers, which this IP-only surface does not accept.
        .filter(|ip| !matches!(ip, IpAddr::V6(ip) if ip.is_unicast_link_local()))
        .map(|ip| ip.to_string())
        .collect();
    values.push("127.0.0.1".into());
    values.sort();
    values.dedup();
    values
}

impl ControlPanel {
    pub(crate) fn restore(state: &Arc<AgentState>) {
        let result = (|| -> anyhow::Result<()> {
            let path = settings_path(state);
            if !path.exists() {
                return Ok(());
            }
            let bytes = zeroize::Zeroizing::new(std::fs::read(path)?);
            let stored: Stored = serde_json::from_slice(&bytes)?;
            let mut inner = state
                .control_panel
                .inner
                .lock()
                .map_err(|_| anyhow::anyhow!("lock"))?;
            inner.stored = stored;
            if inner.stored.settings.enabled {
                let address = socket_address(&inner.stored.settings)
                    .map_err(|_| anyhow::anyhow!("invalid address"))?;
                let config = if inner.stored.settings.https {
                    let identity = inner
                        .stored
                        .identity
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("missing certificate"))?;
                    Some(tls::server_config(identity, address.ip())?)
                } else {
                    None
                };
                anyhow::ensure!(
                    inner.stored.access_code_hash.is_some(),
                    "missing access code"
                );
                inner.listener = Some(transport::Listener::start(state, address, config, None)?);
            }
            Ok(())
        })();
        if result.is_err() {
            if let Ok(mut inner) = state.control_panel.inner.lock() {
                inner.error = Some("Could not start the control panel. Check the address, port and certificate in Settings.".into());
            }
            crate::logging::write_component_log(
                crate::logging::AGENT_LOG,
                "WARN",
                "event=control_panel.start outcome=failed",
            );
        }
    }

    pub(crate) fn status(&self) -> ServiceResult<ControlPanelStatus> {
        let inner = self.inner.lock().map_err(|_| unavailable())?;
        Ok(status(&inner))
    }

    pub(crate) fn configure(
        state: &Arc<AgentState>,
        vault_id: uuid::Uuid,
        settings: ControlPanelSettings,
        certificate: Option<ControlPanelCertificate>,
        regenerate: bool,
    ) -> ServiceResult<ControlPanelStatus> {
        let address = socket_address(&settings)?;
        if !addresses().contains(&settings.address) {
            return Err(invalid("Choose an address assigned to this computer."));
        }
        if certificate.is_some() && regenerate {
            return Err(invalid(
                "Choose either an imported or a generated certificate.",
            ));
        }
        if certificate.as_ref().is_some_and(|cert| {
            cert.certificate_pem.len() > 64 * 1024
                || cert.private_key_pem.expose().len() > 16 * 1024
        }) {
            return Err(invalid("Certificate or private key file is too large."));
        }
        let mut inner = state
            .control_panel
            .inner
            .lock()
            .map_err(|_| unavailable())?;
        if settings.enabled
            && (inner.stored.access_code_hash.is_none()
                || inner.stored.access_code_vault != Some(vault_id))
        {
            return Err(invalid(
                "Generate a panel access code before enabling the service.",
            ));
        }
        let identity = if !settings.https {
            inner.stored.identity.clone()
        } else if let Some(certificate) = certificate {
            Some(tls::Identity::import(certificate))
        } else if regenerate
            || inner.stored.identity.is_none()
            || (!inner.stored.identity.as_ref().is_some_and(|i| i.imported)
                && inner.stored.settings.address != settings.address)
        {
            Some(
                tls::Identity::generate(address.ip())
                    .map_err(|_| invalid("Could not generate the HTTPS certificate."))?,
            )
        } else {
            inner.stored.identity.clone()
        };
        let config = if settings.https {
            Some(tls::server_config(identity.as_ref().ok_or_else(unavailable)?, address.ip())
                .map_err(|_| invalid("Certificate must be valid for the selected IP address, unexpired, and match the private key. Include the full certificate chain."))?)
        } else {
            None
        };
        let next = if settings.enabled {
            let socket = inner
                .listener
                .as_ref()
                .filter(|old| old.address == address)
                .map(|old| old.socket.try_clone())
                .transpose()
                .map_err(|_| unavailable())?;
            Some(transport::Listener::prepare(address, config, socket)
                .map_err(|_| invalid("Could not listen on this address and port. Check whether another service is using it."))?)
        } else {
            None
        };
        let stored = Stored {
            settings,
            identity,
            access_code_hash: inner.stored.access_code_hash,
            access_code_vault: inner.stored.access_code_vault,
            remote_unlock: inner.stored.remote_unlock.clone(),
        };
        // Atomic replacement also keeps the private key owner-only (NamedTempFile).
        let bytes =
            zeroize::Zeroizing::new(serde_json::to_vec(&stored).map_err(|_| unavailable())?);
        atomic_write_bytes(settings_path(state), &bytes).map_err(|_| unavailable())?;
        inner.listener.take(); // revoke sessions and close every old TLS connection
        inner.stored = stored;
        inner.error = None;
        inner.listener = next.map(|prepared| prepared.launch(state));
        Ok(status(&inner))
    }

    pub(crate) fn stop(state: &Arc<AgentState>) -> ServiceResult<ControlPanelStatus> {
        let mut inner = state
            .control_panel
            .inner
            .lock()
            .map_err(|_| unavailable())?;
        // Stopping is always possible locally, even when the vault is locked or a NIC disappeared.
        inner.listener.take();
        inner.stored.settings.enabled = false;
        let bytes =
            zeroize::Zeroizing::new(serde_json::to_vec(&inner.stored).map_err(|_| unavailable())?);
        atomic_write_bytes(settings_path(state), &bytes).map_err(|_| unavailable())?;
        inner.error = None;
        Ok(status(&inner))
    }

    pub(crate) fn shutdown(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.listener.take();
        }
    }

    pub(crate) fn rotate_access_code(
        state: &Arc<AgentState>,
        vault: &Vault,
        allow_remote_unlock: bool,
    ) -> ServiceResult<aipass_agent_protocol::ControlPanelAccessCode> {
        use sha2::{Digest, Sha256};
        let access_code = aipass_agent_protocol::SensitiveString::from(sessions::random_token());
        let mut inner = state
            .control_panel
            .inner
            .lock()
            .map_err(|_| unavailable())?;
        let mut next = inner.stored.clone();
        next.access_code_vault = Some(vault.vault_id());
        next.access_code_hash = Some(Sha256::digest(access_code.expose().as_bytes()).into());
        next.remote_unlock = if allow_remote_unlock {
            Some(
                vault
                    .seal_remote_unlock(&aipass_crypto::SecretString::new(access_code.expose()))
                    .map_err(|_| unavailable())?,
            )
        } else {
            None
        };
        let bytes = zeroize::Zeroizing::new(serde_json::to_vec(&next).map_err(|_| unavailable())?);
        atomic_write_bytes(settings_path(state), &bytes).map_err(|_| unavailable())?;
        inner.stored = next;
        if let Some(listener) = &inner.listener {
            listener.sessions.revoke_all();
        }
        Ok(aipass_agent_protocol::ControlPanelAccessCode { access_code })
    }

    pub(crate) fn disable_remote_unlock(
        state: &Arc<AgentState>,
    ) -> ServiceResult<ControlPanelStatus> {
        let mut inner = state
            .control_panel
            .inner
            .lock()
            .map_err(|_| unavailable())?;
        let mut next = inner.stored.clone();
        next.remote_unlock = None;
        next.access_code_hash = None;
        next.access_code_vault = None;
        next.settings.enabled = false;
        let bytes = zeroize::Zeroizing::new(serde_json::to_vec(&next).map_err(|_| unavailable())?);
        atomic_write_bytes(settings_path(state), &bytes).map_err(|_| unavailable())?;
        inner.stored = next;
        inner.listener.take();
        Ok(status(&inner))
    }

    fn login(
        state: &Arc<AgentState>,
        sessions: &Arc<sessions::Sessions>,
        code: &str,
        generation: u64,
    ) -> ServiceResult<(aipass_agent_protocol::SensitiveString, String)> {
        use sha2::{Digest, Sha256};
        // Same lock order as local rotation/configuration: session, then panel.
        // Hold both through unlock + cookie issuance so revocation cannot race activation.
        let mut session = state.session.lock().map_err(|_| unavailable())?;
        let inner = state
            .control_panel
            .inner
            .lock()
            .map_err(|_| unavailable())?;
        if code.len() != 43
            || generation != sessions.generation()
            || !inner
                .listener
                .as_ref()
                .is_some_and(|listener| Arc::ptr_eq(&listener.sessions, sessions))
        {
            return Err(sessions::denied());
        }
        let actual: [u8; 32] = Sha256::digest(code.as_bytes()).into();
        let expected = inner.stored.access_code_hash.ok_or_else(sessions::denied)?;
        if actual
            .iter()
            .zip(expected)
            .fold(0_u8, |v, (a, b)| v | (a ^ b))
            != 0
        {
            return Err(sessions::denied());
        }
        let vault_id = inner
            .stored
            .access_code_vault
            .ok_or_else(sessions::denied)?;
        let unlocked = matches!(*session, SessionState::Locked);
        if unlocked {
            let envelope = inner.stored.remote_unlock.as_ref().ok_or_else(|| {
                ServiceError::new(AgentErrorCode::Locked, "This code cannot unlock the vault.")
            })?;
            let vault = Vault::open_with_remote_unlock(
                &state.vault_dir,
                &aipass_crypto::SecretString::new(code),
                envelope,
            )
            .map_err(|_| sessions::denied())?;
            if vault.vault_id() != vault_id {
                return Err(sessions::denied());
            }
            vault.ensure_sync_ready().map_err(|_| unavailable())?;
            crate::session::replace_session_vault(&mut session, vault);
        }
        let SessionState::Unlocked(info) = &*session else {
            return Err(sessions::denied());
        };
        if info.vault.vault_id() != vault_id
            || inner
                .stored
                .remote_unlock
                .as_ref()
                .is_some_and(|grant| !grant.matches_vault(&info.vault))
        {
            return Err(sessions::denied());
        }
        info.vault.ensure_sync_ready().map_err(|_| unavailable())?;
        let result = sessions.issue(sessions::Binding::from_session(info), generation);
        drop(inner);
        drop(session);
        if unlocked {
            crate::session::complete_unlock(state);
        }
        result
    }
}

fn socket_address(settings: &ControlPanelSettings) -> ServiceResult<SocketAddr> {
    let ip: IpAddr = settings
        .address
        .parse()
        .map_err(|_| invalid("Select a local IP address."))?;
    if ip.is_unspecified() || ip.is_multicast() || settings.port == 0 {
        return Err(invalid(
            "Select a specific local IP address and a port from 1 to 65535.",
        ));
    }
    Ok(SocketAddr::new(ip, settings.port))
}

fn status(inner: &Inner) -> ControlPanelStatus {
    ControlPanelStatus {
        settings: inner.stored.settings.clone(),
        running: inner.listener.is_some(),
        url: inner.listener.as_ref().map(|listener| {
            format!(
                "{}://{}",
                if inner.stored.settings.https {
                    "https"
                } else {
                    "http"
                },
                transport::authority(listener.address, inner.stored.settings.https)
            )
        }),
        addresses: addresses(),
        certificate_pem: inner
            .stored
            .identity
            .as_ref()
            .map(|i| i.certificate_pem.clone()),
        fingerprint: inner
            .stored
            .identity
            .as_ref()
            .and_then(|i| i.fingerprint().ok()),
        imported_certificate: inner.stored.identity.as_ref().is_some_and(|i| i.imported),
        error: inner.error.clone(),
        has_access_code: inner.stored.access_code_hash.is_some(),
        remote_unlock_enabled: inner.stored.remote_unlock.is_some(),
    }
}

#[cfg(test)]
mod tests;
