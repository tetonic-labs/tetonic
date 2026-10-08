# Coordinated work product experience tasks

Date: October 8, 2026. Status: proposed. See the [ranked backlog and delivery rules](../october-1-coherent-workspace/product-experience-priorities-2026-10-08.md) for impact/effort definitions, dependencies, placement rules and shared acceptance. Every task below is an incremental extension of existing owners. Parent OCT tickets retain their full acceptance; these tasks add no calendar sprint.

## 3 Direction that reaches the work

<a id="task-31"></a>

### 3.1 Preview which work a direction change affects

Impact **5/5** · Effort **3** · ROI index **1.7**. Parents: OCT-202, OCT-203. Prerequisites: 2.1, 2.4.

**Work:** Use saved assignment relationships and direction revisions to identify explicit impact. Separate queued, running and completed work; treat model-suggested semantic impact as a proposal. Avoid claiming all conflicts are mechanically detectable.

**What the user experiences:** Beside the edit, show a concise impact preview with named assignments and current states. Keep unrelated efforts visually stable and distinguish a proposed update from one already applied.

**Completion check:** A change with queued, running and completed dependents produces the correct categories and does not pretend completed side effects were undone.

<a id="task-32"></a>

### 3.2 Apply queued changes from the effort itself

Impact **4/5** · Effort **2** · ROI index **2.0**. Parents: OCT-202, OCT-203. Prerequisites: 3.1, 2.3.

**Work:** Expose the existing revision-checked upcoming-assignment amendment through contextual controls and the authorized Guide operation. Preserve admission races, affected-dependency checks and idempotency; do not build another amendment store.

**What the user experiences:** Offer Apply to upcoming work next to the proposed change. If an assignment started meanwhile, retain the draft and explain the changed impact rather than returning a generic error.

**Completion check:** A successful update is used by upcoming assignments; a dispatch race produces an honest conflict and neither duplicates work nor loses the proposed edit.

<a id="task-33"></a>

### 3.3 Deliver direction to supported running agents

Impact **5/5** · Effort **8** · ROI index **0.6**. Parents: OCT-202, OCT-203. Prerequisites: 3.1, 2.4.

**Work:** Add persisted direction delivery and acknowledgment at supported managed-runtime boundaries. Define cancellation, tool-in-flight, restart and supersession behavior. Begin with a bounded runtime proof before UI integration. Reject unsupported live redirection explicitly.

**What the user experiences:** Show Update pending, Received for the next step, or Stop and replan required from engine receipts. Keep existing effects visible. Do not imply that an agent has complied semantically merely because it received instructions.

**Completion check:** A supported active agent receives the latest scoped revision exactly as intended; restart and stop do not resurrect superseded work, and an in-flight external effect is never reported as reversed.

<a id="task-34"></a>

### 3.4 Show whether direction actually reached the team

Impact **5/5** · Effort **3** · ROI index **1.7**. Parents: OCT-202, OCT-205. Prerequisites: 3.1, 3.2.

**Work:** Project saved amendment receipts, assignment revision pins and delivery acknowledgments. Ship queued coverage first; active acknowledgment requires 3.3. Keep applied instructions distinct from verified result compliance.

**What the user experiences:** Show a short receipt such as Two upcoming assignments updated; one active assignment still uses the previous direction. Expand to per-agent evidence only when requested.

**Completion check:** Every displayed application state matches an engine record, persists after refresh and does not turn pending or unsupported delivery into success.

## 6 Dependable autonomous continuation

<a id="task-61"></a>

### 6.1 Make the scope of autonomous continuation clear

Impact **5/5** · Effort **3** · ROI index **1.7**. Parents: OCT-201, OCT-202. Prerequisites: 2.1.

**Work:** Present a coherent work agreement using the existing finite plan, revision, agent grants, shared allowance, deadline and stop controls. State which assignments can continue without another prompt. Broader model-added work needs an explicit future authority contract.

**What the user experiences:** Place the compact agreement beside Start: outcome, team, scope and allowance. Use plain language about what continues and what will come back for judgment. Keep lower-level settings secondary.

**Completion check:** The user can tell what starts, what can continue independently and when the team must return; the displayed agreement matches engine admission and cannot imply unlimited autonomy.

<a id="task-62"></a>

### 6.2 Persist eligible work and fair dispatch

Impact **5/5** · Effort **5** · ROI index **1.0**. Parents: OCT-201, OCT-203. Prerequisites: 6.1.

**Work:** Complete pending dispatch durability and reconciliation within TeamWorkController and existing admission records. Recheck dependencies, authority, budget and occupancy. Prevent duplicate accepted assignments and starvation within the supported local profile.

