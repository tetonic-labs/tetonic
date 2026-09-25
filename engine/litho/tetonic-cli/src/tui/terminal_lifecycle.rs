//! Scoped terminal ownership, including partial setup and dropped TUI futures.
use crossterm::{
    cursor::Show,
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    Raw,
    Alternate,
    Mouse,
    Paste,
}

pub(super) trait Control {
    fn raw_enabled(&self) -> io::Result<bool>;
    fn set(&mut self, mode: Mode, enabled: bool) -> io::Result<()>;
    fn show_cursor(&mut self) -> io::Result<()>;
}

pub(super) struct Native;
impl Control for Native {
    fn raw_enabled(&self) -> io::Result<bool> {
        terminal::is_raw_mode_enabled()
    }
    fn set(&mut self, mode: Mode, enabled: bool) -> io::Result<()> {
        let mut stdout = io::stdout();
        match (mode, enabled) {
            (Mode::Raw, true) => terminal::enable_raw_mode(),
            (Mode::Raw, false) => terminal::disable_raw_mode(),
            (Mode::Alternate, true) => execute!(stdout, EnterAlternateScreen),
            (Mode::Alternate, false) => execute!(stdout, LeaveAlternateScreen),
            (Mode::Mouse, true) => execute!(stdout, EnableMouseCapture),
            (Mode::Mouse, false) => execute!(stdout, DisableMouseCapture),
            (Mode::Paste, true) => execute!(stdout, EnableBracketedPaste),
            (Mode::Paste, false) => execute!(stdout, DisableBracketedPaste),
        }
    }
    fn show_cursor(&mut self) -> io::Result<()> {
        execute!(io::stdout(), Show)
    }
}

pub(super) fn enter() -> io::Result<Guard<Native>> {
    Guard::start(Native)
}

pub(super) struct Guard<C: Control> {
    control: C,
    restore: Vec<Mode>,
    cursor_pending: bool,
}

impl<C: Control> Guard<C> {
    fn start(control: C) -> io::Result<Self> {
        let raw_was_enabled = control.raw_enabled()?;
        let mut guard = Self {
            control,
            restore: Vec::new(),
            cursor_pending: false,
        };
        for mode in [Mode::Raw, Mode::Alternate, Mode::Mouse, Mode::Paste] {
            if mode == Mode::Raw && raw_was_enabled {
                continue;
            }
            // Register before attempting: terminal writes can partially succeed.
            guard.restore.push(mode);
            guard.cursor_pending = true;
            guard.control.set(mode, true)?;
        }
        Ok(guard)
    }

    fn restore(&mut self) -> io::Result<()> {
        let mut first_error = None;
        let mut retry = Vec::new();
        for mode in self.restore.drain(..).rev() {
            if let Err(error) = self.control.set(mode, false) {
                first_error.get_or_insert(error);
                retry.push(mode);
            }
        }
        retry.reverse();
        self.restore = retry;
        if self.cursor_pending {
            match self.control.show_cursor() {
                Ok(()) => self.cursor_pending = false,
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    pub(super) fn finish(mut self, run: anyhow::Result<()>) -> anyhow::Result<()> {
        match (run, self.restore()) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) => Err(error),
            (Ok(()), Err(error)) => Err(error.into()),
            (Err(run), Err(cleanup)) => Err(anyhow::anyhow!(
                "{run:#}; terminal restoration also failed: {cleanup}"
            )),
        }
    }
}

impl<C: Control> Drop for Guard<C> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    type Events = Arc<Mutex<Vec<(Mode, bool)>>>;
    struct Fake {
        events: Events,
        fail_setup: Option<Mode>,
        fail_cleanup: bool,
        raw: bool,
        shows: Arc<std::sync::atomic::AtomicUsize>,
    }
    impl Control for Fake {
        fn raw_enabled(&self) -> io::Result<bool> {
            Ok(self.raw)
        }
        fn set(&mut self, mode: Mode, enabled: bool) -> io::Result<()> {
            self.events.lock().unwrap().push((mode, enabled));
            if (enabled && self.fail_setup == Some(mode)) || (!enabled && self.fail_cleanup) {
                Err(io::Error::other("injected terminal failure"))
            } else {
                Ok(())
            }
        }
        fn show_cursor(&mut self) -> io::Result<()> {
            self.shows.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
    }
    fn fake(events: &Events) -> Fake {
        Fake {
            events: events.clone(),
            fail_setup: None,
            fail_cleanup: false,
            raw: false,
            shows: Arc::default(),
        }
    }

    #[test]
    fn every_partial_setup_rolls_back_all_attempted_modes() {
        for mode in [Mode::Raw, Mode::Alternate, Mode::Mouse, Mode::Paste] {
            let events = Events::default();
            let mut control = fake(&events);
            control.fail_setup = Some(mode);
            assert!(Guard::start(control).is_err());
            let events = events.lock().unwrap();
            for (mode, _) in events.iter().filter(|(_, enabled)| *enabled) {
                assert!(events.contains(&(*mode, false)));
            }
        }
    }

    #[test]
    fn cleanup_attempts_all_modes_and_preserves_run_error() {
        let events = Events::default();
        let mut guard = Guard::start(fake(&events)).unwrap();
        let shows = guard.control.shows.clone();
        guard.control.fail_cleanup = true;
        let error = guard
            .finish(Err(anyhow::anyhow!("render failed")))
            .unwrap_err()
            .to_string();
        assert!(error.contains("render failed"));
        assert!(error.contains("restoration also failed"));
        assert_eq!(shows.load(std::sync::atomic::Ordering::SeqCst), 1);
        for mode in [Mode::Raw, Mode::Alternate, Mode::Mouse, Mode::Paste] {
            assert!(events.lock().unwrap().contains(&(mode, false)));
        }
    }

    #[test]
    fn success_restores_once_and_preserves_preexisting_raw_mode() {
        let events = Events::default();
        let mut control = fake(&events);
        control.raw = true;
        Guard::start(control).unwrap().finish(Ok(())).unwrap();
        let events = events.lock().unwrap();
        assert!(!events.iter().any(|(mode, _)| *mode == Mode::Raw));
        assert_eq!(events.iter().filter(|(_, enabled)| !enabled).count(), 3);
    }

    #[tokio::test]
    async fn aborting_a_suspended_ui_future_restores_terminal() {
        let events = Events::default();
        let control = fake(&events);
        let (ready, entered) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _guard = Guard::start(control).unwrap();
            ready.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        entered.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(
            events
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, enabled)| !enabled)
                .count(),
            4
        );
    }

    #[test]
    fn run_and_cleanup_failures_are_not_swallowed() {
        let events = Events::default();
        let error = Guard::start(fake(&events))
            .unwrap()
            .finish(Err(anyhow::anyhow!("draw failed")))
            .unwrap_err();
        assert_eq!(error.to_string(), "draw failed");
        let mut guard = Guard::start(fake(&events)).unwrap();
        guard.control.fail_cleanup = true;
        assert!(guard.finish(Ok(())).is_err());
    }
}
