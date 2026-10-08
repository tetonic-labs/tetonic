# Conversation to execution handoff — October 8, 2026

This slice improves the existing map inspector's delegation journey. It uses the
saved, model-authored proposal and existing plan/agent endpoints; it adds no
prescribed work, automatic grants, or alternative orchestration layer.

## Changes

- A saved proposal or execution opens ahead of conversation history. Ordinary
  discussion stays conversational; continuing or refining work opens the same
  discussion and preserves the unsent draft.
- The proposal shows its outcome, named contributors, deliverables and actual
  dependencies. Detailed instructions and individual allowances are secondary.
- Engine-reported start blockers and proposal questions are visible. Missing
  grants link to the relevant agent; unavailable tools link to the workspace
  library. The Guide can help resolve issues through a prefilled, unsent message.
- Saving or canceling setup entered from work returns to that work. Setup changes
  refresh the existing engine snapshot and the proposal reloads its readiness.
- Starting still uses revision-bound agreement, destination consent, saved
  permissions and idempotent start receipts. A confirmed start refreshes the map.
- Execution distinguishes current workers, unresolved dependencies, waiting to
  start and completion without a recorded combined answer. Activity motion stops
  when disconnected and respects reduced-motion preferences.

## Validation

- Production web build passed (existing bundle-size warning remains).
- Full web suite: 201 tests passed. Follow-up focused checks: 38 passed.
- Added coverage for contextual resolution without dispatch/grants; draft
  preservation; return after agent setup; active parallel workers; resolved
  dependencies; and missing combined output. Existing uncertain-start, consent,
  cancellation and recovery tests remain passing.
- Browser review against the running engine verified proposal-first layout,
  collapsed history, allowance presentation and refinement into the same composer.
  No model request or plan execution was launched during browser inspection.

The existing saved examples were used only to check rendering and navigation.
They are not evidence of orchestration quality or user value. A new real-goal,
frontier-model execution and user usefulness assessment remain separate work.
