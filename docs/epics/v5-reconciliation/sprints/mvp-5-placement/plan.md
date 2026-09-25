# Sprint 5 — Enrolled workstations and remote execution

Status: planned. Depends on the preceding MVP sprint; security and bounded admission are enforced incrementally, never deferred until final hardening. Tickets may be split into smaller implementation commits without weakening exit criteria.

## MVP-501 — Enroll and operate a workstation as an execution location

Add organization sign-in/enrollment with distinct revocable device credentials and outbound connection. Require owner-approved local resource grants; shared assignment is opt-in. Make placement constraints visible in team setup. Keep inference location independent, with explicit data-disclosure policy.

Acceptance: joining a team does not grant laptop access; another employee cannot schedule unauthorized work; sleeping/offline device parks pinned work instead of uploading/moving it; credential revocation has defined effect; reconnect does not duplicate tasks. Platform/OS support is explicitly listed.

## MVP-502 — Integrate remote ownership and recovery

Use durable assignment generations, authenticated worker capabilities, admission, drain and accepted-result checks. Reuse appropriate fabric/TLS/lease components without pretending Infer workers run agent harnesses. Workers access authorized inference/tools/data directly; centralized APIs do not proxy token streams. Test partitions and clock skew.

Acceptance: stale workers cannot obtain new mediated effects or commit outcomes after fencing; controller restart does not reset generations; external effect acknowledgment loss triggers reconciliation/unknown outcome; remote stop reports true confirmation state. Document unsupported checkpoint/migration cases.

Reuse: REC-401/402; replace D06 prototype ownership after parity. Production multi-controller correctness is exercised in sprint 7 before HA claims.

Commit discipline: characterization/contract, replacement behavior, caller cutover, then gated removal. Record focused tests and remaining compatibility obligations. No production changes were made by the planning ticket.

