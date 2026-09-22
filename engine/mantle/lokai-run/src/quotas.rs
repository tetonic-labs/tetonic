//! Storage quota and limits management (M3-4).

use lokai_domain::StorageLimits;
use std::sync::Mutex;
use tracing::{error, warn};

pub struct StorageQuotaManager {
    limits: StorageLimits,
    current_global_usage_bytes: Mutex<u64>,
}

impl Default for StorageQuotaManager {
    fn default() -> Self {
        Self {
            limits: StorageLimits::default(),
            current_global_usage_bytes: Mutex::new(0),
        }
    }
}

impl StorageQuotaManager {
    pub fn new(limits: StorageLimits) -> Self {
        Self {
            limits,
            current_global_usage_bytes: Mutex::new(0),
        }
    }

    pub fn check_before_append(&self, payload_size: u64) -> Result<bool, &'static str> {
        let usage = *self.current_global_usage_bytes.lock().unwrap();
        if usage + payload_size > self.limits.global_quota_bytes {
            error!("Hard storage quota exceeded. Blocking side effects and writes.");
            return Err("Storage limit exceeded");
        }

        let soft_limit = (self.limits.global_quota_bytes as f64 * 0.8) as u64;
        if usage + payload_size > soft_limit {
            warn!("Soft storage quota exceeded. Compaction should be triggered.");
            return Ok(true);
        }

        Ok(false)
    }

    pub fn record_usage(&self, size: u64) {
        let mut usage = self.current_global_usage_bytes.lock().unwrap();
        *usage += size;
    }

    pub fn free_usage(&self, size: u64) {
        let mut usage = self.current_global_usage_bytes.lock().unwrap();
        *usage = usage.saturating_sub(size);
    }
}
