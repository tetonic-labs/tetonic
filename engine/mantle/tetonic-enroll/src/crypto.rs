use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};

use crate::code::new_worker_id;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PublicKeyBytes(pub Vec<u8>);

impl PublicKeyBytes {
    pub fn from_verifying(key: &VerifyingKey) -> Self {
        Self(key.to_bytes().to_vec())
    }

    pub fn to_verifying(&self) -> Result<VerifyingKey, String> {
        if self.0.len() != 32 {
            return Err("invalid public key length".into());
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&self.0);
        VerifyingKey::from_bytes(&arr).map_err(|e| e.to_string())
    }

    pub fn fingerprint(&self) -> String {
        use sha2::{Digest, Sha256};
        let h = Sha256::digest(&self.0);
        hex_short(&h[..8])
    }
}

#[derive(Clone)]
pub struct KeyPair {
    signing: SigningKey,
}

impl std::fmt::Debug for KeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyPair")
            .field("public", &self.public().fingerprint())
            .finish()
    }
}

impl KeyPair {
    pub fn generate() -> Self {
        Self {
            signing: SigningKey::generate(&mut OsRng),
        }
    }

    pub fn from_signing_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() != 32 {
            return Err("signing key must be 32 bytes".into());
        }
        let mut arr = zeroize::Zeroizing::new([0u8; 32]);
        arr.copy_from_slice(bytes);
        Ok(Self {
            signing: SigningKey::from_bytes(&arr),
        })
    }

    pub fn signing_bytes(&self) -> zeroize::Zeroizing<[u8; 32]> {
        zeroize::Zeroizing::new(self.signing.to_bytes())
    }

    pub fn public(&self) -> PublicKeyBytes {
        PublicKeyBytes::from_verifying(&self.signing.verifying_key())
    }

    pub fn sign(&self, msg: &[u8]) -> Vec<u8> {
        self.signing.sign(msg).to_bytes().to_vec()
    }

    pub fn worker_id(&self) -> String {
        new_worker_id(&self.public().fingerprint())
    }
}

fn hex_short(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signing_key_round_trip() {
        let kp = KeyPair::generate();
        let back = KeyPair::from_signing_bytes(kp.signing_bytes().as_ref()).unwrap();
        assert_eq!(kp.public(), back.public());
    }
}
