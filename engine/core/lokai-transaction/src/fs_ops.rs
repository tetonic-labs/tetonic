//! Content digests and path helpers.

use std::io::Read;
use std::path::{Path, PathBuf};

use lokai_domain::{ContentDigest, WorkspacePath};
use sha2::{Digest, Sha256};

use crate::error::TransactionError;

pub fn digest_bytes(data: &[u8]) -> ContentDigest {
    let mut h = Sha256::new();
    h.update(data);
    ContentDigest::new(format!("sha256:{:x}", h.finalize()))
}

pub fn is_symlink_or_reparse(path: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn refuse_symlink_chain(root: &Path, path: &Path) -> Result<(), TransactionError> {
    let rel = path
        .strip_prefix(root)
        .map_err(|_| TransactionError::OutsideWorkspace(path.display().to_string()))?;
    let mut current = root.to_path_buf();
    for comp in rel.components() {
        current.push(comp);
        if is_symlink_or_reparse(&current) {
            return Err(TransactionError::OutsideWorkspace(
                path.display().to_string(),
            ));
        }
    }
    Ok(())
}

pub fn digest_file(path: &Path) -> Result<ContentDigest, TransactionError> {
    let mut f = open_read_nofollow(path)?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| TransactionError::Io(e.to_string()))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(ContentDigest::new(format!("sha256:{:x}", h.finalize())))
}

pub fn digest_string(s: &str) -> ContentDigest {
    digest_bytes(s.as_bytes())
}

pub fn rel_path(root: &Path, path: &Path) -> Result<WorkspacePath, TransactionError> {
    if path.exists() && is_symlink_or_reparse(path) {
        return Err(TransactionError::OutsideWorkspace(
            path.display().to_string(),
        ));
    }
    let canon = if path.exists() {
        std::fs::canonicalize(path).map_err(|e| TransactionError::Io(e.to_string()))?
    } else {
        path.to_path_buf()
    };
    let root = std::fs::canonicalize(root).map_err(|e| TransactionError::Io(e.to_string()))?;
    let rel = canon
        .strip_prefix(&root)
        .map_err(|_| TransactionError::OutsideWorkspace(canon.display().to_string()))?;
    Ok(WorkspacePath::new(rel.to_string_lossy()))
}

pub fn resolve_under_root(root: &Path, rel: &str) -> Result<PathBuf, TransactionError> {
    use std::path::Component;
    if Path::new(rel).is_absolute() {
        return Err(TransactionError::OutsideWorkspace(rel.to_string()));
    }
    let mut current = root.to_path_buf();
    for comp in Path::new(rel).components() {
        match comp {
            Component::CurDir => {}
            Component::Normal(name) => {
                current.push(name);
                if is_symlink_or_reparse(&current) {
                    return Err(TransactionError::OutsideWorkspace(rel.to_string()));
                }
            }
            Component::ParentDir => {
                if current == root {
                    return Err(TransactionError::OutsideWorkspace(rel.to_string()));
                }
                current.pop();
            }
            _ => return Err(TransactionError::OutsideWorkspace(rel.to_string())),
        }
    }
    if current.starts_with(root) {
        refuse_symlink_chain(root, &current)?;
        Ok(current)
    } else {
        Err(TransactionError::OutsideWorkspace(rel.to_string()))
    }
}

