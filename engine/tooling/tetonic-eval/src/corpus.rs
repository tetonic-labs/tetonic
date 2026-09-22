//! Filesystem corpus: copy `engine/corpus/fixtures/<id>` and verify digests.

use crate::traits::CorpusProvider;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// SHA-256 of file bytes in a directory, matching `corpus/scripts/integrity.py`.
pub fn hash_directory(dir: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut files = Vec::new();
    collect_files(dir, dir, &mut files)?;
    files.sort();
    for rel in files {
        let path = dir.join(&rel);
        if path.symlink_metadata()?.file_type().is_symlink() {
            continue;
        }
        let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        hasher.update(&bytes);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("list {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            collect_files(root, &path, out)?;
        } else if ft.is_file() {
            out.push(path.strip_prefix(root).unwrap_or(&path).to_path_buf());
        }
    }
    Ok(())
}

pub fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Relative path → file bytes, skipping `.lokai`.
pub fn snapshot_files(dir: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut files = Vec::new();
    collect_files(dir, dir, &mut files)?;
    let mut map = BTreeMap::new();
    for rel in files {
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if rel_str.starts_with(".lokai/")
            || rel_str == ".lokai"
            || rel_str.starts_with("target/")
            || rel_str == "target"
            || rel_str.contains("/__pycache__/")
            || rel_str.starts_with("__pycache__/")
            || rel_str.ends_with(".pyc")
        {
            continue;
        }
        map.insert(rel_str, fs::read(dir.join(&rel))?);
    }
    Ok(map)
}

pub fn load_digests(corpus_root: &Path) -> Result<BTreeMap<String, String>> {
    let path = corpus_root.join("digests.json");
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn write_digests(corpus_root: &Path) -> Result<BTreeMap<String, String>> {
    let fixtures = corpus_root.join("fixtures");
    let mut map = BTreeMap::new();
    for entry in fs::read_dir(&fixtures).with_context(|| format!("list {}", fixtures.display()))? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        map.insert(name, hash_directory(&entry.path())?);
    }
    let path = corpus_root.join("digests.json");
    fs::write(&path, serde_json::to_string_pretty(&map)?)?;
    Ok(map)
}

pub fn check_integrity(corpus_root: &Path) -> Result<()> {
    let expected = load_digests(corpus_root)?;
    let fixtures = corpus_root.join("fixtures");
    for (id, want) in &expected {
        let dir = fixtures.join(id);
        let got = hash_directory(&dir)?;
        if got != *want {
            anyhow::bail!("INTEGRITY FAILURE: {id}: expected {want}, got {got}");
        }
    }
    Ok(())
}

pub fn resolve_corpus_root(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = explicit {
        return Ok(p.to_path_buf());
    }
    if let Ok(env) = std::env::var("LOKAI_CORPUS") {
        return Ok(PathBuf::from(env));
    }
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidate = manifest.join("../../corpus");
    if candidate.join("manifests").is_dir() {
        return Ok(candidate.canonicalize()?);
    }
    let cwd = std::env::current_dir()?;
    if cwd.join("corpus/manifests").is_dir() {
        return Ok(cwd.join("corpus"));
    }
    if cwd.join("manifests").is_dir() && cwd.join("fixtures").is_dir() {
        return Ok(cwd);
    }
    Err(anyhow!(
        "cannot find evaluation corpus; pass --corpus or set LOKAI_CORPUS"
    ))
}

pub fn load_manifest(
    corpus_root: &Path,
    scenario_id: &str,
) -> Result<crate::manifest::EvaluationManifest> {
    let path = corpus_root
        .join("manifests")
        .join(format!("{scenario_id}.json"));
    let raw =
        fs::read_to_string(&path).with_context(|| format!("read manifest {}", path.display()))?;
    Ok(serde_json::from_str(&raw)?)
}

pub struct FilesystemCorpus {
    root: PathBuf,
    /// Keep temp dirs alive until unmount.
    mounted: std::sync::Mutex<Vec<TempDir>>,
}

impl FilesystemCorpus {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            mounted: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn fixture_dir(&self, snapshot_id: &str) -> PathBuf {
        self.root.join("fixtures").join(snapshot_id)
    }
}

#[async_trait]
impl CorpusProvider for FilesystemCorpus {
    async fn mount_snapshot(&self, snapshot_id: &str) -> Result<PathBuf> {
        let src = self.fixture_dir(snapshot_id);
        if !src.is_dir() {
            anyhow::bail!("missing fixture directory {}", src.display());
        }
        let tmp = TempDir::new().context("eval workspace tempfile")?;
        copy_dir(&src, tmp.path())?;
        let path = tmp.path().to_path_buf();
        self.mounted.lock().unwrap().push(tmp);
        Ok(path)
    }

    async fn unmount_snapshot(&self, workspace: &Path) -> Result<()> {
        let mut mounted = self.mounted.lock().unwrap();
        mounted.retain(|d| d.path() != workspace);
        Ok(())
    }
}

#[async_trait]
impl crate::traits::SandboxProvider for FilesystemCorpus {
    async fn setup_sandbox(
        &self,
        workspace: &Path,
        _manifest: &crate::manifest::EvaluationManifest,
    ) -> Result<()> {
        if !workspace.is_dir() {
            anyhow::bail!("sandbox workspace missing: {}", workspace.display());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn tampered_fixture_fails_integrity() {
        let tmp = TempDir::new().unwrap();
        let fixtures = tmp.path().join("fixtures").join("snap-x");
        std::fs::create_dir_all(&fixtures).unwrap();
        std::fs::write(fixtures.join("a.txt"), b"hello").unwrap();
        let digest = hash_directory(&fixtures).unwrap();
        let mut map: BTreeMap<String, String> = BTreeMap::new();
        map.insert("snap-x".into(), digest);
        std::fs::write(
            tmp.path().join("digests.json"),
            serde_json::to_string(&map).unwrap(),
        )
        .unwrap();
        check_integrity(tmp.path()).expect("clean fixture");
        std::fs::write(fixtures.join("a.txt"), b"tampered").unwrap();
        let err = check_integrity(tmp.path()).unwrap_err().to_string();
        assert!(err.contains("INTEGRITY FAILURE"), "{err}");
    }
}
