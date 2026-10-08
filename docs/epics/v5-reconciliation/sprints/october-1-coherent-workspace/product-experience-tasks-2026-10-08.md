# Coherent workspace product experience tasks

Date: October 8, 2026. Status: proposed. See the [ranked backlog and delivery rules](product-experience-priorities-2026-10-08.md) for impact/effort definitions, dependencies, placement rules and shared acceptance. Every task below is an incremental extension of existing owners. Parent OCT tickets retain their full acceptance; these tasks add no calendar sprint.

## 1 One continuous effort workspace

<a id="task-11"></a>

### 1.1 Keep one identity for an effort

Impact **5/5** · Effort **5** · ROI index **1.0**. Parents: OCT-103, OCT-104. Prerequisites: existing baseline.

**Work:** Resolve the existing source discussion, execution, assignments and continuation links into one scoped effort projection. Preserve old links and attempt history; do not introduce another work or run store.

**What the user experiences:** Keep the same effort name, map selection and location through proposal, execution and continuation. Opening an old link lands on the right effort with its historical context available.

**Completion check:** A failed plan continued with retained contributions still appears as one effort, with both attempts inspectable and no duplicate totals.

<a id="task-12"></a>

### 1.2 Keep work and conversation available together

Impact **5/5** · Effort **3** · ROI index **1.7**. Parents: OCT-104. Prerequisites: 1.1.

**Work:** Restructure TeamWorkspace and LiveShaping around a persistent effort focus. Separate lightweight inspection from editing and long-form reading. Reuse the current map, routing and composer.

**What the user experiences:** Keep a compact, explicitly scoped composer available while inspecting work. Let proposal, activity or result occupy the main reading space without mandatory phase tabs. Make starting a separate effort explicit and preserve the current draft.

**Completion check:** A person starts effort B while A runs, inspects A, and returns to B without losing drafts, scroll position or understanding which effort will receive their next message.

<a id="task-13"></a>

### 1.3 Preserve context during setup detours

Impact **5/5** · Effort **2** · ROI index **2.5**. Parents: OCT-104, OCT-106. Prerequisites: existing baseline.

**Work:** Finish the existing return trail and draft preservation across agent, team, tool and connection editors. Add consistent dirty-form handling, pending-save behavior and focus restoration. Reuse the recently added setup return callbacks.

**What the user experiences:** Use targeted editors with a clear Save and return action. Preserve the originating effort, selected agent and unsent message. Avoid stacks of dialogs and duplicate Back controls.

**Completion check:** Changing an agent setting and canceling a connection edit both return to the originating work with its draft intact; closing with unsaved changes never silently discards them.

<a id="task-14"></a>

### 1.4 Give results a proper reading space

Impact **4/5** · Effort **3** · ROI index **1.3**. Parents: OCT-105, OCT-206. Prerequisites: 1.1, 1.2.

**Work:** Project the existing combined result, contributions and artifact references into a focused reader. Distinguish current, partial, superseded and missing outputs without generating a replacement answer.

**What the user experiences:** Expand the result within the workspace. Keep evidence links beside relevant claims and provide a stable return to the effort. Put run records and the unabridged blackboard one level deeper.

**Completion check:** The user reads a long result, follows a contribution or source, and returns to the same reading position; a finished run without a result is never presented as a ready deliverable.

## 2 Maintained direction

<a id="task-21"></a>

### 2.1 Extend the versioned brief with structured direction

Impact **5/5** · Effort **5** · ROI index **1.0**. Parents: OCT-103. Prerequisites: 1.1.

**Work:** Extend the existing brief revision contract with outcome, constraints, assumptions and open decisions, including source and author. Keep accepted direction distinct from proposals. Execution authority continues to come from existing grants and agreements.

**What the user experiences:** Present a short current understanding attached to the effort; never require users to complete all fields before starting discussion. Mark tentative assumptions plainly.

