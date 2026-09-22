use aipass_agent_protocol::{ControlPanelCertificate, SensitiveString};
use anyhow::{Context, Result};
use rustls::{
    client::danger::ServerCertVerifier,
    pki_types::{ServerName, UnixTime},
    RootCertStore, ServerConfig,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io::BufReader, net::IpAddr, sync::Arc};

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Identity {
    pub certificate_pem: String,
    private_key_pem: SensitiveString,
    pub imported: bool,
}

impl Identity {
    pub fn generate(address: IpAddr) -> Result<Self> {
        let mut params = rcgen::CertificateParams::new(vec![address.to_string()])?;
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "AIPass LAN Control Panel");
        params.not_before = time::OffsetDateTime::now_utc() - time::Duration::minutes(5);
        params.not_after = time::OffsetDateTime::now_utc() + time::Duration::days(365);
        params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
        let key = rcgen::KeyPair::generate()?;
        let cert = params.self_signed(&key)?;
        Ok(Self {
            certificate_pem: cert.pem(),
            private_key_pem: key.serialize_pem().into(),
            imported: false,
        })
    }

    pub fn import(certificate: ControlPanelCertificate) -> Self {
        Self {
            certificate_pem: certificate.certificate_pem,
            private_key_pem: certificate.private_key_pem,
            imported: true,
        }
    }

    pub fn fingerprint(&self) -> Result<String> {
        let cert = rustls_pemfile::certs(&mut BufReader::new(self.certificate_pem.as_bytes()))
            .next()
            .context("missing certificate")??;
        Ok(Sha256::digest(cert)
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(":"))
    }
}

pub(super) fn server_config(identity: &Identity, ip: IpAddr) -> Result<Arc<ServerConfig>> {
    let certs = rustls_pemfile::certs(&mut BufReader::new(identity.certificate_pem.as_bytes()))
        .collect::<std::io::Result<Vec<_>>>()?;
    let first = certs.first().context("missing certificate")?;
    let key = rustls_pemfile::private_key(&mut BufReader::new(
        identity.private_key_pem.expose().as_bytes(),
    ))?
    .context("missing key")?;
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let mut roots = RootCertStore::empty();
    roots.add(certs.last().context("missing certificate")?.clone())?;
    let verifier = rustls::client::WebPkiServerVerifier::builder_with_provider(
        Arc::new(roots),
        provider.clone(),
    )
    .build()?;
    verifier.verify_server_cert(
        first,
        &certs[1..],
        &ServerName::IpAddress(ip.into()),
        &[],
        UnixTime::now(),
    )?;
    let mut config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}
