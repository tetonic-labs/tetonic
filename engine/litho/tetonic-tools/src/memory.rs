//! Memory configuration, read-only connections, and bound recall dispatch.
use super::*;

thread_local! {
    static MEMORY_CACHE: RefCell<HashMap<PathBuf, Rc<tetonic_memory::Store>>> = RefCell::new(HashMap::new());
}

impl Tools {
    /// Enable episodic `recall` against the audit store at `lokai.db`.
    pub fn with_memory(
        mut self,
        memory_db: impl Into<PathBuf>,
        session_id: Option<String>,
    ) -> Self {
        if self.session_id != session_id {
            self.reset_lsp_binding();
        }
        if self.recall_scope.is_none() {
            self.memory_db = Some(memory_db.into());
        }
        self.session_id = session_id;
        self
    }

    /// Trusted composition only: the application authenticates this actor and
    /// context before binding. Every recall rechecks current durable membership.
    /// This does not grant workspace access or authorize any other tool.
    pub fn with_context_memory(
        mut self,
        memory_db: PathBuf,
        actor: String,
        context: String,
    ) -> Self {
        self.memory_db = Some(memory_db);
        self.recall_scope = Some((actor, context));
        self
    }

    fn open_memory(&self) -> Result<Rc<tetonic_memory::Store>, ToolError> {
        let path = self
            .memory_db
            .as_ref()
            .ok_or_else(|| ToolError::Other("audit memory not enabled for this session".into()))?;
        MEMORY_CACHE.with(|c| -> Result<Rc<tetonic_memory::Store>, ToolError> {
            let mut map = c.borrow_mut();
            if let Some(s) = map.get(path) {
                return Ok(s.clone());
            }
            let opened = Rc::new(
                // Recall is read-only; migrations and writes belong to the
                // application's SharedStore writer, never a thread-local cache.
                tetonic_memory::Store::open_readonly(path)
                    .map_err(|e| ToolError::Other(format!("opening audit store: {e}")))?,
            );
            map.insert(path.clone(), opened.clone());
            Ok(opened)
        })
    }

    pub(super) fn recall(&self, args: Value) -> Result<ToolOutcome, ToolError> {
        let store = self.open_memory().map_err(|error| {
            if self.recall_scope.is_some() {
                ToolError::Other("context recall unavailable or access denied".into())
            } else {
                error
            }
        })?;
        if let Some((actor, context)) = &self.recall_scope {
            retrieval::recall_context(&store, actor, context, args)
        } else {
            retrieval::recall(&store, &self.ws, self.session_id.as_deref(), args)
        }
    }
}
