//! Pinned server-certificate TLS for enrollment HTTPS (no system CA trust).

use std::sync::{Arc, OnceLock};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::verify_tls13_signature_with_raw_key;
use rustls::pki_types::{CertificateDer, ServerName, SubjectPublicKeyInfoDer};
use rustls::{DigitallySignedStruct, Error as RustlsError, SignatureScheme};
use x509_parser::prelude::{FromDer, X509Certificate};

static SIG_ALGS: OnceLock<rustls::crypto::WebPkiSupportedAlgorithms> = OnceLock::new();

fn signature_algorithms() -> &'static rustls::crypto::WebPkiSupportedAlgorithms {
    SIG_ALGS
        .get_or_init(|| rustls::crypto::ring::default_provider().signature_verification_algorithms)
}

fn spki_from_cert(
    cert: &CertificateDer<'_>,
) -> Result<SubjectPublicKeyInfoDer<'static>, RustlsError> {
    let (_, x509) = X509Certificate::from_der(cert.as_ref())
        .map_err(|e| RustlsError::General(format!("cert parse: {e}")))?;
    Ok(SubjectPublicKeyInfoDer::from(
        x509.public_key().raw.to_vec(),
    ))
}

fn verify_tls13_handshake_signature(
    message: &[u8],
    cert: &CertificateDer<'_>,
    dss: &DigitallySignedStruct,
) -> Result<HandshakeSignatureValid, RustlsError> {
    let spki = spki_from_cert(cert)?;
    verify_tls13_signature_with_raw_key(message, &spki, dss, signature_algorithms())
}

#[derive(Debug)]
struct PinnedServerVerifier {
    cert: Vec<u8>,
}

impl ServerCertVerifier for PinnedServerVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        if end_entity.as_ref() == self.cert.as_slice() {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(RustlsError::General("unknown server certificate".into()))
        }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Err(RustlsError::PeerIncompatible(
            rustls::PeerIncompatible::Tls12NotOffered,
        ))
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
        vec![SignatureScheme::ED25519]
    }
}

pub fn client_config_pinned_server(
    cert_der: &[u8],
) -> Result<Arc<rustls::ClientConfig>, RustlsError> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinnedServerVerifier {
            cert: cert_der.to_vec(),
        }))
        .with_no_client_auth();
    config.resumption = rustls::client::Resumption::disabled();
    Ok(Arc::new(config))
}
