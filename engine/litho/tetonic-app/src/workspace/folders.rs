//! Folder selection narrows host/owner approvals; agent revisions retain the
//! chosen canonical path. Runtime grants and no-follow tools still authorize I/O.
use super::*;
use std::path::{Component, Path, PathBuf};
#[cfg(test)]
#[path = "folders_tests.rs"]
mod tests;

#[derive(Serialize)]
pub struct WorkspaceFolderCatalog {
    pub revision: i64,
    pub paths: Vec<String>,
    pub configured_paths: Vec<String>,
    pub can_manage: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FolderInspection {
    pub path: String,
}

impl WorkspaceServices {
    pub(crate) fn available_folders(&self) -> Result<Vec<String>, AppError> {
        let mut roots = self.configured_folders();
        if self.owner_folder_registration {
            let saved = self.saved_folders()?;
            roots.extend(saved.paths.into_iter().filter(|p| {
                self.inspect_folder_path(p)
                    .is_ok_and(|canonical| canonical == *p)
            }));
        }
        roots.sort();
        roots.dedup();
        Ok(roots)
    }

    fn configured_folders(&self) -> Vec<String> {
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

    fn saved_folders(&self) -> Result<tetonic_memory::WorkspaceFolders, AppError> {
        self.local
            .store()
            .read_sync(|db| {
                db.workspace_folders(
                    self.scope.principal(),
                    self.scope.organization(),
                    self.scope.team(),
                )
            })
            .map_err(AppError::hide_store_failure)?
            .map_err(|e| resource(e.into()))
    }

    pub(crate) async fn folder_catalog(&self) -> Result<WorkspaceFolderCatalog, AppError> {
        self.authorized_scope().await?;
        let saved = self.saved_folders()?;
        Ok(WorkspaceFolderCatalog {
            revision: saved.revision,
            paths: saved.paths,
            configured_paths: self.configured_folders(),
            can_manage: self.owner_folder_registration,
        })
    }

    pub(crate) async fn inspect_folder(&self, input: FolderInspection) -> Result<String, AppError> {
        self.authorized_scope().await?;
        self.require_folder_management()?;
        self.inspect_folder_path(&input.path)
    }

    fn require_folder_management(&self) -> Result<(), AppError> {
        if !self.owner_folder_registration {
            return Err(AppError::PolicyDenied(
                "Working folders are managed by the host operator on this engine.".into(),
            ));
        }
        Ok(())
    }

    fn inspect_folder_path(&self, path: &str) -> Result<String, AppError> {
        // A local-folder check must not contact an SMB host or inspect device
        // namespaces before the owner has reviewed anything.
        let remote_or_device = matches!(Path::new(path).components().next(), Some(Component::Prefix(prefix))
            if !matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)));
        if path.len() > 4096
            || path.chars().any(char::is_control)
            || !Path::new(path).is_absolute()
            || remote_or_device
        {
            return Err(AppError::InvalidRequest(
                "Enter an absolute local folder path on the computer running the engine.".into(),
            ));
        }
        let root = Path::new(path).canonicalize().map_err(|_| AppError::InvalidRequest(
            "This folder could not be opened on the engine computer. Check that it exists and is accessible.".into(),
        ))?;
        self.check_additional_folder(&root)?;
        Ok(root.to_string_lossy().into_owned())
    }

    pub(crate) async fn save_folders(
        &self,
        mut input: SaveWorkspaceFolders,
    ) -> Result<WorkspaceFolderCatalog, AppError> {
        let scope = self.authorized_scope().await?;
        self.require_folder_management()?;
        validate_request_id(&input.request_id)?;
        if input.paths.len() > 32 {
            return Err(AppError::InvalidRequest(
                "Choose at most 32 additional working folders.".into(),
            ));
        }
        // Validate the exact reviewed canonical path, including on retries. Aliases
        // cannot be repointed between review and save to expand the approved scope.
        for path in &input.paths {
            if self.inspect_folder_path(path)? != *path {
                return Err(AppError::Conflict(
                    "A folder changed since it was checked. Check it again before adding it."
                        .into(),
                ));
            }
        }
        input.paths.sort();
        input.paths.dedup();
        self.local
            .store()
            .write(move |db| {
                db.save_workspace_folders(
                    scope.principal(),
                    scope.organization(),
                    scope.team(),
                    &input,
                )
            })
            .await
            .map_err(AppError::hide_store_failure)?
            .map_err(|e| resource(e.into()))?;
        // Return the current version even for a retry of an older mutation.
        self.folder_catalog().await
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
        if root.to_str() != Some(path) || !self.available_folders()?.iter().any(|p| p == path) {
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
