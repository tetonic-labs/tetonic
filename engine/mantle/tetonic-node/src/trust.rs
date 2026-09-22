use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

use tetonic_enroll::PublicKeyBytes;
use tetonic_memory::CoordinatorPinRow;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TrustError {
    #[error("no coordinator pinned — enroll first")]
    NoCoordinators,
    #[error("unknown client certificate")]
    UnknownCert,
}

/// Pinned coordinator keys authorized for owner (estate) fabric ingress.
#[derive(Debug)]
pub struct TrustStore {
    pinned_coordinator_pubkeys: RwLock<HashSet<[u8; 32]>>,
    pin_estates: RwLock<HashMap<[u8; 32], String>>,
}

impl TrustStore {
    pub fn from_pinned_pubkeys(
        keys: impl IntoIterator<Item = Vec<u8>>,
    ) -> Result<Self, TrustError> {
        let mut set = HashSet::new();
        for k in keys {
            if k.len() != 32 {
                tracing::warn!(
                    len = k.len(),
                    "skipping coordinator pin with invalid pubkey length"
                );
                continue;
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&k);
            set.insert(arr);
        }
        if set.is_empty() {
            return Err(TrustError::NoCoordinators);
        }
        Ok(Self {
            pinned_coordinator_pubkeys: RwLock::new(set),
            pin_estates: RwLock::new(HashMap::new()),
        })
    }

    /// Build trust from enrolled coordinator pin rows (pubkey + estate binding).
    pub fn from_coordinator_pins(pins: &[CoordinatorPinRow]) -> Result<Self, TrustError> {
        let mut set = HashSet::new();
        let mut estates = HashMap::new();
        for pin in pins {
            if pin.coordinator_pubkey.len() != 32 {
                tracing::warn!(
                    label = %pin.label,
                    len = pin.coordinator_pubkey.len(),
                    "skipping coordinator pin with invalid pubkey length"
                );
                continue;
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&pin.coordinator_pubkey);
            set.insert(arr);
            estates.insert(arr, pin.estate_id.clone());
        }
        if set.is_empty() {
            return Err(TrustError::NoCoordinators);
        }
        Ok(Self {
            pinned_coordinator_pubkeys: RwLock::new(set),
            pin_estates: RwLock::new(estates),
        })
    }

    pub fn estate_id_for(&self, pk: &[u8; 32]) -> Option<String> {
        self.pin_estates
            .read()
            .expect("trust poisoned")
            .get(pk)
            .cloned()
    }

    pub fn is_pinned_pubkey(&self, pk: &[u8; 32]) -> bool {
        self.pinned_coordinator_pubkeys
            .read()
            .expect("trust poisoned")
            .contains(pk)
    }

    pub fn peer_id_for_pubkey(pk: &[u8; 32]) -> String {
        PublicKeyBytes(pk.to_vec()).fingerprint()
    }

    pub fn authorize_owner_pubkey(&self, pk: &[u8; 32]) -> Result<String, TrustError> {
        if self.is_pinned_pubkey(pk) {
            Ok(Self::peer_id_for_pubkey(pk))
        } else {
            Err(TrustError::UnknownCert)
        }
    }

    /// Remove a coordinator from live ingress authorization (N0.3 revoke).
    pub fn revoke_pubkey(&self, pk: &[u8; 32]) -> bool {
        self.pinned_coordinator_pubkeys
            .write()
            .expect("trust poisoned")
            .remove(pk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tetonic_memory::CoordinatorPinRow;

    #[test]
    fn from_coordinator_pins_binds_estate() {
        let pins = vec![CoordinatorPinRow {
            estate_id: "estate_a".into(),
            coordinator_pubkey: vec![7u8; 32],
            label: "coord".into(),
            enrolled_at: "t".into(),
            epoch: 0,
        }];
        let trust = TrustStore::from_coordinator_pins(&pins).unwrap();
        let mut pk = [0u8; 32];
        pk.fill(7);
        assert_eq!(trust.estate_id_for(&pk).as_deref(), Some("estate_a"));
    }
}
