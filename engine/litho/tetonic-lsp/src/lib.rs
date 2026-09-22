//! Local LSP adapter (D9): spawn language servers as subprocesses, no network.

mod client;
pub mod detect;
pub mod framing;
mod inbox;
mod launcher;
mod pool;
pub mod state;
pub mod util;
mod writer;

pub use client::{path_to_uri, DiagnosticHit, LocationHit, LspError, LspSession};
pub use detect::{available_languages, detect_server, lsp_enabled_by_env, Lang, ServerSpec};
pub use launcher::{
    clear_process_launcher, set_process_launcher, spawned_from_io, LspProcessHandle,
    LspProcessLauncher, SpawnedLspProcess,
};
pub use pool::{LspPool, LspPoolError};
pub use state::LspLifecycle;
pub use util::{normalize_character, utf16_col_at_byte};
