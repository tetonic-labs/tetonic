//! Blocking LSP client over a language-server subprocess (stdio JSON-RPC).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use lsp_types::{
    Diagnostic, GotoDefinitionParams, Location, PartialResultParams, Position,
    PublishDiagnosticsParams, ReferenceContext, ReferenceParams, TextDocumentIdentifier,
    TextDocumentItem, TextDocumentPositionParams, Uri, WorkDoneProgressParams,
};
use serde_json::{json, Value};
use thiserror::Error;
use url::Url as ExternalUrl;

use crate::detect::{DetectError, Lang, ServerSpec};
use crate::framing::FrameError;
use crate::inbox::{spawn_reader, Inbox};
use crate::launcher::{spawn_process_with_launcher, LspProcessLauncher, SpawnedLspProcess};
use crate::state::LspLifecycle;
use crate::util::{content_hash, normalize_character};
use crate::writer::Writer;

const INIT_TIMEOUT: Duration = Duration::from_secs(30);
const REQ_TIMEOUT: Duration = Duration::from_secs(20);
const DIAG_DRAIN: Duration = Duration::from_millis(1200);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Error)]
pub enum LspError {
    #[error("frame: {0}")]
    Frame(#[from] FrameError),
    #[error("detect: {0}")]
    Detect(#[from] DetectError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("server: {0}")]
    Server(String),
    #[error("timeout during LSP write or response")]
    Timeout,
}

#[derive(Debug, Clone)]
pub struct LocationHit {
    pub path: String,
    pub line: u32,
    pub character: u32,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct DiagnosticHit {
    pub path: String,
    pub line: u32,
    pub character: u32,
    pub severity: String,
    pub message: String,
    pub source: Option<String>,
}

#[derive(Debug, Clone)]
struct OpenDoc {
    version: i32,
    content_hash: String,
}

pub struct LspSession {
    spec: ServerSpec,
    _process: ProcessSlot,
    stdin: Writer,
    root: PathBuf,
    root_uri: Uri,
    next_id: i64,
    lifecycle: LspLifecycle,
    open_docs: HashMap<String, OpenDoc>,
    inbox: Inbox,
    reader_alive: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    _reader: Option<JoinHandle<()>>,
    diags: HashMap<String, Vec<DiagnosticHit>>,
}

struct ProcessSlot {
    inner: SpawnedLspProcess,
    stopped: bool,
}

impl ProcessSlot {
    fn stop(&mut self) {
        if !self.stopped {
            self.inner.stop();
            self.stopped = !self.inner.is_alive();
        }
    }
}

impl Drop for ProcessSlot {
    fn drop(&mut self) {
        self.stop();
    }
}

impl LspSession {
    pub fn spawn(spec: ServerSpec, root: &Path) -> Result<Self, LspError> {
        Self::spawn_with_launcher(spec, root, None)
    }

    pub fn spawn_with_launcher(
        spec: ServerSpec,
        root: &Path,
        launcher: Option<&dyn LspProcessLauncher>,
    ) -> Result<Self, LspError> {
        let root = std::fs::canonicalize(root)?;
        let root_uri = path_to_uri(&root)?;
        let reader_alive = Arc::new(AtomicBool::new(true));
        let mut lifecycle = LspLifecycle::NotStarted;
        lifecycle = lifecycle
            .transition(LspLifecycle::Starting)
            .map_err(|e| LspError::Server(e.to_string()))?;

        let mut spawned = spawn_process_with_launcher(&spec, &root, launcher)?;
        let stdin = std::mem::replace(&mut spawned.stdin, Box::new(std::io::sink()));
        let stdout = std::mem::replace(&mut spawned.stdout, Box::new(std::io::empty()));
        let (rx, reader) = spawn_reader(stdout, reader_alive.clone());
        let mut session = Self {
            spec,
            _process: ProcessSlot {
                inner: spawned,
                stopped: false,
            },
            stdin: Writer::new(stdin),
            root,
            root_uri,
            next_id: 1,
            lifecycle,
            open_docs: HashMap::new(),
            inbox: rx,
            reader_alive,
            stop: Arc::new(AtomicBool::new(false)),
            _reader: Some(reader),
            diags: HashMap::new(),
        };
        match session.initialize() {
            Ok(()) => {
                session.lifecycle = session
                    .lifecycle
                    .transition(LspLifecycle::Ready)
                    .map_err(|e| LspError::Server(e.to_string()))?;
            }
            Err(e) => {
                let _ = session.lifecycle.transition(LspLifecycle::Unhealthy);
                return Err(e);
            }
        }
        Ok(session)
    }

    /// Later requests observe this flag. A set flag stops the owned process.
    pub(crate) fn share_stop_flag(&mut self, flag: Arc<AtomicBool>) {
        self.stop = flag;
    }

    pub fn lang(&self) -> Lang {
        self.spec.lang
    }

    pub fn label(&self) -> &str {
        &self.spec.label
    }

    /// Whether the reader thread is still running and lifecycle is usable (AC2-10).
    pub fn is_healthy(&self) -> bool {
        self.reader_alive.load(Ordering::SeqCst)
            && self.lifecycle.is_usable()
            && self._process.inner.is_alive()
    }

    pub fn lifecycle(&self) -> LspLifecycle {
        self.lifecycle
    }

    fn mark_unhealthy(&mut self) {
        self.lifecycle = LspLifecycle::Unhealthy;
        self.inbox.stop();
        self.stdin.stop();
        self._process.stop();
    }

    /// Stop process/transport activity and join workers. Launcher `stop` must
    /// unblock its IO endpoints promptly. Arbitrary injected Read/Write objects
    /// cannot be force-interrupted portably; a missed join deadline is an error.
    pub fn close(&mut self) -> Result<(), LspError> {
        if self.lifecycle == LspLifecycle::Stopped {
            return Ok(());
        }
        self.lifecycle = LspLifecycle::Stopping;
        self.inbox.stop();
        self.stdin.stop();
        let deadline = Instant::now() + CLOSE_TIMEOUT;
        self._process.stop();
        let writer_joined = self.stdin.close(deadline);
        while self
            ._reader
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
        {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            std::thread::sleep(remaining.min(Duration::from_millis(2)));
        }
        let reader_joined = if self
            ._reader
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
        {
            false
        } else {
            self._reader
                .take()
                .is_none_or(|worker| worker.join().is_ok())
        };
        if writer_joined && reader_joined && self._process.stopped {
            self.lifecycle = LspLifecycle::Stopped;
            Ok(())
        } else {
            self.lifecycle = LspLifecycle::Unhealthy;
            Err(LspError::Server(
                "LSP cleanup incomplete: process or IO workers did not stop cleanly".into(),
            ))
        }
    }

    fn initialize(&mut self) -> Result<(), LspError> {
        let id = self.next_id();
        let params = json!({
            "processId": std::process::id(),
            "rootUri": self.root_uri.as_str(),
            "capabilities": {
                "textDocument": {
                    "definition": { "dynamicRegistration": false },
                    "references": { "dynamicRegistration": false },
                    "publishDiagnostics": { "relatedInformation": false }
                }
            },
            "clientInfo": { "name": "lokai", "version": "0.0.0" }
        });
        let resp = self.request_id(id, "initialize", params, INIT_TIMEOUT)?;
        if resp.get("error").is_some() {
            return Err(LspError::Server(format!("initialize failed: {resp}")));
        }
        self.notify("initialized", json!({}))?;
        Ok(())
    }

    pub fn goto_definition(
        &mut self,
        rel_path: &str,
        line: u32,
        character: u32,
    ) -> Result<Vec<LocationHit>, LspError> {
        let file_text = self.ensure_open(rel_path)?;
        let character = normalize_character(&file_text, line, character);
        let uri = self.doc_uri(rel_path)?;
        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: line.saturating_sub(1),
                    character,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let resp = self.request("textDocument/definition", serde_json::to_value(params)?)?;
        Ok(parse_locations(&self.root, resp))
    }

    pub fn find_references(
        &mut self,
        rel_path: &str,
        line: u32,
        character: u32,
    ) -> Result<Vec<LocationHit>, LspError> {
        let file_text = self.ensure_open(rel_path)?;
        let character = normalize_character(&file_text, line, character);
        let uri = self.doc_uri(rel_path)?;
        let params = ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: line.saturating_sub(1),
                    character,
                },
            },
            context: ReferenceContext {
                include_declaration: true,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let resp = self.request("textDocument/references", serde_json::to_value(params)?)?;
        Ok(parse_locations(&self.root, resp))
    }

    pub fn diagnostics(&mut self, rel_path: &str) -> Result<Vec<DiagnosticHit>, LspError> {
        self.ensure_open(rel_path)?;
        self.drain_notifications(DIAG_DRAIN);
        if !self.is_healthy() {
            return Err(LspError::Server(
                "LSP transport failed; diagnostics are incomplete".into(),
            ));
        }
        Ok(self.diags.get(rel_path).cloned().unwrap_or_default())
    }

    /// Sync document with disk; returns full file text for position normalization.
    fn ensure_open(&mut self, rel_path: &str) -> Result<String, LspError> {
        if !self.lifecycle.is_usable() {
            return Err(LspError::Server("LSP not ready".into()));
        }
        let abs = self.root.join(rel_path);
        let text = std::fs::read_to_string(&abs).map_err(|e| LspError::Server(e.to_string()))?;
        let hash = content_hash(text.as_bytes());
        let uri = self.doc_uri(rel_path)?;

        if let Some(doc) = self.open_docs.get(rel_path) {
            if doc.content_hash == hash {
                return Ok(text);
            }
            let version = doc.version + 1;
            self.notify(
                "textDocument/didChange",
                json!({
                    "textDocument": { "uri": uri.as_str(), "version": version },
                    "contentChanges": [{ "text": text }]
                }),
            )?;
            self.open_docs.insert(
                rel_path.to_string(),
                OpenDoc {
                    version,
                    content_hash: hash,
                },
            );
            return Ok(text);
        }

        let language_id = language_id_for(self.spec.lang, rel_path);
        let item = TextDocumentItem {
            uri,
            language_id: language_id.into(),
            version: 1,
            text: text.clone(),
        };
        self.notify("textDocument/didOpen", json!({ "textDocument": item }))?;
        self.open_docs.insert(
            rel_path.to_string(),
            OpenDoc {
                version: 1,
                content_hash: hash,
            },
        );
        Ok(text)
    }

    fn doc_uri(&self, rel_path: &str) -> Result<Uri, LspError> {
        path_to_uri(&self.root.join(rel_path))
    }

    fn next_id(&mut self) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, LspError> {
        let id = self.next_id();
        self.request_id(id, method, params, REQ_TIMEOUT)
    }

    fn request_id(
        &mut self,
        id: i64,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, LspError> {
        let deadline = Instant::now() + timeout;
        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.send_message(&msg, deadline)?;
        loop {
            if self.stop.load(Ordering::SeqCst) {
                self.mark_unhealthy();
                return Err(LspError::Server("LSP stopped".into()));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                self.mark_unhealthy();
                return Err(LspError::Timeout);
            }
            match self
                .inbox
                .recv_timeout(remaining.min(Duration::from_millis(200)))
            {
                Ok(msg) => {
                    if msg.get("id").and_then(|v| v.as_i64()) == Some(id) {
                        if let Some(err) = msg.get("error") {
                            return Err(LspError::Server(err.to_string()));
                        }
                        return Ok(msg.get("result").cloned().unwrap_or(Value::Null));
                    }
                    self.ingest_notification(msg);
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    self.mark_unhealthy();
                    return Err(LspError::Server("LSP reader exited".into()));
                }
            }
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), LspError> {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.send_message(&msg, Instant::now() + REQ_TIMEOUT)
    }

    fn send_message(&mut self, msg: &Value, deadline: Instant) -> Result<(), LspError> {
        if !matches!(self.lifecycle, LspLifecycle::Starting | LspLifecycle::Ready) {
            return Err(LspError::Server("LSP transport is not usable".into()));
        }
        if let Err(error) = self.stdin.write(serde_json::to_vec(msg)?, deadline) {
            self.mark_unhealthy();
            return Err(error);
        }
        Ok(())
    }

    fn drain_notifications(&mut self, wait: Duration) {
        let deadline = Instant::now() + wait;
        while Instant::now() < deadline {
            match self.inbox.recv_timeout(Duration::from_millis(100)) {
                Ok(msg) => self.ingest_notification(msg),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    self.mark_unhealthy();
                    break;
                }
            }
        }
    }

    fn ingest_notification(&mut self, msg: Value) {
        if msg.get("method").and_then(|m| m.as_str()) != Some("textDocument/publishDiagnostics") {
            return;
        }
        if let Ok(params) = serde_json::from_value::<PublishDiagnosticsParams>(
            msg.get("params").cloned().unwrap_or(Value::Null),
        ) {
            if let Some(rel) = uri_to_rel(&self.root, &params.uri) {
                let hits: Vec<DiagnosticHit> = params
                    .diagnostics
                    .into_iter()
                    .map(|d| diagnostic_hit(&rel, d))
                    .collect();
                self.diags.insert(rel, hits);
            }
        }
    }
}

impl Drop for LspSession {
    fn drop(&mut self) {
        if let Err(error) = self.close() {
            tracing::error!(%error, "LSP session dropped with incomplete cleanup");
        }
    }
}

fn language_id_for(lang: Lang, rel_path: &str) -> &'static str {
    match lang {
        Lang::Rust => "rust",
        Lang::Python => "python",
        Lang::TypeScript => {
            let ext = Path::new(rel_path)
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase);
            match ext.as_deref() {
                Some("js") | Some("jsx") | Some("mjs") | Some("cjs") => "javascript",
                _ => "typescript",
            }
        }
    }
}

fn parse_locations(root: &Path, result: Value) -> Vec<LocationHit> {
    let mut out = Vec::new();
    let push_loc = |loc: Location, out: &mut Vec<LocationHit>| {
        if let Some(rel) = uri_to_rel(root, &loc.uri) {
            out.push(LocationHit {
                path: rel,
                line: loc.range.start.line + 1,
                character: loc.range.start.character,
                message: format!("{}:{}", loc.range.start.line + 1, loc.range.start.character),
            });
        }
    };
    if result.is_null() {
        return out;
    }
    if let Ok(loc) = serde_json::from_value::<Location>(result.clone()) {
        push_loc(loc, &mut out);
        return out;
    }
    if let Ok(locs) = serde_json::from_value::<Vec<Location>>(result) {
        for loc in locs {
            push_loc(loc, &mut out);
        }
    }
    out
}

fn diagnostic_hit(rel: &str, d: Diagnostic) -> DiagnosticHit {
    DiagnosticHit {
        path: rel.to_string(),
        line: d.range.start.line + 1,
        character: d.range.start.character,
        severity: d
            .severity
            .map(|s| format!("{s:?}"))
            .unwrap_or_else(|| "unknown".into()),
        message: d.message,
        source: d.source,
    }
}

pub fn path_to_uri(path: &Path) -> Result<Uri, LspError> {
    let u = ExternalUrl::from_file_path(path)
        .map_err(|_| LspError::Server(format!("invalid path for file URI: {}", path.display())))?;
    Uri::from_str(u.as_str()).map_err(|e| LspError::Server(format!("uri parse: {e}")))
}

fn uri_to_rel(root: &Path, uri: &Uri) -> Option<String> {
    let url = ExternalUrl::parse(uri.as_str()).ok()?;
    let path = url.to_file_path().ok()?;
    let path = std::fs::canonicalize(path).ok()?;
    let root = std::fs::canonicalize(root).ok()?;
    path.strip_prefix(&root)
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
}

#[allow(dead_code)] // kept for future in-crate spawn helpers; tests use their own allowlist
pub(crate) fn apply_minimal_env(cmd: &mut std::process::Command) {
    use std::collections::HashMap;
    const ALLOW: &[&str] = &[
        "PATH",
        "PATHEXT",
        "HOME",
        "USERPROFILE",
        "SYSTEMROOT",
        "SystemRoot",
        "WINDIR",
        "ComSpec",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "TMP",
        "TEMP",
    ];
    let keep: HashMap<String, String> = std::env::vars()
        .filter(|(k, _)| ALLOW.iter().any(|a| k.eq_ignore_ascii_case(a)))
        .collect();
    cmd.env_clear();
    for (k, v) in keep {
        cmd.env(k, v);
    }
}
