# The Lokai Charter — A Developer's Digital Rights

**This is the founding document of the project.** It is the shining star every other
decision is steered by. Designs, contracts, gates, code reviews, and product choices
must all trace back here. When a decision conflicts with this Charter, **the Charter wins** —
the decision changes, not the Charter.

---

## Why this exists

Treat a computer as what it actually is: a **home**. It holds more of a person's papers,
thoughts, unfinished ideas, and private effects than any 18th-century house ever did.

The principle that the home is a protected sanctuary is ancient — *"a man's house is his
castle"* (English common law, Semayne's Case, 1604) — and the American framers encoded it
twice over: the **Third Amendment** (no quartering — no forced presence of the state inside
your walls) and the **Fourth Amendment** (no unreasonable search or seizure of your papers
and effects). Beneath both sits the **Declaration's** promise of *life, liberty, and the
pursuit of happiness* — the reason the protections exist at all: human **autonomy and dignity**.

The law has **not** extended these protections to the digital home. Meanwhile the dominant
business model of modern software is built on continuous, frictionless extraction from
exactly that space. The intrusion is no longer a soldier in the living room; it is a
default-on telemetry pipe and a "we need to send this to the cloud to help you." A person's
rights should not erode simply because the average person does not know enough about
technology to stop the violation.

So our thesis:

> **Where the law has been slow to defend the digital home, we defend it in software.
> Rights you cannot rely on being granted, we make structurally true — and verifiable.**

We do not *ask* anyone to respect your privacy. We build a tool where violating it is
**architecturally impossible**, and where **you can check that yourself** — because trust
you cannot verify is just a nicer brand of the same problem.

Two clarifications that matter:

- **The adversary is anyone outside your system** — an overreaching state, an extractive
  corporation, or any third party. Today the most constant intruder is commercial
  surveillance, not a soldier. The principle is identical: *it is your home, and no one has
  an inherent right to be inside it.*
- **This is not the "nothing to hide" argument.** Privacy is not for concealing wrongdoing.
  It is the **precondition for free thought and creation** — surveillance makes people build
  more timidly. Privacy is the substrate of liberty, not an exception to it.

---

## The Rights

Each right is paired with the **mechanism that enforces it** and is meant to be **testable**
(see [`docs/implementation/gates/GATE-privacy-sovereignty.md`](./implementation/gates/GATE-privacy-sovereignty.md)).
A right with no enforcing mechanism is just marketing; a mechanism with no test is just a hope.

### 1. The right to sanctuary
Your machine is your home. Nothing watches, measures, or reports what you do there.
> **Enforced by:** no telemetry (removed from source, not merely disabled); **default-deny egress**.

### 2. The right to ownership
Your code, your prompts, and the agent's work are **yours** — in open formats you control.
Nothing is held hostage.
> **Enforced by:** local-only storage; open formats; no lock-in; no proprietary vault.

### 3. The right to verify, not trust
You never have to take our word for it. You can inspect and **prove** every claim we make.
> **Enforced by:** open source; reproducible + signed builds; the **Network Activity Panel**.

### 4. The right to see what the machine sees
Full visibility into what is in the model's context and into every action the agent takes.
> **Enforced by:** the **Context Inspector**; the local audit trail.

### 5. The right to consent
Nothing irreversible and nothing outbound happens without your explicit say-so.
> **Enforced by:** approval gates on shell/writes/egress; default-deny everywhere.

### 6. The right to leave and to be forgotten
Delete everything, export everything — no residue, no permission required.
> **Enforced by:** one-click purge; full export; no hidden caches or background sync.

### 7. The right to persist
The tool works offline, indefinitely, and **cannot be remotely disabled, revoked, or
ransomed**.
> **Enforced by:** offline-first; no account; no kill-switch; no mandatory updates.

### 8. The right to compute on your own terms
Your hardware, your models, scaled however you choose — from one laptop to a cluster you own — and, when you opt in, **trusted peers you explicitly enrolled** (see [Circle Charter](./implementation/circle-charter.md)).
> **Enforced by:** self-hosted inference only; the owned-node trust boundary; bring-your-own-model; optional **Circles** (peer pooling under separate Circle Rights).

### 9. The right to parity
Local-only must not mean second-class. You deserve tooling as capable as the best cloud
agents — within the compute you provide — not a consolation prize for caring about privacy.
> **Enforced by:** benchmark-gated development (graded task suites with pass-rate targets);
> token economy and retrieval discipline; verification loops; project memory; orchestration
> without cloud dependency. Capability is measured, not assumed.

