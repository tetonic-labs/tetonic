//! Local-owner bootstrap and compatibility API. Work behavior lives in `work`;
//! agent/provider/capability setup lives in `workspace`. This module owns neither.
mod bootstrap;
pub use crate::work::WorkService as LocalWorkspace;
pub use crate::work::*;
pub use crate::workspace::INPUT_LIMIT;
pub(crate) const OWNER: &str = "local-ui-owner";
pub(crate) const ORG: &str = "local-ui";
pub(crate) const TEAM: &str = "local-work";
const AUDIENCE: &str = "tetonic-local-ui-v1";
