//! Per-worker circuit breaker (M6-2).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

struct WorkerCircuit {
    state: CircuitState,
    failures: u32,
    opened_at: Option<Instant>,
    half_open_successes: u32,
}

pub struct CircuitBreakerRegistry {
    inner: Mutex<HashMap<String, WorkerCircuit>>,
    failure_threshold: u32,
    open_cooldown: Duration,
    half_open_success_threshold: u32,
}

impl Default for CircuitBreakerRegistry {
    fn default() -> Self {
        Self::new(3, Duration::from_secs(30), 1)
    }
}

impl CircuitBreakerRegistry {
    pub fn new(
        failure_threshold: u32,
        open_cooldown: Duration,
        half_open_success_threshold: u32,
    ) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            failure_threshold,
            open_cooldown,
            half_open_success_threshold,
        }
    }

    pub fn state(&self, worker_id: &str) -> CircuitState {
        let mut g = self.inner.lock().unwrap();
        let entry = g.entry(worker_id.to_string()).or_insert(WorkerCircuit {
            state: CircuitState::Closed,
            failures: 0,
            opened_at: None,
            half_open_successes: 0,
        });
        if entry.state == CircuitState::Open {
            if let Some(at) = entry.opened_at {
                if at.elapsed() >= self.open_cooldown {
                    entry.state = CircuitState::HalfOpen;
                    entry.half_open_successes = 0;
                }
            }
        }
        entry.state
    }

    pub fn allows_dispatch(&self, worker_id: &str) -> bool {
        !matches!(self.state(worker_id), CircuitState::Open)
    }

    pub fn record_success(&self, worker_id: &str) {
        let mut g = self.inner.lock().unwrap();
        let entry = g.entry(worker_id.to_string()).or_insert(WorkerCircuit {
            state: CircuitState::Closed,
            failures: 0,
            opened_at: None,
            half_open_successes: 0,
        });
        match entry.state {
            CircuitState::HalfOpen => {
                entry.half_open_successes += 1;
                if entry.half_open_successes >= self.half_open_success_threshold {
                    entry.state = CircuitState::Closed;
                    entry.failures = 0;
                    entry.opened_at = None;
                }
            }
            _ => {
                entry.state = CircuitState::Closed;
                entry.failures = 0;
                entry.opened_at = None;
            }
        }
    }

    pub fn record_failure(&self, worker_id: &str) {
        let mut g = self.inner.lock().unwrap();
        let entry = g.entry(worker_id.to_string()).or_insert(WorkerCircuit {
            state: CircuitState::Closed,
            failures: 0,
            opened_at: None,
            half_open_successes: 0,
        });
        entry.failures = entry.failures.saturating_add(1);
        if entry.failures >= self.failure_threshold || matches!(entry.state, CircuitState::HalfOpen)
        {
            entry.state = CircuitState::Open;
            entry.opened_at = Some(Instant::now());
            entry.half_open_successes = 0;
        }
    }
}
