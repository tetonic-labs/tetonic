# Sprint 6 — A cohesive product and legacy retirement

Status: planned. Depends on the preceding MVP sprint; security and bounded admission are enforced incrementally, never deferred until final hardening. Tickets may be split into smaller implementation commits without weakening exit criteria.

## MVP-601 — Finish the focused team experience

Polish the thin UI developed since sprint 1: guided team setup, private/team conversation, goal/huddle/work cards, approval/clarification inbox, pause/stop controls and evidence inspection. Keep assignments, discussion and authorization distinct. Users direct outcomes without managing per-agent tabs; leadership and scope are understandable.

Acceptance: a prepared installation supports the first-use team scenario without bespoke code or shell orchestration; a team carries out and reports work while UI is closed; an approval wait does not halt the entire team. Run actual-model usability trials separately from deterministic enforcement tests. No voice/document suite or workflow editor.

## MVP-602 — Converge binaries/configuration and close obsolete paths

Complete supported server/client packaging and configuration; preserve compatibility or explicit migration errors for old CLI/RPC. Remove prototype fleet/operator/Keeper authorities, parallel world bootstrap, fake statuses/quotas and unused strategies after their gates. Update semantic architecture checks and release scripts. Record every surviving D item with a concrete compatibility reason.

Acceptance: clean installation and upgrade preserve histories; config validation and redacted inspection work; no supported activation bypasses managed execution; no default repository dependency for noncoding work. Net LOC is not the success metric. Require removal commits and caller checks.

Reuse: REC-501/502 and D01–D15 register. Build required functionality; do not keep old authorities behind permanent flags.

Commit discipline: characterization/contract, replacement behavior, caller cutover, then gated removal. Record focused tests and remaining compatibility obligations. No production changes were made by the planning ticket.

