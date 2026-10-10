# Working setup: local folder access

Priority 2 is configuring models, agents, tools and access without confusing
detours. This slice closes the folder-registration detour in EXP-002; it does not
claim completion of the whole priority or supersede the Guide execution work.

## Delivered behavior

- In the existing agent editor, choose Add a working folder, check the path, then
  add and select it. File and terminal capabilities become available once there
  is an approved folder. Select the abilities needed; the form stays mounted.
- Paths refer to the engine computer. Only existing directories are accepted;
  resolved paths are shown before approval. There is no browser upload, host file
  browser or implicit directory creation.
- Registration only expands the workspace's available-folder catalog. Saving the
  agent is a separate revisioned grant. Other agents, running work and budgets do
  not change. A different hosted-model folder requires renewed disclosure.
- Missing or stale selected folders disable agent save with an adjacent next
  action. Canceling setup retains the agent draft. Lost-response retries reuse
  the same request identity; a conflict requires a fresh folder check.
- Host operators can lock registration with `lock_agent_folders`. Startup folders
  retain their authority; UI approvals cannot override the lock.

## Existing owners reused

`workspace/folders` validates paths and resolves agent scope; `WorkspaceServices`
owns configuration; the local owner API supplies authenticated access; the shared
SQLite store owns scoped immutable approval versions and optimistic concurrency.
Saved agent definitions, hosted disclosure, execution grants, the tool broker and
the existing runtime continue to enforce tool access. There is no second agent,
permission evaluator, host-config writer or model-facing management tool.

## Validation

Focused tests cover folder validation, persistence, unchanged existing agents,
actual managed read-tool execution against the chosen test folder, stale writes,
exact retries, host lockdown and hosted disclosure separation. UI tests cover
in-place creation, Enter handling, cancellation, retry identity and disclosure
reset. These use temporary workspaces and deterministic inference fixtures, not
the owner's live data or paid provider requests.

- Rust application, memory and CLI regression: 620 passed, 5 existing ignored.
- Final folder-specific regression: 3 passed, including removal/retry, host
  lockdown, Windows remote/device path rejection and real managed file reading.
- Frontend: 13 Node tests and 296 Vitest tests passed. The run used two workers
  while another agent was compiling against this checkout. Production build passed.
- A fresh temporary engine with no initial folder passed HTTP checks for
  authentication, protected paths, inspection without a grant, publication,
  identical retry, stale-write rejection and immediate tool-catalog refresh.
  Creating a read-only agent against the newly added folder also succeeded.
  No inference calls were made in that smoke test; its process was stopped.
- Package verification passed Clippy, architecture and quality-static checks.
  Its sole finding was rustfmt in another agent's uncommitted Village integration
  test. That concurrent change is excluded from this commit and was not modified.
  The exact staged source tree was exported to a temporary directory and passed
  `cargo fmt --all -- --check` independently.

The existing preview engine and its data were left running unchanged. The new
control requires the updated engine executable; frontend behavior is covered by
component tests, not a claimed full browser/real-provider acceptance run.

## Still open

Full real-provider setup qualification, reference-model default limits, polished
workspace-wide folder removal, and end-to-end first-use acceptance remain open.
Existing agent edits can remove its tools or choose another folder; catalog
changes do not kill an already admitted run. Use existing stop controls for that.
