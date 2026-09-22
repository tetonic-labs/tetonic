//! Policy mode: homelab estate stub vs full Circle (N0 / N2).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum PolicyMode {
    /// Owner estate remote OK; circle social denied (N0–N1 homelab).
    #[default]
    EstateStub,
    /// Full bilateral disclosure + circle routing (N2+).
    Full,
}

impl PolicyMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "estate_stub" | "EstateStub" => Some(Self::EstateStub),
            "full" | "Full" => Some(Self::Full),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::EstateStub => "estate_stub",
            Self::Full => "full",
        }
    }
}
