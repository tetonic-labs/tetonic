//! TLS for the enrollment listener (server-only auth, reuses worker fabric cert).

use std::sync::Arc;

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TlsError {
    #[error("rustls: {0}")]
    Rustls(String),
}

pub fn build_enrollment_server_config(
    cert_der: &[u8],
    key_der: &[u8],
) -> Result<Arc<ServerConfig>, TlsError> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cert = CertificateDer::from(cert_der.to_vec());
    let key =
        PrivateKeyDer::try_from(key_der.to_vec()).map_err(|e| TlsError::Rustls(e.to_string()))?;
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .map_err(|e| TlsError::Rustls(e.to_string()))?;
    Ok(Arc::new(config))
}