pub fn open_read_nofollow(path: &Path) -> Result<std::fs::File, TransactionError> {
    if is_symlink_or_reparse(path) {
        return Err(TransactionError::OutsideWorkspace(
            path.display().to_string(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .map_err(|e| TransactionError::Io(e.to_string()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .map_err(|e| TransactionError::Io(e.to_string()))?;
        if is_symlink_or_reparse(path) {
            return Err(TransactionError::OutsideWorkspace(
                path.display().to_string(),
            ));
        }
        Ok(file)
    }
    #[cfg(not(any(unix, windows)))]
    {
        std::fs::File::open(path).map_err(|e| TransactionError::Io(e.to_string()))
    }
}

pub fn read_to_string_nofollow(path: &Path) -> Result<String, TransactionError> {
    let mut file = open_read_nofollow(path)?;
    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|e| TransactionError::Io(e.to_string()))?;
    Ok(text)
}

pub fn write_bytes_nofollow(path: &Path, content: &[u8]) -> Result<(), TransactionError> {
    if path.exists() && is_symlink_or_reparse(path) {
        return Err(TransactionError::OutsideWorkspace(
            path.display().to_string(),
        ));
    }
    use std::io::Write;
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .map_err(|e| TransactionError::Io(e.to_string()))?;
        file.write_all(content)
            .map_err(|e| TransactionError::Io(e.to_string()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        if path.exists() && is_symlink_or_reparse(path) {
            return Err(TransactionError::OutsideWorkspace(
                path.display().to_string(),
            ));
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .map_err(|e| TransactionError::Io(e.to_string()))?;
        if is_symlink_or_reparse(path) {
            return Err(TransactionError::OutsideWorkspace(
                path.display().to_string(),
            ));
        }
        file.write_all(content)
            .map_err(|e| TransactionError::Io(e.to_string()))
    }
    #[cfg(not(any(unix, windows)))]
    {
        std::fs::write(path, content).map_err(|e| TransactionError::Io(e.to_string()))
    }
}

pub fn create_dir_all_nofollow(root: &Path, dir: &Path) -> Result<(), TransactionError> {
    if dir == root {
        return Ok(());
    }
    let rel = dir
        .strip_prefix(root)
        .map_err(|_| TransactionError::OutsideWorkspace(dir.display().to_string()))?;
    let mut current = root.to_path_buf();
    for comp in rel.components() {
        current.push(comp);
        if current.exists() {
            if is_symlink_or_reparse(&current) {
                return Err(TransactionError::OutsideWorkspace(
                    dir.display().to_string(),
                ));
            }
            let meta = std::fs::symlink_metadata(&current)
                .map_err(|e| TransactionError::Io(e.to_string()))?;
            if !meta.file_type().is_dir() {
                return Err(TransactionError::Io(format!(
                    "not a directory: {}",
                    current.display()
                )));
            }
        } else {
            std::fs::create_dir(&current).map_err(|e| TransactionError::Io(e.to_string()))?;
        }
    }
    Ok(())
}

pub fn remove_nofollow(path: &Path) -> Result<(), TransactionError> {
    if !path.exists() && !is_symlink_or_reparse(path) {
        return Ok(());
    }
    if is_symlink_or_reparse(path) {
        std::fs::remove_file(path).or_else(|_| std::fs::remove_dir(path))
    } else if path.is_file() {
        std::fs::remove_file(path)
    } else if path.is_dir() {
        std::fs::remove_dir(path)
    } else {
        std::fs::remove_file(path)
    }
    .map_err(|e| TransactionError::Io(e.to_string()))
}

pub fn is_excluded_artifact(rel: &str) -> bool {
    let lower = rel.replace('\\', "/").to_ascii_lowercase();
    let trimmed = lower.trim_matches('/');
    trimmed == "target"
        || trimmed.starts_with("target/")
        || trimmed.contains("/target/")
        || trimmed == "node_modules"
        || trimmed.starts_with("node_modules/")
        || trimmed.contains("/node_modules/")
        || trimmed == "__pycache__"
        || trimmed.starts_with("__pycache__/")
        || trimmed.contains("/__pycache__/")
        || trimmed == ".git"
        || trimmed.starts_with(".git/")
        || trimmed.contains("/.git/")
        || lower.ends_with(".o")
        || lower.ends_with(".pyc")
        || lower.ends_with(".exe")
        || lower.ends_with(".dll")
        || lower.ends_with(".so")
        || lower.ends_with(".dylib")
}

#[cfg(unix)]
pub fn file_mode(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::symlink_metadata(path)
        .ok()
        .map(|m| m.permissions().mode())
}

#[cfg(windows)]
pub fn file_mode(_path: &Path) -> Option<u32> {
    None
}
