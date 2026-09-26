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

Operator identity in administrative audit, organization policy ceilings, remote transport, end-user UI and managed team activation remain pending. Local issuance/revocation rely on the operator's database access, not an authenticated remote role. Do not expose those provisioning methods as unauthenticated network routes.

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

## Explicit team membership

`add-team-member --org acme --team maintainers --principal local/bob` and `remove-team-member` accept a credential on stdin, like the organization membership commands. Only a current team owner or organization administrator can manage these grants. The recipient must already belong to that organization. Organization membership alone does not grant access to every team's metadata.

Removal changes explicit membership only: it does not transfer ownership or remove rights held through the organization administrator role. Removing an owner's organization membership blocks their owner permissions. Each accepted change records actor, organization, team, subject and operation in the same transaction as the membership change. Schema 33 adds a nullable team identifier to existing administrative audit records; historical organization events remain intact.

## Private and team discussions

These local commands create durable human discussion history. They do not start inference or agents. All take a bearer credential through stdin. As elsewhere in this runbook, filesystem/database operators remain trusted; this is not confidentiality against someone who can open the database or issue credentials for other principals.

After bootstrap, issue a currently valid credential for the intended principal (substitute an existing organization member):

```powershell
$discussionKey = & .\engine\target\debug\tetonic.exe @controlArgs issue-credential --principal local/admin | ConvertFrom-Json
$discussionKey.credential | & .\engine\target\debug\tetonic.exe @controlArgs context private --org acme --context my-private-context
$opened = $discussionKey.credential | & .\engine\target\debug\tetonic.exe @controlArgs context open --context my-private-context | ConvertFrom-Json
$discussionKey.credential | & .\engine\target\debug\tetonic.exe @controlArgs context send --context my-private-context --session $opened.session --request message-1 --message-file .\message.txt
$discussionKey.credential | & .\engine\target\debug\tetonic.exe @controlArgs context history --context my-private-context --session $opened.session --limit 50
Remove-Variable discussionKey
```

Create `message.txt` as a nonempty UTF-8 file of at most 64 KiB before sending. Message text stays out of command arguments. Reuse `message-1` only to retry the same author/content; a new message requires a new request ID. History emits JSON and can contain sensitive content, so choose where to display or redirect it accordingly.

For a shared discussion, use `context team --org acme --team maintainers --context maintenance-discussion`, then the same open/send/history operations. The caller must be the current team owner or an explicit team member and still belong to the organization. Organization metadata administrator status alone does not grant team-content access. Private contexts are owned by the authenticated principal; there is no owner-override argument.

History returns the latest 1–200 messages in chronological order, not an assertion that all earlier history was returned. Human author attribution is stored separately from agent identity; the initial CLI history output exposes sequence, role and content. Full participant UI, streaming, archival and agent activation remain pending.

`context open` without `--session` creates a discussion and prints a server-chosen id. Passing `--session` only reopens an open discussion in that context; a missing id and an id from another context are both denied and create nothing. Closed discussions stay closed.

Search authorized messages with `context recall --context my-private-context --query "deployment decision" --limit 8`, piping a currently valid credential on stdin. This searches only that context's messages and revalidated tool results, excludes system messages and rolled-back/deleted source messages, and returns at most 30 snippets. It does not search project digests. Results are ordered by session start time rather than global relevance statistics. Treat retrieved text as untrusted content. Query text is a CLI argument; avoid putting secrets in a search query if process command lines are recorded.

Scoped discussions can be closed with `tetonic control --database <path> --audience <audience> context close --context <context> --session <session>` using the existing stdin credential convention. Close preserves history and allows identical retries only while access remains valid. It does not cancel agent execution. Closed discussions reject new messages and an `open` retry does not reopen them; explicit resumable discussion lifecycle is not yet implemented.

## Organization-owned agent registration

Issue a fresh administrator credential if the walkthrough credential has expired or was revoked. Registration currently requires organization administration; organization members can read these organization-owned definitions. This does not create a personal/private agent.

