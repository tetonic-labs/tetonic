# lokai-cli

Phase A headless CLI (`lokai`): single-agent runs, interactive chat, audit/history, time-travel, code index, project memory, orchestration, and `lokai estate` fleet commands.

Package name is **`lokai-cli`**; the binary on PATH is **`lokai`**.

## Entry points

| Command / flag | Purpose |
|----------------|---------|
| `lokai "task"` | One-shot agent run |
| `lokai` (no task) | Interactive multi-turn chat |
| `lokai --explain "…"` | Force read-only explain mode (no edits, verify, or critic) |
| `lokai --orchestrate auto` | Keyword router + specialists + optional critic (D11/D12) |
| `lokai --orchestrate auto --no-critic` | Specialists without post-edit critic |
| `lokai --index`, `--search`, etc. | Code index operations |
| `lokai --checkpoint`, `--undo`, `--redo` | Time travel |
| `lokai --project-status`, `--project-note` | Project memory (D4) |
| `lokai estate …` | Worker enrollment / fleet (N0.1) |
| `lokai estate worker trust get <worker>` | Show persisted M5-3 trust and audit history |
| `lokai estate worker trust set <worker> <tier>` | Persist a globally versioned coordinator-owned trust assignment |

## Modules

| Module | Purpose |
|--------|---------|
| `main.rs` | Argument routing, transport preflight, session bootstrap |
| `app_kernel.rs` | `TerminalEventSink`, `TerminalRenderer`, approval coordinator |
| `session.rs` | `CliSessionConfig`, `CliTurnContext`; live session via `SessionLiveStore` |
| `signal.rs` | One-shot: Ctrl+C → live cancel + `SessionService::cancel_session`. TUI: Ctrl+C cancels the turn; Ctrl+Q / `/exit` quit. |
| `chat.rs` | Interactive / one-shot turns via `Application::submit_chat_turn`; TUI slash palette |
| `tui/` | Interactive chat. Left pane is a **transcript** (`you` / `lokai` / `tool` / `error`); telemetry is Activity (Ctrl+L). Status names the phase. Approvals default to Deny; arrows select and Enter confirms; Esc denies. Opening `lokai` resumes only when the last workspace session was left **running** (crash / incomplete turn). A finished chat starts fresh. |
| `app_bootstrap.rs` | Shared `Application::bootstrap` for estate subcommands |
| `args.rs` | Clap CLI surface |
| `offline.rs` | History, time-travel, index, project commands (infra) |
| `estate.rs` / `capacity.rs` | Estate subcommands; status/doctor via `lokai-app` services |

Agent turns submit through `lokai-app::Application::submit_chat_turn` (product owns Conversation and the turn future). Session start/end/cancel use the same `DefaultSessionService` + `SessionLiveStore` as `lokaid`. The CLI is transport: it submits, awaits or subscribes, relays approval responses, and renders.

## Interactive TUI

Press **F5** or enter **`/model`** to open the model picker. Type to search by
model or provider, use Up/Down to choose, and press Enter to apply. Esc closes it;
F5 preserves your unfinished prompt. Current models are marked, and availability
labels distinguish startup discovery from configuration. The picker selects one
model for both tiers. Newly installed local models appear after restarting.

Use `/model MODEL` to change both model tiers in the existing idle session, or
`/model FAST_MODEL HARD_MODEL` to set them separately. `/inference` shows the
current selection and host-registered profiles; `/inference PROFILE MODEL`
switches profile and model together. Active turns reject changes. Selection is
live-session state, preserved across turns but not across process restarts.
See [inference selection](../../product/lokai-app/INFERENCE-SELECTION.md).

The conversation is the default view. The optional sidebar has a direct toggle;
explicit slash reports open it automatically. Assistant responses use a separate
blue reading palette, with a role band that also covers wrapped lines. Older
responses have quieter backgrounds. The footer emphasizes only the primary action.

The composer grows to six rows, wraps with the caret, and preserves multiline
pastes without sending them. Ctrl+J and Alt+Enter insert newlines; Shift+Enter is
also accepted when the terminal reports it. Enter sends. During generation, Enter
keeps the draft and explains that the user must send it after the turn finishes.
Prompt history restores both the original draft and its caret on return.

Scrolling up freezes a reading snapshot. New output continues in the live session
and is announced above the conversation; Ctrl+End resumes live output. Reaching
the bottom with PageDown also resumes it. Focused panes have labeled borders;
mouse-wheel scrolling targets the hovered pane. Modals receive their own scrolling.

