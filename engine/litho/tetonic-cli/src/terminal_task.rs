//! Own the submission worker for exactly the lifetime of its terminal view.
use std::future::Future;
use tokio::task::JoinHandle;

/// Bound retained UI diagnostics; truncation is explicit and UTF-8 safe.
pub fn bounded_error(mut text: String) -> String {
    const LIMIT: usize = 64 * 1024;
    const SUFFIX: &str = "\n[diagnostic truncated at the terminal size limit]";
    if text.len() > LIMIT {
        let mut end = LIMIT - SUFFIX.len();
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str(SUFFIX);
        // truncate alone retains the original (possibly enormous) allocation.
        text.shrink_to_fit();
    }
    text
}

struct Worker(JoinHandle<()>);

impl Drop for Worker {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub fn supervise<T>(
    worker: JoinHandle<()>,
    view: impl Future<Output = anyhow::Result<T>>,
) -> impl Future<Output = anyhow::Result<T>> {
    // Construct the guard before the first poll so dropping an unpolled
    // supervision future also aborts its already-spawned worker.
    let worker = Worker(worker);
    async move {
        let mut worker = worker;
        tokio::pin!(view);
        tokio::select! {
            result = &mut view => {
                worker.0.abort();
                // Join before the caller starts session cleanup; queued commands
                // cannot race that cleanup. Drop also aborts if this future is canceled.
                let stopped = (&mut worker.0).await;
                if let Err(error) = stopped {
                    if !error.is_cancelled() && result.is_ok() {
                        anyhow::bail!("Terminal submission worker failed: {error}");
                    }
                }
                result
            }
            stopped = &mut worker.0 => {
                match stopped {
                    Ok(()) => anyhow::bail!("Terminal submission worker stopped unexpectedly"),
                    Err(error) => anyhow::bail!("Terminal submission worker failed: {error}"),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    struct Finished(Arc<AtomicBool>);
    impl Drop for Finished {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[test]
    fn diagnostic_limit_preserves_unicode_and_marks_truncation() {
        assert_eq!(bounded_error("ordinary error".into()), "ordinary error");
        let error = bounded_error("🦀".repeat(65536));
        assert!(error.len() <= 65536);
        assert!(error.ends_with("[diagnostic truncated at the terminal size limit]"));
        assert!(error.starts_with("🦀"));
    }

    #[tokio::test]
    async fn terminal_error_joins_worker_before_returning() {
        let finished = Arc::new(AtomicBool::new(false));
        let guard = Finished(finished.clone());
        let worker = tokio::spawn(async move {
            let _guard = guard;
            std::future::pending::<()>().await;
        });
        let result = supervise(worker, async {
            Err::<(), _>(anyhow::anyhow!("terminal failed"))
        })
        .await;
        assert_eq!(result.unwrap_err().to_string(), "terminal failed");
        assert!(finished.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn worker_panic_stops_the_terminal_view() {
        let worker = tokio::spawn(async {
            panic!("worker failure");
        });
        let result = supervise(worker, std::future::pending::<anyhow::Result<()>>()).await;
        assert!(result.unwrap_err().to_string().contains("worker failure"));
    }

    #[tokio::test]
    async fn dropping_unpolled_supervisor_aborts_worker() {
        let finished = Arc::new(AtomicBool::new(false));
        let guard = Finished(finished.clone());
        let worker = tokio::spawn(async move {
            let _guard = guard;
            std::future::pending::<()>().await;
        });
        drop(supervise(
            worker,
            std::future::pending::<anyhow::Result<()>>(),
        ));
        tokio::task::yield_now().await;
        assert!(finished.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn canceling_supervisor_aborts_worker() {
        let finished = Arc::new(AtomicBool::new(false));
        let guard = Finished(finished.clone());
        let worker = tokio::spawn(async move {
            let _guard = guard;
            std::future::pending::<()>().await;
        });
        let (started, ready) = tokio::sync::oneshot::channel();
        let supervisor = tokio::spawn(supervise(worker, async {
            started.send(()).unwrap();
            std::future::pending::<anyhow::Result<()>>().await
        }));
        ready.await.unwrap();
        supervisor.abort();
        assert!(supervisor.await.unwrap_err().is_cancelled());
        for _ in 0..100 {
            if finished.load(Ordering::SeqCst) {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("submission worker remained alive");
    }
}
