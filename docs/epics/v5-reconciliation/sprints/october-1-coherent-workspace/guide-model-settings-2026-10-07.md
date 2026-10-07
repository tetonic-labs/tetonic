# Choose the Guide's model

Date: October 7, 2026. Baseline: `1519e510`.

## Product behavior

The Guide now has editable model settings in the existing Agents view, with direct **Guide model** shortcuts from the map composer and planning conversation. The owner can choose Ollama, OpenAI, Anthropic or Google, discover account models, save a provider key through the existing credential-store flow, and set per-reply limits. The conversation draft survives visiting settings and returning.

The Guide retains its identity and planning purpose. Model choice does not change worker agents, their capabilities, budgets or existing assignments. Its form contains model/connection settings and reply limits; it does not offer arbitrary instructions, workspace tools or team reassignment. The existing map and work-panel layout remain intact.

Hosted consent explicitly describes the conversation, workspace activity summaries, agent capabilities, plans and inspected work results sent to the selected provider. This uses provider API credentials and their billing, not a ChatGPT or other consumer subscription. Native Codex/Claude Code harness integration is not part of this slice.

## Existing systems reused

- Registered agent identity, revision editing, expected-definition conflict checks and idempotent edit receipts persist the choice. Bootstrap preserves it across reopening the engine.
- Existing provider discovery, OS credential storage, hosted inference adapters, egress guard, secret scanning and token accounting serve the Guide. No additional inference registry or planning runtime was introduced.
- `work_plan` still requires its host-bound conversation, turn and revision. The hosted execution check now permits this bounded internal control with prompt consent, while continuing to deny workspace roots, MCP, dispatch, human-handoff mixing, child execution and unbound planning authority in this path.
- Existing versioned briefs/plans and the owner's **Start this plan** action remain authoritative. A Guide tool call saves a proposal; it does not start a team.
- A running reply retains its admitted model/settings. Later replies use the saved revision. Removing a key produces a setup error with no local fallback.
- Model/credential validation occurs before a new work record is created, avoiding phantom “starting” work when setup is incomplete. Confirmed retries still return the original run before preflight.

## Validation

The managed provider fixture executes inspect → propose → ordinary text completion through all three hosted wire adapters with Ollama offline and a workspace folder present. It verifies saved proposals, no worker dispatch or separate generation run, no ambient file/shell/MCP tools, unchanged identity, restart persistence, stale-edit rejection, repeat-save/repeat-submit identity, active-turn limit pinning, explicit consent, and no fallback or phantom work after key removal. Existing tests continue to cover local Guide execution and proposal scope.

UI checks cover direct settings access, preserved conversation drafts, hidden managed fields, explicit planning-context consent, saved provider/model payloads, ordinary agent tool-edit behavior, and missing-key recovery without submission.

Full application tests: 233 passed, 3 existing opt-in live tests ignored. Frontend suite: 159 passed before the final missing-key UI check; the final focused shaping checks (6 tests) and TypeScript check passed. Application Clippy with warnings denied, CLI build and frontend build passed.

The rebuilt local engine runs on port 3004 with the existing workspace, connected to Vite on 5177. Browser verification opened settings directly from the map, saved the current local model/limits successfully, and reopened the saved Guide. The local model choice was preserved; no hosted credentials or paid inference were used in the manual check. UI evidence is retained locally at `.lokai/manual-testing/guide-model-settings.jpg`.

These provider tests use controlled transports. They prove runtime integration and authority boundaries, not current live account/model availability or planning quality. No paid hosted inference was used. The prior local-model planning limitations remain model-dependent; configuring a different Guide makes that path available without replacing the orchestration architecture. This slice does not close the full October MVP sprint.
