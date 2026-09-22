//! File data must be synced before publication; Unix directory entries follow.
use std::{fs::File, io, path::Path};

pub(crate) fn replace(root: &Path, destination: &Path, source: &Path) -> io::Result<()> {
    replace_with_sync(root, destination, source, File::sync_all)
}

/// Recovery restores the backup's permissions, not the mutated destination's.
pub(crate) fn restore(root: &Path, destination: &Path, backup: &Path) -> io::Result<()> {
    publish_with_sync(root, destination, backup, true, File::sync_all)
}

pub(crate) fn remove(root: &Path, path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => std::fs::remove_dir_all(path)?,
        Ok(_) => std::fs::remove_file(path)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    }
    sync_directory_chain(path.parent().unwrap_or(root), root)
}

pub(crate) fn rename(root: &Path, source: &Path, destination: &Path) -> io::Result<()> {
    rename_with_sync(root, source, destination, sync_directory_chain)
}

fn rename_with_sync(
    root: &Path,
    source: &Path,
    destination: &Path,
    mut sync: impl FnMut(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    let source_parent = source.parent().unwrap_or(root);
    let destination_parent = destination.parent().unwrap_or(root);
    std::fs::create_dir_all(destination_parent)?;
    #[cfg(unix)]
    File::open(source)?.sync_all()?;
    std::fs::rename(source, destination)?;
    // Persist both namespace changes, even across different directories.
    sync(destination_parent, root)?;
    if source_parent != destination_parent {
        sync(source_parent, root)?;
    }
    Ok(())
}

pub(crate) fn change_mode(path: &Path, mode: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Retain an open handle before chmod can remove our read permission.
        let file = File::open(path)?;
        file.set_permissions(std::fs::Permissions::from_mode(mode))?;
        file.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Unix mode changes are unsupported on this platform",
        ))
    }
}

fn replace_with_sync(
    root: &Path,
    destination: &Path,
    source: &Path,
    sync: impl FnOnce(&File) -> io::Result<()>,
) -> io::Result<()> {
    publish_with_sync(root, destination, source, false, sync)
}

