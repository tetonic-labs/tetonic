# Teams use the chosen coordination model

Date: October 7, 2026. Baseline: `96091352`.

## Product behavior

The approved-plan coordinator now uses the provider and model selected for the Guide. Previously, Guide planning could use a hosted model while plan execution still required the engine's default Ollama model. A fully hosted team can now start with Ollama offline.

The existing plan view displays the coordination model next to the start controls. **Change model** opens the existing Guide settings. Missing credentials, unavailable local models and unavailable saved tools produce readiness messages with **Review agent setup** shortcuts. These checks do not perform inference. There is no new screen, map redesign, provider registry or scheduler.

Before a hosted start, the owner approves sending the shared brief, plan, contributions and plan clarifications to the displayed provider. The start command carries that exact provider/model. A changed choice requires review; consent does not silently move to another destination. Agreement and start remain the existing recoverable two-operation flow. An uncertain start retains its original request, model and consent across closing the panel.

The coordination model is pinned for each execution. Editing the Guide affects later plans, not an active one. Each worker continues to use its own saved model, granted tools, workspace disclosure and execution limits. The coordinator retains its separate managed role and plan allowance; copying the Guide's model does not copy its planning capabilities or enlarge its budget.

## Architecture reused

- The registered coordinator definition stores inference preferences; the existing execution receipt's `coordinator_digest` pins that revision. No new persistence table or migration is needed.
- Worker and coordinator inference share the existing provider/egress resolver. Hosted adapters, OS-backed credentials, disclosure checks, managed admission, authority, accounting and traces remain in the existing execution path.
- The coordinator's host-bound `dispatch_assignment` control selects pre-authorized plan assignments. It cannot grant arbitrary tools. The managed executor permits this internal control with hosted prompt consent, while retaining root-binding checks and separately enforcing each worker's grants and disclosure.
- Existing dependency scheduling and capacity handling dispatch independent agents concurrently. Human questions pause their owning assignment while independent work continues. Contributions return through the existing scoped plan context.
- Readiness reuses agent execution checks and performs at most one local model discovery per plan read. A hosted-only plan does not perform local model discovery.
- A confirmed retry returns the original receipt before requiring current credentials or looking up today's Guide model. A retry naming a different coordinator is rejected.
- Older execution receipts whose definitions did not store inference preferences expose no coordinator model. They are not relabeled with the current default. Legacy local start clients can omit the new choice fields; hosted starts require an explicit model and consent.

## Validation

One end-to-end fixture runs the managed coordinator through OpenAI, Anthropic and Google wire adapters, with Ollama pointing to an unreachable endpoint. Two workers use different providers. A barrier requires both workers to reach inference concurrently. One reads an actual temporary file and requests a human answer; the other calls the fixture MCP server and completes while the first waits. After the answer, the coordinator receives both actual contributions and produces the combined fixture result.

Assertions cover exact agent revision pins, shared managed-run lineage, no private conversation canary in requests, no ambient write/shell grants, missing-consent rejection, stale-model rejection, no execution receipt on missing worker credentials, unchanged active model after editing the Guide, duplicate-start identity and persistence after reopening the database without live credentials. The provider transport is controlled: these tests prove integration, not model intelligence, account availability or paid-provider behavior.

Frontend checks cover model disclosure, direct settings navigation, hosted consent, consent reset when the choice changes, the narrower agreement payload, retained destination/consent after a lost start response and actionable setup failures. Existing tests cover plan stop, dependency handling, budget accounting, local execution, hosted worker tools and human handoff.

Validation results: 234 application tests passed, 3 existing opt-in tests ignored; all 163 frontend tests passed. The final focused plan tests (17) and TypeScript check passed after removing duplicate readiness messages. Application Clippy, including tests, passed with warnings denied. The CLI and production frontend builds passed; Vite retained its existing large-chunk advisory.

The rebuilt local engine is running on port 3004 against the existing manual workspace, with the UI on 5177. Browser verification confirmed the saved draft shows `qwen3.5:latest · Ollama`, its setup is ready, and **Change model** opens the saved Guide editor. The draft remains unstarted and the local model setting is preserved. The existing map and conversation layout remain intact. Local UI evidence: `.lokai/manual-testing/hosted-team-coordination.jpg`.

## Remaining boundaries

This uses Tetonic's general harness and existing provider adapters, not native Codex/Claude Code harness execution or consumer-plan billing. The coordination allowance still follows the existing plan ceiling (at most 4,096 tokens on this host); selecting a more capable model does not expand it. Live frontier-provider quality and larger-plan behavior remain to be evaluated with a deliberately bounded real-provider run. No paid inference was used in this slice. The October MVP remains open.