**Completion check:** An early constraint survives reload, plan generation and continuation; a conflicting update is retained for resolution instead of silently overwriting it.

<a id="task-22"></a>

### 2.2 Make current direction readable and editable

Impact **4/5** · Effort **2** · ROI index **2.0**. Parents: OCT-103, OCT-104. Prerequisites: 2.1.

**Work:** Add focused edits and revision comparisons over the brief API. Keep optimistic concurrency and save receipts. Separate editing desired direction from authorizing execution.

**What the user experiences:** Show What we are working toward near the effort heading. Edit one statement in place; offer source discussion and history on demand. Never place a full brief form above every conversation.

**Completion check:** The user can correct one assumption without leaving the effort and distinguish a saved change from an unsaved or conflicting one.

<a id="task-23"></a>

### 2.3 Give the Guide explicit direction operations

Impact **5/5** · Effort **5** · ROI index **1.0**. Parents: OCT-103, OCT-202. Prerequisites: 2.1.

**Work:** Extend existing Guide tools to read direction, propose an interpretation and record authorized changes. Separate informational questions from changes in priority, constraints or outcome. Keep uncertain interpretations as proposals with provenance.

**What the user experiences:** A meaningful correction produces a compact, inspectable acknowledgment describing what changed. Routine questions stay conversational and do not create confirmation cards. Ask a short clarification only when interpretation materially changes the work.

**Completion check:** A status question leaves direction untouched; an explicit scoped correction is saved; an ambiguous preference remains tentative and cannot grant access or start work.

<a id="task-24"></a>

### 2.4 Deliver scoped direction to each assignment

Impact **5/5** · Effort **3** · ROI index **1.7**. Parents: OCT-103, OCT-201. Prerequisites: 2.1.

**Work:** Extend the current pinned shared brief and assignment-context assembly with the applicable direction revision and references. Filter by existing visibility and grants. Preserve private conversation boundaries and enforce context limits explicitly.

**What the user experiences:** Expose the direction and shared context an agent actually received from its assignment inspector. Private discussion is not copied into team context merely because it shaped an effort.

**Completion check:** Two agents receive their relevant shared constraints without unrelated private discussion; a missing or oversized context source is visible rather than silently lost.

## 4 Bounded exploration of uncertain goals

<a id="task-41"></a>

### 4.1 Handle simple requests without creating a planning ceremony

Impact **4/5** · Effort **3** · ROI index **1.3**. Parents: OCT-102, OCT-103. Prerequisites: existing baseline.

**Work:** Refine existing Guide routing and tool contracts for direct answers, material clarification, suggested exploration and work proposals. Keep the model responsible for domain content; validate actions through existing services.

**What the user experiences:** Use the same input. A short answer stays short; an uncertain request receives one useful next question or exploration offer. No compulsory mode selector, wizard or team form.

**Completion check:** A small question receives a useful answer without tickets; a materially ambiguous request does not silently launch a broad plan. Validate with real model trials as well as tool-contract tests.

<a id="task-42"></a>

### 4.2 Run bounded discovery before committing to implementation

Impact **5/5** · Effort **5** · ROI index **1.0**. Parents: OCT-102, OCT-202. Prerequisites: 2.1, 2.4, 4.1, 6.1.

**Work:** Represent discovery using the existing proposal, assignment, tool and allowance paths. Let the Guide choose available agents for independent investigations, define a stopping condition and request authorization within existing policy.

**What the user experiences:** Offer a compact Investigate first proposal describing what will be learned, the allowance and missing access. Show findings in the same effort without requiring the user to construct a DAG.

**Completion check:** A vague goal yields evidence-backed alternatives from bounded work; the agreed discovery stops and does not turn itself into unauthorized implementation.

<a id="task-43"></a>

### 4.3 Make exploration results into understandable choices

Impact **4/5** · Effort **3** · ROI index **1.3**. Parents: OCT-103, OCT-202. Prerequisites: 4.2.

