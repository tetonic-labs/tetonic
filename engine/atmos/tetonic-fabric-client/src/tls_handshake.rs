//! TLS handshake signature verification for pinned mTLS (SEC-001).
//!
//! Custom cert verifiers must prove the peer holds the private key matching the
//! pinned certificate — not merely present cert bytes.

use std::sync::OnceLock;

use rustls::client::danger::HandshakeSignatureValid;
use rustls::crypto::verify_tls13_signature_with_raw_key;
use rustls::pki_types::{CertificateDer, SubjectPublicKeyInfoDer};
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

/// Verify a TLS 1.2 handshake signature using the public key in `cert`.
///
/// Fabric uses TLS 1.3 only; TLS 1.2 is rejected at the verifier boundary.
pub fn verify_tls12_handshake_signature(
    _message: &[u8],
    _cert: &CertificateDer<'_>,
    _dss: &DigitallySignedStruct,
) -> Result<HandshakeSignatureValid, RustlsError> {
    Err(RustlsError::PeerIncompatible(
        rustls::PeerIncompatible::Tls12NotOffered,
    ))
}

/// Verify a TLS 1.3 handshake signature using the public key in `cert`.
pub fn verify_tls13_handshake_signature(
    message: &[u8],
    cert: &CertificateDer<'_>,
    dss: &DigitallySignedStruct,
) -> Result<HandshakeSignatureValid, RustlsError> {
    let spki = spki_from_cert(cert)?;
    verify_tls13_signature_with_raw_key(message, &spki, dss, signature_algorithms())
}

/// Signature schemes offered by pinned Ed25519 fabric identities.
pub fn supported_ed25519_schemes() -> Vec<SignatureScheme> {
    vec![SignatureScheme::ED25519]
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::DigitallySignedStruct;
    use rustls::SignatureScheme;
    use tetonic_enroll::KeyPair;

    use crate::client_cert_from_keypair;

    fn test_dss(scheme: SignatureScheme, sig: Vec<u8>) -> DigitallySignedStruct {
        use rustls::internal::msgs::codec::{Codec, Reader};
        let mut bytes = Vec::new();
        scheme.encode(&mut bytes);
        let len = u16::try_from(sig.len()).expect("signature length fits u16");
        bytes.extend_from_slice(&len.to_be_bytes());
        bytes.extend_from_slice(&sig);
        let mut reader = Reader::init(&bytes);
        DigitallySignedStruct::read(&mut reader).expect("valid digitally signed struct")
    }

    #[test]
    fn tls13_rejects_tampered_handshake_signature() {
        let kp = KeyPair::generate();
        let (cert_der, _) = client_cert_from_keypair(&kp, "test-worker").unwrap();
        let cert = CertificateDer::from(cert_der);
        let message = b"fabric handshake transcript sample";

        let mut sig = kp.sign(message);
        assert!(verify_tls13_handshake_signature(
            message,
            &cert,
            &test_dss(SignatureScheme::ED25519, sig.clone())
        )
        .is_ok());

        sig[0] ^= 0xff;
        assert!(verify_tls13_handshake_signature(
            message,
            &cert,
            &test_dss(SignatureScheme::ED25519, sig)
        )
        .is_err());
    }

    #[test]
    fn tls13_rejects_signature_from_wrong_key() {
        let kp_a = KeyPair::generate();
        let kp_b = KeyPair::generate();
        let (cert_a, _) = client_cert_from_keypair(&kp_a, "worker-a").unwrap();
        let cert = CertificateDer::from(cert_a);
        let message = b"fabric handshake transcript sample";
        let sig_b = kp_b.sign(message);
        assert!(verify_tls13_handshake_signature(
            message,
            &cert,
            &test_dss(SignatureScheme::ED25519, sig_b)
        )
        .is_err());
    }

    #[test]
    fn tls12_handshake_signature_rejected() {
        let kp = KeyPair::generate();
        let (cert_der, _) = client_cert_from_keypair(&kp, "test-worker").unwrap();
        let cert = CertificateDer::from(cert_der);
        let dss = test_dss(SignatureScheme::ED25519, kp.sign(b"msg"));
        assert!(verify_tls12_handshake_signature(b"msg", &cert, &dss).is_err());
    }
}
