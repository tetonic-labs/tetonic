//! LSP subprocess lifecycle FSM (AC2-10).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LspLifecycle {
    NotStarted,
    Starting,
    Ready,
    Unhealthy,
    Stopping,
    Stopped,
}

impl LspLifecycle {
    pub fn transition(self, next: LspLifecycle) -> Result<LspLifecycle, &'static str> {
        let ok = matches!(
            (self, next),
            (LspLifecycle::NotStarted, LspLifecycle::Starting)
                | (LspLifecycle::Starting, LspLifecycle::Ready)
                | (LspLifecycle::Starting, LspLifecycle::Unhealthy)
                | (LspLifecycle::Ready, LspLifecycle::Unhealthy)
                | (LspLifecycle::Ready, LspLifecycle::Stopping)
                | (LspLifecycle::Unhealthy, LspLifecycle::Stopping)
                | (LspLifecycle::Unhealthy, LspLifecycle::Starting)
                | (LspLifecycle::Stopping, LspLifecycle::Stopped)
                | (LspLifecycle::Stopped, LspLifecycle::Starting)
        );
        if ok {
            Ok(next)
        } else {
            Err("invalid LSP lifecycle transition")
        }
    }

    pub fn is_usable(self) -> bool {
        matches!(self, LspLifecycle::Ready)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_to_ready() {
        let s = LspLifecycle::NotStarted;
        let s = s.transition(LspLifecycle::Starting).unwrap();
        let s = s.transition(LspLifecycle::Ready).unwrap();
        assert!(s.is_usable());
    }

    #[test]
    fn crash_marks_unhealthy_and_recovers() {
        let s = LspLifecycle::Ready;
        let s = s.transition(LspLifecycle::Unhealthy).unwrap();
        assert!(!s.is_usable());
        let s = s.transition(LspLifecycle::Starting).unwrap();
        let s = s.transition(LspLifecycle::Ready).unwrap();
        assert!(s.is_usable());
    }

    #[test]
    fn invalid_transition_rejected() {
        assert!(LspLifecycle::NotStarted
            .transition(LspLifecycle::Ready)
            .is_err());
    }
}
