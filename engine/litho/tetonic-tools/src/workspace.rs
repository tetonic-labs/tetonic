//! Workspace path resolution and safe file mutation helpers.

use std::path::{Path, PathBuf};

use crate::types::ToolError;

/// A canonicalized workspace root. All tool paths resolve under here.
#[derive(Clone)]
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    pub fn new(root: impl AsRef<Path>) -> std::io::Result<Self> {
        Ok(Self {
            root: std::fs::canonicalize(root)?,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn resolve(&self, rel: &str) -> Result<PathBuf, ToolError> {
        tetonic_transaction::fs_ops::resolve_under_root(&self.root, rel).map_err(map_txn)
    }

    pub(crate) fn display_rel(&self, p: &Path) -> String {
        p.strip_prefix(&self.root)
            .unwrap_or(p)
            .to_string_lossy()
            .replace('\\', "/")
    }
}

fn map_txn(err: tetonic_transaction::TransactionError) -> ToolError {
    match err {
        tetonic_transaction::TransactionError::OutsideWorkspace(p) => {
            ToolError::OutsideWorkspace(p)
        }
        tetonic_transaction::TransactionError::Io(s) => ToolError::Io(s),
        other => ToolError::Other(other.to_string()),
    }
}

/// Finds closely matching files in the workspace for typo recovery suggestions (UX-502).
pub fn find_similar_paths(root: &Path, requested: &str) -> Option<String> {
    let req_norm = requested.replace('\\', "/");
    let req_file_name = Path::new(&req_norm)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&req_norm);

    let mut best_match: Option<(usize, String)> = None;
    let mut visited = 0;

    let root_str = root.to_string_lossy();
    let root_clean = root_str
        .strip_prefix(r"\\?\")
        .unwrap_or(&root_str)
        .trim_end_matches(['\\', '/']);
    let walk_root = Path::new(root_clean);

    for entry in ignore::WalkBuilder::new(walk_root)
        .max_depth(Some(6))
        .parents(false)
        .git_ignore(true)
        .hidden(true)
        .follow_links(false)
        .build()
        .flatten()
    {
        visited += 1;
        if visited > 500 {
            break;
        }
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        if tetonic_transaction::fs_ops::is_symlink_or_reparse(entry.path()) {
            continue;
        }
        let p = entry.path();
        let p_str = p.to_string_lossy();
        let p_clean = p_str.strip_prefix(r"\\?\").unwrap_or(&p_str);

        let rel_str = if let Some(rest) = p_clean.strip_prefix(root_clean) {
            rest.trim_start_matches(['\\', '/']).replace('\\', "/")
        } else {
            p.strip_prefix(root)
                .map(|r| r.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| p_clean.to_string().replace('\\', "/"))
        };

        if rel_str == req_norm {
            continue;
        }

        // Exact filename match in another directory
        if let Some(fname) = p.file_name().and_then(|n| n.to_str()) {
            if fname.eq_ignore_ascii_case(req_file_name) {
                return Some(format!(". Did you mean '{rel_str}'?"));
            }
        }

        // Levenshtein distance on relative path
        let dist = str_distance(&rel_str, &req_norm);
        if (dist <= 3 || (req_norm.len() >= 6 && dist <= req_norm.len() / 3))
            && best_match.as_ref().map(|(d, _)| dist < *d).unwrap_or(true)
        {
            best_match = Some((dist, rel_str));
        }
    }

    best_match.map(|(_, path)| format!(". Did you mean '{path}'?"))
}

fn str_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b_chars.len()).collect();
    let mut curr: Vec<usize> = vec![0; b_chars.len() + 1];

    for (i, &ca) in a_chars.iter().enumerate() {
        curr[0] = i + 1;
        for (j, &cb) in b_chars.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        prev.clone_from_slice(&curr);
    }
    prev[b_chars.len()]
}

/// Re-check the path is still a lexical nofollow child of the workspace root.
pub(crate) fn refresh_mutating_path(ws: &Workspace, path: &Path) -> Result<PathBuf, ToolError> {
    let rel = path
        .strip_prefix(ws.root())
        .map_err(|_| ToolError::OutsideWorkspace(ws.display_rel(path)))?;
    let rel = rel.to_string_lossy().replace('\\', "/");
    ws.resolve(&rel)
}

pub fn read_to_string_nofollow(path: &Path) -> Result<String, ToolError> {
    if file_starts_with_sqlite_header(path) {
        return Err(ToolError::Other(
            "file is outside this execution grant".into(),
        ));
    }
    tetonic_transaction::fs_ops::read_to_string_nofollow(path).map_err(map_txn)
}

/// A SQLite database, its write-ahead log, or the shared-memory file beside it.
/// Only headers are read. The shared-memory file has no stable header, so it
/// is recognized as the `-shm` or `-wal` sibling of a database.
pub fn file_starts_with_sqlite_header(path: &Path) -> bool {
    if sqlite_header_prefix(path) {
        return true;
    }
    sqlite_sidecar_sibling(path).is_some_and(|sibling| sqlite_header_prefix(&sibling))
}

fn sqlite_header_prefix(path: &Path) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut magic = [0u8; 16];
    let Ok(n) = std::io::Read::read(&mut file, &mut magic) else {
        return false;
    };
    sqlite_family_header(&magic[..n])
}

fn sqlite_sidecar_sibling(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    let stem = name
        .strip_suffix("-wal")
        .or_else(|| name.strip_suffix("-shm"))
        .or_else(|| name.strip_suffix("-journal"))?;
    if stem.is_empty() {
        return None;
    }
    Some(path.with_file_name(stem))
}

fn sqlite_family_header(magic: &[u8]) -> bool {
    if magic.len() >= 15 && magic.starts_with(b"SQLite format 3") {
        return true;
    }
    magic.len() >= 4
        && matches!(
            [magic[0], magic[1], magic[2], magic[3]],
            [0x37, 0x7f, 0x06, 0x82]
                | [0x37, 0x7f, 0x06, 0x83]
                | [0x82, 0x06, 0x7f, 0x37]
                | [0x83, 0x06, 0x7f, 0x37]
        )
}

pub fn write_bytes_nofollow(path: &Path, content: &[u8]) -> Result<(), ToolError> {
    tetonic_transaction::fs_ops::write_bytes_nofollow(path, content).map_err(map_txn)
}

pub fn create_dir_all_nofollow(root: &Path, dir: &Path) -> Result<(), ToolError> {
    tetonic_transaction::fs_ops::create_dir_all_nofollow(root, dir).map_err(map_txn)
}

pub fn remove_nofollow(path: &Path) -> Result<(), ToolError> {
    tetonic_transaction::fs_ops::remove_nofollow(path).map_err(map_txn)
}

#[allow(dead_code)] // revert-on-syntax-error path not wired to production mutation yet
pub(crate) fn revert_on_syntax_error_enabled() -> bool {
    std::env::var("LOKAI_REVERT_ON_SYNTAX_ERROR")
        .ok()
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

#[allow(dead_code)] // revert-on-syntax-error path not wired to production mutation yet
pub(crate) fn revert_file(
    path: &Path,
    before: Option<&str>,
    existed: bool,
) -> Result<(), ToolError> {
    match before {
        Some(content) => write_bytes_nofollow(path, content.as_bytes()),
        None if !existed => {
            let _ = remove_nofollow(path);
            Ok(())
        }
        None => Ok(()),
    }
}
