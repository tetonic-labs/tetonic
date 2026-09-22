//! Production approval hook wrapper (AC2-1).

use lokai_core::ApprovalHook;

/// How the host gates destructive/networked tools before assembly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalKind {
    /// Interactive host prompt (daemon RPC or CLI stdin).
    HostInteractive,
    /// Auto-allow only verify-finish (CLI without store/shell hook).
    VerifyFinishOnly,
    /// Pre-approved shell (`--allow-shell`); not valid for session assembly.
    AllowAll,
}

/// Host approval gate with an explicit kind for runtime validation.
pub struct ProductionApproval {
    hook: ApprovalHook,
    kind: ApprovalKind,
}

impl ProductionApproval {
    pub fn host(hook: ApprovalHook) -> Self {
        Self {
            hook,
            kind: ApprovalKind::HostInteractive,
        }
    }

    pub fn verify_finish_only(hook: ApprovalHook) -> Self {
        Self {
            hook,
            kind: ApprovalKind::VerifyFinishOnly,
        }
    }

    pub fn allow_all() -> Self {
        Self {
            hook: std::sync::Arc::new(|_| Box::pin(async { true })),
            kind: ApprovalKind::AllowAll,
        }
    }

    pub fn kind(&self) -> ApprovalKind {
        self.kind
    }

    pub fn into_hook(self) -> ApprovalHook {
        self.hook
    }
}

impl ProductionApproval {
    /// Default verify-finish-only hook for ephemeral CLI runs.
    pub fn cli_verify_finish_only() -> Self {
        Self::verify_finish_only(std::sync::Arc::new(|req| {
            Box::pin(async move { req.kind == "verify_finish" })
        }))
    }
}