**What the user experiences:** Show why an assignment is queued and what it is waiting for, distinguishing a dependency from capacity. Keep unrelated effort activity visible without requiring a chat message to advance it.

**Completion check:** Eligible queued work advances when capacity is available, survives the supported restart path and is not admitted twice after retries or reconnects.

<a id="task-63"></a>

### 6.3 Park and resume supported waiting team work

Impact **5/5** · Effort **8** · ROI index **0.6**. Parents: OCT-203. Prerequisites: 6.2.

**Work:** Finish the existing checkpoint, durable-human-question and controller integration for the declared team/subtree profile. Release capacity only after safe checkpointing; retain original context, grants, accounting and stop lineage. Recheck authority and deadlines on resume.

**What the user experiences:** Distinguish waiting for a person, safely parked, queued to resume and actually running. Pause, cancel and recovery must describe their real effects; hide unsupported actions rather than presenting pretend controls.

**Completion check:** With capacity constrained, one supported parked assignment releases capacity for another, then resumes from an authorized answer after restart without replaying uncertain effects or escaping earlier limits.

<a id="task-64"></a>

### 6.4 Control several efforts without controlling every agent

Impact **5/5** · Effort **5** · ROI index **1.0**. Parents: OCT-203. Prerequisites: 1.1, 6.2.

**Work:** Add or connect authorized effort-level queue priority and stop operations to existing controller state and lineage. Preserve declared resource-conflict enforcement. Add pause/resume UI only for the profile completed in 6.3.

**What the user experiences:** Offer compact controls at the effort level. Explain when priority affects the next available slot and which descendants a stop targets. Never require visiting each agent to stop a team.

**Completion check:** Changing priority changes eligible scheduling without duplicating work; stopping one effort affects its scope and leaves unrelated efforts running. The interface waits for engine acknowledgment.

## 7 Understanding several efforts at a glance

<a id="task-71"></a>

### 7.1 Show meaningful progress on the map

Impact **5/5** · Effort **3** · ROI index **1.7**. Parents: OCT-104, OCT-205. Prerequisites: 1.1.

**Work:** Derive a compact effort summary from actual assignment states, unresolved requests and result references. Use source-backed summaries with stale/unknown handling, not a parallel status store or fabricated percentage.

**What the user experiences:** At a distance show purpose, one meaningful development, participation and anything needing judgment. Show deeper contributors and interactions on zoom or selection; preserve camera position.

**Completion check:** A person can identify what moved, what is blocked and what is ready from the map; every summary can be opened to the underlying evidence.

<a id="task-72"></a>

### 7.2 Show changes since the user last looked

Impact **4/5** · Effort **3** · ROI index **1.3**. Parents: OCT-205. Prerequisites: 1.1, 7.1.

**Work:** Persist a viewer-scoped seen cursor over existing events or revisions and reconcile reconnect gaps. Start with deterministic change summaries. Define when an item is marked seen and avoid cross-user or cross-workspace leakage.

**What the user experiences:** Offer a small While you were away view ordered around decisions and usable results. Link each change to the same effort. Let users keep items unread; do not cover the map with an automatic digest modal.

**Completion check:** Returning after disconnection shows the missed meaningful changes once without losing newer arrivals, manufacturing events or marking unseen results read.

<a id="task-73"></a>

### 7.3 Organize related efforts at scale

Impact **4/5** · Effort **3** · ROI index **1.3**. Parents: OCT-104, OCT-205. Prerequisites: 1.1, 7.1.

**Work:** Use actual plan/team relationships for grouping, with explicit user overrides if needed. Keep map and list filters on the same effort projection and test layout at larger counts. Do not require a manual taxonomy before work can start.

**What the user experiences:** Keep purposeful spatial groups and readable labels. Use group identity separately from status color, so blocked red and completed green retain consistent meanings. Offer a compact list for finding and comparing without moving the user into another workflow.

**Completion check:** With many clearly labeled test efforts, filtering, selection and returning preserve context; the map remains navigable, labels do not collide and color is not the only status cue.

<a id="task-74"></a>

### 7.4 Make activity signals trustworthy and restrained

Impact **3/5** · Effort **2** · ROI index **1.5**. Parents: OCT-104, OCT-205, OCT-207. Prerequisites: existing baseline.

**Work:** Audit remaining motion and status mappings against authoritative model/tool/task events. Stop or mark stale animation on disconnect and completion. Reuse the existing renderer and reduced-motion support.

**What the user experiences:** Use subtle motion for observed activity, distinct static cues for waiting, and clear result availability. Do not move or reorder a focused item while someone is reading or deciding.

**Completion check:** Event replay and disconnect checks produce matching visible states; reduced motion and keyboard use preserve all information without fabricated activity.

