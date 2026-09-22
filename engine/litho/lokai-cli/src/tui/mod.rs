mod approval;
pub(crate) mod clipboard;
mod composer;
mod events;
mod failure;
mod input;
mod interaction;
mod models;
mod overlays;
mod preferences;
mod retention;
mod scheduling;
pub(crate) mod slash;
mod status;
mod terminal_lifecycle;
mod transcript;
mod ui;

use std::io;
use std::sync::Arc;
use std::time::Duration;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tetonic_app::Application;
use tokio::sync::mpsc;

use approval::PendingApproval;
use input::InputEffect;
use status::TurnPhase;
use transcript::{LineKind, TranscriptLine};

pub enum TuiAction {
    Submit(String),
    Quit,
}

pub struct TuiLaunch {
    pub model: String,
    pub workspace_root: String,
    pub session_id: String,
    pub resumed: bool,
    pub resume_state: String,
    pub history: Vec<(String, String)>,
    pub coordinator: Arc<crate::app_kernel::TerminalApprovalCoordinator>,
    pub kernel: Arc<Application>,
    pub debug: bool,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum LayoutMode {
    Split,
    FullChat,
    FullInspector,
}

pub(crate) struct App {
    pub model_picker: Option<models::Picker>,
    pub transcript: Vec<TranscriptLine>,
    pub reading: Option<Vec<TranscriptLine>>,
    pub view: std::cell::RefCell<interaction::ViewState>,
    pub preferences: preferences::Preferences,
    pub draft_before_history: Option<(String, usize)>,
    pub approval_selection: usize,
    pub last_turn_elapsed: Option<Duration>,
    pub activity: Vec<String>,
    pub inspector_text: String,
    pub show_activity: bool,
    pub show_help: bool,
    pub model: String,
    pub workspace_root: String,
    pub session_id: String,
    pub resume_state: String,
    pub pending_approval: Option<PendingApproval>,
    queued_approvals: std::collections::VecDeque<PendingApproval>,
    portal_error: Option<String>,
    pub should_quit: bool,
    pub input_buffer: String,
    pub input_cursor: usize,
    pub input_tx: mpsc::Sender<TuiAction>,
    pub chat_scroll_offset: u16,
    pub inspector_scroll_offset: u16,
    pub approval_scroll_offset: u16,
    pub thinking: bool,
    pub tick: usize,
    pub layout_mode: LayoutMode,
    pub input_history: Vec<String>,
    pub history_index: Option<usize>,
    pub turn_start: Option<std::time::Instant>,
    pub turn_failed: bool,
    pub last_reported_error: Option<String>,
    pub phase: TurnPhase,
    pub degraded: bool,
    pub capacity_warned: bool,
    pub recovery_chip: bool,
    pub status_hint: Option<String>,
    pub hint_ticks: u32,
    pub tab_cycle: usize,
    pub tab_prefix: Option<String>,
    pub expand_thoughts: bool,
    pub turn_settled: bool,
    pub last_event_at: Option<std::time::Instant>,
    pub debug: bool,
}

pub(crate) struct AppParams<'a> {
    pub model: String,
    pub workspace_root: String,
    pub session_id: String,
    pub resumed: bool,
    pub resume_state: String,
    pub history: &'a [(String, String)],
    pub debug: bool,
}

