//! Immutable pre-migration snapshots and cooperating upgrade ownership.

use crate::{Result, Store, StoreError};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

pub const SCHEMA_TARGET_VERSION: i64 = 28;

pub fn is_ephemeral_db_path(path: &Path) -> bool {
    let s = path.to_string_lossy();
    s == ":memory:"
        || s.starts_with("file:")
            && (s.starts_with("file::memory:")
                || s.split_once('?').is_some_and(|(_, query)| {
                    query.split('&').any(|parameter| parameter == "mode=memory")
                }))
}

/// Legacy backup location. New snapshots live in `pre_migrate_backup_directory`.
/// Existing legacy backups are never removed or overwritten.
pub fn pre_migrate_backup_path(db_path: &Path) -> PathBuf {
    let mut s = db_path.as_os_str().to_owned();
    s.push(".pre-migrate.bak");
    PathBuf::from(s)
}

pub fn pre_migrate_backup_directory(db_path: &Path) -> PathBuf {
    let mut s = db_path.as_os_str().to_owned();
    s.push(".backups");
    PathBuf::from(s)
}

pub(crate) fn schema_version(conn: &Connection) -> Result<i64> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_versions')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(0);
    }
    Ok(conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
        [],
        |r| r.get(0),
    )?)
}

pub(crate) fn ensure_supported_schema(conn: &Connection) -> Result<()> {
    let found = schema_version(conn)?;
    if found > SCHEMA_TARGET_VERSION {
        return Err(StoreError::FutureSchema {
            found,
            supported: SCHEMA_TARGET_VERSION,
        });
    }
    Ok(())
}

fn durable_path(conn: &Connection) -> Result<PathBuf> {
    let path = conn.path().filter(|p| !p.is_empty()).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "cannot backup ephemeral database",
        )
    })?;
    Ok(std::fs::canonicalize(path)?)
}

pub(crate) fn upgrade_lock(conn: &Connection) -> Result<Option<std::fs::File>> {
    if conn.path().map_or(true, |p| p.is_empty()) {
        return Ok(None);
    }
    let mut path = durable_path(conn)?.into_os_string();
    path.push(".migration.lock");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(PathBuf::from(path))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match fs2::FileExt::try_lock_exclusive(&file) {
            Ok(()) => return Ok(Some(file)),
            Err(e)
                if e.raw_os_error() == fs2::lock_contended_error().raw_os_error()
                    && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(std::time::Duration::from_millis(10))
            }
            Err(e) => return Err(e.into()),
        }
    }
}

/// Caller holds upgrade ownership. VACUUM publishes only into a unique staging
/// path, and a verified, synced snapshot is renamed to a unique final filename.
/// Failed/interrupted snapshots retain a .partial suffix; older backups survive.
pub(crate) fn backup_connection(conn: &Connection, version: i64) -> Result<PathBuf> {
    let directory = pre_migrate_backup_directory(&durable_path(conn)?);
    std::fs::create_dir_all(&directory)?;
    let id = uuid::Uuid::new_v4();
    let staging = directory.join(format!("v{version}-{id}.partial"));
    let dest = directory.join(format!("v{version}-{id}.sqlite"));
    let path = staging.to_str().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "database backup path is not UTF-8",
        )
    })?;
    conn.execute("VACUUM INTO ?1", [path])?;
    {
        let copy =
            Connection::open_with_flags(&staging, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let integrity: String = copy.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        if integrity != "ok" || schema_version(&copy)? != version {
            return Err(StoreError::Io(std::io::Error::other(
                "backup verification failed",
            )));
        }
    }
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&staging)?
        .sync_all()?;
    std::fs::rename(&staging, &dest)?;
    #[cfg(unix)]
    std::fs::File::open(&directory)?.sync_all()?;
    tracing::info!(version, backup = %dest.display(), "pre-migration backup verified");
    Ok(dest)
}

/// Create a new, verified snapshot without replacing any previous backup.
pub fn pre_migration_backup(db_path: &Path) -> Result<PathBuf> {
    if is_ephemeral_db_path(db_path) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "cannot backup ephemeral database",
        )
        .into());
    }
    let conn = Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    let _owner = upgrade_lock(&conn)?;
    backup_connection(&conn, schema_version(&conn)?)
}

impl Store {
    pub fn create_pre_migration_backup(&self) -> Result<PathBuf> {
        let _owner = upgrade_lock(&self.conn)?;
        backup_connection(&self.conn, schema_version(&self.conn)?)
    }
}
