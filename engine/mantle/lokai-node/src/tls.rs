use std::sync::Arc;

use lokai_fabric_client::{
    supported_ed25519_schemes, verify_tls12_handshake_signature, verify_tls13_handshake_signature,
};
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair as RcgenKeyPair, SanType};
use rustls::client::danger::HandshakeSignatureValid;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::danger::ClientCertVerified;
use rustls::server::danger::ClientCertVerifier;
use rustls::server::{NoServerSessionStorage, ProducesTickets};
use rustls::{DigitallySignedStruct, Error as RustlsError, ServerConfig, SignatureScheme};
use thiserror::Error;
use x509_parser::prelude::*;

use crate::trust::TrustStore;

#[derive(Debug, Error)]
pub enum TlsError {
    #[error("rcgen: {0}")]
    Rcgen(String),
    #[error("rustls: {0}")]
    Rustls(String),
    #[error("x509: {0}")]
    X509(String),
}

/// Issue a self-signed Ed25519 TLS certificate for mTLS.
pub fn issue_self_signed_cert(common_name: &str) -> Result<(Vec<u8>, Vec<u8>), TlsError> {
    let key_pair = RcgenKeyPair::generate_for(&rcgen::PKCS_ED25519)
        .map_err(|e| TlsError::Rcgen(e.to_string()))?;
    let mut params = CertificateParams::new(vec![common_name.to_string()])
        .map_err(|e| TlsError::Rcgen(e.to_string()))?;
    params.distinguished_name = DistinguishedName::new();
    params
        .distinguished_name
        .push(DnType::CommonName, common_name);
    params
        .subject_alt_names
        .push(SanType::DnsName(common_name.try_into().unwrap()));
    let cert = params
        .self_signed(&key_pair)
        .map_err(|e| TlsError::Rcgen(e.to_string()))?;
    Ok((cert.der().to_vec(), key_pair.serialize_der()))
}

pub fn ed25519_pubkey_from_cert(cert_der: &[u8]) -> Result<[u8; 32], TlsError> {
    let (remaining, cert) =
        X509Certificate::from_der(cert_der).map_err(|e| TlsError::X509(e.to_string()))?;
    let spki = cert.public_key();
    if !remaining.is_empty()
        || spki.algorithm.algorithm.to_id_string() != "1.3.101.112"
        || spki.algorithm.parameters.is_some()
        || spki.subject_public_key.unused_bits != 0
        || spki.subject_public_key.data.len() != 32
    {
        return Err(TlsError::X509(
            "expected an Ed25519 public key with absent parameters and 32 key bytes".into(),
        ));
    }
    let mut pk = [0u8; 32];
    pk.copy_from_slice(&spki.subject_public_key.data);
    Ok(pk)
}

#[derive(Debug)]
struct NoTicketProducer;

impl ProducesTickets for NoTicketProducer {
    fn enabled(&self) -> bool {
        false
    }
    fn lifetime(&self) -> u32 {
        0
    }
    fn encrypt(&self, _bytes: &[u8]) -> Option<Vec<u8>> {
        None
    }
    fn decrypt(&self, _bytes: &[u8]) -> Option<Vec<u8>> {
        None
    }
}

#[derive(Debug)]
struct PinnedClientVerifier {
    trust: Arc<TrustStore>,
}

impl ClientCertVerifier for PinnedClientVerifier {
    fn offer_client_auth(&self) -> bool {
        true
    }

    fn client_auth_mandatory(&self) -> bool {
        true
    }

    fn root_hint_subjects(&self) -> &[rustls::DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<ClientCertVerified, RustlsError> {
        let pk = ed25519_pubkey_from_cert(end_entity.as_ref())
            .map_err(|e| RustlsError::General(e.to_string()))?;
        self.trust
            .authorize_owner_pubkey(&pk)
            .map_err(|_| RustlsError::General("unknown client certificate".into()))?;
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        verify_tls12_handshake_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        verify_tls13_handshake_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        supported_ed25519_schemes()
    }
}

pub fn build_server_config(
    cert_der: &[u8],
    key_der: &[u8],
    trust: Arc<TrustStore>,
) -> Result<Arc<ServerConfig>, TlsError> {
    let cert = CertificateDer::from(cert_der.to_vec());
    let key =
        PrivateKeyDer::try_from(key_der.to_vec()).map_err(|e| TlsError::Rustls(e.to_string()))?;
    let verifier = Arc::new(PinnedClientVerifier { trust });
    let mut config = ServerConfig::builder()
        .with_client_cert_verifier(verifier)
        .with_single_cert(vec![cert], key)
        .map_err(|e| TlsError::Rustls(e.to_string()))?;
    // Revoke must apply on the next connection — no TLS session resumption.
    config.session_storage = Arc::new(NoServerSessionStorage {});
    config.ticketer = Arc::new(NoTicketProducer);
    Ok(Arc::new(config))
}

pub use lokai_fabric_client::{build_client_config, client_cert_from_keypair};

#[cfg(test)]
mod tests {
    use super::*;
    use lokai_enroll::KeyPair;

    #[test]
    fn rejects_other_key_algorithms_and_trailing_data() {
        let key = RcgenKeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        let cert = CertificateParams::new(vec!["test".into()])
            .unwrap()
            .self_signed(&key)
            .unwrap();
        assert!(ed25519_pubkey_from_cert(cert.der()).is_err());
        let (mut cert, _) = issue_self_signed_cert("test").unwrap();
        cert.push(0);
        assert!(ed25519_pubkey_from_cert(&cert).is_err());
    }

    #[test]
    fn rejects_a_non_ed25519_spki_even_with_the_same_key_bytes() {
        let (mut cert, _) = issue_self_signed_cert("test").unwrap();
        let (_, parsed) = X509Certificate::from_der(&cert).unwrap();
        let raw = parsed.public_key().raw.to_vec();
        let start = cert.windows(raw.len()).position(|w| w == raw).unwrap();
        let oid = [0x06, 0x03, 0x2b, 0x65, 0x70];
        let offset = raw.windows(oid.len()).position(|w| w == oid).unwrap();
        // Change just the SPKI algorithm to X25519; leave all 32 bytes intact.
        cert[start + offset + 4] = 0x6e;
        assert!(ed25519_pubkey_from_cert(&cert).is_err());
    }

    #[test]
    fn client_cert_pubkey_matches_keypair() {
        let coord = KeyPair::generate();
        let (cert, _key) = client_cert_from_keypair(&coord, "c").unwrap();
        let pk = ed25519_pubkey_from_cert(&cert).unwrap();
        let mut expected = [0u8; 32];
        expected.copy_from_slice(&coord.public().0);
        assert_eq!(pk, expected);
    }

    #[test]
    fn verifier_honors_live_revoke() {
        let coord = KeyPair::generate();
        let mut pk = [0u8; 32];
        pk.copy_from_slice(&coord.public().0);
        let trust = Arc::new(
            TrustStore::from_pinned_pubkeys(std::iter::once(coord.public().0.clone())).unwrap(),
        );
        let (client_cert, _) = client_cert_from_keypair(&coord, "coord").unwrap();
        let end_entity = CertificateDer::from(client_cert);
        let verifier = PinnedClientVerifier {
            trust: trust.clone(),
        };
        assert!(verifier
            .verify_client_cert(&end_entity, &[], rustls::pki_types::UnixTime::now())
            .is_ok());
        trust.revoke_pubkey(&pk);
        assert!(verifier
            .verify_client_cert(&end_entity, &[], rustls::pki_types::UnixTime::now())
            .is_err());
    }
}