impl App {
    fn new(params: AppParams<'_>, input_tx: mpsc::Sender<TuiAction>) -> Self {
        let mut transcript = Vec::new();
        if params.debug {
            transcript::push_line(
                &mut transcript,
                TranscriptLine::new(
                    LineKind::Sys,
                    "[system] Debug mode active. Detailed error traces enabled.",
                ),
            );
        }
        if params.resumed {
            if let Some(banner) = status::resume_banner(&params.resume_state) {
                transcript::push_line(&mut transcript, TranscriptLine::new(LineKind::Sys, banner));
            }
            for (role, content) in params.history {
                if let Some(line) = transcript::history_line(role, content) {
                    transcript::push_line(&mut transcript, line);
                }
            }
        }
        let recovery_chip = params.resumed && params.resume_state != "fresh";
        Self {
            model_picker: None,
            transcript,
            reading: None,
            view: std::cell::RefCell::new(interaction::ViewState::default()),
            preferences: preferences::Preferences::default(),
            draft_before_history: None,
            approval_selection: 0,
            last_turn_elapsed: None,
            activity: Vec::new(),
            inspector_text: "Your working context, in one place.\n\nFiles, reports, and results appear here as you work.\n\n/status   Session details\n/doctor   Diagnostics\n/help     All commands\n\nCtrl+L switches to live activity."
                .into(),
            show_activity: false,
            show_help: false,
            model: params.model,
            workspace_root: params.workspace_root,
            session_id: params.session_id,
            resume_state: params.resume_state,
            pending_approval: None,
            queued_approvals: std::collections::VecDeque::new(),
            portal_error: None,
            should_quit: false,
            input_buffer: String::new(),
            input_cursor: 0,
            input_tx,
            chat_scroll_offset: 0,
            inspector_scroll_offset: 0,
            approval_scroll_offset: 0,
            thinking: false,
            tick: 0,
            layout_mode: LayoutMode::FullChat,
            input_history: Vec::new(),
            history_index: None,
            turn_start: None,
            turn_failed: false,
            last_reported_error: None,
            phase: TurnPhase::Idle,
            degraded: false,
            capacity_warned: false,
            recovery_chip,
            status_hint: None,

            hint_ticks: 0,
            tab_cycle: 0,
            tab_prefix: None,
            expand_thoughts: false,
            turn_settled: false,
            last_event_at: None,
            debug: params.debug,
        }
    }

    #[cfg(test)]
    pub(crate) fn test_stub() -> Self {
        let (tx, _) = mpsc::channel(32);
        Self::new(
            AppParams {
                model: "test".into(),
                workspace_root: "/tmp".into(),
                session_id: "sess".into(),
                resumed: false,
                resume_state: "fresh".into(),
                history: &[],
                debug: false,
            },
            tx,
        )
    }

    pub(crate) fn push_transcript(&mut self, line: TranscriptLine) {
        transcript::push_line(&mut self.transcript, line);
    }

    pub(crate) fn push_activity(&mut self, mut line: String) {
        retention::bound(&mut line, 16 * 1024);
        self.activity.push(line);
        if self.activity.len() > 200 {
            self.activity.remove(0);
        }
    }

    pub(crate) fn set_artifact(&mut self, mut text: String) {
        retention::bound(&mut text, retention::INSPECTOR_BYTES);
        self.inspector_text = text;
        self.inspector_scroll_offset = 0;
        self.show_activity = false;
        self.view.get_mut().inspector_follow = false;
    }

    pub(crate) fn begin_turn(&mut self) {
        self.thinking = true;
        self.turn_settled = false;
        self.last_turn_elapsed = None;
        self.turn_failed = false;
        self.last_reported_error = None;
        let now = std::time::Instant::now();
        self.turn_start = Some(now);
        self.last_event_at = Some(now);
        self.phase = TurnPhase::Generating;
        self.recovery_chip = false;
        self.status_hint = None;
        self.activity.clear();
    }

    pub(crate) fn finish_turn_ok(&mut self) {
        if !self.turn_settled {
            self.last_turn_elapsed = self.turn_start.map(|start| start.elapsed());
            let label = self
                .last_turn_elapsed
                .map(|elapsed| format!("Completed in {:.1}s", elapsed.as_secs_f32()))
                .unwrap_or_else(|| "Completed".into());
            self.push_transcript(TranscriptLine::new(LineKind::Sys, label));
        }
        self.thinking = false;
        self.turn_settled = true;
        self.turn_failed = false;
        self.turn_start = None;
        self.last_event_at = None;
        self.phase = TurnPhase::Idle;
        self.recovery_chip = false;
        self.degraded = false;
    }

