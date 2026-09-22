//! Worker GPU job scheduler — priority queues + owner preemption (N1.1).

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use lokai_inference::{JobPriority, JobStatus};
use tokio::sync::{oneshot, watch, Mutex};

const DEFAULT_OWNER_TTL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelReason {
    OwnerPreempt,
}

#[derive(Debug)]
pub enum SchedulerError {
    OwnerActiveBlocksCircle,
    Preempted(CancelReason),
}

impl std::fmt::Display for SchedulerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SchedulerError::OwnerActiveBlocksCircle => {
                write!(f, "owner active — circle jobs not accepted")
            }
            SchedulerError::Preempted(CancelReason::OwnerPreempt) => {
                write!(f, "preempted for owner")
            }
        }
    }
}

#[derive(Debug)]
struct RunningJob {
    job_id: String,
    priority: JobPriority,
    cancel: watch::Sender<bool>,
}

#[derive(Debug)]
struct WaitEntry {
    job_id: String,
    priority: JobPriority,
    grant: oneshot::Sender<Result<watch::Receiver<bool>, SchedulerError>>,
}

#[derive(Debug)]
struct Inner {
    running: Option<RunningJob>,
    owner_active_until: Option<Instant>,
    waiters: Vec<WaitEntry>,
}

/// Serializes fabric inference on one GPU; owner jobs preempt circle work.
#[derive(Debug, Clone)]
pub struct WorkerScheduler {
    inner: Arc<Mutex<Inner>>,
    queue_depth: Arc<AtomicU32>,
}

