use super::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
struct Probe {
    opens: AtomicUsize,
    drops: AtomicUsize,
}
struct Opener(Arc<Probe>);
struct Session(Arc<Probe>);
impl Drop for Session {
    fn drop(&mut self) {
        self.0.drops.fetch_add(1, Ordering::SeqCst);
    }
}
impl LspSessionOpen for Opener {
    fn open(&self, _: &Path) -> Result<Box<dyn LspSession>, String> {
        self.0.opens.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(Session(self.0.clone())))
    }
    fn available(&self, _: &Path) -> bool {
        true
    }
}
impl LspSession for Session {
    fn goto_definition(&self, _: &str, _: u32, _: u32) -> Result<ToolOutcome, String> {
        Ok(ToolOutcome::ok("definition", "ok"))
    }
    fn find_references(&self, _: &str, _: u32, _: u32) -> Result<ToolOutcome, String> {
        Ok(ToolOutcome::ok("references", "ok"))
    }
    fn diagnostics(&self, _: &str) -> Result<ToolOutcome, String> {
        Ok(ToolOutcome::ok("diagnostics", "ok"))
    }
}
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "lokai-lsp-owner-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn tools(&self, probe: &Arc<Probe>) -> Tools {
        Tools::new(crate::Workspace::new(&self.0).unwrap(), false)
            .with_lsp_open(Arc::new(Opener(probe.clone())))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn same_workspace_bindings_do_not_share_sessions_or_retain_them_globally() {
    let dir = Directory::new();
    let first = Arc::new(Probe::default());
    let second = Arc::new(Probe::default());
    let a = dir.tools(&first);
    let b = dir.tools(&second);
    let one = open_lsp(&a).unwrap();
    let two = open_lsp(&b).unwrap();
    assert!(!Arc::ptr_eq(&one, &two));
    assert_eq!(first.opens.load(Ordering::SeqCst), 1);
    assert_eq!(second.opens.load(Ordering::SeqCst), 1);
    drop((one, two, a));
    assert_eq!(first.drops.load(Ordering::SeqCst), 1);
    assert_eq!(second.drops.load(Ordering::SeqCst), 0);
    drop(b);
    assert_eq!(second.drops.load(Ordering::SeqCst), 1);
}

#[test]
fn simultaneous_clones_open_once_and_retain_ownership_until_last_user_drops() {
    let dir = Directory::new();
    let probe = Arc::new(Probe::default());
    let tools = dir.tools(&probe);
    let clone = tools.clone();
    std::thread::scope(|scope| {
        let a = scope.spawn(|| open_lsp(&tools).unwrap());
        let b = scope.spawn(|| open_lsp(&clone).unwrap());
        assert!(Arc::ptr_eq(&a.join().unwrap(), &b.join().unwrap()));
    });
    assert_eq!(probe.opens.load(Ordering::SeqCst), 1);
    let active = open_lsp(&tools).unwrap();
    drop((tools, clone));
    assert_eq!(probe.drops.load(Ordering::SeqCst), 0);
    drop(active);
    assert_eq!(probe.drops.load(Ordering::SeqCst), 1);
}

#[test]
fn replacing_or_disabling_a_binding_does_not_reuse_its_previous_opener() {
    let dir = Directory::new();
    let old = Arc::new(Probe::default());
    let new = Arc::new(Probe::default());
    let original = dir.tools(&old);
    drop(open_lsp(&original).unwrap());
    let changed = original
        .clone()
        .with_lsp_open(Arc::new(Opener(new.clone())));
    drop(open_lsp(&changed).unwrap());
    assert_eq!(new.opens.load(Ordering::SeqCst), 1);
    assert_eq!(old.opens.load(Ordering::SeqCst), 1);
    let disabled = changed.with_lsp(false);
    assert!(open_lsp(&disabled).is_err());
    assert_eq!(new.drops.load(Ordering::SeqCst), 1);
    assert_eq!(old.drops.load(Ordering::SeqCst), 0);
    drop(original);
    assert_eq!(old.drops.load(Ordering::SeqCst), 1);
}

struct Consumer;
impl lokai_domain::CapabilityConsumer for Consumer {
    fn authorize(
        &self,
        _: &lokai_domain::AuthorizedAction,
    ) -> Result<(), lokai_domain::CapabilityError> {
        Ok(())
    }
}

#[test]
fn authority_and_session_changes_require_rebinding_the_opener() {
    let dir = Directory::new();
    let probe = Arc::new(Probe::default());
    let original = dir.tools(&probe);
    drop(open_lsp(&original).unwrap());
    let changes = [
        original
            .clone()
            .with_capability_consumer(Arc::new(Consumer)),
        original
            .clone()
            .with_enforcement_level(crate::EnforcementLevel::Sandboxed),
        original
            .clone()
            .with_memory(dir.0.join("memory.db"), Some("other-session".into())),
    ];
    for changed in changes {
        assert!(!changed.has_lsp());
        assert!(open_lsp(&changed.with_lsp(true)).is_err());
    }
    assert_eq!(probe.opens.load(Ordering::SeqCst), 1);
    assert!(open_lsp(&original).is_ok());
}