    pub(crate) fn report_turn_failure(&mut self, raw: &str) {
        let raw = raw.trim();
        if raw.is_empty() {
            return;
        }
        if self
            .last_reported_error
            .as_deref()
            .is_some_and(|prev| failure::is_same_failure(prev, raw))
        {
            self.thinking = false;
            self.turn_settled = true;
            self.turn_failed = true;
            self.turn_start = None;
            self.last_event_at = None;
            self.phase = TurnPhase::Failed(status::failure_object(raw));
            return;
        }
        self.last_reported_error = Some(raw.to_string());
        let copy = failure::explain_turn_failure(raw);
        self.push_transcript(TranscriptLine::new(LineKind::Error, copy.headline));
        self.push_transcript(TranscriptLine::new(LineKind::Error, copy.summary));
        if let Some(hint) = &copy.hint {
            self.push_transcript(TranscriptLine::new(LineKind::Error, hint.clone()));
        }
        if self.debug {
            self.push_transcript(TranscriptLine::new(
                LineKind::Error,
                format!("[DEBUG] Raw error: {}", raw),
            ));
        }
        self.set_artifact(failure::inspector_failure_text(raw));
        self.thinking = false;
        self.turn_settled = true;
        self.turn_failed = true;
        self.degraded = true;
        self.turn_start = None;
        self.last_event_at = None;
        self.phase = TurnPhase::Failed(status::failure_object(raw));
    }
}