fn publish_with_sync(
    root: &Path,
    destination: &Path,
    source: &Path,
    restoring: bool,
    sync: impl FnOnce(&File) -> io::Result<()>,
) -> io::Result<()> {
    let parent = destination.parent().unwrap_or(root);
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".lokai_commit_tmp_")
        .tempfile_in(parent)?;
    std::io::copy(&mut File::open(source)?, &mut temporary)?;
    if restoring {
        temporary
            .as_file()
            .set_permissions(std::fs::metadata(source)?.permissions())?;
    }
    #[cfg(unix)]
    if !restoring {
        match std::fs::metadata(destination) {
            Ok(metadata) => temporary
                .as_file()
                .set_permissions(metadata.permissions())?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    sync(temporary.as_file())?;
    temporary
        .persist(destination)
        .map_err(|error| error.error)?;
    sync_directory_chain(parent, root)
}

/// Sync each directory up through the established root, including newly created
/// ancestors. Windows directory durability requires a separate native protocol.
pub(crate) fn sync_directory_chain(directory: &Path, root: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        if !directory.starts_with(root) {
            return Err(io::Error::other(
                "publication directory is outside its root",
            ));
        }
        let mut current = directory;
        loop {
            File::open(current)?.sync_all()?;
            if current == root {
                break;
            }
            current = current
                .parent()
                .ok_or_else(|| io::Error::other("missing publication parent"))?;
        }
    }
    #[cfg(not(unix))]
    let _ = (directory, root);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_restore_sync_preserves_current_file_for_retry() {
        let dir = tempfile::tempdir().unwrap();
        let backup = dir.path().join("backup");
        let destination = dir.path().join("file");
        std::fs::write(&backup, "original").unwrap();
        std::fs::write(&destination, "mutated").unwrap();
        assert!(
            publish_with_sync(dir.path(), &destination, &backup, true, |_| {
                Err(io::Error::other("injected restore sync failure"))
            })
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(&destination).unwrap(), "mutated");
        restore(dir.path(), &destination, &backup).unwrap();
        assert_eq!(std::fs::read_to_string(destination).unwrap(), "original");
    }

    #[cfg(unix)]
    #[test]
    fn restore_uses_backup_permissions_instead_of_mutated_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let backup = dir.path().join("backup");
        let destination = dir.path().join("file");
        std::fs::write(&backup, "original").unwrap();
        std::fs::set_permissions(&backup, std::fs::Permissions::from_mode(0o750)).unwrap();
        std::fs::write(&destination, "mutated").unwrap();
        std::fs::set_permissions(&destination, std::fs::Permissions::from_mode(0o000)).unwrap();
        restore(dir.path(), &destination, &backup).unwrap();
        assert_eq!(
            std::fs::metadata(destination).unwrap().permissions().mode() & 0o777,
            0o750
        );
    }

    #[test]
    fn cross_directory_rename_syncs_both_sides_and_delete_removes_destination() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source/file");
        let dest = dir.path().join("new/nested/file");
        std::fs::create_dir(source.parent().unwrap()).unwrap();
        std::fs::write(&source, "content").unwrap();
        let mut synced = Vec::new();
        rename_with_sync(dir.path(), &source, &dest, |path, root| {
            synced.push(path.to_path_buf());
            sync_directory_chain(path, root)
        })
        .unwrap();
        assert_eq!(synced, [dest.parent().unwrap(), source.parent().unwrap()]);
        assert!(!source.exists());
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "content");
        remove(dir.path(), &dest).unwrap();
        assert!(!dest.exists());
    }

    #[test]
    fn post_rename_sync_failure_is_not_reported_as_success() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let dest = dir.path().join("dest");
        std::fs::write(&source, "content").unwrap();
        let error = rename_with_sync(dir.path(), &source, &dest, |_, _| {
            Err(io::Error::other("injected directory sync failure"))
        })
        .unwrap_err();
        assert!(error.to_string().contains("sync failure"));
        // Publication may have happened: the caller must retain its started
        // journal entry for recovery rather than mark this operation complete.
        assert!(dest.exists());
        assert!(!source.exists());
    }

    #[cfg(not(unix))]
    #[test]
    fn unsupported_mode_change_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file");
        std::fs::write(&path, "content").unwrap();
        assert_eq!(
            change_mode(&path, 0o755).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), "content");
    }

    #[cfg(unix)]
    #[test]
    fn mode_change_can_sync_after_removing_read_permission() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file");
        std::fs::write(&path, "content").unwrap();
        change_mode(&path, 0o000).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0
        );
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[test]
    fn failed_data_sync_does_not_publish_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("target");
        let source = dir.path().join("staged");
        std::fs::write(&old, "original").unwrap();
        std::fs::write(&source, "replacement").unwrap();
        let result = replace_with_sync(dir.path(), &old, &source, |file| {
            assert_eq!(file.metadata()?.len(), 11);
            Err(io::Error::other("injected fsync failure"))
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(old).unwrap(), "original");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn publishes_complete_content_in_new_directory_tree() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("staged");
        let destination = dir.path().join("new/nested/file");
        std::fs::write(&source, "complete content").unwrap();
        replace(dir.path(), &destination, &source).unwrap();
        assert_eq!(
            std::fs::read_to_string(destination).unwrap(),
            "complete content"
        );
    }

    #[cfg(unix)]
    #[test]
    fn replacement_preserves_executable_and_nonexecutable_modes() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("staged");
        let destination = dir.path().join("script");
        std::fs::write(&source, "new").unwrap();
        for mode in [0o755, 0o750, 0o640] {
            std::fs::write(&destination, "old").unwrap();
            std::fs::set_permissions(&destination, std::fs::Permissions::from_mode(mode)).unwrap();
            replace(dir.path(), &destination, &source).unwrap();
            assert_eq!(
                std::fs::metadata(&destination)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o7777,
                mode
            );
        }
    }
}
