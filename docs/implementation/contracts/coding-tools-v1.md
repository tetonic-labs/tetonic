# Coding tools — v1 (clean-slate design)

**Status:** Proposed (new codebase). The tool surface the agent loop is allowed to call.
**Owner:** `engine/crates/lokai-tools` (Rust) + tool schemas consumed by `lokai-inference` (constrained decoding) and surfaced over `agent-rpc-v1`.
**Relates to:** `pivot-agentic-code-editor.md` §5; approval/diff flow in `agent-rpc-v1`; all FS/shell access is workspace-scoped (see §Sandboxing).

## Why this exists

This is the heart of "make a *local* model good at tool use." A weak model fails when given many fiddly tools with loose schemas. This contract fixes a **small, forgiving, strictly-typed tool set** whose JSON schemas are the exact grammars fed to constrained decoding — so the model *cannot* emit a structurally invalid call, and the few semantic mistakes it can still make (bad path, ambiguous edit) come back as corrective errors it can recover from.

Two properties are non-negotiable:
1. **Every tool is defined once, in Rust, as a typed struct deriving `schemars::JsonSchema`.** The same definition produces (a) the JSON Schema used for constrained decoding, (b) the validated deserialization target, and (c) the docs the model sees. No drift.
2. **The tool set is small.** v1 is exactly nine tools. Adding tools is a deliberate decision, not a reflex — surface area is the enemy of local-model reliability.

## The tool set (v1)

| Tool | Class | Approval | Summary |
|---|---|---|---|
| `read_file` | read | auto | Read a file (optionally a line range). |
| `list_dir` | read | auto | List a directory (gitignore-aware). |
| `grep` | read | auto | Regex content search (ripgrep engine). |
| `glob` | read | auto | Find files by glob (gitignore-aware). |
| `edit_file` | write | audit diff | Replace an exact, unique string in a file. |
| `write_file` | write | audit diff | Create/overwrite a whole file. |
| `run_shell` | exec | **always prompted** | Run a shell command in the workspace. |
| `finish` | control | n/a | Signal the task is complete (with a summary). |
| `ask_user` | control | n/a | Ask the user a clarifying question and pause. |

- **read** tools may be auto-approved per the approval policy (`agent-rpc-v1`).
- **write** tools apply immediately; the daemon emits `event/diff` for audit/review in the UI but does **not** block the write behind per-edit approval (SEC-009). Use checkpoints/undo for recovery.
- **exec** (`run_shell`) is **always** gated by an `event/approval_request`.

## Schemas (illustrative)

Defined as Rust types; `schemars` emits the JSON Schema. Shown abbreviated:

```rust
/// Read a UTF-8 text file from the workspace.
pub struct ReadFile {
    pub path: String,                 // workspace-relative
    pub start_line: Option<u32>,      // 1-based, inclusive
    pub end_line: Option<u32>,
}

/// Regex search over file contents (ripgrep semantics).
pub struct Grep {
    pub pattern: String,              // Rust regex syntax
    pub path: Option<String>,         // subtree to search; default = workspace root
    pub glob: Option<String>,         // optional file filter
    pub case_insensitive: Option<bool>,
    pub max_results: Option<u32>,     // server caps regardless
}

/// Replace ONE exact, unique occurrence of `old` with `new` in `path`.
pub struct EditFile {
    pub path: String,
    pub old_string: String,           // must match EXACTLY once
    pub new_string: String,
}

/// Run a shell command in the workspace (ALWAYS user-approved).
pub struct RunShell {
    pub command: String,
    pub cwd: Option<String>,          // workspace-relative; default = root
    pub timeout_secs: Option<u32>,    // server caps (e.g. 600)
}

/// End the task.
pub struct Finish { pub summary: String }
```

The **outer call envelope** (what constrained decoding actually produces) is a tagged union so the grammar forces a valid tool name *and* valid args together:

```jsonc
// Conceptual; generated from a Rust enum with #[serde(tag = "tool", content = "args")]
{ "tool": "edit_file",
  "args": { "path": "src/lib.rs", "old_string": "...", "new_string": "..." } }
```

`lokai-inference` passes this schema to Ollama's `format` field (XGrammar/GBNF). Result: the model literally cannot emit an unknown tool name or a missing required arg.

## `edit_file` semantics (the careful one)

String-replace is the most reliable edit primitive for weak models, but only with strict rules:

- `old_string` MUST match **exactly once** in the file.
  - **0 matches** → error `no_match` (with a hint: nearest fuzzy line).
  - **>1 matches** → error `ambiguous_match` (with the count and a request to add surrounding context).
- **TOCTOU guard (AR1-5):** the daemon re-reads the file immediately before write and re-validates the match count. If the file changed between the model's read and the write, the edit fails with `no_match` and a message to re-read — no silent merge.
- Whitespace/indentation is significant and preserved.
- An empty `old_string` is rejected (use `write_file` to create).
- On success, the daemon computes the diff and emits `event/diff` for audit; the change is applied **immediately** (not held for per-edit approval — see SEC-009).

This turns the model's most common failure (vague edits) into a **recoverable, self-correcting** loop rather than a silent wrong edit.

### Python syntax check (optional revert)

After `edit_file` / `write_file` on `.py` paths, the daemon may run `python -m py_compile`. On failure the model sees a structured `syntax_error` result (default: broken file stays on disk so the model can fix it). Set `LOKAI_REVERT_ON_SYNTAX_ERROR=1` to restore pre-edit content instead.