```powershell
$issued = & .\engine\target\debug\tetonic.exe @controlArgs issue-credential --principal local/admin --lifetime-seconds 3600 | ConvertFrom-Json
if (-not $issued.credential) { throw 'Credential issuance failed' }
@{ instructions = 'Investigate assigned research questions'; requested_tools = @('recall') } |
    ConvertTo-Json | Set-Content -Encoding utf8 .\researcher.json
$issued.credential | & .\engine\target\debug\tetonic.exe @controlArgs agent register --org acme --agent researcher --harness general --config-file .\researcher.json
$issued.credential | & .\engine\target\debug\tetonic.exe @controlArgs agent get --org acme --agent researcher
```

UTF-8 JSON files with or without a byte-order mark are accepted. Configuration must be an object; the input file and stored envelope are each limited to 64 KiB, so envelope overhead reduces the maximum configuration payload. Identical registration retries retain identity and digest. A changed registration conflicts; use `agent publish` to add a revision. The harness name/configuration is registered data, not proof the harness is installed or executable. Requested tools grant no access. Output reports `privilege_class: unconfigured` and `agent_activated: false`; no inference or agent execution is started.

Publish another immutable revision after editing the configuration file:

```powershell
$published = $issued.credential | & .\engine\target\debug\tetonic.exe @controlArgs agent publish --org acme --agent researcher --harness general --config-file .\researcher.json | ConvertFrom-Json
$issued.credential | & .\engine\target\debug\tetonic.exe @controlArgs agent get --org acme --agent researcher --revision $published.definition_digest
```

Publishing requires organization administration. It retains the identity, returns the configuration digest, and does not activate an agent or change the registration default. `agent get` without `--revision` still returns the original registration; `--revision` selects an exact authorized snapshot. Unknown digests fail rather than falling back to another version. Default selection and activation remain separate, unfinished operations.

## Launch a registered job

`tetonic job run` is a local launch, not a remote UI. Host settings (workspace, model, tool ceiling, deadline, context size) come from an operator JSON file. The employee request supplies the organization, information context, agent, definition digest, grant, request id, recovery id, and input file. Pipe the bearer credential to stdin.

```powershell
$issued.credential | & .\engine\target\debug\tetonic.exe job --database .\acme.db --audience acme --host-settings .\host.json run --org acme --context '<context-id>' --agent researcher --definition-digest '<digest>' --grant '<grant-id>' --request-id request-1 --recovery-id job-1 --input-file .\task.txt
```

The command waits for a terminal outcome. Success prints `run_id`, `task_id`, `audit_session_id`, `launched`, and `outcome` (`completed`, `canceled`, `limited`, or `failed`). It does not report Running. `launched` is true only for the winning process. The same request id returns the original locators with `launched: false` and does not start a second execution. Invalid host settings fail before admission and print no receipt. Inspect events with `inspect-run` and `replay-run`.

## Inspect a governed run

For a run already admitted by a trusted governed host, use a current credential belonging to a member of its information context:

```powershell
$issued.credential | & .\engine\target\debug\tetonic.exe @controlArgs inspect-run --org acme --context '<context-id>' --run '<run-id>'
```

Replace the IDs with the actual admitted context and run. This prints the authoritative run snapshot, including task scope, selected grant ID, states and output receipts. Every task must belong to that exact organization/context. Legacy, mixed-context, unknown and unauthorized runs deny without returning the snapshot. Revoking execution permission does not remove otherwise-authorized access to completed results; expired/revoked credentials or removed content membership deny inspection. This is local operator tooling, not a remote login or an agent launch command. Live subscriptions are not exposed here; retained journal replay is described below.

Replay retained lifecycle events for the same governed run:

```powershell
$issued.credential | & .\engine\target\debug\tetonic.exe @controlArgs replay-run --org acme --context '<context-id>' --run '<run-id>' --after 0 --limit 100
```

The result contains either `events` or `gap`. Continue using the last returned event's `sequence` as `--after`. Limits must be 1–1000. A gap reports unavailable retained history; inspect the current snapshot instead of treating missing events as success. This replays the durable lifecycle journal, not model tokens or a live subscription. The existing backend currently loads the retained journal internally, so the response limit is not an internal memory bound.