impl Default for WorkerScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkerScheduler {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                running: None,
                owner_active_until: None,
                waiters: Vec::new(),
            })),
            queue_depth: Arc::new(AtomicU32::new(0)),
        }
    }

    pub fn queue_depth(&self) -> u32 {
        self.queue_depth.load(Ordering::Relaxed)
    }

    fn owner_slot_active(inner: &Inner) -> bool {
        inner.owner_active_until.is_some_and(|t| Instant::now() < t)
    }

    fn sync_queue_depth(inner: &Inner, depth: &AtomicU32) {
        let n = inner.waiters.len() as u32 + u32::from(inner.running.is_some());
        depth.store(n, Ordering::Relaxed);
    }

    fn push_waiter(waiters: &mut Vec<WaitEntry>, entry: WaitEntry) {
        let rank = priority_rank(entry.priority);
        let pos = waiters
            .iter()
            .position(|w| priority_rank(w.priority) < rank)
            .unwrap_or(waiters.len());
        waiters.insert(pos, entry);
    }

    fn reject_circle_waiters(inner: &mut Inner) {
        let mut keep = Vec::new();
        for w in inner.waiters.drain(..) {
            if is_circle_priority(w.priority) {
                let _ = w.grant.send(Err(SchedulerError::OwnerActiveBlocksCircle));
            } else {
                keep.push(w);
            }
        }
        inner.waiters = keep;
    }

    fn try_take_slot(
        inner: &mut Inner,
        job_id: &str,
        priority: JobPriority,
    ) -> Result<Option<watch::Receiver<bool>>, SchedulerError> {
        if is_circle_priority(priority) && Self::owner_slot_active(inner) {
            return Err(SchedulerError::OwnerActiveBlocksCircle);
        }

        if let Some(running) = &inner.running {
            if running.job_id == job_id {
                let (_tx, rx) = watch::channel(false);
                return Ok(Some(rx));
            }
            if should_preempt(running.priority, priority) {
                let _ = running.cancel.send(true);
            } else {
                return Ok(None);
            }
        }

        let (tx, rx) = watch::channel(false);
        inner.running = Some(RunningJob {
            job_id: job_id.to_string(),
            priority,
            cancel: tx,
        });
        tracing::debug!(job_id, ?priority, "scheduler acquire");
        Ok(Some(rx))
    }

    fn grant_next_waiter(inner: &mut Inner) {
        while !inner.waiters.is_empty() {
            let next = inner.waiters.remove(0);
            if next.grant.is_closed() {
                continue;
            }
            match Self::try_take_slot(inner, &next.job_id, next.priority) {
                Ok(Some(cancel_rx)) => {
                    if next.grant.send(Ok(cancel_rx)).is_ok() {
                        return;
                    }
                    // The waiter disappeared after reservation. Nobody can release it.
                    inner.running = None;
                }
                Ok(None) => {
                    inner.waiters.insert(0, next);
                    return;
                }
                Err(e) => {
                    let _ = next.grant.send(Err(e));
                }
            }
        }
    }

    pub async fn owner_active(&self) -> bool {
        let inner = self.inner.lock().await;
        Self::owner_slot_active(&inner)
    }

    /// Coordinator or local combined-mode: owner is using the machine.
    pub async fn signal_owner_activity(&self, ttl: Duration) {
        let mut inner = self.inner.lock().await;
        inner.owner_active_until = Some(Instant::now() + ttl);
        if let Some(running) = &inner.running {
            if is_circle_priority(running.priority) {
                let _ = running.cancel.send(true);
            }
        }
        Self::reject_circle_waiters(&mut inner);
        Self::sync_queue_depth(&inner, &self.queue_depth);
    }

    pub async fn signal_owner_activity_default(&self) {
        self.signal_owner_activity(DEFAULT_OWNER_TTL).await;
    }

    /// Reserve the GPU slot; waits in a priority queue when busy instead of rejecting.
    pub async fn acquire(
        &self,
        job_id: &str,
        priority: JobPriority,
    ) -> Result<watch::Receiver<bool>, SchedulerError> {
        loop {
            let mut inner = self.inner.lock().await;

            match Self::try_take_slot(&mut inner, job_id, priority)? {
                Some(rx) => {
                    Self::sync_queue_depth(&inner, &self.queue_depth);
                    return Ok(rx);
                }
                None => {
                    let (grant_tx, grant_rx) = oneshot::channel();
                    Self::push_waiter(
                        &mut inner.waiters,
                        WaitEntry {
                            job_id: job_id.to_string(),
                            priority,
                            grant: grant_tx,
                        },
                    );
                    Self::sync_queue_depth(&inner, &self.queue_depth);
                    drop(inner);

                    match grant_rx.await {
                        Ok(result) => return result,
                        Err(_) => {
                            let mut inner = self.inner.lock().await;
                            inner.waiters.retain(|w| w.job_id != job_id);
                            Self::sync_queue_depth(&inner, &self.queue_depth);
                            continue;
                        }
                    }
                }
            }
        }
    }

    pub async fn release(&self, job_id: &str) {
        let mut inner = self.inner.lock().await;
        if inner.running.as_ref().is_some_and(|r| r.job_id == job_id) {
            inner.running = None;
            tracing::debug!(job_id, "scheduler release");
            Self::grant_next_waiter(&mut inner);
        }
        Self::sync_queue_depth(&inner, &self.queue_depth);
    }

    /// Cancel any in-flight job (e.g. coordinator revoke, SEC-008).
    pub async fn cancel_running(&self) {
        let inner = self.inner.lock().await;
        if let Some(running) = &inner.running {
            let _ = running.cancel.send(true);
        }
    }

    /// Cancel the running job when `job_id` matches (typed `/v1/jobs/cancel`, R7-1).
    /// Returns whether a matching in-flight job was signaled.
    pub async fn cancel_job(&self, job_id: &str) -> bool {
        let inner = self.inner.lock().await;
        if let Some(running) = &inner.running {
            if running.job_id == job_id {
                let _ = running.cancel.send(true);
                return true;
            }
        }
        false
    }

    pub async fn is_running(&self, job_id: &str) -> bool {
        let inner = self.inner.lock().await;
        inner.running.as_ref().is_some_and(|r| r.job_id == job_id)
    }

    pub fn check_cancelled(cancel: &watch::Receiver<bool>) -> Result<(), SchedulerError> {
        if *cancel.borrow() {
            Err(SchedulerError::Preempted(CancelReason::OwnerPreempt))
        } else {
            Ok(())
        }
    }

    pub fn job_status_for_error(err: &SchedulerError) -> JobStatus {
        match err {
            SchedulerError::Preempted(_) => JobStatus::Preempted,
            _ => JobStatus::Denied,
        }
    }
}

pub fn priority_rank(p: JobPriority) -> u8 {
    match p {
        JobPriority::OwnerInteractive => 4,
        JobPriority::OwnerBackground => 3,
        JobPriority::CircleInteractive => 2,
        JobPriority::CircleBackground => 1,
    }
}

pub fn is_circle_priority(p: JobPriority) -> bool {
    matches!(
        p,
        JobPriority::CircleInteractive | JobPriority::CircleBackground
    )
}

