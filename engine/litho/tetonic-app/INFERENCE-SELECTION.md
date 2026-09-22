# Changing inference without changing agent identity

Inference is a replaceable dependency, not the agent's identity. Conversation,
tools, approval policy, workspace, run ownership, and cancellation remain owned by
their existing components.

## Live sessions

`Application::session_inference` reads the current profile, fast/hard models, and
revision. `Application::change_session_inference(ChangeInferenceCommand)` is the
single mutation door used by both terminal and daemon adapters.

Changes are accepted only while the session is idle, and affect the next turn.
Active or closing sessions reject a change; nothing is queued implicitly. Turn
admission and selection changes use the same lock, so a switch cannot split an
active turn across configurations. The expected revision prevents stale callers
from overwriting another edit. Children created during a turn inherit that turn's
execution host. Individual running child agents are not retargeted.

The selection lives once in the live session's turn state. At admission, the
execution host snapshots the model selection and provider/broker pair. Existing
conversation text survives; model-specific token-count caches are invalidated.
Switched sessions use the explicitly estimated heuristic tokenizer rather than
silently retaining a tokenizer for the old model. Context limits for named
profiles are registered by the host; `default` retains the installed context limit.

This selection is **live-session state**. It is not written to persisted session
configuration and is not restored after daemon restart. Reopening/resuming uses
the normal startup selection.

## Provider profiles

`default` selects the currently installed application compute plane. A host may
call `register_inference_profile(name, &ComputePlane, allowed_models, num_ctx)` to
make additional complete compute planes selectable. Registration checks that the
provider is scanner-equipped and points to the supplied broker. Names cannot be
overwritten; publish a new name for a new configuration. Host composition remains
responsible for policy/lifecycle management of every registered plane.
Registration attaches the same supervisor and fabric run bridge used by the
default plane. Session cancellation selects the session's broker as well.

Commands select registered names only. They cannot register endpoints, supply
credentials, widen egress, or construct providers. Named profiles enforce their
model allowlist. `default` accepts syntactically valid model IDs; runtime model
availability is checked by the existing inference path on the next request.

This preserves the existing broker boundary. The hosted adapter is still not a
complete admitted compute plane: hosted broker integration is required before
registering it for production sessions. Never disguise hosted inference as the
local slot of a fabric pool.

## Terminal commands

Open `/model` or press **F5** in the TUI to choose a model. Type to filter by
model or provider, use Up/Down to choose, Enter to apply, and Esc to dismiss.
F5 preserves any unfinished prompt. The picker marks current choices and applies
one model to both tiers. Busy or stale selections show an error without changing
the session; close and reopen the picker to refresh a stale revision.

The application owns `model_catalog` and `select_session_model`. Catalog IDs are
opaque to portals: selection resolves to a registered profile/model and then uses
the same revision-checked inference mutation as advanced commands. Neither core
nor inference adapters know about pickers or catalog IDs.

Host bootstrap supplies the default provider's discovered model inventory using
`set_default_model_inventory`; named profiles contribute their allowed models.
This is a startup snapshot, not a live health check or capability guarantee.
Configured session models remain visible when discovery is unavailable. Labels
distinguish discovered and configured entries. No unconfigured cloud providers
are invented or enrolled by the picker. Hosts can replace the inventory snapshot
after discovery; users can restart to discover newly installed local models.

Advanced commands remain available:

```
/inference
/model MODEL
/model FAST_MODEL HARD_MODEL
/inference PROFILE MODEL
/inference PROFILE FAST_MODEL HARD_MODEL
```

With one model argument, both tiers change. The current tier-routing behavior is
otherwise preserved. `/inference` shows the selection and registered profile names.
An in-progress turn must finish, or be canceled and finish stopping, before a change.

## Daemon RPC

For a picker or simple model selection, call `session/models` with
`{ "session_id": "..." }`. It returns `revision` and `models`, each containing
`id`, `name`, `provider_label`, `availability`, and `current`.
Pass an entry's opaque ID to `session/selectModel`:

```json
{
  "session_id": "existing-session",
  "selection_id": "ID returned by session/models",
  "expected_revision": 0
}
```

The result is the updated inference selection. Unknown selections and stale
revisions are rejected. The advanced APIs below remain supported.

`session/inference` accepts `{ "session_id": "..." }` and returns `profile`,
`model_fast`, `model_hard`, `revision`, and `available_profiles`.

`session/setInference` accepts:

```json
{
  "session_id": "existing-session",
  "profile": "default",
  "model_fast": "chosen-model",
  "model_hard": "chosen-model",
  "expected_revision": 0
}
```

Use the revision returned by the read operation. Failed validation or a busy/stale
session leaves the previous selection intact. These are authenticated session
operations and expose no cloud-enrollment or credential-management API.

## Existing core agents

`Agent::replace_inference(&mut conversation, AgentInferenceBinding)` lets the owning
runtime replace provider, model, context budget, and tokenizer on an already
constructed idle agent. Exclusive mutable access prevents concurrent turns on
that agent. It preserves agent/run identity, tools, hooks, policy, and history;
recounts tool-schema tokens; invalidates history token counts; and clears old tier
and speculative-draft hints. Invalid bindings leave the agent unchanged.

The runtime supplies an already admitted provider. Core does not resolve profile
names or learn endpoint/credential configuration. Session turns currently assemble
their transient execution agents from the selected host, so they do not need to
reach into an active core agent to implement the session command.
