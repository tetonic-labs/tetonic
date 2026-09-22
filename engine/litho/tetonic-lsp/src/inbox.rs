//! Bounded raw-frame handoff. Overflow invalidates the transport, never silently
//! drops a response or reports a partial diagnostics stream as successful.
use crate::framing::read_frame;
use serde_json::Value;
use std::io::{BufReader, Read};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc, Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const MAX_MESSAGES: usize = 64;
const MAX_QUEUED_BYTES: usize = 64 * 1024 * 1024;

struct Frame {
    bytes: Vec<u8>,
    charged: Arc<AtomicUsize>,
}

impl Drop for Frame {
    fn drop(&mut self) {
        self.charged.fetch_sub(self.bytes.len(), Ordering::SeqCst);
    }
}

pub(crate) struct Inbox {
    rx: mpsc::Receiver<Frame>,
    failed: Arc<AtomicBool>,
}

impl Inbox {
    pub fn stop(&self) {
        self.failed.store(true, Ordering::SeqCst);
        // Release retained frames while the process owner unblocks any pipe read.
        for _ in self.rx.try_iter() {}
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Result<Value, mpsc::RecvTimeoutError> {
        if self.failed.load(Ordering::SeqCst) {
            return Err(mpsc::RecvTimeoutError::Disconnected);
        }
        let frame = self.rx.recv_timeout(timeout)?;
        if self.failed.load(Ordering::SeqCst) {
            return Err(mpsc::RecvTimeoutError::Disconnected);
        }
        serde_json::from_slice(&frame.bytes).map_err(|_| {
            self.failed.store(true, Ordering::SeqCst);
            mpsc::RecvTimeoutError::Disconnected
        })
    }
}

pub(crate) fn spawn_reader(
    stdout: Box<dyn Read + Send>,
    alive: Arc<AtomicBool>,
) -> (Inbox, JoinHandle<()>) {
    let (tx, rx) = mpsc::sync_channel(MAX_MESSAGES);
    let failed = Arc::new(AtomicBool::new(false));
    let failure = failed.clone();
    let reader = thread::spawn(move || reader_loop(stdout, tx, alive, failure, MAX_QUEUED_BYTES));
    (Inbox { rx, failed }, reader)
}

fn reader_loop(
    stdout: Box<dyn Read + Send>,
    tx: mpsc::SyncSender<Frame>,
    alive: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    budget: usize,
) {
    let mut reader = BufReader::new(stdout);
    let charged = Arc::new(AtomicUsize::new(0));
    while !failed.load(Ordering::SeqCst) {
        let bytes = match read_frame(&mut reader) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => break,
            Err(error) => {
                tracing::warn!(%error, "LSP framing failed");
                failed.store(true, Ordering::SeqCst);
                break;
            }
        };
        if charged
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |used| {
                used.checked_add(bytes.len())
                    .filter(|total| *total <= budget)
            })
            .is_err()
        {
            tracing::warn!("LSP inbox byte budget exceeded");
            failed.store(true, Ordering::SeqCst);
            break;
        }
        let frame = Frame {
            bytes,
            charged: charged.clone(),
        };
        if let Err(error) = tx.try_send(frame) {
            if matches!(error, mpsc::TrySendError::Full(_)) {
                tracing::warn!("LSP inbox message limit exceeded");
                failed.store(true, Ordering::SeqCst);
            }
            break;
        }
    }
    alive.store(false, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framing::frame;
    use std::io::Cursor;

    #[test]
    fn flood_exits_without_waiting_for_a_consumer() {
        let data = frame(br#"{"method":"notification"}"#).repeat(MAX_MESSAGES + 1);
        let alive = Arc::new(AtomicBool::new(true));
        let (inbox, worker) = spawn_reader(Box::new(Cursor::new(data)), alive.clone());
        worker.join().unwrap();
        assert!(!alive.load(Ordering::SeqCst));
        assert!(inbox.failed.load(Ordering::SeqCst));
        assert!(matches!(
            inbox.recv_timeout(Duration::ZERO),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
        assert_eq!(inbox.rx.try_iter().count(), MAX_MESSAGES);
    }

    #[test]
    fn byte_budget_is_enforced_and_charges_release_on_drop() {
        let bytes = br#"{"id":1}"#;
        let (tx, rx) = mpsc::sync_channel(MAX_MESSAGES);
        let failed = Arc::new(AtomicBool::new(false));
        reader_loop(
            Box::new(Cursor::new(frame(bytes).repeat(3))),
            tx,
            Arc::new(AtomicBool::new(true)),
            failed.clone(),
            bytes.len() * 2,
        );
        assert!(failed.load(Ordering::SeqCst));
        let first = rx.recv().unwrap();
        let charged = first.charged.clone();
        assert_eq!(charged.load(Ordering::SeqCst), bytes.len() * 2);
        drop(first);
        drop(rx);
        assert_eq!(charged.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn ordered_responses_decode_and_malformed_json_fails_closed() {
        let mut data = frame(br#"{"id":1}"#);
        data.extend(frame(b"invalid json"));
        let (inbox, worker) =
            spawn_reader(Box::new(Cursor::new(data)), Arc::new(AtomicBool::new(true)));
        worker.join().unwrap();
        assert_eq!(inbox.recv_timeout(Duration::ZERO).unwrap()["id"], 1);
        assert!(inbox.recv_timeout(Duration::ZERO).is_err());
        assert!(inbox.failed.load(Ordering::SeqCst));
    }
}
