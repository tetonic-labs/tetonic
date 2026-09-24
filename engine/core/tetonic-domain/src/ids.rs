//! Opaque identifiers shared by local and remote execution paths.

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! id_newtype {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(raw: impl Into<String>) -> Self {
                Self(raw.into())
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

id_newtype!(TurnId);
id_newtype!(ActionId);
id_newtype!(JobId);
id_newtype!(AttemptId);
id_newtype!(ApprovalId);
id_newtype!(CapabilityId);
id_newtype!(ExecutionId);
id_newtype!(SessionId);
id_newtype!(RunId);
id_newtype!(TaskId);
id_newtype!(AgentId);
id_newtype!(TransactionId);
id_newtype!(LeaseId);
id_newtype!(ArtifactId);
id_newtype!(WorkerId);
id_newtype!(EvidenceId);
id_newtype!(ExpansionHandleId);
id_newtype!(EventId);
id_newtype!(TraceId);
id_newtype!(CommandId);
id_newtype!(ResultId);
id_newtype!(KeyId);
id_newtype!(CoordinatorId);
id_newtype!(ReservationId);
id_newtype!(IdentityId);
id_newtype!(OrgId);
id_newtype!(SquadId);

pub use crate::workspace::WorkspaceVersion;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_json() {
        let id = ActionId::new("act_01");
        let j = serde_json::to_string(&id).unwrap();
        assert_eq!(j, "\"act_01\"");
        let back: ActionId = serde_json::from_str(&j).unwrap();
        assert_eq!(back, id);
    }
}