### 10. The right to bounded execution
The agent operates inside boundaries **you** define. It is a guest in your home, not a
process with unrestricted access to your machine, credentials, and network.
> **Enforced by:** a unified **policy engine** (allow / ask / deny per action); workspace
> confinement; optional **git worktree** isolation per session; **sandboxed subprocess**
> execution for shell and verify commands (timeouts, output caps, no subprocess network);
> resource limits where the OS permits.

### 11. The right to continuity
Work is **project-shaped**, not chat-shaped. Closing a session must not erase what the
agent learned about *this* codebase, its conventions, and open threads — without shipping
that knowledge to a third party.
> **Enforced by:** durable **project memory** (bounded digest in the stable context prefix);
> episodic recall from the local audit/index on demand; session briefing from index + digest
> at start — all on disk you control.

---

## Engineering doctrine

These are not Rights in the legal sense; they are **non-negotiable build rules** derived
from the Rights above.

### Bones before skin
The engine must work flawlessly as a **headless agent** (`lokai-cli` + `lokaid`) before
investing in editor chrome. The VS Code fork is **skin**, not skeleton: chat panels,
diff views, and activity panels come **last**, after the core loop, policy, execution,
memory, and orchestration layers meet their exit criteria. Until then, the CLI is the
product and the proof.

### One orchestrator, many specialists
`lokaid` owns sessions, policy, storage, and (later) routing. `lokai-core::Agent::turn`
stays the **single-agent loop** — unchanged in shape when sub-agents arrive. Swarm logic
wraps the loop; it does not replace it.

### Measured capability
Every major engine change is justified against the **regression bench**
(`engine/bench/`): non-LLM hot paths, raw inference throughput, graded agent tasks, and
long-context stress. We do not ship "feels better"; we ship **numbers that moved**.

### Where we aim to exceed cloud agents
Privacy is the headline, but parity is the bar and **honest superiority** is the goal
where architecture allows:

| Dimension | Cloud agents (typical) | Lokai target |
|-----------|------------------------|--------------|
| Data custody | Code on vendor infra | **Structural local-only** — verifiable |
| Audit & undo | Opaque or partial | **Full local audit** + checkpoints/time-travel |
| Context visibility | Black box | **Inspectable context layers** (CLI today, panel later) |
| Policy | Vendor decides | **Explicit allow/ask/deny** you configure and persist |
| Compute scaling | Their bill, their caps | **Your hardware** — laptop → homelab cluster |
| Offline / longevity | Account-dependent | **Works offline indefinitely** |
| Project memory | Their embedding of your repo | **Your digest + your index** — portable, purgeable |

We do not claim to beat cloud on raw model IQ (that is the user's model choice). We claim
to **remove the tax** they pay in privacy, opacity, and lock-in — and to **win on step
efficiency, verification, and continuity** through better local infrastructure.

---

## The honesty clause

Overclaiming would betray the very trust this Charter is built on. Therefore we state our
limits as plainly as our promises.

- **What we guarantee:** *the application will not betray you.* Within the software's own
  conduct, the Rights above hold and are verifiable.
- **What we do not claim:** we cannot defend a compromised operating system, malicious
  hardware, a user who deliberately invites an intruder in (e.g. enrolling a hostile node or
  installing a malicious networked extension and granting it access), or an adversary with
  physical access to the machine.

We reduce attack surface and make our behavior observable. We do not sell the illusion of
invulnerability. **"Privacy theater" is itself a Charter violation.**

---

## How this Charter governs the work

- Every design doc, contract, and gate **traces to this Charter** and cites the Right(s) it serves.
- [`docs/implementation/gates/GATE-privacy-sovereignty.md`](./implementation/gates/GATE-privacy-sovereignty.md)
  is the Charter's enforcement arm: each Right maps to pass/fail checks, and a failure is a
  **release blocker**.
- [`docs/PRODUCT-PLAN.md`](./PRODUCT-PLAN.md) is the **product plan SSOT**: every feature,
  its status, and sequencing in one place (done, cancelled, deferred, remaining).
- [`docs/implementation/bones-first-roadmap.md`](./implementation/bones-first-roadmap.md)
  expands **engine delegations D1–D16** with exit criteria (subset of the product plan).
- In any tradeoff between capability and a Right, the **Right is the constraint** and we find
  a capable design *within* it — not the other way around.
- The Charter is amended deliberately and rarely, never weakened for convenience.

> *Freedom, liberty, and the pursuit of happiness — in the world of technology, too.*
> This is what the product stands to be.

---

**Adopted:** 2026-06-25 · **Amended:** 2026-06-27 (Rights 9–11, engineering doctrine, bones-first; Right 8 Circle cross-ref) · **Status:** Founding document (supersedes nothing; governs everything).
