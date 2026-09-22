//! Poison-tolerant mutex helper (H3-3).
//!
//! A panic that held a `Mutex` still happened. Recovering the guard lets other
//! sessions continue instead of aborting on the next `lock()`.

use std::sync::{Mutex, MutexGuard};

/// Recover a poisoned `Mutex` instead of panicking.
pub fn mutex_lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// `lock_recover()` on `Mutex` and `Arc<Mutex<_>>` (coordinator hot paths).
pub trait RecoverMutex<T> {
    fn lock_recover(&self) -> MutexGuard<'_, T>;
}

impl<T> RecoverMutex<T> for Mutex<T> {
    fn lock_recover(&self) -> MutexGuard<'_, T> {
        mutex_lock(self)
    }
}

impl<T> RecoverMutex<T> for std::sync::Arc<Mutex<T>> {
    fn lock_recover(&self) -> MutexGuard<'_, T> {
        mutex_lock(self.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn poisoned_mutex_recovers() {
        let m = Arc::new(Mutex::new(1u32));
        let m2 = m.clone();
        let _ = thread::spawn(move || {
            let _g = m2.lock().unwrap();
            panic!("intentional poison");
        })
        .join();
        *mutex_lock(&m) = 2;
        assert_eq!(*m.lock_recover(), 2);
    }
}