## Multi-file mutations (no transaction)

Each tool call is an independent FS operation. There is **no cross-file transaction**: a turn that edits `a.rs` and `b.rs` can leave `a.rs` written while `b.rs` fails — the model must recover on the next turn. Checkpoint/undo in `lokai-memory` can restore prior snapshots per file but does not auto-rollback sibling edits. Undo-on-verify-fail at `finish` remains a future spike.

## Audit vs filesystem ordering

File changes are persisted to the audit store **after** a successful write. A crash between FS write and audit insert can leave disk ahead of the timeline — accepted for v1 homelab; write-ahead logging is deferred (AR1-5 WS5).

## Tool results

Every tool returns a typed result that becomes the `event/tool_result` payload *and* the message fed back to the model:

```rust
pub struct ToolResult {
    pub ok: bool,
    pub summary: String,        // short, model-facing ("3 matches", "edit applied")
    pub content: Option<String>,// file text, search hits, shell output (truncated)
    pub error: Option<ToolError>,
}

pub enum ToolError {
    NotFound, NoMatch { hint: String }, AmbiguousMatch { count: u32 },
    OutsideWorkspace, Denied, Timeout, TooLarge { limit_bytes: u64 }, Other(String),
}
```

- **Errors are first-class and verbatim-fed-back.** A failed tool call is *not* fatal; the model sees the structured error and retries. This is the single most important reliability mechanism after constrained decoding.
- **Output is bounded.** Large files/searches/shell output are truncated with an explicit `[truncated N more lines]` marker and a hint to narrow the query. Local models degrade with long context, so caps are a feature.

### Tool-output discipline (token economy)

Tool results are the **biggest hidden token sink** in the loop, and on local hardware every token is wall-clock the user feels (`performance-and-scale` §4.2). So results return **windows + summaries + handles**, not full dumps:

- `read_file` returns the requested **range** (default a bounded window), plus a `more` handle (`{ next_offset, total_lines }`) so the model can fetch the next slice on demand instead of receiving the whole file.
- `grep` returns **match lines with tight surrounding context**, capped, with a count of suppressed matches — never the full file.
- `list_dir`/`glob` are capped and summarized when large.
- Prefer a **structural outline** over a full file when the model only needs shape (function/class signatures), once the index lands.

The principle: the tool returns *enough to decide the next action* and a way to ask for more — not everything it could. This keeps the model's context dense with signal and the prefix cache intact.

## Sandboxing (safety + privacy)

- **Workspace-scoped FS.** All `path`/`cwd` are resolved relative to the workspace root and **canonicalized** for existing components; any path escaping the root (`..`, symlink, junction, absolute) → `OutsideWorkspace`. On Windows, directory junctions are followed the same way as symlinks when canonicalizing.
- **No network from Rust HTTP clients in tools.** Tool code does not open sockets directly. **`run_shell`, verify-at-finish, LSP, and git subprocesses are NOT filtered by EgressGuard** (AR2-3) — they inherit a minimal env allowlist but can reach the network unless the OS blocks it. Treat shell tools as full user privilege.
- **`run_shell` is the sharp edge.** It is user-approved (daemon/CLI), runs with the workspace as cwd, has a wall-clock timeout, and uses a minimal environment. `approval/respond.remember` persists **exact** rules; prefix rules require a following space (e.g. `pytest*` matches `pytest -q`, not `pytest; evil`).
- **Verify-at-finish** runs argv-only allowlisted commands (`cargo test`, `python -m pytest`, `python -m py_compile`, workspace scripts) without a shell; it uses the same approval hook as shell when configured (kind `verify_finish`).
- **Checkpoints.** Before a batch of writes, the daemon snapshots affected files so an undo can restore prior state (Phase D).

## Read tools are native, not subprocesses

`grep`/`glob`/`list_dir` are implemented with **ripgrep's own crates** (`grep`, `ignore`, `globset`) in-process — gitignore-aware, fast, no `rg` binary dependency, no subprocess to sandbox. `read_file` is direct FS. This keeps the read path entirely inside the daemon and the trust boundary.

## Compatibility & extensibility

- Additive-only within v1: a tool may gain new **optional** args; results may gain fields. Consumers tolerate unknowns.
- Adding/removing a **tool** or changing an arg from optional→required is a **v2** change (it alters the grammar the model was trained-in-context against) and requires a deliberate decision.
- Candidate future tools (kept *out* of v1 on purpose): `apply_patch` (multi-hunk), `rename_symbol` (tree-sitter-assisted), `run_tests` (structured), `read_many`, and **index-backed structural lookups** (`find_definition`, `find_references`, `outline`) once `lokai-index` (`code-index-v1`) lands — these let the agent resolve "where is `X`" *exactly and for free* instead of burning tokens on `grep`. Each must justify its reliability cost before landing.

## Mission alignment

| Principle | How honored |
|---|---|
| Capable | Constrained decoding + small forgiving tools + verbatim error feedback = local models that actually complete multi-step edits. |
| Private by architecture | FS tools are workspace-scoped; **subprocess tools are not network-sandboxed** — EgressGuard applies to Rust HTTP only. |
| Inspectable & reversible | Every call/result is an event; writes are diffs the user approves; checkpoints enable undo. |
| Sovereignty | Workspace-scoped, deterministic, offline — no tool depends on anything outside the user's machine. |

---

**Last updated:** 2026-06-28 (AR1-5: TOCTOU re-read, symlink canonicalize, syntax revert flag, batch/audit notes).
