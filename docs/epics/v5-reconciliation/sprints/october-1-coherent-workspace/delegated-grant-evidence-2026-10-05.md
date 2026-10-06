# COORD-A: parent-derived execution permission

October 5, 2026. This completes the permission-derivation substep after the
[allocation and inherited-stop work](delegation-admission-evidence-2026-10-05.md).
**COORD-A remains open.** This is backend work; it does not enable team dispatch
or change the approved team-work UI.

## Integration

`ResourceService::derive_execution_grant` extends the existing execution-grant
store and audit trail. The verified actor must currently manage the team and
be the principal on the parent grant. One existing funded delegation supplies
the child work, allowance, payer and stop scope. The caller cannot pick different
lineage or convert an independent grant into a child grant.

Derivation requires a running parent work item bound to an active, claimed,
unquiesced managed attempt. The receipt pins the parent run, task, task version,
attempt and lease identity/epoch/holder. A changed ownership fence, expired lease,
elapsed deadline, recovery state or terminal attempt denies permission. Routine
renewal of the same lease does not change its ownership fence.

The child receives the same principal, organization and shared team context.
Private and participation contexts cannot be silently delegated. Tools and
artifact bindings must be subsets of the parent's grant, the selected agent and
definition must be registered in that organization, and the expiry cannot extend
the parent's grant. Every stored ancestor is rechecked, with bounded traversal.

Schema **56** adds immutable `execution_grant_lineage` beside the existing grants.
Grant creation, lineage and the existing audit event commit in one transaction.
Exact retries return the same receipt after checking current authority; changed
payloads conflict. One delegation cannot produce several independent grants.
An old receipt does not restore revoked permission or allocate budget again.
The existing migration backup/transaction path is retained. Existing root grants
are preserved; migration does not manufacture child permission.

`ManagedRunService::delegation_parent` supplies an opaque live parent handle for
`ContextService::bind_delegated_execution_grant`. It is not deserializable from
an employee request. The handle holds a weak runtime-registry reference and
rechecks the parent's original credential/job authority, work-scope cancellation
and deadline. Parent liveness is checked again after awaiting authorization.
A separate valid child credential cannot preserve access after the parent's
credential is revoked. The existing `ExecutionAuthority` interface composes
these checks with the child's own credential/context/identity checks and stored
grant validation; no second execution owner or permission system is introduced.

Ordinary grant validation refuses derived grants. Managed root admission also
rejects a stored child grant, even if passed an otherwise valid inherited
authority. Governed child admission still fails closed pending execution-budget
integration. Existing trusted, unscoped runtime paths are not expanded here.

## Validation

- Storage regressions cover exact job/scope matching, attenuated permissions,
  private-context denial, ancestor revocation, ownership fencing, lifecycle,
  duplicate concurrent delivery, transaction rollback and restart handling.
  Nested lineage uses explicit durable storage fixtures, not live child execution.
- The application integration test runs a real managed parent through the
  existing compute/broker path against a local test inference server. It derives
  child permission, denies an independent-root launch, revokes the parent's
  credential and observes inherited denial plus a canceled parent. There is one
  provider request and no child run. No external model quality is claimed.
- `cargo test -p tetonic-memory -p tetonic-app --lib --quiet`: **155 memory and
  190 application tests passed** before the additional version-55 upgrade test.
- `cargo test -p tetonic-run --test managed_service_tests --test execution_claim
  -j 2 --quiet`: **34 passed**, including both new parent-handle scope/lifetime
  and cancellation-during-authorization regressions. Existing governed-child
  denial, activation retry, deadline and execution-claim tests remain green.
- `cargo test -p tetonic-memory --lib upgrading_version_55_preserves_roots
  --quiet`: **1 passed**, bringing the memory coverage to 156 distinct passing
  tests across the full suite and this subsequent addition. The new case checks
  the version-55 upgrade, preserved root authority/allowance, no automatic child
  permission, explicit derivation after upgrade and creation of a backup directory.
- `git diff --check`: passed. No unrelated changes were staged or discarded.
- The first application integration run expected a failed outcome on revocation;
  the existing runtime correctly reports cancellation. The assertion was fixed
  to match that contract and the test passed.

## Next execution slice

1. Bind own-effort reservations to durable run/task/attempt ownership, including
   the orchestrating parent's work, retries and duplicate submissions.
2. Enforce the reserved limit before model/tool effects. Provider-reported token
   usage is not a hard aggregate spending boundary. No automatic refunds on
   cancellation, unknown effects or restart are implied by these grants.
3. Admit children through the existing managed execution owner using the derived
   authority, inherited lifetime and reconciliation path. Prove cancellation and
   ownership fencing while real child effects are in flight.
4. Enable authorized plan dispatch only after those gates pass, then project
   actual contributors and activity into the existing team-work map.

`execution_available` remains false. No live preview database was migrated or
runtime restarted in this substep. Changes remain local in the mixed worktree.
