//! Result envelope signing over canonical JSON (M5-4).

use ed25519_dalek::{Signature as DalekSig, Signer, SigningKey, Verifier, VerifyingKey};

use crate::canonical::to_canonical_json;
use crate::result::{ResultEnvelope, Signature, SignedResultBody};
use crate::{FabricError, FabricErrorCode};

pub fn canonical_signed_bytes(body: &SignedResultBody) -> Result<Vec<u8>, FabricError> {
    to_canonical_json(body).map_err(|e| FabricError {
        code: FabricErrorCode::InvalidEnvelope,
        message: format!("canonical result serialization failed: {e}"),
        details: None,
    })
}

pub fn sign_result_body(
    body: &SignedResultBody,
    signing_key: &SigningKey,
) -> Result<Signature, FabricError> {
    let bytes = canonical_signed_bytes(body)?;
    Ok(Signature(signing_key.sign(&bytes).to_bytes().to_vec()))
}

pub fn sign_result_envelope(
    body: SignedResultBody,
    signing_key: &SigningKey,
    payload: serde_json::Value,
) -> Result<ResultEnvelope, FabricError> {
    let signature = sign_result_body(&body, signing_key)?;
    Ok(ResultEnvelope {
        body,
        signature,
        payload,
    })
}

pub fn verify_result_signature(
    envelope: &ResultEnvelope,
    verifying_key: &VerifyingKey,
) -> Result<(), FabricError> {
    let bytes = canonical_signed_bytes(&envelope.body)?;
    if envelope.signature.0.len() != 64 {
        return Err(FabricError {
            code: FabricErrorCode::InvalidSignature,
            message: "result signature must be 64 bytes".into(),
            details: None,
        });
    }
    let mut sig_arr = [0u8; 64];
    sig_arr.copy_from_slice(&envelope.signature.0);
    let sig = DalekSig::from_bytes(&sig_arr);
    verifying_key.verify(&bytes, &sig).map_err(|_| FabricError {
        code: FabricErrorCode::InvalidSignature,
        message: "result signature verification failed".into(),
        details: None,
    })
}

pub fn verifying_key_from_bytes(bytes: &[u8]) -> Result<VerifyingKey, FabricError> {
    if bytes.len() != 32 {
        return Err(FabricError {
            code: FabricErrorCode::InvalidSignature,
            message: "verifying key must be 32 bytes".into(),
            details: None,
        });
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(bytes);
    VerifyingKey::from_bytes(&arr).map_err(|e| FabricError {
        code: FabricErrorCode::InvalidSignature,
        message: format!("invalid verifying key: {e}"),
        details: None,
    })
}

pub fn signing_key_from_bytes(bytes: &[u8]) -> Result<SigningKey, FabricError> {
    if bytes.len() != 32 {
        return Err(FabricError {
            code: FabricErrorCode::InvalidSignature,
            message: "signing key must be 32 bytes".into(),
            details: None,
        });
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(bytes);
    Ok(SigningKey::from_bytes(&arr))
}

/// Content digest helper for result payloads (sha256 hex).
pub fn digest_payload(
    payload: &serde_json::Value,
) -> Result<tetonic_domain::workspace::ContentDigest, FabricError> {
    use sha2::{Digest, Sha256};
    let bytes = to_canonical_json(payload).map_err(|e| FabricError {
        code: FabricErrorCode::InvalidEnvelope,
        message: format!("payload canonicalization failed: {e}"),
        details: None,
    })?;
    let hash = Sha256::digest(&bytes);
    Ok(tetonic_domain::workspace::ContentDigest::new(format!(
        "sha256:{}",
        hex::encode(hash)
    )))
}
