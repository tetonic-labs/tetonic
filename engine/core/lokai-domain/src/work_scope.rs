//! In-process effect lifetime contract. No executor, timer, or product policy.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

/// Read-only observation of cooperative cancellation; no executor or OS policy.
#[derive(Clone, Default)]
pub struct CancellationSignal(Arc<AtomicBool>);
impl CancellationSignal {
    pub fn from_flag(flag: Arc<AtomicBool>) -> Self {
        Self(flag)
    }
    pub fn is_canceled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Default)]
struct State {
    closed: bool,
    signal: CancellationSignal,
    outstanding: usize,
}

/// Cancellation closes admission. A lease belongs to the actual worker, not
/// the async future waiting for it. Quiescence is distinct from cancellation.
#[derive(Clone, Default)]
pub struct WorkScope(Arc<Mutex<State>>);

impl WorkScope {
    pub fn try_enter(&self) -> Option<WorkLease> {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.closed {
            return None;
        }
        state.outstanding += 1;
        Some(WorkLease(self.clone()))
    }
    pub fn cancel(&self) {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        state.closed = true;
        state.signal.0.store(true, Ordering::Release);
    }
    pub fn cancellation_signal(&self) -> CancellationSignal {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .signal
            .clone()
    }
    pub fn is_canceled(&self) -> bool {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).closed
    }
    pub fn is_quiescent(&self) -> bool {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).outstanding == 0
    }
}

pub struct WorkLease(WorkScope);
impl Drop for WorkLease {
    fn drop(&mut self) {
        self.0
             .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .outstanding -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_closes_admission_but_does_not_release_workers() {
        let scope = WorkScope::default();
        let lease = scope.try_enter().unwrap();
        let signal = scope.cancellation_signal();
        assert!(!signal.is_canceled());
        scope.cancel();
        assert!(signal.is_canceled());
        assert!(!WorkScope::default().cancellation_signal().is_canceled());
        assert!(scope.try_enter().is_none());
        assert!(!scope.is_quiescent());
        drop(lease);
        assert!(scope.is_quiescent());
    }
}