| Key | Action |
|-----|--------|
| Enter | Send a message, or confirm the selected approval |
| Ctrl+J / Alt+Enter | Insert a newline |
| Up / Down | Move within a multiline draft; browse history for a single-row draft |
| Alt+Up / Alt+Down | Browse prompt history from any draft |
| Left / Right / Home / End | Move the caret / jump to line boundaries |
| Tab / Shift+Tab | Cycle composer, conversation, and visible sidebar focus |
| Tab after `/` | Complete commands |
| Esc | Return to composer; close an overlay; deny an approval |
| PgUp / PgDn | Scroll the focused pane (composer focus scrolls conversation) |
| Ctrl+Up / Ctrl+Down | Scroll conversation one row |
| Ctrl+P / Ctrl+N | Previous / next conversation turn |
| Ctrl+End | Return to live output |
| Ctrl+E | Show / hide sidebar |
| Ctrl+L | Open sidebar and toggle Activity / Context |
| F6 | Expand / restore inspector |
| `[` / `]` | Scroll inspector when it has focus |
| Ctrl+O | Expand / collapse thoughts |
| Ctrl+T | Collapse / reveal tool updates (failures remain visible) |
| F1 | Scrollable keyboard help |
| F2 | Presentation preferences |
| F3 | Copy view for the visible assistant answer; Tab selects answer / code blocks |
| F4 | Toggle app mouse handling to allow native terminal selection |
| Ctrl+C | Cancel the in-flight turn; idle shows a hint |
| Ctrl+Q / `/exit` | Quit |

Approval dialogs preserve the draft and initially select **Deny**. Left/Right or
Tab selects an option; Enter confirms. Typing letters and pasting cannot approve.
High-risk confinement hides the persistent Always option. Decision controls stay
separate from the scrollable details.

F2 preferences include reduced motion, terminal-native colors (no RGB), tool
collapsing, sidebar visibility, and mouse handling. They are saved to `tui.json` in
the OS user configuration directory. `NO_COLOR` enables terminal-native colors at
startup. Font and text size remain terminal settings. No special font is required
for the ASCII waveform; reduced motion removes it. Completion is recorded in the
conversation, with elapsed time when available.

F3 preserves the original Markdown and extracts fenced code blocks for copying.
Enter sends an OSC52 clipboard request; support and permission depend on the
terminal/multiplexer. The UI does not claim clipboard success without acknowledgment.
Mouse capture is temporarily disabled in this view so native selection and the
terminal's Copy command remain available. PgUp/PgDn scroll; Esc restores the prior
mouse setting. Screen-reader behavior still requires validation in real terminal hosts.

Assistant Markdown supports headings, emphasis, lists, code fences, quotes, links
with visible destinations, and responsive tables. This is a lightweight renderer,
not a full CommonMark or syntax-highlighting engine.

## Command → service map (R3)

| Command / path | Application service or infra |
|----------------|------------------------------|
| `lokai "task"` / interactive chat | `admit_chat_turn` then `RunService::run_turn`; Infer uses the shared `lokai-app` compute plane. Degraded saved profile warns and proceeds. |
| `lokai estate status` / worker add/remove | `EstateService` |
| `lokai estate capacity doctor/status/profiles` | `CapacityService` |
| TUI `/doctor` `/capacity` `/status` | `CapacityService` + session resume state. Shows **this chat's `--model`** vs the saved optimize profile. |
| TUI `/ps` `/evict` | `EgressGuard` HTTP to local Ollama (same pin as inference) |
| TUI `/egress` | estate store + live `EgressGuard` reload |
| `--index` / `--embed` / `--search` | **Infra leftover:** `lokai-index` + local embed HTTP, not Infer/chat compute plane |
| `--checkpoint` / `--undo` / `--redo` | **Infra leftover:** `lokai-memory` checkpoints, not session orchestration |
| `--project-status` / `--sessions` | **Infra leftover:** audit-store reads |
| TUI `/optimize` `/lokai` `/kill-ollama` | **Infra leftover:** `/help more`; run from a terminal. Inspector does not spawn `Command`. |

The standalone CLI trust command writes the shared `lokai.db`. The coordinator
re-reads that assignment and global policy epoch immediately before each remote
attempt, so a running daemon cannot dispatch using the old tier. The live
`fabric/worker.trust.set` RPC additionally cancels queued attempts at mutation
time.

## Tests

`cargo test -p lokai-cli` — CLI arg parsing, explain heuristics, runtime/policy wiring, shared compute-plane assembly (broker Some, Secret local-only), TUI transcript vs activity, slash Tab cycle, approval Always/high-risk, status phases, failure copy (stale caps, hop-lease miss, GPU spill, capacity warn-and-proceed). Orchestration integration is covered by `lokai-orchestrator` and `lokaid`.

## Related docs

- [PRODUCT-PLAN.md](../../../docs/PRODUCT-PLAN.md) — Phase A single-agent bar
- [../crates/lokai-orchestrator/README.md](../crates/lokai-orchestrator/README.md)
