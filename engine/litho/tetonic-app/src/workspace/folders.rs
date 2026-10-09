//! Folder selection narrows operator configuration; agent revisions retain the
//! chosen canonical path. Runtime grants and no-follow tools still authorize I/O.
use super::*;
use std::path::{Component, Path, PathBuf};
#[cfg(test)]
#[path = "folders_tests.rs"]
mod tests;

impl WorkspaceServices {
    pub(crate) fn available_folders(&self) -> Vec<String> {
        let base = self
            .host
            .settings
            .workspace_root
            .as_ref()
            .and_then(|p| p.canonicalize().ok());
        let mut roots: Vec<_> = base
            .into_iter()
            .chain(self.folders.iter().filter_map(|p| {
                let root = p.canonicalize().ok()?;
                self.check_additional_folder(&root).ok()?;
                Some(root)
            }))
            .filter(|p| p.is_dir())
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        roots.sort();
        roots.dedup();
        roots
    }

    pub(crate) fn resolve_folder(
        &self,
        requested: Option<&str>,
    ) -> Result<Option<PathBuf>, AppError> {
        let Some(path) = requested else {
            return Ok(None);
        };
        let root = Path::new(path)
            .canonicalize()
            .map_err(|_| AppError::WorkspaceUnavailable)?;
        // The exact path displayed to the owner is pinned. Repointed symlinks,
        // aliases and stale operator configurations cannot substitute a folder.
        if root.to_str() != Some(path) || !self.available_folders().iter().any(|p| p == path) {
            return Err(AppError::PolicyDenied("This folder is no longer approved by the host. Choose an available working folder in agent settings.".into()));
        }
        Ok(Some(root))
    }

    fn check_additional_folder(&self, root: &Path) -> Result<(), AppError> {
        let database = self
            .local
            .store()
            .path()
            .canonicalize()
            .map_err(|_| AppError::WorkspaceUnavailable)?;
        let protected = root.components().any(|c| match c {
            Component::Normal(name) => matches!(
                name.to_string_lossy()
                    .trim_end_matches([' ', '.'])
                    .to_ascii_lowercase()
                    .as_str(),
                ".lokai" | ".tetonic" | ".ssh" | ".aws" | ".azure" | ".kube" | ".codex" | ".gnupg"
            ),
            _ => false,
        });
        // Configured artifact/log roots may live outside the conventional hidden
        // directories. Neither their children nor a containing folder is a new grant.
        let overlaps_control = self.protected_folders.iter().any(|path| {
            let absolute = path.canonicalize().or_else(|_| std::path::absolute(path));
            absolute.is_ok_and(|path| root.starts_with(&path) || path.starts_with(root))
        });
        if !root.is_dir()
            || root.parent().is_none()
            || protected
            || database.starts_with(root)
            || overlaps_control
        {
            return Err(AppError::PolicyDenied(
                "Choose a working folder separate from engine state and credential storage.".into(),
            ));
        }
        Ok(())
    }
}
