//! Workspace-versioned transactions (M2-4).

pub mod commit;
pub mod diff;
pub mod error;
pub mod fs_ops;
pub mod fuzzy_patch;
pub mod journal;
pub mod lock;
mod publication;
pub mod recovery;
pub mod security;
pub mod service;
pub mod staging;
pub mod txn_meta;
pub mod verify_view;
pub mod version;

pub use commit::{
    apply_journal, apply_journal_operation, build_journal, detect_conflicts, rollback_journal,
};
pub use error::TransactionError;
pub use fs_ops::digest_string;
pub use journal::CommitJournal;
pub use lock::WriterLock;
pub use recovery::{
    ensure_no_unresolved_recovery, list_unresolved_recovery, recover_at_startup, RecoveryReport,
};
pub use security::SecurityLimits;
pub use service::{
    clear_active_transaction, with_active_transaction, WorkspaceTransactionService,
    WorkspaceTxnConfig,
};
pub use staging::{ReadWriteSets, StagingArea};
pub use txn_meta::TransactionMeta;
pub use verify_view::{detect_unexpected_mutations, snapshot_read_digests};
