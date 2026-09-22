# Database upgrades and recovery

`Store::open` upgrades the schema in one SQLite `BEGIN IMMEDIATE` transaction.
DDL, row copying and all version markers commit together; an error or interrupted
process rolls the transaction back. A sidecar OS file lock coordinates upgrades
and backups across cooperating processes. Lock contention has a five-second
limit and fails the open rather than proceeding without ownership. Newer schema
versions are refused by both writable and read-only store opens.

For an existing versioned on-disk database, the upgrade first creates a verified
snapshot in `<database>.backups/v<source-version>-<uuid>.sqlite`. Snapshot creation
uses SQLite `VACUUM INTO`, including committed WAL data. It verifies integrity and
schema version, syncs the snapshot file, then renames it from a unique `.partial`
name. A failed backup aborts the upgrade. Prior snapshots and the historical
`<database>.pre-migrate.bak` file are never overwritten or deleted. The returned
path and `pre-migration backup verified` log identify the completed snapshot.

No retention pruning is automatic. Allow disk space for another full database
copy; a retry creates another snapshot. In-memory databases migrate transactionally
but have no on-disk backup. Partially upgraded databases produced by an older
binary are not repaired by guessing which statements ran.

## Operator recovery workflow

1. Stop every CLI, daemon and worker using the affected database. Preserve the
   original database, its `-wal` and `-shm` files, and backup directory together.
   Do not overwrite the original or copy just its live main file while it is open.
2. Select a completed `.sqlite` snapshot with the desired source version. Never
   restore a `.partial` file. Copy the snapshot into a **new, empty recovery
   directory**, using the intended database filename. This avoids attaching stale
   WAL/SHM files from another database to the restored copy.
3. With SQLite tooling, run `PRAGMA integrity_check;` (must return `ok`) and
   `SELECT MAX(version) FROM schema_versions;` on the copy. Inspect expected
   session/run rows before directing any application instance at it.
4. For upgrade retry, open the restored database with the corrected binary;
   `Store::open` backs it up again and upgrades it atomically. For version rollback,
   use a binary compatible with the snapshot's schema. A newer binary will
   automatically upgrade it again. Point the application's database configuration
   at the recovered copy only after all old instances have stopped.
5. Keep the original and snapshots until recovery is verified. Restore returns
   database state to snapshot time; later writes are absent. It does not roll back
   workspace files or external effects. Reconcile those using their owning
   recovery workflows before resuming work.

The regression suite executes this copy-to-a-fresh-path workflow from a v26
snapshot, verifies retained run data, and upgrades the restored copy to v28.
It also injects SQL failure and abrupt process exit before and after every v1-v28
version marker, including v27's destructive table replacement.

These checks establish transaction/process-interruption behavior, not a complete
power-loss certification for every OS/filesystem. Unix backup directory entries
are synced; Windows backup publication has not been hardware power-loss tested.
Upgrade locks require filesystem lock support and cooperating application
versions. Stop older binaries before upgrading; SQLite atomicity does not make
mixed-version application semantics safe.
