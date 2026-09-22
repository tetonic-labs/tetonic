//! Active typed-job leases (R7-1 continuous heartbeat / lease timeout).

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Default lease TTL when the coordinator does not negotiate a shorter window.
pub fn default_lease_ttl() -> Duration {
    let ms = std::env::var("LOKAI_FABRIC_LEASE_TTL_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(30_000);
    Duration::from_millis(ms.max(200))
}

/// How often the coordinator should renew (≈ TTL / 3).
#[allow(dead_code)]
pub fn default_renew_interval(ttl: Duration) -> Duration {
    let third = ttl / 3;
    if third.is_zero() {
        Duration::from_millis(50)
    } else {
        third
    }
}

#[derive(Debug, Clone)]
pub struct ActiveLease {
    pub job_id: String,
    pub attempt_id: String,
    pub lease_epoch: u64,
    pub expires_at: Instant,
}

#[derive(Debug, Default)]
pub struct LeaseTable {
    by_attempt: HashMap<String, ActiveLease>,
}

impl LeaseTable {
    pub fn insert(&mut self, lease: ActiveLease) {
        self.by_attempt.insert(lease.attempt_id.clone(), lease);
    }

    pub fn remove(&mut self, attempt_id: &str) {
        self.by_attempt.remove(attempt_id);
    }

    pub fn renew(
        &mut self,
        attempt_id: &str,
        job_id: &str,
        lease_epoch: u64,
        ttl: Duration,
    ) -> Option<Instant> {
        let entry = self.by_attempt.get_mut(attempt_id)?;
        if entry.job_id != job_id || entry.lease_epoch != lease_epoch {
            return None;
        }
        entry.expires_at = Instant::now() + ttl;
        Some(entry.expires_at)
    }

    pub fn expires_at(&self, attempt_id: &str) -> Option<Instant> {
        self.by_attempt.get(attempt_id).map(|l| l.expires_at)
    }

    /// Attempts whose leases have elapsed (need cancel).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn expired_job_ids(&self, now: Instant) -> Vec<(String, String)> {
        self.by_attempt
            .values()
            .filter(|l| l.expires_at <= now)
            .map(|l| (l.job_id.clone(), l.attempt_id.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renew_extends_expiry() {
        let mut t = LeaseTable::default();
        let start = Instant::now();
        t.insert(ActiveLease {
            job_id: "j".into(),
            attempt_id: "a".into(),
            lease_epoch: 1,
            expires_at: start + Duration::from_millis(100),
        });
        let next = t.renew("a", "j", 1, Duration::from_secs(5)).expect("renew");
        assert!(next > start + Duration::from_millis(100));
    }

    #[test]
    fn expired_lists_stale_jobs() {
        let mut t = LeaseTable::default();
        t.insert(ActiveLease {
            job_id: "j".into(),
            attempt_id: "a".into(),
            lease_epoch: 1,
            expires_at: Instant::now() - Duration::from_millis(1),
        });
        let expired = t.expired_job_ids(Instant::now());
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].0, "j");
    }
}