**Work:** Link model-proposed options to retained findings, tradeoffs and unresolved assumptions. Selecting or editing a direction updates the existing brief/proposal; it is not an automatic execution grant.

**What the user experiences:** Show a few meaningful alternatives beside the discussion, with evidence available inline. Allow combining, rejecting or writing another approach rather than forcing a preset selection.

**Completion check:** The user can make an informed choice and see it reflected in the same effort; an unsupported recommendation is visibly separated from observed findings.

<a id="task-44"></a>

### 4.4 Resolve missing capabilities without abandoning the work

Impact **5/5** · Effort **3** · ROI index **1.7**. Parents: OCT-102, OCT-103, OCT-105. Prerequisites: 1.3, 5.2.

**Work:** Connect a work-scoped missing-capability request to existing workspace installation, connection and agent-grant operations. Revalidate actual availability and admission afterward. Keep workspace installation, persistent agent access and execution consent distinct.

**What the user experiences:** Open a targeted connection or grant flow from the blocker. Show exactly which agent gains what persistent access and which work needs it. Return with Available, Still blocked or Ready to resume based on real validation.

**Completion check:** Adding a connection alone cannot masquerade as granting access; canceling preserves the work; successful setup removes only the blocker it actually resolves.

## 5 Human decisions in context

<a id="task-51"></a>

### 5.1 Present decisions in human terms

Impact **5/5** · Effort **2** · ROI index **2.5**. Parents: OCT-105. Prerequisites: existing baseline.

**Work:** Restructure existing Decision and HumanQuestion components around the requested choice, known effects and scope. Reuse exact approval payloads, expiry and receipt checks. Label unknown consequences instead of inventing reassuring prose.

**What the user experiences:** Lead with what the user is being asked to decide, why it matters and what is affected. Keep response controls adjacent. Show consequential details before approval; place internal identifiers in secondary detail.

**Completion check:** A person can explain the action and its scope from the card without opening raw logs; missing exact action data still prevents approval.

<a id="task-52"></a>

### 5.2 Resolve the same request in the effort or inbox

Impact **5/5** · Effort **3** · ROI index **1.7**. Parents: OCT-105. Prerequisites: 5.1.

**Work:** Render existing approvals and questions in their related effort as well as Needs you. Use the same request identities, authorization and resolution operations; reconcile simultaneous views and stale requests.

**What the user experiences:** Show the request alongside the work it blocks. Answer there or in the inbox and see both update. Opening evidence preserves the decision and return position.

**Completion check:** Resolving a request in one view updates the other without a second action; expired or already resolved requests cannot appear actionable after reconciliation.

<a id="task-53"></a>

### 5.3 Group related requests without merging authority

Impact **4/5** · Effort **5** · ROI index **0.8**. Parents: OCT-105, OCT-202. Prerequisites: 5.2.

**Work:** Add explicit relationships for a shared blocker where the engine can substantiate them. Group by effort and permission scope; preserve child request identities. Do not deduplicate requests solely by similar model-generated wording.

**What the user experiences:** Present one issue with the affected assignments underneath. If multiple decisions remain necessary, show them explicitly. Keep permissions and personal/team context boundaries visible.

**Completion check:** A shared dependency produces one navigable group, while unrelated or differently scoped requests remain separate; one answer cannot silently approve extra actions.

<a id="task-54"></a>

### 5.4 Acknowledge decisions and show the next state

Impact **4/5** · Effort **2** · ROI index **2.0**. Parents: OCT-105. Prerequisites: 5.1, 5.2.

**Work:** Distinguish persisted response, eligibility to resume, queued execution and actual resumed activity using existing receipts and task state. Keep duplicate submission protection and offline uncertainty.

**What the user experiences:** After answering, show Answer saved, Waiting for capacity or Working again as appropriate. Preserve a brief decision receipt and let the user move to the next request without an unexpected jump.

**Completion check:** A saved answer never falsely claims resumed execution; an uncertain response remains recoverable without duplicate answers or approvals.

