# Worker TLS key storage

Worker enrollment and fabric serving store private TLS keys through the
`lokai-domain::key_storage::KeyStorage` port. `lokai-secrets` supplies native
credential-store adapters. `lokai-memory` persists public certificates, opaque
versioned references and compare-and-swap revisions. `lokai-node` coordinates
identity creation, migration, rotation and revocation. Product code only assembles
and invokes those components.

## Platform requirements

- Windows: Credential Manager under the account running the worker.
- macOS: that account's Keychain, available and authorized for the executable.
  The OS may request access; native unattended behavior still needs validation.
- Linux: a persistent, encrypted Secret Service collection on the service
  account's session D-Bus, already unlocked. The adapter uses an encrypted D-Bus
  session, rejects the standard session-only collection, and disables interactive
  prompts. Configure keyring persistence/encryption using the service's own tools.
  It does not start/unlock a keyring or keep a master password in Lokai's config.
- Other platforms, missing services, inaccessible stores and invalid references:
  fail closed. There is no raw-file, environment-variable, SQLite or in-memory
  fallback in production. Headless workers must provision their account's secure
  store before enrollment/serving; this requirement affects worker TLS, not local
  inference-only CLI sessions.

References use `os-keyring:v1:<uuid>` within the fixed
`org.lokai.private-keys.v1` namespace. Unsupported versions and arbitrary credential
names are rejected before consulting the OS. Native Windows/macOS builders are
selected directly, avoiding the keyring library's replaceable global/mock default.
The Linux adapter uses `dbus-secret-service` directly to disable prompts.

## Existing workers

Stop old worker/enrollment processes before upgrading. Schema v7 adds opaque
references and lifecycle metadata in one SQLite transaction; newer worker schemas
are refused. The legacy `key_der` column is retained only for migration and is
empty on protected records. The persistence API no longer accepts private key
bytes for writes.

On first identity use, Lokai validates the old certificate/key, stores and reads
back the private key through the OS adapter, then atomically publishes its reference
and clears the raw column. This preserves the pinned certificate. A vault failure
leaves the original identity intact and startup fails. Concurrent initializers use
revision checks and converge without overwriting another process's identity.

`secure_delete` is enabled before mutation and a truncating WAL checkpoint must
finish before migration reports readiness. If another reader blocks cleanup, a
persistent pending flag causes a later retry. These operations remove tested
live-database/WAL remnants; they are not a forensic secure-erasure guarantee for
SSDs, storage snapshots, crash dumps, or earlier backups.

**Previous plaintext backups are still sensitive.** This change does not find or
delete historical copies. Restrict access to them and use an operator-controlled
retention/disposal policy. If exposure is suspected, rotate and re-enroll; moving
the same key into a vault cannot undo an earlier disclosure. Do not restore old
binaries against the upgraded database.

## Restore, rotation and revocation

A protected database backup contains references, not private key material. Restore
with access to the matching OS account/credential-store backup. Copying `worker.db`
alone to a different account or machine is insufficient. An absent, inaccessible,
revoked or mismatched key is an error; startup never silently regenerates one.

Offline lifecycle APIs `rotate_tls_identity` and `revoke_tls_identity` live in
`lokai-node`. Stop serving before calling them. There is no new CLI command in this
batch. Rotation creates a new certificate/key and requires updating coordinator
pins through re-enrollment. Previous OS entries are retained so known-good backups
can still restore their identities. Deleting a historical reference invalidates
backups that rely on it; make that an explicit retention decision.

Revocation first persists a tombstone, then deletes the current OS entry. Failed
OS deletion can be retried while startup stays disabled. Rotation explicitly
re-enables a tombstoned worker. These APIs do not terminate already running
listeners or invalidate certificates held by external peers, and revoking the
current key does not delete every retained historical key.

OS key creation and SQLite publication cannot share a transaction. A crash or
ambiguous publication error can leave an unreferenced OS entry. Such entries are
retained rather than risking deletion of a live key. Automatic orphan reconciliation
is not yet implemented. Definite CAS losers clean up only their own new entries.

## Validation

Portable tests use an injected in-memory test adapter and cover migration, CAS
races, read-back failures/missing keys, restore, rotation, revocation, SQL failures
and blocked WAL cleanup. Test files are searched for the actual DER bytes.

The explicit native smoke test creates and deletes one disposable credential:

```text
cargo test -p lokai-secrets key_storage::tests::native_round_trip_delete_and_missing -- --ignored --exact
```

It passed on Windows outside the restricted execution sandbox. Linux/macOS native
credential-store tests remain pending. The Unix CI lifecycle job compiles their
adapters and runs portable worker lifecycle tests, but has not been dispatched here.
A compile check is not native credential-store or encrypted-vault certification.

These protections reduce exposure from application database/WAL/backup disclosure.
They do not protect against compromise of the running worker, its unlocked OS
account, administrator/root access, or a compromised credential-store service.

Adapter API reference: [keyring 3.6.3](https://docs.rs/keyring/3.6.3/keyring/).
