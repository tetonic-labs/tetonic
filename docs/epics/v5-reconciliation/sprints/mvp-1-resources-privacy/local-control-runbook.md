# Local operator control preview

This is a local administration path inside the existing `tetonic` CLI. It creates durable organization/team metadata without starting a model, coding runtime or agent session. It does not yet start a managed team of agents. The CLI binary is `tetonic` and its Cargo package/source folder is `tetonic-cli`.

The operator chooses an explicit database and deployment audience. Filesystem access to this database is administrative authority: these commands are not a remote employee client and the database must not be shared with untrusted users. Schema upgrades use the existing backup/migration machinery. This is the single-machine SQLite profile, not an HA deployment or a network-shared database.

## PowerShell walkthrough

From the repository root, build the CLI:

```powershell
cargo build --manifest-path engine/Cargo.toml -p tetonic-cli
$controlArgs = @('control', '--database', '.\tetonic-control.db', '--audience', 'local-preview')
```

Initialize the first administrator and organization once:

```powershell
& .\engine\target\debug\tetonic.exe @controlArgs bootstrap --principal local/admin --org acme --name Acme
```

Bootstrap uses a single transaction for the enabled principal, platform role, organization administrator membership and bootstrap audit event. Any existing control principal or previous bootstrap event rejects this operation, even if the administrator is disabled. It cannot replace an administrator or serve as password recovery. Legacy agent identities are separate from control principals.

Issue a short-lived credential to that existing principal. Capture the result in memory; do not paste the credential into command arguments or write it into a script:

```powershell
$issued = & .\engine\target\debug\tetonic.exe @controlArgs issue-credential --principal local/admin --lifetime-seconds 3600 | ConvertFrom-Json
if (-not $issued.credential) { throw 'Credential issuance failed' }
$issued.credential | & .\engine\target\debug\tetonic.exe @controlArgs create-team --org acme --team maintainers --name Maintainers
$issued.credential | & .\engine\target\debug\tetonic.exe @controlArgs get-team --org acme --team maintainers
```

Issuance intentionally prints the bearer secret to stdout for delivery to the operator. Protect that output and avoid terminal transcription when issuing credentials. Subsequent team commands accept only bounded, piped stdin credentials; interactive entry is rejected to avoid echoing a secret. The secret is not a CLI argument. Team ownership is the verified principal, and output is actual stored metadata.

Revoke this credential by its public identifier:

```powershell
& .\engine\target\debug\tetonic.exe @controlArgs revoke-credential --credential-id $issued.credential_id
$issued.credential | & .\engine\target\debug\tetonic.exe @controlArgs get-team --org acme --team maintainers
Remove-Variable issued
```

The final read should return access denied. Revoke is idempotent and reports that the request was processed, not that an unknown credential existed. Another independently issued credential remains valid until revoked, expired or its principal disabled.

## Recovery and remaining scope

Bootstrap and credential issuance are separate operations. If issuance/output delivery fails, keep the initialized database and issue a new credential; do not repeat bootstrap or delete the database. A credential whose output was lost expires at its recorded deadline; credential listing/recovery ergonomics remain unfinished. Keep the audience stable across restarts. A separately cloned deployment should change its audience if old credentials must not carry over.

Team-specific membership administration, operator identity in administrative audit, organization policy ceilings, remote transport, end-user UI and managed team activation remain pending. Local issuance/revocation rely on the operator's database access, not an authenticated remote role. Do not expose those provisioning methods as unauthenticated network routes.

Verification: `cargo test --manifest-path engine/Cargo.toml -p tetonic-cli --test control_cli` runs bootstrap, takeover rejection, credential issuance, team creation/read in separate CLI processes, missing-credential rejection and persisted revocation against a temporary database. It never prints the generated secret. Storage tests additionally cover concurrent bootstrap and rollback if the audit write fails.

## Organization membership administration

Register a second identity through the trusted local operator door, then use an existing organization administrator credential to grant metadata permissions:

```powershell
& .\engine\target\debug\tetonic.exe @controlArgs register-principal --principal local/bob
$issued.credential | & .\engine\target\debug\tetonic.exe @controlArgs set-member --org acme --principal local/bob --role team-creator
$issued.credential | & .\engine\target\debug\tetonic.exe @controlArgs remove-member --org acme --principal local/bob
```

Use a currently valid administrator credential (the earlier revocation example invalidates `$issued`). Roles are `administrator`, `team-creator`, or `member`. Registration grants no organization access and retries never re-enable an existing identity or change its platform role. Issue the new user's credential separately through the local operator door.

Membership changes record the authenticated actor, subject, organization and requested role/removal atomically. The write transaction rechecks enabled administrator membership, so a demoted actor cannot rely on an earlier membership decision. The last enabled administrator cannot be removed or demoted through this door. Removing organization membership also removes explicit team memberships. These grants authorize resource metadata only; they do not authorize execution, tools or private knowledge.

Credential validation happens at admission; revoking a credential does not cancel a change already admitted. Direct database operators remain trusted and can bypass these application controls. Remote employee access, administrator recovery and audit browsing remain unfinished.
