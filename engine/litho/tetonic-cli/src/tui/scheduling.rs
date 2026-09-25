//! Portal-local scheduling: preserve event order while returning control to input.

use std::time::{Duration, Instant};

const MAX_EVENTS_PER_BATCH: usize = 128;
const MAX_BATCH_TIME: Duration = Duration::from_millis(4);

/// Coalesce updates without slowing the independent event/input polling loop.
pub(super) struct Frames {
    last: Option<Instant>,
    dirty: bool,
}

impl Frames {
    pub fn new() -> Self {
        Self {
            last: None,
            dirty: true,
        }
    }
    pub fn changed(&mut self) {
        self.dirty = true;
    }
    pub fn due(&mut self, now: Instant, animated: bool) -> bool {
        if !(self.dirty || animated)
            || self
                .last
                .is_some_and(|last| now.duration_since(last) < Duration::from_millis(33))
        {
            return false;
        }
        self.last = Some(now);
        self.dirty = false;
        true
    }
}

pub(super) fn drain_batch<T>(
    mut receive: impl FnMut() -> Option<T>,
    apply: impl FnMut(T),
) -> usize {
    drain_with_budget(&mut receive, MAX_EVENTS_PER_BATCH, MAX_BATCH_TIME, apply)
}

fn drain_with_budget<T>(
    mut receive: impl FnMut() -> Option<T>,
    limit: usize,
    budget: Duration,
    mut apply: impl FnMut(T),
) -> usize {
    let started = Instant::now();
    let mut processed = 0;
    // Always allow one event to make progress. The time budget is cooperative:
    // it cannot preempt an individual synchronous handler.
    while processed < limit && (processed == 0 || started.elapsed() < budget) {
        let Some(event) = receive() else {
            break;
        };
        apply(event);
        processed += 1;
    }
    processed
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[test]
    fn output_bursts_coalesce_but_pending_changes_are_not_lost() {
        let now = Instant::now();
        let mut frames = Frames::new();
        assert!(frames.due(now, false));
        let mut draws = 1;
        for ms in 1..=1000 {
            frames.changed();
            draws += usize::from(frames.due(now + Duration::from_millis(ms), false));
        }
        assert_eq!(draws, 31);
        assert!(frames.due(now + Duration::from_millis(1023), false));
        assert!(!frames.due(now + Duration::from_secs(10), false));
    }

    #[test]
    fn animation_redraws_without_dirty_events_and_idle_stops() {
        let now = Instant::now();
        let mut frames = Frames::new();
        assert!(frames.due(now, false));
        assert!(!frames.due(now + Duration::from_millis(10), true));
        assert!(frames.due(now + Duration::from_millis(33), true));
        assert!(!frames.due(now + Duration::from_secs(1), false));
        frames.changed();
        assert!(frames.due(now + Duration::from_secs(1), false));
    }

    #[test]
    fn replenished_queue_returns_control_without_losing_or_reordering_events() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        tx.send(0).unwrap();
        let mut seen = Vec::new();
        let count = drain_with_budget(
            || rx.try_recv().ok(),
            128,
            Duration::from_secs(60),
            |value| {
                seen.push(value);
                tx.send(value + 1).unwrap();
            },
        );
        assert_eq!(count, 128);
        assert_eq!(seen, (0..128).collect::<Vec<_>>());
        assert_eq!(rx.try_recv().unwrap(), 128);
    }

    #[test]
    fn expired_budget_leaves_remaining_work_for_next_iteration() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        tx.send("output").unwrap();
        tx.send("approval").unwrap();
        let mut seen = Vec::new();
        assert_eq!(
            drain_with_budget(|| rx.try_recv().ok(), 128, Duration::ZERO, |e| seen.push(e)),
            1
        );
        assert_eq!(seen, ["output"]);
        assert_eq!(drain_batch(|| rx.try_recv().ok(), |e| seen.push(e)), 1);
        assert_eq!(seen, ["output", "approval"]);
    }

    #[test]
    fn disconnected_queue_is_drained_before_returning_empty() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        tx.send("failure").unwrap();
        drop(tx);
        let mut seen = Vec::new();
        assert_eq!(drain_batch(|| rx.try_recv().ok(), |e| seen.push(e)), 1);
        assert_eq!(seen, ["failure"]);
        assert_eq!(
            drain_batch(|| rx.try_recv().ok(), |_| panic!("empty queue")),
            0
        );
    }
}
