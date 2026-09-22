//! Per-workspace LSP session pool (one subprocess per language).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use thiserror::Error;

use crate::client::{DiagnosticHit, LocationHit, LspError, LspSession};
use crate::detect::{detect_server, lsp_enabled_by_env, Lang};
use crate::launcher::LspProcessLauncher;

#[derive(Debug, Error)]
pub enum LspPoolError {
    #[error("lsp disabled (set LOKAI_LSP=1 or install a language server)")]
    Disabled,
    #[error("no language server for this file type")]
    Unsupported,
    #[error("detect: {0}")]
    Detect(#[from] crate::detect::DetectError),
    #[error("{0}")]
    Lsp(#[from] LspError),
}

/// Cached LSP sessions keyed by `(workspace root, language)`.
///
/// `LspPool` is thread-safe (`Send + Sync`) and shares sessions via `RwLock` and `Arc<Mutex<LspSession>>`.
pub struct LspPool {
    root: PathBuf,
    launcher: Option<Arc<dyn LspProcessLauncher>>,
    sessions: RwLock<HashMap<Lang, Arc<Mutex<LspSession>>>>,
}

impl LspPool {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, LspPoolError> {
        Self::new_with_launcher(root, None)
    }

    pub fn new_with_launcher(
        root: impl AsRef<Path>,
        launcher: Option<Arc<dyn LspProcessLauncher>>,
    ) -> Result<Self, LspPoolError> {
        if !lsp_enabled_by_env() {
            return Err(LspPoolError::Disabled);
        }
        let root = std::fs::canonicalize(root.as_ref()).map_err(LspError::Io)?;
        Ok(Self {
            root,
            launcher,
            sessions: RwLock::new(HashMap::new()),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn evict(&self, lang: Lang) {
        if let Ok(mut map) = self.sessions.write() {
            map.remove(&lang);
        }
    }

    fn spawn_session(&self, lang: Lang) -> Result<Arc<Mutex<LspSession>>, LspPoolError> {
        let spec = detect_server(lang)?;
        let session = LspSession::spawn_with_launcher(spec, &self.root, self.launcher.as_deref())?;
        let arc = Arc::new(Mutex::new(session));
        if let Ok(mut map) = self.sessions.write() {
            map.insert(lang, arc.clone());
        }
        Ok(arc)
    }

    fn session(&self, lang: Lang) -> Result<Arc<Mutex<LspSession>>, LspPoolError> {
        if let Ok(map) = self.sessions.read() {
            if let Some(s) = map.get(&lang) {
                if let Ok(guard) = s.lock() {
                    if guard.is_healthy() {
                        return Ok(s.clone());
                    }
                }
            }
        }
        self.evict(lang);
        self.spawn_session(lang)
    }

    fn with_session<T, F>(&self, lang: Lang, mut f: F) -> Result<T, LspPoolError>
    where
        F: FnMut(&mut LspSession) -> Result<T, LspError>,
    {
        let s = self.session(lang)?;
        let first = {
            let mut guard = s.lock().map_err(|_| {
                LspPoolError::Lsp(LspError::Server("LSP session mutex poisoned".into()))
            })?;
            f(&mut guard)
        };
        match first {
            Ok(v) => Ok(v),
            Err(LspError::Server(msg)) if msg.contains("reader exited") => {
                drop(s);
                self.evict(lang);
                let s2 = self.spawn_session(lang)?;
                let result = {
                    let mut guard = s2.lock().map_err(|_| {
                        LspPoolError::Lsp(LspError::Server("LSP session mutex poisoned".into()))
                    })?;
                    f(&mut guard)
                };
                result.map_err(LspPoolError::from)
            }
            Err(e) => Err(LspPoolError::from(e)),
        }
    }

    pub fn goto_definition(
        &self,
        rel_path: &str,
        line: u32,
        character: u32,
    ) -> Result<Vec<LocationHit>, LspPoolError> {
        let lang = Lang::from_path(Path::new(rel_path)).ok_or(LspPoolError::Unsupported)?;
        self.with_session(lang, |session| {
            session.goto_definition(rel_path, line, character)
        })
    }

    pub fn find_references(
        &self,
        rel_path: &str,
        line: u32,
        character: u32,
    ) -> Result<Vec<LocationHit>, LspPoolError> {
        let lang = Lang::from_path(Path::new(rel_path)).ok_or(LspPoolError::Unsupported)?;
        self.with_session(lang, |session| {
            session.find_references(rel_path, line, character)
        })
    }

    pub fn diagnostics(&self, rel_path: &str) -> Result<Vec<DiagnosticHit>, LspPoolError> {
        let lang = Lang::from_path(Path::new(rel_path)).ok_or(LspPoolError::Unsupported)?;
        self.with_session(lang, |session| session.diagnostics(rel_path))
    }

    #[doc(hidden)]
    pub fn inject_session_for_test(&self, lang: Lang, session: LspSession) {
        if let Ok(mut map) = self.sessions.write() {
            map.insert(lang, Arc::new(Mutex::new(session)));
        }
    }
}