pub async fn run_tui(
    mut rx: crate::event_queue::Receiver,
    mut submission_errors: mpsc::Receiver<String>,
    input_tx: mpsc::Sender<TuiAction>,
    launch: TuiLaunch,
) -> anyhow::Result<()> {
    let terminal_guard = terminal_lifecycle::enter()?;
    let stdout = io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.show_cursor()?;

    let coordinator = launch.coordinator.clone();
    let kernel = launch.kernel.clone();
    let mut app = App::new(
        AppParams {
            model: launch.model.clone(),
            workspace_root: launch.workspace_root.clone(),
            session_id: launch.session_id.clone(),
            resumed: launch.resumed,
            resume_state: launch.resume_state.clone(),
            history: &launch.history,
            debug: launch.debug,
        },
        input_tx,
    );

    app.preferences = preferences::load();
    app.layout_mode = if app.preferences.sidebar {
        LayoutMode::Split
    } else {
        LayoutMode::FullChat
    };
    let res = run_app(
        &mut terminal,
        &mut app,
        &mut rx,
        &mut submission_errors,
        &coordinator,
        &kernel,
    )
    .await;

    drop(terminal);
    terminal_guard.finish(res)
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    rx: &mut crate::event_queue::Receiver,
    submission_errors: &mut mpsc::Receiver<String>,
    coordinator: &crate::app_kernel::TerminalApprovalCoordinator,
    kernel: &Application,
) -> anyhow::Result<()> {
    let mut captured = true;
    let mut last_hint_tick = std::time::Instant::now();
    let started = std::time::Instant::now();
    let mut frames = scheduling::Frames::new();
    loop {
        rx.check_health()?;
        if let Some(error) = &app.portal_error {
            anyhow::bail!("{error}");
        }
        let desired_capture = app.preferences.mouse
            && (app.view.borrow().copy.is_none() || app.pending_approval.is_some());
        if desired_capture != captured {
            if desired_capture {
                execute!(terminal.backend_mut(), EnableMouseCapture)?;
            } else {
                execute!(terminal.backend_mut(), DisableMouseCapture)?;
            }
            captured = desired_capture;
        }
        if app.view.borrow().preferences_dirty {
            frames.changed();
            app.view.borrow_mut().preferences_dirty = false;
            if let Err(error) = preferences::save(&app.preferences) {
                app.status_hint = Some(format!(
                    "Preferences apply for this session; saving failed: {error}"
                ));
                app.hint_ticks = 400;
            }
        }
        let hint_elapsed = last_hint_tick.elapsed().as_millis() / 10;
        if hint_elapsed > 0 {
            if app.hint_ticks > 0 {
                app.hint_ticks = app
                    .hint_ticks
                    .saturating_sub(hint_elapsed.min(u32::MAX as u128) as u32);
                if app.hint_ticks == 0 {
                    app.status_hint = None;
                    frames.changed();
                }
            }
            last_hint_tick = std::time::Instant::now();
        }
        if frames.due(std::time::Instant::now(), app.thinking) {
            // Animation time follows elapsed time, not event throughput.
            app.tick = (started.elapsed().as_millis() / 10) as usize;
            if app.pending_approval.is_some() {
                terminal.hide_cursor()?;
            } else {
                terminal.show_cursor()?;
            }
            terminal.draw(|f| ui::draw(f, app))?;
        }

        if app.should_quit {
            let _ = app.input_tx.try_send(TuiAction::Quit);
            return Ok(());
        }

        if event::poll(Duration::ZERO)? {
            frames.changed();
            match event::read()? {
                Event::Key(key) if key.kind == event::KeyEventKind::Press => {
                    match input::handle_key(app, key) {
                        InputEffect::Quit => {
                            deny_pending(app, coordinator);
                            app.should_quit = true;
                        }
                        InputEffect::CancelTurn => {
                            deny_pending(app, coordinator);
                            cancel_turn(kernel, &app.session_id).await;
                        }
                        InputEffect::RespondApproval { approved, remember } => {
                            respond_approval(app, coordinator, approved, remember);
                        }
                        InputEffect::Submit(msg) => {
                            let trimmed = msg.trim();
                            if trimmed == "/model" {
                                open_models(app, kernel);
                            } else if trimmed == "/debug" {
                                app.debug = !app.debug;
                                let state_str = if app.debug { "enabled" } else { "disabled" };
                                app.status_hint = Some(format!("Debug mode {}", state_str));
                                app.hint_ticks = 300;
                                app.push_transcript(TranscriptLine::new(
                                    LineKind::Sys,
                                    format!("[system] Debug mode {}", state_str),
                                ));
                            } else {
                                on_submit(app, msg);
                            }
                        }

                        InputEffect::OpenModels => open_models(app, kernel),
                        InputEffect::SelectModel { id, revision } => {
                            match kernel.select_session_model(&app.session_id, &id, revision) {
                                Ok(selection) => {
                                    app.model = selection.model_fast;
                                    app.model_picker = None;
                                    app.push_transcript(TranscriptLine::new(
                                        LineKind::Sys,
                                        format!(
                                            "Model changed to {} for the next turn.",
                                            app.model
                                        ),
                                    ));
                                }
                                Err(error) => {
                                    if let Some(picker) = &mut app.model_picker {
                                        picker.notice =
                                            Some(format!("{error}. Close and reopen to refresh."));
                                    }
                                }
                            }
                        }
                        InputEffect::AuthenticateAndSelectModel {
                            provider_id,
                            key,
                            model,
                        } => {
                            match kernel.configure_and_select_cloud_model(
                                &app.session_id,
                                &provider_id,
                                &key,
                                &model,
                            ) {
                                Ok(selection) => {
                                    app.model = selection.model_fast;
                                    app.model_picker = None;
                                    app.push_transcript(TranscriptLine::new(
                                        LineKind::Sys,
                                        format!(
                                            "Connected provider successfully. Model changed to {} for the next turn.",
                                            app.model
                                        ),
                                    ));
                                }
                                Err(error) => {
                                    if let Some(picker) = &mut app.model_picker {
                                        if let Some(auth) = &mut picker.auth_prompt {
                                            auth.notice =
                                                Some(format!("Authentication failed: {error}"));
                                        } else {
                                            picker.notice = Some(format!("{error}"));
                                        }
                                    }
                                }
                            }
                        }
                        InputEffect::Copy(text) => {
                            let result = execute!(
                                terminal.backend_mut(),
                                crossterm::clipboard::CopyToClipboard::to_clipboard_from(text)
                            );
                            app.view.borrow_mut().copy_notice = Some(if result.is_ok() {
                                "Clipboard request sent. If unsupported, select text and use your terminal's Copy command.".into()
                            } else {
                                "Clipboard unavailable. Select text and use your terminal's Copy command.".into()
                            });
                        }
                        InputEffect::None => {}
                    }
                }
                Event::Mouse(mouse) => input::handle_mouse(app, mouse),
                Event::Paste(text) => input::handle_paste(app, &text),
                _ => {}
            }
        }

        let mut events_processed = scheduling::drain_batch(
            || rx.try_recv(),
            |event| {
                events::apply(app, event, coordinator);
            },
        );
        // Independent budgets ensure output cannot starve submission failures.
        events_processed += scheduling::drain_batch(
            || submission_errors.try_recv().ok(),
            |error| {
                app.report_turn_failure(&error);
            },
        );
        rx.check_health()?;
        if events_processed > 0 {
            frames.changed();
            if let Ok(selection) = kernel.session_inference(&app.session_id) {
                app.model = if selection.model_fast == selection.model_hard {
                    selection.model_fast
                } else {
                    format!("{} / {}", selection.model_fast, selection.model_hard)
                };
            }
        }

        if app.thinking {
            let last_activity = app.last_event_at.or(app.turn_start);
            if let Some(last) = last_activity {
                if last.elapsed() > Duration::from_secs(600) {
                    app.report_turn_failure("turn timed out: no events received for 10 minutes");
                    frames.changed();
                }
            }
        }

        if events_processed == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        } else {
            // try_recv does not provide Tokio's cooperative scheduling budget.
            tokio::task::yield_now().await;
        }
    }
}

