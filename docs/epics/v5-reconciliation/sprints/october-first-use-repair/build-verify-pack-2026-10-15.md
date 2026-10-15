# Build & verify pack — October 15, 2026

Status: implemented and verified offline. No paid inference, large-project
qualification rerun, or restart of the user's running engine.

## Delivered behavior

The agent editor and workspace tool library now offer **Build & verify**, an
immutable built-in revision containing nine existing/new tools. It adds three
small capabilities to the existing file tools:

- `read_files`: one to four targeted file/range reads, sharing a bounded source
  allowance and preserving per-file continuations, errors and staged visibility.
- `inspect_changes`: private pending changes, compact text diffs, and the actual
  check receipt. Paginated report hashes reject stale continuation. Live baseline
  conflicts are explicit. Inspection neither publishes work nor starts inference.
- `run_checks`: explicit commands against pending edits, using the existing
  shell approval, process executor, staged-check binding and verification receipt.
  Failed checks block delivery; subsequent edits require a new passing check.

The pack includes `read_file`, `read_files`, `list_dir`, `grep`, `glob`,
`write_file`, `edit_file`, `inspect_changes`, and `run_checks`. General Terminal
access remains a separate selection. Existing Read files/Edit files/Terminal
revision 1 memberships and saved agent grants are unchanged. Owners can customize
the pack through the existing workspace pack editor and individual selections.

## Owners and limits

`tetonic-tools` owns schemas, bounded observations and tool dispatch;
`tetonic-transaction` owns the inspected patch and verification state.
Application assembly, disclosure, catalog and approvals extend their existing
paths. No new execution owner, persistence table or provider-specific tool loop.
The existing agent editor presents the pack and command-approval explanation;
the map and product navigation are unchanged.

Commands still require approval and platform confinement disclosures. Checks need
pending file-tool edits, use relative paths, and do not publish generated files.
Dependency/build caches are excluded from the disposable copy. Passing a command
does not certify the complete task. Inspection is not Git status. Read/report
pages bound model-visible content, not whole-file memory use. Code-index/LSP
tools were not added to this pack: their dependencies are not attached to the
current saved-agent runtime.

## Validation

- `cargo test --locked -p tetonic-tools -p tetonic-transaction --lib --quiet`:
  82 tool tests passed, one previously ignored; 12 transaction tests passed.
  Covers source budgets, ranges, protected paths, stale reads, private staged
  visibility, real diff content, report paging, check identity, revocation,
  cancellation, stale approval, failed-check delivery blocking and rechecking.
- `cargo test --locked -p tetonic-app --lib --quiet`: 229 passed, five previously
  ignored. Real created agents selected the pack, staged output, received owner
  approvals, failed/repaired/rechecked, inspected the diff and source, and
  delivered through OpenAI, Anthropic and Google adapters with scripted model
  responses. The local registered path also exercised batch reads and inspection.
- `npm test`: 13 architecture/appearance checks and 333 UI/unit tests passed.
  Vite emitted a dependency-scan shutdown diagnostic during the appearance
  fixture teardown; no test failed.
- `npm run build`: passed; existing large-chunk advisory remains.
- `cargo run --locked -p tetonic-arch-gate -- verify package`: formatting,
  workspace/all-target Clippy, architecture and static quality checks passed.

The running engine must be rebuilt/restarted to expose the new catalog entries.
These offline checks do not replace the deferred real-model project qualification.

## Follow-up: reliable draft repair

The repair cycle exposed defects beneath the pack: `edit_file` required a live
file and always edited live bytes, so newly staged files could not be repaired
and successive edits could discard earlier changes. Repeated staging could also
recapture a peer's newer file as its baseline, defeating conflict detection.
Staging names flattened `/` to `_`, aliasing files such as `src/a.txt` and
`src_a.txt`. Six regression tests reproduced these failures before the fix.

Transaction-owned edits now use the validated pending version, preserve the
original publication baseline, normalize lexical path aliases into one entry,
and enforce file/transaction size limits before replacing pending content.
Staged content names use path digests. Tampered drafts remain errors. Tool
outcomes report the previous pending bytes, and ordinary non-Python edits no
longer copy the whole workspace merely to call a Python-only syntax checker.
Python syntax checks and explicit governed verification remain in place.

The created-agent regression now repairs with `edit_file` instead of rewriting
the whole file. It covers new and existing files through OpenAI, Anthropic and
Google adapters, with both individual tool grants and the Build & verify pack.
Those twelve scripted repair cases passed, as did the negative cases that refuse
publication after a failed check. Transaction and tool regression suites also
passed. A passing check is invalidated by a later edit and must be repeated.

Final follow-up validation:

- `cargo test --locked -p tetonic-app -p tetonic-tools -p tetonic-transaction
  --lib --tests --quiet`: 598 passed, six previously ignored. Includes application
  integration tests as well as provider, transaction and tool unit tests.
- `cargo run --locked -p tetonic-arch-gate -- verify package`: passed formatting,
  workspace/all-target Clippy, architecture and static quality checks.
- `git diff --check`: passed.

This changes transaction/tool behavior, not the Guide's prompt or the UI. No
model API calls, paid qualification attempts, user-workspace writes, or production
engine restarts were performed. The running engine needs a rebuild/restart to
load this follow-up. Real-model large-project qualification remains deferred.
