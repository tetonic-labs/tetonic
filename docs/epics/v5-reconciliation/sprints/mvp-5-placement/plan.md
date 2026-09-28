# Sprint 5 — Enrolled workstations and remote execution

Status: **exited** (local MVP preview). Schema 50 adds org-scoped workstations with distinct device credentials, owner-approved resource grants, opt-in shared assignment, work placement pins, durable assignment generations and claim fencing. Team membership alone never grants laptop access. Offline parks pinned work in place (no upload/move); reconnect keeps generation and does not duplicate work. Drain refuses new claims; revoke bumps generation, clears grants, parks pins and fences in-flight claims. `activate_team_work` denies when a pin targets an offline/draining/revoked device. ResourceService + `tetonic control workstation` expose enroll/grant/pin/offline/reconnect/drain/revoke. Existing fabric estate enrollment remains for Infer workers and is not a second work-assignment authority.

Deferred: full fabric TLS/lease cutover for harness workers; multi-controller HA (sprint 7); D06 prototype ownership deletion after broader parity; unsupported checkpoint/migration catalog polish. Depends on sprints 1–4. See [implementation progress](../../progress.md).

## MVP-501 — Enroll and operate a workstation as an execution location

Add organization sign-in/enrollment with distinct revocable device credentials and outbound connection. Require owner-approved local resource grants; shared assignment is opt-in. Make placement constraints visible in team setup. Keep inference location independent, with explicit data-disclosure policy.

Acceptance: joining a team does not grant laptop access; another employee cannot schedule unauthorized work; sleeping/offline device parks pinned work instead of uploading/moving it; credential revocation has defined effect; reconnect does not duplicate tasks. Platform/OS support is explicitly listed.

## MVP-502 — Integrate remote ownership and recovery

Use durable assignment generations, authenticated worker capabilities, admission, drain and accepted-result checks. Reuse appropriate fabric/TLS/lease components without pretending Infer workers run agent harnesses. Workers access authorized inference/tools/data directly; centralized APIs do not proxy token streams. Test partitions and clock skew.

Acceptance: stale workers cannot obtain new mediated effects or commit outcomes after fencing; controller restart does not reset generations; external effect acknowledgment loss triggers reconciliation/unknown outcome; remote stop reports true confirmation state. Document unsupported checkpoint/migration cases.

Reuse: REC-401/402; replace D06 prototype ownership after parity. Production multi-controller correctness is exercised in sprint 7 before HA claims.

Commit discipline: characterization/contract, replacement behavior, caller cutover, then gated removal. Record focused tests and remaining compatibility obligations.