pub fn should_preempt(running: JobPriority, incoming: JobPriority) -> bool {
    priority_rank(incoming) > priority_rank(running)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lokai_inference::FabricJob;

    fn circle_job(id: &str) -> FabricJob {
        FabricJob {
            job_id: id.into(),
            attempt_id: None,
            estate_id: "e".into(),
            session_id: None,
            agent_id: "a".into(),
            step_index: 0,
            model: "m".into(),
            tier: None,
            messages: vec![],
            tools: vec![],
            options: Default::default(),
            priority: JobPriority::CircleBackground,
            data_class: Default::default(),
            disclosure_tier: Default::default(),
            audit_envelope: None,
            circle_id: None,
            consumer_peer_id: None,
            policy_epoch: 0,
            turn_affinity: None,
        }
    }

    #[test]
    fn priority_ordering() {
        assert!(
            priority_rank(JobPriority::OwnerInteractive)
                > priority_rank(JobPriority::OwnerBackground)
        );
        assert!(
            priority_rank(JobPriority::OwnerBackground)
                > priority_rank(JobPriority::CircleInteractive)
        );
        assert!(should_preempt(
            JobPriority::CircleBackground,
            JobPriority::OwnerInteractive
        ));
        assert!(!should_preempt(
            JobPriority::OwnerInteractive,
            JobPriority::CircleBackground
        ));
    }

    #[tokio::test]
    async fn cancel_job_signals_matching_running() {
        let sched = WorkerScheduler::new();
        let job = circle_job("typed_job");
        let mut rx = sched.acquire(&job.job_id, job.priority).await.unwrap();
        assert!(!sched.cancel_job("other").await);
        assert!(sched.cancel_job("typed_job").await);
        assert!(*rx.borrow_and_update());
    }

    #[tokio::test]
    async fn owner_activity_blocks_circle_submit() {
        let sched = WorkerScheduler::new();
        sched.signal_owner_activity(Duration::from_secs(60)).await;

        let job = circle_job("c1");
        assert!(matches!(
            sched.acquire(&job.job_id, job.priority).await,
            Err(SchedulerError::OwnerActiveBlocksCircle)
        ));
    }

    #[tokio::test]
    async fn owner_interactive_preempts_circle_running() {
        let sched = WorkerScheduler::new();
        let circle = circle_job("circle");
        let mut owner = circle_job("owner");
        owner.priority = JobPriority::OwnerInteractive;

        let circle_rx = sched
            .acquire(&circle.job_id, circle.priority)
            .await
            .unwrap();
        let _owner_rx = sched.acquire(&owner.job_id, owner.priority).await.unwrap();
        assert!(*circle_rx.borrow());
        sched.release("owner").await;
    }

    #[tokio::test]
    async fn signal_owner_activity_cancels_running_circle() {
        let sched = WorkerScheduler::new();
        let circle = circle_job("circle");
        let mut rx = sched
            .acquire(&circle.job_id, circle.priority)
            .await
            .unwrap();
        sched.signal_owner_activity(Duration::from_secs(30)).await;
        assert!(*rx.borrow_and_update());
    }

    #[tokio::test]
    async fn busy_jobs_wait_in_queue_instead_of_rejection() {
        let sched = WorkerScheduler::new();
        let first = circle_job("first");
        let second = circle_job("second");

        let _rx1 = sched.acquire(&first.job_id, first.priority).await.unwrap();
        assert_eq!(sched.queue_depth(), 1);

        let sched2 = sched.clone();
        let second_id = second.job_id.clone();
        let second_priority = second.priority;
        let waiter = tokio::spawn(async move { sched2.acquire(&second_id, second_priority).await });

        tokio::task::yield_now().await;
        assert_eq!(sched.queue_depth(), 2);

        sched.release("first").await;
        assert!(waiter.await.unwrap().is_ok());
        assert_eq!(sched.queue_depth(), 1);

        sched.release("second").await;
        assert_eq!(sched.queue_depth(), 0);
    }

    #[tokio::test]
    async fn owner_activity_rejects_queued_circle_waiters() {
        let sched = WorkerScheduler::new();
        let first = circle_job("first");
        let second = circle_job("second");

        let _rx1 = sched.acquire(&first.job_id, first.priority).await.unwrap();
        let sched2 = sched.clone();
        let waiter =
            tokio::spawn(async move { sched2.acquire(&second.job_id, second.priority).await });

        tokio::task::yield_now().await;
        sched.signal_owner_activity(Duration::from_secs(30)).await;

        assert!(matches!(
            waiter.await.unwrap(),
            Err(SchedulerError::OwnerActiveBlocksCircle)
        ));
    }
    #[tokio::test]
    async fn dropped_waiter_does_not_reserve_slot() {
        use std::future::Future;
        let sched = WorkerScheduler::new();
        let _running = sched
            .acquire("first", JobPriority::OwnerBackground)
            .await
            .unwrap();
        let mut abandoned = Box::pin(sched.acquire("abandoned", JobPriority::OwnerBackground));
        std::future::poll_fn(|cx| {
            assert!(abandoned.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        drop(abandoned);
        sched.release("first").await;
        assert!(!sched.is_running("abandoned").await);
        let next = tokio::time::timeout(
            Duration::from_secs(1),
            sched.acquire("next", JobPriority::OwnerBackground),
        )
        .await;
        assert!(next.unwrap().is_ok());
    }
}
