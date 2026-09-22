//! One owned writer per session. Callers wait only until their absolute deadline;
//! a timed-out stream cannot accept another frame while an old write is pending.
use crate::{
    framing::{write_frame, FrameError, MAX_FRAME_BYTES},
    LspError,
};
use std::io::Write;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Instant;

struct Command {
    bytes: Vec<u8>,
    done: mpsc::SyncSender<Result<(), FrameError>>,
}

pub(crate) struct Writer {
    tx: Option<mpsc::SyncSender<Command>>,
    failed: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Writer {
    pub fn stop(&mut self) {
        self.failed.store(true, Ordering::SeqCst);
        self.tx.take();
    }

    pub fn close(&mut self, deadline: Instant) -> bool {
        self.stop();
        while self
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
        {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            thread::sleep(remaining.min(std::time::Duration::from_millis(2)));
        }
        self.worker
            .take()
            .is_none_or(|worker| worker.join().is_ok())
    }

    pub fn new(mut stream: Box<dyn Write + Send>) -> Self {
        let (tx, rx) = mpsc::sync_channel::<Command>(1);
        let failed = Arc::new(AtomicBool::new(false));
        let failure = failed.clone();
        let worker = thread::spawn(move || {
            while let Ok(command) = rx.recv() {
                if failure.load(Ordering::SeqCst) {
                    break;
                }
                let result = write_frame(&mut stream, &command.bytes);
                let broken = result.is_err();
                let _ = command.done.try_send(result);
                if broken {
                    failure.store(true, Ordering::SeqCst);
                    break;
                }
            }
        });
        Self {
            tx: Some(tx),
            failed,
            worker: Some(worker),
        }
    }

    pub fn write(&self, bytes: Vec<u8>, deadline: Instant) -> Result<(), LspError> {
        if self.failed.load(Ordering::SeqCst) {
            return Err(LspError::Server("LSP writer is unusable".into()));
        }
        if bytes.len() > MAX_FRAME_BYTES {
            return Err(FrameError::Oversized(bytes.len()).into());
        }
        if Instant::now() >= deadline {
            self.failed.store(true, Ordering::SeqCst);
            return Err(LspError::Timeout);
        }
        let (done, completion) = mpsc::sync_channel(1);
        if self
            .tx
            .as_ref()
            .unwrap()
            .try_send(Command { bytes, done })
            .is_err()
        {
            self.failed.store(true, Ordering::SeqCst);
            return Err(LspError::Server("LSP writer unavailable or busy".into()));
        }
        match completion.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Ok(())) if Instant::now() <= deadline => Ok(()),
            Ok(Err(error)) => {
                self.failed.store(true, Ordering::SeqCst);
                Err(error.into())
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.failed.store(true, Ordering::SeqCst);
                Err(LspError::Server("LSP writer exited".into()))
            }
            _ => {
                self.failed.store(true, Ordering::SeqCst);
                Err(LspError::Timeout)
            }
        }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.failed.store(true, Ordering::SeqCst);
        self.tx.take();
        // Session's process owner stops the service before this field drops.
        // Arbitrary injected Write implementations cannot be force-interrupted
        // portably; never replace a bounded caller wait with an unbounded join.
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.is_finished())
        {
            let _ = self.worker.take().unwrap().join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Condvar, Mutex};
    use std::time::Duration;

    #[derive(Default)]
    struct State {
        released: bool,
        entered: bool,
        finished: bool,
    }
    struct Stalled(Arc<(Mutex<State>, Condvar)>);
    impl Write for Stalled {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let (lock, ready) = &*self.0;
            let mut state = lock.lock().unwrap();
            state.entered = true;
            ready.notify_all();
            while !state.released {
                state = ready.wait(state).unwrap();
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl Drop for Stalled {
        fn drop(&mut self) {
            let (lock, ready) = &*self.0;
            lock.lock().unwrap().finished = true;
            ready.notify_all();
        }
    }

    #[test]
    fn stalled_write_times_out_and_stream_cannot_be_reused() {
        let state = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let mut writer = Writer::new(Box::new(Stalled(state.clone())));
        let start = Instant::now();
        assert!(matches!(
            writer.write(vec![b'x'; 1024], start + Duration::from_millis(100)),
            Err(LspError::Timeout)
        ));
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(writer
            .write(b"second".to_vec(), Instant::now() + Duration::from_secs(1))
            .is_err());
        let (lock, ready) = &*state;
        assert!(lock.lock().unwrap().entered);
        let closing = Instant::now();
        assert!(!writer.close(closing + Duration::from_millis(25)));
        assert!(closing.elapsed() < Duration::from_secs(1));
        lock.lock().unwrap().released = true;
        ready.notify_all();
        assert!(writer.close(Instant::now() + Duration::from_secs(2)));
        assert!(writer.worker.is_none());
        assert!(writer.close(Instant::now()));
        let (guard, _) = ready
            .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(2), |s| {
                !s.finished
            })
            .unwrap();
        assert!(
            guard.finished,
            "worker must release stream after it is unblocked"
        );
    }

    #[test]
    fn successful_writes_preserve_frame_order() {
        struct Recording(Arc<Mutex<Vec<u8>>>);
        impl Write for Recording {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let writer = Writer::new(Box::new(Recording(recorded.clone())));
        for value in [b"one", b"two"] {
            writer
                .write(value.to_vec(), Instant::now() + Duration::from_secs(2))
                .unwrap();
        }
        let expected = [crate::framing::frame(b"one"), crate::framing::frame(b"two")].concat();
        assert_eq!(*recorded.lock().unwrap(), expected);
    }
}
