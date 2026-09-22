//! Coordinator-assigned worker trust tiers (M5-3).

use serde::{Deserialize, Serialize};

/// Locally assigned trust level for a worker. Workers must not self-assign.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum WorkerTrust {
    LocalMachine,
    #[default]
    OwnerControlledEstate,
    AdministrativelyManaged,
    ExternalUntrusted,
}

impl WorkerTrust {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalMachine => "local_machine",
            Self::OwnerControlledEstate => "owner_controlled_estate",
            Self::AdministrativelyManaged => "administratively_managed",
            Self::ExternalUntrusted => "external_untrusted",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "local_machine" => Some(Self::LocalMachine),
            "owner_controlled_estate" => Some(Self::OwnerControlledEstate),
            "administratively_managed" => Some(Self::AdministrativelyManaged),
            "external_untrusted" => Some(Self::ExternalUntrusted),
            _ => None,
        }
    }
}
