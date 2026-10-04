# Sprint 1 implementation baseline

Recorded October 4, 2026. Starting revision: `c1c64eb`, branch `main`. The checkout already contains substantial modified and untracked local-engine and product work from earlier development. It has been preserved. This record distinguishes newly run checks from historical local-preview exits.

## Proposed execution profile

Primary validation profile: Windows, one local owner, loopback UI API, existing SQLite resource/run storage, Tetonic general harness and the installed Ollama `qwen3.5:latest` model. Explicit file tools operate within a selected test workspace. This is a candidate profile until actual-model and boundary checks pass; it is not a packaged release or a claim of arbitrary process isolation.

Hosted agents retain prompt-only compatibility. Workspace-tool disclosure through hosted providers is not supported by the current execution contract. Shell, arbitrary MCPs, remote workers and shared-human sessions are outside this first profile. No paid provider request or model download is required for the baseline.

At startup the machine had approximately 55.5 GB free disk space. Ollama and the local UI ports were not listening. Ollama has since been started with its existing installed models for bounded validation. The running browser URL alone was not evidence of a connected engine.

## Fresh checks before implementation

| Check | Result |
|---|---|
| `npm run build` in `web` | Failed: missing GraphNode type import and unused imports/parameters in the experimental work views |
| `node node_modules/vitest/vitest.mjs run --no-cache` in `web` | Passed: 132 tests in 21 files |
| `cargo test -p tetonic-app --lib local_workspace -- --test-threads=1` in `engine` | Passed: six focused local-workspace tests |

These tests do not prove the complete browser journey or actual model usefulness. Fixture-based engine tests are recorded separately from real inference.

## Initial implementation findings

- Empty tool selection was sent as an omitted field and expanded to the host's default tools. Fix at both the UI and creation API; reject unavailable tools rather than silently changing a grant.
- Connected setup offered shell/recall and hosted file-tool choices that the runtime cannot honor. Project host capabilities into setup and preserve the existing hosted disclosure boundary.
- `tetonic ui` implicitly granted its current directory when no workspace was specified. Require an explicit folder argument for filesystem access.
- Approval UI showed success before acknowledgement and called both its own resolver and its parent's resolver. Give the mutation one owner and await the engine receipt.
- The approval API exposes a digest without inspectable effect details. Disable approval of that incomplete representation; an opaque digest is not a command or evidence of consent.
- Main UI lifecycle, example fallback, disconnected map activity and duplicate work surfaces still require consolidation. Passing the current component tests does not close OCT-103 through OCT-105.

## Outstanding baseline evidence

Actual file-based model execution, focused tool-boundary tests, current architecture checks and a clean-install package remain to be recorded. Pilot participants for sprint 3 have not been arranged; no outreach has been sent. The source launcher is not a release installer. OCT-101 stays in progress until its missing evidence and pilot arrangements are resolved; implementation can proceed independently.