fn on_submit(app: &mut App, msg: String) {
    if slash::is_status_line(&msg) {
        app.set_artifact(events::status_inspect(app));
        return;
    }
    if app.thinking && !msg.starts_with('/') {
        app.status_hint = Some("A turn is already running".into());
        app.hint_ticks = 400;
        return;
    }
    // Retain the draft and avoid displaying a submitted turn unless admission
    // succeeds. The bounded queue must never silently discard user commands.
    let rejection = if msg.len() > retention::INPUT_BYTES {
        Some("Message exceeds the 1 MiB input limit; shorten the draft before sending")
    } else {
        match app.input_tx.try_send(TuiAction::Submit(msg.clone())) {
            Ok(()) => None,
            Err(mpsc::error::TrySendError::Full(_)) => {
                Some("Command queue is busy; draft preserved. Try again shortly")
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                Some("Command handler is unavailable; draft preserved")
            }
        }
    };
    if let Some(reason) = rejection {
        app.input_cursor = msg.chars().count();
        app.input_buffer = msg;
        app.status_hint = Some(reason.into());
        app.hint_ticks = 400;
        app.view.get_mut().focus = interaction::Focus::Composer;
        return;
    }
    if !msg.starts_with('/') {
        app.begin_turn();
        app.push_transcript(TranscriptLine::new(LineKind::You, msg.clone()));
    }
}

fn open_models(app: &mut App, kernel: &Application) {
    match kernel.model_catalog(&app.session_id) {
        Ok(catalog) => app.model_picker = Some(models::Picker::new(catalog)),
        Err(error) => {
            app.status_hint = Some(error.to_string());
            app.hint_ticks = 400;
        }
    }
}

fn respond_approval(
    app: &mut App,
    coordinator: &crate::app_kernel::TerminalApprovalCoordinator,
    approved: bool,
    remember: bool,
) {
    let Some(pending) = app.pending_approval.as_ref() else {
        return;
    };
    let result = coordinator
        .get_approvals()
        .ok_or_else(|| "Approval service unavailable".to_string())
        .and_then(|svc| {
            svc.respond(tetonic_app::commands::ApprovalResponseCommand {
                session_id: pending.session_id.clone(),
                approval_id: pending.approval_id.clone(),
                approved,
                remember: remember && approved,
                kind: pending.kind.clone(),
                detail: pending.detail.clone(),
                channel_delivered: true,
                attempt_id: None,
            })
            .map_err(|error| error.to_string())
        });
    if let Err(error) = result {
        app.status_hint = Some(format!(
            "Approval response failed: {error}. Retry or cancel the turn."
        ));
        app.hint_ticks = 400;
        return;
    }
    app.pending_approval = app.queued_approvals.pop_front();
    app.approval_scroll_offset = 0;
    app.approval_selection = 0;
    app.phase = if app.pending_approval.is_some() {
        TurnPhase::WaitingApproval
    } else {
        TurnPhase::Generating
    };
}

fn deny_pending(app: &mut App, coordinator: &crate::app_kernel::TerminalApprovalCoordinator) {
    if let Some(svc) = coordinator.get_approvals() {
        svc.fail_session_waits(&app.session_id);
    }
    app.pending_approval = None;
    app.queued_approvals.clear();
}

async fn cancel_turn(kernel: &Application, session_id: &str) {
    let _ = kernel.cancel_chat_turn(session_id).await;
}

#[cfg(test)]
mod ux_tests;

#[cfg(test)]
mod submission_tests;
