//! Publication ordering stays inside the artifact capability.
#[cfg(unix)]
use std::fs::File;
use std::{io, path::Path};

pub(crate) fn publish(root: &Path, temporary: &Path, destination: &Path) -> io::Result<()> {
    publish_with_sync(root, temporary, destination, |path| {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)?
            .sync_all()
    })
}

pub(crate) fn delete(root: &Path, metadata: &Path, object: &Path) -> io::Result<()> {
    delete_with_sync(root, metadata, object, sync_directory)
}

fn delete_with_sync(
    root: &Path,
    metadata: &Path,
    object: &Path,
    mut sync: impl FnMut(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    // Revoke the readable record durably before deleting its required content.
    // An interrupted delete may retain an orphan, never a live dangling record.
    remove_if_present(metadata)?;
    // Sync even if already absent: a previous attempt may have failed at sync.
    sync(root, metadata.parent().unwrap_or(root))?;
    remove_if_present(object)?;
    sync(root, object.parent().unwrap_or(root))
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn publish_with_sync(
    root: &Path,
    temporary: &Path,
    destination: &Path,
    sync: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<()> {
    sync(temporary)?;
    std::fs::rename(temporary, destination)?;
    sync_directory(root, destination.parent().unwrap_or(root))?;
    sync_directory(root, temporary.parent().unwrap_or(root))
}

pub(crate) fn sync_directory(root: &Path, directory: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        if !directory.starts_with(root) {
            return Err(io::Error::other("artifact directory outside store"));
        }
        let mut current = directory;
        loop {
            File::open(current)?.sync_all()?;
            if current == root {
                break;
            }
            current = current
                .parent()
                .ok_or_else(|| io::Error::other("missing store parent"))?;
        }
    }
    // Windows namespace durability requires a native publication protocol.
    #[cfg(not(unix))]
    let _ = (root, directory);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_metadata_removal_or_sync_retains_object_and_can_retry() {
        let dir = tempfile::tempdir().unwrap();
        let meta = dir.path().join("meta");
        let object = dir.path().join("object");
        std::fs::write(&object, "required bytes").unwrap();
        // A directory cannot be removed with remove_file on either platform.
        std::fs::create_dir(&meta).unwrap();
        assert!(delete(dir.path(), &meta, &object).is_err());
        assert!(object.exists());
        std::fs::remove_dir(&meta).unwrap();
        std::fs::write(&meta, "accepted").unwrap();
        assert!(delete_with_sync(dir.path(), &meta, &object, |_, _| {
            Err(io::Error::other("injected metadata directory sync failure"))
        })
        .is_err());
        assert!(!meta.exists());
        assert_eq!(std::fs::read_to_string(&object).unwrap(), "required bytes");
        let mut syncs = 0;
        delete_with_sync(dir.path(), &meta, &object, |root, path| {
            syncs += 1;
            sync_directory(root, path)
        })
        .unwrap();
        assert_eq!(syncs, 2);
        assert!(!object.exists());
        delete(dir.path(), &meta, &object).unwrap();
    }

    #[test]
    fn failed_sync_does_not_publish_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let temp = dir.path().join("pending");
        let dest = dir.path().join("metadata");
        std::fs::write(&temp, "accepted").unwrap();
        std::fs::write(&dest, "sealed").unwrap();
        assert!(
            publish_with_sync(dir.path(), &temp, &dest, |_| Err(io::Error::other(
                "sync failed"
            )))
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "sealed");
        publish(dir.path(), &temp, &dest).unwrap();
        assert_eq!(std::fs::read_to_string(dest).unwrap(), "accepted");
    }
}
