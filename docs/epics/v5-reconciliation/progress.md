# MVP implementation progress

## 2026-09-26 — Sprint 3 second pass: work activates on the managed path

Team work is no longer a disconnected binder. `Application::activate_team_work` loads the work item, applies delegation token ceilings, calls `submit_registered_job`, and binds the returned run/attempt onto the work row (schema 48 adds `run_id`). A parked parent blocks child activation. `launch_team_work` and `tetonic job run --team --work` reuse the same host launch path as ordinary registered jobs. `tetonic control work` covers goal/create/list/park/resume/accept-huddle without starting inference.

Still deferred to sprint 4+: enabling governed `parent_attempt` admission inside ManagedRunService; cumulative spend ledgers; write-claim negotiation; D03 deletion.

Validation: `cargo test -p tetonic-memory --lib -- migration team_work -- --test-threads=1` passed. `cargo test -p tetonic-app --lib -- activate_team_work_binds_managed_run_and_respects_delegation_ceiling resource_service_exposes_goals_huddles_activation_and_delegation` passed.

## 2026-09-26 — Sprint 3 exited (local MVP preview)

Sprint 3 written exits are treated as met for the local single-authority preview. Evidence: schema 46–47 goals/work/huddles; quick task without huddle; huddle accept idempotent by request id; outsider denied; park leaves sibling work open; resume rechecks membership; work binds to an attempt id; event/schedule cursors create at most one work item per event; child budget cannot exceed parent and stop scope inherits; cross-team delegation is an opaque denial. ResourceService exposes the same operations under ManageTeam/ReadTeam.

Deferred to later sprints (explicitly not part of this exit): team-work CLI; automatic `submit_registered_job` from work activation; enabling governed child admission in ManagedRunService; cumulative spend ledgers / waiting queues; write-claim negotiation UI; D03 deletion.

Validation: `cargo test -p tetonic-memory --lib -- migration quick_task_does_not_require_a_huddle_and_retries_are_idempotent accepted_huddle_creates_work_idempotently_and_outsider_is_denied parked_work_survives_and_independent_work_stays_open event_cursor_duplicates_do_not_backlog_work delegation_inherits_budget_and_stop_and_cross_team_is_opaque -- --test-threads=1` passed. `cargo test -p tetonic-app --lib -- resource_service_exposes_goals_huddles_activation_and_delegation` passed.

## 2026-09-26 — Sprint 3: durable team goals, quick tasks and huddle work

Schema 46 stores team goals, work items and huddle proposals. A quick task does not need a huddle. Accepting a huddle creates one open work item per title and retries with the same request id do not duplicate. A non-member is denied. Work can be parked. Binding these items to managed runs and event/schedule activation are still open. Sprint 3 exit criteria remain unmet.

Validation: `cargo test -p tetonic-memory --lib -- quick_task_does_not_require_a_huddle_and_retries_are_idempotent accepted_huddle_creates_work_idempotently_and_outsider_is_denied` passed.

## 2026-09-26 — Sprints 1 and 2 exited (local MVP preview)

Sprint 1 and Sprint 2 written exits are treated as met for the local single-authority preview. Evidence: team create / participation context / unpublished private canary; metadata create does not charge or report Running; `tetonic job run` (+ `--view`) activates and shows journal events; Running only at claim; noncoding recall without a repository; registered shell refused; cancel stops an owned process; ceilings reject when full.

Deferred to later sprints (explicitly not part of this exit): remote multi-user auth/UI; cumulative team spend ledger and waiting queues; Windows OS filesystem jail; standalone server world-loop deletion (D07/D08); fleet prototype / coding-identity pin deletion (D01/D11).

Validation: `cargo test -p tetonic-app --lib -- team_execution_cannot_retrieve_unpublished_private_history team_participation_context_is_the_callers_empty_working_context activation_receipt_names_the_journal_event_and_hides_a_payload admission_does_not_report_running_before_execution_is_claimed metadata_creation_does_not_charge_or_report_running noncoding_recall_job_runs_without_a_repository registered_shell_is_rejected_before_inference` passed. `cargo test -p tetonic-cli --bin tetonic -- activation_view_shows_journal_events_and_not_a_running_status` passed. `cargo test -p tetonic-run --test managed_service_tests -- managed_cancel_stops_the_owned_process` passed.

## 2026-09-26 — Credential store files are not readable through tools or context

Workspace paths under credential stores (for example `.ssh`, `.aws`, `.git-credentials`) are refused by file tools and skipped by context search/read. Ordinary workspace files still work. This is incremental isolation, not a sprint exit by itself.

Validation: `cargo test -p tetonic-tools --lib -- credential_store_files_are_not_read_or_searched` passed. `cargo test -p tetonic-context --lib -- credential_store_is_not_searched_or_read` passed.

## 2026-09-26 — Context errors do not repeat handles, fingerprints, or store bodies

An exhausted or stale expansion handle is rejected without repeating the handle id or workspace fingerprints. A compilation failure for a changed workspace does the same. An artifact-store failure during sealing is reported as `artifact storage failed` and does not include the store error. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-context --lib -- test_expansion_handle_use_count_exhausted test_expansion_stale_workspace_rejected simultaneous_valid_and_foreign_callers_cannot_steal_the_last_use artifact_storage_failure_does_not_repeat_the_store_body stale_workspace_error_does_not_repeat_fingerprints` passed.

## 2026-09-26 — Prototype agent registration does not invent a charge or Running

The legacy in-memory fleet prototype no longer records a 1,000-token charge when it registers agent metadata. A duplicate registration is rejected and still consumes no tokens. The registered record and the supervisor both stay idle. Clearing an emergency stop returns that idle state; it does not report running. This prototype is still not the production control API. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib -- metadata_creation_does_not_charge_or_report_running test_create_agent_within_budget_and_register_with_supervisor test_agent_estop_and_resume_lifecycle test_operator_dashboard_view_aggregates_state` passed. `cargo test -p tetonic-orchestrator --lib -- test_fleet_supervisor_lifecycle_and_steering` passed.

## 2026-09-26 — Daemon tool-call notifications omit argument bodies

The unauthenticated daemon stream still names the tool and the call. It no longer includes the tool arguments, so a file body or command text is not repeated there. The in-process interface still receives the original event. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonicd -- tool_call_notification_does_not_repeat_argument_bodies chat_send_streams_tokens_tools_and_ok` passed.

## 2026-09-26 — A sessionless job is not reported started before execution is claimed

The employee-visible started event for a sessionless, unscoped job is emitted when execution is claimed, not when the attempt is admitted. Admission alone no longer produces that event. Scoped runs still do not use this unauthenticated sink. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib -- sessionless_start_is_not_reported_before_execution_is_claimed admission_does_not_report_running_before_execution_is_claimed` passed.

## 2026-09-26 — Running begins when execution is claimed

Admitting an attempt records it as starting. The attempt and its task become running only when execution is claimed. A failed build before that claim is not reported as running. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-run --tests --lib` passed. `cargo test -p tetonic-app --lib -- admission_does_not_report_running_before_execution_is_claimed spawn_child_task_appears_in_snapshot_dag` passed. `cargo test -p tetonic-app --test v4_proof_09` passed. `cargo test -p tetonic-broker --lib -- cmp02_` passed.

## 2026-09-26 — Cancel stops an in-flight language-server call

A language-server tool watches the attempt's cancel signal. When the attempt is canceled, the call asks the server to stop. The server's request loop then terminates the owned process. A call that has not started is not given a new server after cancel. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-tools --lib -- cancel_stops_an_in_flight_language_server_call same_workspace_bindings_do_not_share_sessions_or_retain_them_globally` passed. `cargo check -p tetonic-lsp -p tetonic-app` passed.

## 2026-09-26 — Job activation can draw its journal events

`tetonic job run --view` still activates through the managed runtime and prints the receipt. It also draws the terminal outcome and the journal event names from that receipt. Payload digests are not drawn. An outcome that is not completed, canceled, limited, or failed is shown as unavailable, not running. This is a local view, not a remote setup UI. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-cli --bin tetonic -- activation_view_shows_journal_events_and_not_a_running_status` passed.

## 2026-09-26 — A scoped attempt does not reuse a carried conversation

A managed attempt with an execution scope discards turns already in its conversation before the model runs. A prior private turn is not sent. The conversation's cancel handle still stops the attempt. An unscoped coding session keeps the conversation it resumed. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-core --lib -- discard_carried_turns_drops_messages_and_keeps_cancel` passed. `cargo test -p tetonic-app --lib -- scoped_attempt_does_not_send_a_carried_conversation admitted_attempt_executes_its_revision_after_identity_update` passed.

## 2026-09-26 — A job receipt names the actual journal event

The launch receipt keeps the journal event name, such as `attempt.started`, instead of collapsing every stored event to `other`. A name that is not a short identifier stays `other`, so a payload is not repeated in the receipt. The outcome is still only a terminal class and never running. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib -- activation_receipt_names_the_journal_event_and_hides_a_payload` passed.

## 2026-09-26 — Placement affinity does not cross information contexts

Worker affinity is kept only for the same session, turn, run, and information context. A later request on another context does not stay pinned to the worker chosen for the previous context, even when the session, turn, and run ids are reused. The same context still keeps its worker. A registered job stamps its information context onto the model request. A request that names no context keeps the previous behavior. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-inference --lib -- sync_turn_drops_affinity_when_the_information_context_changes sync_turn_drops_affinity_when_the_run_changes sync_turn_drops_affinity_when_the_session_changes sync_turn_clears_affinity_on_new_user_turn` passed. `cargo test -p tetonic-app --tests --no-run` compiled.

## 2026-09-26 — A team participant can select their empty working context

A principal who owns a team or belongs to it can resolve their private working context. A missing team and a non-participant are both denied. The context starts empty, so it does not contain that principal's other private history, and another member cannot recall it. `tetonic job run --team` uses this context instead of an arbitrary context id. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-memory --lib -- a_participant_uses_the_working_context_without_private_history` passed. `cargo test -p tetonic-app --lib -- team_participation_context_is_the_callers_empty_working_context` passed. `cargo check -p tetonic-cli` passed.

## 2026-09-26 — Creating a team creates the owner's empty working context

Creating a team for an enabled organization member also creates that owner's private working context. It starts with no sessions or messages, so it does not contain their other private history, and another member cannot read or recall it. Creating the same team again keeps that one context. A team insert whose owner is not an enabled organization member still persists the team and does not invent a working context. `tetonic control` create-team reports the owner's working context id. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-memory --lib -- creating_a_team_creates_the_owners_empty_working_context joining_a_team_creates_an_empty_private_working_context team_keys_are_scoped_and_resources_survive_reopen retries_do_not_overwrite_names_or_ownership` passed. `cargo test -p tetonic-app --lib -- persistent_authority_uses_verified_identity_and_current_membership` passed. `cargo test -p tetonic-cli --test control_cli -- bootstrap_create_reopen_and_revoke_via_cli` passed.

## 2026-09-26 — Index and restore errors do not repeat a store or file body

Code-index failures and workspace-restore failures no longer put the store or file error into the message an employee can see. The employee text is `request failed`. A count mismatch from an embedding model is still reported as an invalid request. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib -- index_failures_do_not_repeat_the_store_body restore_failure_does_not_repeat_file_bytes` passed.

## 2026-09-26 — Joining a team creates an empty private working context

Adding a team member creates a private working context for that principal. It starts with no sessions or messages, so it does not contain their other private history, and other members and organization administrators cannot read it. Adding the member again keeps that same context. Removing membership does not delete it. `tetonic control` add-team-member reports its id. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-memory --lib -- joining_a_team_creates_an_empty_private_working_context team_grants_enforce_scope_owner_and_atomic_audit` passed.

## 2026-09-26 — Worker affinity does not cross runs

Placement affinity is kept only for the same session, turn, and run. A later request on another run does not stay pinned to the worker chosen for the previous run, even when the session and turn ids are reused. The same run still keeps its worker. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-inference --lib -- sync_turn_drops_affinity_when_the_run_changes sync_turn_drops_affinity_when_the_session_changes sync_turn_clears_affinity_on_new_user_turn` passed.

## 2026-09-26 — A specialist is not reported until it is built

The turn reports a specialist node only after that agent is built. A build failure does not emit `NodeStarted`. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib -- specialist_node_is_not_reported_before_the_agent_is_built spawn_does_not_report_started_before_the_specialist_is_built` passed.

## 2026-09-26 — Embedding does not send a preview of a live database

Chunks waiting for an embedding are omitted when the live file is a SQLite database, including a text file replaced by a database before the next index pass. The stored preview is not sent to the embedding model. An ordinary source file in the same workspace is still queued. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-index --lib -- search_hides_a_stale_preview_after_the_file_becomes_a_database stores_and_ranks_embeddings_by_cosine` passed.

## 2026-09-26 — Git context omits a live database file

The context compiler's git diff and status output drops a file whose live bytes are a SQLite database, including a renamed database that is not the reserved store path. An ordinary source diff in the same output is kept. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib -- git_diff_omits_a_protected_store git_diff_omits_a_sqlite_database_that_is_not_the_reserved_store` passed.

## 2026-09-26 — The broker treats a secret context pack as local-only

The compute request used for scheduling and failover now takes the stricter of the host class and the compiled context class. A secret context pack is not offered to a remote worker and does not enter a speculative remote race. A repository-source request without that context class is unchanged. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-broker --lib -- secret_context_class_raises_the_compute_request` passed.

## 2026-09-26 — A secret context pack is not placed on a remote worker

Placement uses the stricter of the host data class and the compiled context class. A secret context pack stays on this machine even when the host class is lower, a hard tier is requested, or a worker is preferred. A repository-source request without a secret context class is unchanged. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-inference --lib -- secret_placement_is_local_only` passed.

## 2026-09-26 — Admission does not report a model request

Planning a turn no longer emits `started`. That status is the semantic model-request signal. It is emitted once, when the built agent enters execution. A plan that is completed without entering the agent does not report it. A daemon chat that does enter the agent still reports one start. A later critic or revision on the same turn does not report a second start. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib -- session_turn_lifecycle_event_order spawn_does_not_report_started_before_the_specialist_is_built`, `cargo test -p tetonic-app --test cli_assembly_parity`, `cargo test -p tetonic-eval -- kernel_lifecycle_semantic_effects`, and `cargo test -p tetonicd -- cli_daemon_kernel_semantic_effect_parity cli_daemon_write_file_mutation_parity` passed.

## 2026-09-26 — Chat failures do not repeat a store or tool body

The local chat inspector, turn-failure panel, model picker, and approval response use the employee failure text. A persistence, tool, or internal error is shown as "request failed" and the body is not repeated in the summary or the technical detail. An ordinary invalid request is still shown. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-cli --bin tetonic -- persistence_and_tool_bodies_are_not_shown execution_failure_does_not_claim_the_model_never_replied` passed.

## 2026-09-26 — Local recovery does not list or abandon a scoped run

`/recovery` and `/recovery abandon` have no employee credential. A run whose task carries an execution scope is omitted from the report, including its session id. Abandon returns the same not-found text as an unauthenticated inspect and does not cancel the run. An unscoped interrupted run can still be reported and abandoned. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib -- unscoped_daemon_does_not_read_or_cancel_a_scoped_run interrupted_run_is_quarantined` passed.

## 2026-09-26 — A failed spawn does not report started

`execute_spawn` no longer emits `started` before the specialist is built. Depth, budget, and agent-build failures finish without that status. The status is reported only when the built specialist enters execution. A root turn still reports admission separately and does not emit a second start. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib -- spawn_does_not_report_started_before_the_specialist_is_built` passed. `code02_execute_spawn_keeps_inner_audit`, `code03_execute_spawn_still_turn_none`, `code03_execute_spawn_keeps_inner_audit`, `code03_approotexecute_delegates_to_manager_without_owning_attempt`, `code03_root_still_app_root_execute`, `code03_empty_execute_spawn_is_root_job`, and `gate01_app_root_execute_bound` passed.

## 2026-09-26 — Spawn rollback cannot be written onto a discussion

Recording a rolled-back spawn requires a legacy-local session or an execution-audit history. A private or team discussion id is refused. A rollback row planted directly in the database still hides that branch from authorized recall. A legacy session can still record one. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-memory --lib -- recall_filters_before_limit private_and_team_content_require_participation resume_omits_rolled_back_spawn_agent` passed.

## 2026-09-26 — Revoking authority drops an in-flight intent classification

While the coding intent classifier is calling the model, revocation of its execution authority is polled. The in-flight call is dropped when that authority is revoked, the same way cancellation drops it. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib -- classifier_stops_when_the_attempt_is_canceled classifier_stops_when_authority_is_revoked` passed.

## 2026-09-26 — The intent classifier does not run after the attempt is closed

Before the coding intent classifier calls the model, the attempt is checked for cancellation, an elapsed deadline, a closed work scope, and a revoked execution authority. An in-flight classification is dropped when cancellation, the deadline, or the work scope closes. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-run --test managed_service_tests -- elapsed_deadline_blocks_inference_before_the_gate revoked_authority_blocks_inference_before_the_gate` passed. The application crate compiled.

## 2026-09-26 — Outline drops symbols once the file is a database

File outline uses the same live-file check. A file that has become a SQLite database has an empty outline. A neighboring source file still has its symbols. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-index --lib search_hides_a_stale_preview_after_the_file_becomes_a_database` passed.

## 2026-09-26 — Definition search drops a symbol once the file is a database

Definition search uses the same live-file check as keyword search. A symbol whose source file has been replaced by a SQLite database is not returned, and a neighboring source file still is. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-index --lib search_hides_a_stale_preview_after_the_file_becomes_a_database` passed.

## 2026-09-26 — Search drops a preview once the file is a database

Keyword search, mention search, and semantic search omit a hit when the live file is a SQLite database, write-ahead log, shared-memory file, or rollback journal. That happens before the next index pass, so a stale text preview is not returned after the file is replaced. An ordinary source file still matches. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-index --lib search_hides_a_stale_preview_after_the_file_becomes_a_database` passed. `control_database_is_not_indexed` still passed.

## 2026-09-26 — Secret inference does not leave the local model resident

A local Ollama request whose data class is secret, including a secret context class, sends `keep_alive` of `0` so the runtime unloads the model when the call finishes. A non-secret request keeps the caller’s own `keep_alive`. This is not a remote-placement proof and not a team spend ledger. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-inference --lib secret_inference_does_not_keep_the_model_resident` passed.

## 2026-09-26 — Egress records cannot be attached to a discussion

An egress log row may omit a session or name a legacy-local session or an execution-audit history. A private discussion id is refused. A row planted directly in the database is still hidden from the legacy egress count. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-memory --lib -- legacy_readers_cannot_consume migrate_and_persist_roundtrip` passed.

## 2026-09-26 — Approval records cannot be stored on a discussion

Recording an approval or a proposed tool call requires a legacy-local session or an execution-audit history. A private discussion id is refused, so the approval detail is not written. A row planted directly in the database is still hidden from the legacy approval reader. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-memory --lib legacy_readers_cannot_consume_private_history_or_derived_summaries` passed.

## 2026-09-26 — Tool results and file snapshots cannot be stored on a discussion

`record_tool_call` and `record_file_change` accept a legacy-local session or an execution-audit history. A private or team discussion id is refused. Readers still hide a row that was planted directly in the database. Scoped recall of a tool result still works when that result is on the context's audit history. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-memory --lib -- legacy_readers_cannot_consume recall_filters_before_limit recall_finds_prior` passed. The product audit writers still passed.

## 2026-09-26 — A private session does not receive the legacy project briefing

Starting a private or team discussion does not load legacy project notes or the legacy session briefing, and it does not write a project link onto that session. A legacy session still receives its own project note. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-orchestrator --lib private_session_does_not_receive_legacy_project_memory` passed.

## 2026-09-26 — Session events cannot be attached to a private discussion

An event write is accepted only for a legacy-local session or an execution-audit history. A private discussion id is refused, so a redaction record is not stored there. An execution-audit history can still record one. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib store_sink_` and `cargo test -p tetonic-memory --lib migrate_and_persist_roundtrip` passed.

## 2026-09-26 — Legacy audit writers cannot append to a private session

The unscoped product audit and the coding projection both require a legacy-local session before writing a message. A private discussion id is refused, and the failure log does not include the message body. A legacy session still records its audit message. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib -- unscoped_audit_does_not_write_a_private_session projection_does_not_write_a_private_session scoped_audit_namespaces` passed.

## 2026-09-26 — Session errors do not repeat the session id

A missing session is reported as `unknown session_id`. The display text and the daemon error mapping no longer include the identifier. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib employee_message_hides_store_and_tool_bodies` and `cargo test -p tetonicd -- session_errors_do_not_echo golden_rpc_error_shape` passed.

## 2026-09-26 — Shell cannot start PowerShell or a credential helper

A model shell cannot run PowerShell, Bash, `sh`, or WSL with an inline-code flag, including `powershell -Command`. `cmdkey`, `vaultcmd`, and `git credential` are refused before they start. An ordinary command still runs. Verify parsing rejects the same inline flags regardless of letter case. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-tools --lib shell_refuses_powershell_and_credential_helpers` and `cargo test -p tetonic-tools --test exec_tests split_verify_rejects_shell_injection` passed.

## 2026-09-26 — Revoking execution stops the tool already running

A managed attempt rechecks its execution authority while it is running. When that authority is revoked, the attempt cancels and the cancellation reaches the tool it owns. Admission-time authorities that do not opt into revocation keep their original decision. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-run --test managed_service_tests revoking_execution_stops_the_owned_tool` passed. The existing managed-cancel and world-denial tests still passed.

## 2026-09-26 — Automatic approval cannot start an unconfined shell

When the operating system cannot enforce a high-risk control, such as network denial, automatic approval denies the shell. A person can still accept that gap. A confined shell can still be approved automatically. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib auto_grant_` passed.

## 2026-09-26 — Sandboxed commands do not use the operator temp directory

Model shell and the minimal command environment set `TMP`, `TEMP`, and `TMPDIR` to the workspace. Parent temp values are not copied. A profile that already chose a private temp directory keeps that directory instead of the operator temp root. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-sandbox --test process_executor_tests minimal_env_does_not_expose` passed.

## 2026-09-26 — Shell cannot run inline interpreter code

A model shell command cannot run `python`, `node`, `ruby`, `perl`, or `deno` with `-c`, `--command`, `-e`, or `--eval`, including when wrapped in another shell. An ordinary command still runs. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-tools --lib shell_refuses_inline_interpreter_code` passed.

## 2026-09-26 — Verify cannot run inline code

A verify command cannot pass `-c`, `--command`, `-e`, or `--eval`. `python -m pytest` and ordinary test commands still parse. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-tools --test exec_tests split_verify_rejects_shell_injection` passed.

## 2026-09-26 — Verify cannot use a path outside the workspace

A verify command that names an absolute path outside the workspace, walks up with `..`, or uses a home shortcut is refused before it runs. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-tools --lib verify_refuses_an_absolute_path_outside_the_workspace` passed.

## 2026-09-26 — Shell cannot walk out of the workspace

A model shell command that uses `..` is refused even when no protected store is configured. An ordinary command in the workspace still runs. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-tools --lib shell_refuses_parent_traversal_without_a_protected_store` passed.

## 2026-09-26 — Shell cannot use an absolute path outside the workspace

A model shell command that names an absolute path outside the workspace, or a home shortcut, is refused before it runs. An ordinary command in the workspace still runs. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-tools --lib shell_refuses_an_absolute_path_outside_the_workspace` passed.

## 2026-09-26 — Sandboxed commands do not see the operator profile

HOME and USERPROFILE for a sandboxed shell, verify command, or git process are the workspace directory, not the operator profile. A command cannot use those variables to read profile credential files. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-sandbox --test process_executor_tests minimal_env_does_not_expose_the_operator_profile` passed.

## 2026-09-26 — Git context does not include a protected store

Workspace git diff and status text used for model context omits a protected store file. A git argument that walks above the workspace is refused. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib git_diff_omits_a_protected_store` passed.

## 2026-09-26 — Stop aborts the intent classifier

The coding-session classifier sends the user text before the agent loop. Cancel during that call now drops it instead of waiting for the model to finish. Every managed attempt, including a coding session, uses the execution gate before later model and tool calls. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib classifier_stops_when_the_attempt_is_canceled` passed.

## 2026-09-26 — Shell cannot walk up to a protected store

A workspace shell or verify command that uses `..`, or names a protected store file, does not read that file when the store sits outside the workspace. An ordinary command in the workspace still runs. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-tools --lib shell_refuses_a_protected_store -- --test-threads=1` passed.

## 2026-09-26 — Local history commands do not repeat store errors

Checkpoint, restore, project, estate, and index commands no longer copy a database or lock error into the failure text. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib employee_message_hides_store_and_tool_bodies` passed.

## 2026-09-26 — Offline history does not show private sessions

`tetonic` history treats a private or missing session as an empty transcript. A store failure on that read does not include the database text. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib offline_transcript_hides_private_history` passed.

## 2026-09-26 — The local daemon does not read scoped runs

The daemon has no employee credential. Snapshot, replay, and cancel of a run with an execution scope now fail as an unknown run and do not return that run or cancel it. Credentialed control replay is unchanged. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib unscoped_daemon_does_not_read_or_cancel_a_scoped_run` passed.

## 2026-09-26 — Workspace recall does not return private history

A private message indexed under the same workspace as a local session is not returned by legacy recall. A recall failure no longer copies the database error into the tool result. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-memory --lib recall_history_does_not_return_private_context_in_the_same_workspace` passed. `cargo test -p tetonic-tools --lib` compiled the recall error change.

## 2026-09-26 — Default effectful admission is one attempt

A coding session run no longer opts into two simultaneous attempts on the same task. The recorded budget is one attempt, and a second attempt on that task is rejected. Registered jobs already used that default. Inference hops keep their own budget. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib default_effectful_admission_allows_one_attempt` passed.

## 2026-09-26 — Intent classifier keeps the session data class

The coding-session classifier sends the user text before the agent loop. That request now uses the session data class and disclosure tier. A secret session is not placed on a remote worker by the classifier default. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib classifier_keeps_the_session_data_class` passed.

## 2026-09-26 — Managed outcomes do not repeat store failures

When a managed run cannot inspect, claim, finalize, or persist cancellation, the candidate outcome says `request failed` instead of the database error. A project note whose bytes are a SQLite write-ahead log is not loaded into project memory. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-run --lib employee_message_hides_persistence_bodies`, `cargo test -p tetonic-domain --lib employee_message_hides_store_bodies`, and `cargo test -p tetonic-memory --lib project_md_write_ahead_log_is_not_loaded` passed.

## 2026-09-26 — Finalization failures do not publish store bodies

When a turn cannot be finalized, the completion event uses the employee message. A persistence failure is `request failed` and does not include the database text. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib employee_message_hides_store_and_tool_bodies` passed (library compile includes the finalization path).

## 2026-09-26 — Governed jobs stay classified as secret

Private and team registered jobs are classified as secret even when the operator host class is lower. A legacy context keeps the host class. The operator's requested class stays in the activation fingerprint, so two host classes do not collapse into one retry. The effective class is what inference placement sees, which keeps that prompt off a remote worker. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib governed_contexts_floor_the_host_data_class_to_secret` passed.

## 2026-09-26 — A private context job is not placed as repository data

A registered job in a private information context is classified as secret even when the operator host class is lower. Team and unknown contexts keep the host class. The effective class is part of the activation fingerprint. This keeps that prompt off a remote worker. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib private_context_floors_the_host_data_class_to_secret` passed.

## 2026-09-26 — Project memory does not load a SQLite project file

`.lokai/project.md` is not loaded when it is a SQLite database or a symlink. A normal project file is still included and still marked untrusted. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-memory --lib project_md_database_is_not_loaded` and `project_md_marked_untrusted` passed.

## 2026-09-26 — Language-server tools do not read a SQLite store

Definition, references, and diagnostics now refuse a SQLite database and its log, shared-memory, and journal files before the language server is opened. A normal source file can still use the language server. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-tools --lib lsp_does_not_read_a_sqlite_database` passed.

## 2026-09-26 — Briefing and verify detection do not read a SQLite store

Session briefing no longer follows a symlink or loads a conventions file that is a SQLite database. Verify-command detection uses the same no-follow reader, so a Makefile that is a database does not become a command and its bytes are not kept. A rollback journal beside a database is refused by workspace reads as well. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-orchestrator --lib sqlite_database_is_not_loaded_as_conventions`, `cargo test -p tetonic-tools --lib sqlite_makefile_is_not_a_verify_command`, and `cargo test -p tetonic-tools --lib read_refuses_a_sqlite_rollback_journal` passed.

## 2026-09-26 — A SQLite shared-memory file is not read or indexed

The `-shm` file beside a SQLite database has no header of its own, so readers and the code index now treat it as part of that database. `read_file`, search, and context compilation refuse it, and code search does not keep its text. A file whose name merely ends in `-shm`, with no database beside it, can still be read. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-tools --lib read_and_search_refuse_a_sqlite_shared_memory_file`, `cargo test -p tetonic-context --lib sqlite_shared_memory_file_is_not_searched_or_read`, and `cargo test -p tetonic-index --lib sqlite_shared_memory_file_is_not_indexed` passed.

## 2026-09-26 — A SQLite write-ahead log is not read or indexed

Readers and the code index now recognize the write-ahead log header as well as the database header. A log file with another name is refused by `read_file`, search, and context compilation, and it is not added to code search. The error and the index do not include the log bytes. A normal source file in the same directory remains available. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-tools --lib read_and_search_refuse_a_sqlite_write_ahead_log`, `cargo test -p tetonic-context --lib sqlite_write_ahead_log_is_not_searched_or_read`, and `cargo test -p tetonic-index --lib sqlite_write_ahead_log_is_not_indexed` passed.

## 2026-09-26 — Workspace reads refuse a SQLite database by header

`read_file`, search, edit, and context compilation refuse a file that starts with the SQLite header, even when it is not named `lokai.db`. The error does not include the file bytes. A normal text file in the same directory can still be read. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-tools --lib read_and_search_refuse_a_sqlite_database_by_header` and `cargo test -p tetonic-context --lib sqlite_database_is_not_searched_or_read` passed.

## 2026-09-26 — A SQLite file is not kept in the code index

Any file that starts with the SQLite header is skipped, not only `lokai.db`. If that path was previously indexed as text, the next pass deletes those search rows and does not read the rest of the file. A normal source file in the same tree remains searchable. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-index --lib sqlite_database_replacing_a_text_file_is_dropped_from_the_index` passed.

## 2026-09-26 — Turn failures do not repeat secrets

A failed, limited, or canceled turn outcome is scanned before it is stored as the completion error. A secret in that text is redacted. An ordinary message such as "file not found" is unchanged when no scanner is installed. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib failure_text_does_not_repeat_a_secret` passed.

## 2026-09-26 — Stopped diagnostics do not repeat secrets

A finish or stop reason is scanned before it is written to the event stream. A secret in that reason is redacted. With no scanner, the reason is not emitted. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib stopped_reason_does_not_repeat_a_secret` passed.

## 2026-09-26 — Private and missing resume errors are identical

Resuming a private discussion and resuming an unknown id now return the same `unknown session_id` error. The session id is not included. Daemon session start and reclassify use that employee text instead of the error's display form, so a store body is not added. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib private_history_resume_stays_unknown_after_restart` passed. `cargo check -p tetonicd --offline` passed.

## 2026-09-26 — Cancel wins before finish is recorded

A model response that asks to finish after the turn is already canceled does not record a completed outcome. The attempt gate also rejects further inference and tool starts once its work scope is canceled, including after an approval wait. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-core --lib cancel_before_finish_does_not_record_completion` passed. `cargo check -p tetonic-run --offline` passed.

## 2026-09-26 — Cancel during compaction does not start the next model call

The agent checks cancellation again after a summary call and before the next inference request. A turn that is canceled while older history is being summarized does not send the current user request. The same check applies when the attempt's work scope is already canceled. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-core --lib cancel_during_compaction_does_not_start_the_next_model_call` passed.

## 2026-09-26 — A later index pass drops a previously indexed control database

Skipping `lokai.db` and its sidecars no longer leaves older search rows in place. The next workspace index deletes those rows before search can return them. A normal source file in the same tree remains searchable. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-index --lib control_database_is_not_indexed` passed.

## 2026-09-26 — Inference placement affinity does not cross sessions

The shared provider keeps a worker preference only for the same session and turn. A different session, including another information context, clears that preference before the next placement. A call with no session and no turn clears it as well. This is not a conversation cache and does not isolate a remote worker's own memory. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-inference --lib sync_turn` passed.

## 2026-09-26 — Employee turn and job errors hide store bodies

A failed chat turn, one-shot task, and `tetonic job run` now show `request failed` for a persistence, tool, or internal failure. The event and the CLI no longer repeat the database or tool body. A missing session stays `unknown session_id`. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib employee_message_hides_store_and_tool_bodies` and `tui_mvp_planning_failure_reports_error_and_releases_session` passed. `cargo check -p tetonic-cli --offline` passed.

## 2026-09-26 — Session model lookups do not echo identifiers

Daemon model catalog, model selection, and inference lookups now return `unknown session_id` for a missing or conflicting session. A persistence failure is `request failed`. Neither response includes the session id or the store body. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonicd session_errors_do_not_echo_the_identifier_or_payload` passed.

## 2026-09-26 — Discussion status rechecks membership

Reading whether a discussion is open now uses one snapshot and checks membership again before the status is returned. After the member is removed, the status is denied. A missing id for a current member is still empty rather than an error. Scoped recall's tool description now matches the search: authorized messages and revalidated tool results, not other contexts. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-memory --lib closing_requires_current_access_and_preserves_history` passed.

## 2026-09-26 — Hidden daemon failures are not logged

The server log for a hidden persistence, tool, or internal failure no longer includes the error body. The client still receives `request failed`. Recall index failures log the session id without the index error text. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonicd hidden_failures_are_not_written_to_the_log` passed.

## 2026-09-26 — Remaining daemon failures stay generic

Capacity, policy, fabric, initialize, and secret-rule failures now use the same hiding as the other RPC methods. A persistence or internal failure is `request failed`. A failed capacity optimize reports that same text in its job error instead of the provider or database body. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo check -p tetonicd --tests` passed.

## 2026-09-26 — Revocation blocks a context note before it is stored

Adding a project note and consolidating a scoped session now hold the database write lock across the membership check and the write. A member who has been removed cannot store a new note. The rejected text is not inserted. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-memory --lib private_project_memory_stays_out_of_other_contexts` passed.

## 2026-09-26 — Session end, cancel, and approval errors stay generic

Daemon session end, session cancel, and approval responses now use the same error mapping as the other RPC methods. A persistence or tool failure is returned as `request failed` and the private text stays in the server log. A missing live session stays `unknown session_id`. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo check -p tetonicd --tests` passed. The payload hiding itself is covered by `cargo test -p tetonicd internal_failures_do_not_echo_payloads`.

## 2026-09-26 — Context search does not read control-store sidecars

Workspace context search and direct file reads refuse `lokai.db` and its write-ahead sidecars before opening them. A text-shaped sidecar is not returned as search text, and the refusal does not include the file bytes. A normal source file in the same directory is still searchable. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-context --lib control_store_sidecar_is_not_searched_or_read` passed.

## 2026-09-26 — Guessed expansion handles look unknown

An expansion handle from another session, run, or task returns the same error as a handle that does not exist. An expired handle does too. The error does not include the expanded text. A handle that matches the caller can still be expanded. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-context --lib each_owner_dimension_is_required_before_reading_or_consuming_use` passed.

## 2026-09-26 — Reported token ceiling stops the next model call

A host can set `reported_token_ceiling` on a registered job. After the provider reports prompt and output tokens, the next model call does not start once that total reaches the ceiling. Usage the provider does not report is not counted and is not invented. This is a per-job stop, not a team spend ledger. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-core --lib token_ceiling` passed.

## 2026-09-26 — Context bindings are not tool grants

A requested capability is admitted only when the agent advertises that tool. Listing the same name on the identity's context bindings no longer satisfies the check. A job that requests `recall` while the agent does not offer it is rejected before admission. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-run --test managed_service_tests context_bindings_do_not_grant_unadvertised_capabilities` passed.

## 2026-09-26 — Legacy turns cannot write private sessions

Planning a legacy turn now requires a legacy-local session before it stores the user message. A private discussion and a missing id return the same unknown-session error, and neither gains a message. The private canary stays the only row. A real legacy session still records its turn. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib legacy_turn_cannot_write_a_private_or_missing_session` passed.

## 2026-09-26 — Canceled attempts do not call the intent classifier

The coding-session intent classifier checks cancellation immediately before it would call the model. A denied or canceled attempt keeps the keyword route and does not send that request. The turn also stops when the managed attempt is already canceled, instead of continuing into the agent loop. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-orchestrator --lib llm_route_task_does_not_call_the_provider_when_execution_is_denied` passed.

## 2026-09-26 — Job retry returns the same event receipts

`tetonic job run` now includes the durable journal locators for the run: sequence, event type, and payload digest. The event payload stays in the run log. Retrying a finished request returns those same receipts and does not start another execution. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib launch_reports_a_terminal_outcome_and_rejects_bad_host_settings` passed.

## 2026-09-26 — Backup restore keeps teams and private history

A verified pre-migration snapshot of the control database restores the team, its membership, and both private and team discussions. The private canary is readable by its owner after restore and still denied to another organization member. The team message remains readable by that member. The original database is unchanged. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-memory --lib backup_restore_keeps_team_resources_and_private_history` passed.

## 2026-09-26 — Managed cancel stops the process the attempt owns

`cancel_run` now reaches a process started by the attempt's tool. The tool runs `ping` through the same sandbox waiter used for commands, and cancellation stops that process and returns before the command's own timeout. The receipt is canceled. Registered jobs still do not start a shell. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-run --test managed_service_tests managed_cancel_stops_the_owned_process` passed.

## 2026-09-26 — Code search does not index the control database

The workspace index skips `lokai.db` and its write-ahead sidecars before reading them, so a text-shaped database file is not searchable as source. A normal source file in the same directory still indexes. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-index --lib control_database_is_not_indexed` passed.

## 2026-09-26 — Directory listings omit the control database

`list_dir` and `glob` skip the protected audit database, and language-server tools refuse that path before they open a session. Search already skipped it. A normal workspace file is still listed and readable. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-tools --lib read_and_search_refuse_the_control_database` passed.

## 2026-09-26 — Daemon errors no longer echo internal payloads

Persistence, tool, and internal failures returned over the daemon RPC now say `request failed`. The private text stays in the server log. Unknown sessions stay `unknown session_id`. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonicd internal_failures_do_not_echo_payloads` passed.

## 2026-09-26 — Verify commands cannot name a protected store file

Host verify runs through a separate command path from model shell. A verify command that names the audit database or its full path now fails before launch and does not return the file bytes. Other verify commands are unchanged. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-tools --lib shell_refuses_a_protected_store_inside_the_workspace` passed.

## 2026-09-26 — Shell cannot read a protected store inside the workspace

File tools already skip the audit database. A shell in that same workspace could still open it, because the sandbox does not carve one file out of the workspace tree. Shell now refuses before launch when a protected store file is inside the workspace, or when the command names that path. The refusal does not include the file bytes. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-tools --lib shell_refuses_a_protected_store_inside_the_workspace` passed.

## 2026-09-26 — Context compilation cannot read the control database

The workspace file hook used by context compilation now refuses the audit database and its SQLite sidecars, the same grant as file tools. A normal workspace file is still readable. The error does not include the database bytes. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib compiler_file_hook_refuses_the_control_database` passed.

## 2026-09-26 — Workspace tools cannot read the control database

File reads, writes, and search skip the audit database and its SQLite sidecars when that path is bound to the tool host. A file in the same workspace is still readable. The refusal does not include the database bytes. Registered jobs protect the store path even when recall is not requested. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-tools --lib read_and_search_refuse_the_control_database` passed.

## 2026-09-26 — A live session handle rechecks membership

`open_live` returns a handle instead of the raw conversation. Taking or restoring that conversation checks membership again. After the member is removed, the handle that was already issued is denied, and the conversation stays in the registry. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib scoped_live_lookup_requires_current_membership` passed.

## 2026-09-26 — Guessed discussion ids do not create history

Employee `context open --session` only reopens an open discussion in that context. A missing id and an id from another context are both denied, and neither inserts a row. `context open` without `--session` creates a discussion and returns a server-chosen id. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-memory --lib guessed_discussion_id_does_not_create_or_reveal_another_context` passed.

## 2026-09-26 — A finished job retry keeps its terminal outcome

`tetonic job run` for an already finished request does not start another execution. The receipt now repeats the stored terminal state: completed, failed, or canceled. Active, canceling, and recovery stay blank rather than being called running. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib launch_reports_a_terminal_outcome_and_rejects_bad_host_settings` passed.

## 2026-09-26 — Legacy live listing omits scoped sessions

`SessionLiveStore::all` returns only `legacy-local` registrations. A private live session stays out of that list. The full map remains available only to in-process shutdown drain. Context recall now includes authorized tool results, and the control command says so. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib legacy_registry_cannot_read_or_remove_a_scoped_session` passed.

## 2026-09-26 — Cancel stops an owned command and the processes it started

Outside the OS sandbox, cancellation used to stop only the direct child. It now stops that process and the processes it started: a Windows job tree via `taskkill /T`, or the Unix process group created for the command. A ping child is stopped by cancel in under five seconds. The sandboxed Windows job path was already covered. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-sandbox --lib cancel_stops_the_spawned_process` passed.

## 2026-09-26 — Restart does not resume private history as a legacy session

After the process reopens the database, resuming a private discussion id through the legacy session door returns the same unknown-session error as a missing id. The error does not contain the private text, and no live session is created. The legacy transcript API still cannot read that discussion. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib private_history_resume_stays_unknown_after_restart` passed.

## 2026-09-26 — Managed cancel reaches the attempt's tool

A managed identity attempt that is inside a blocking tool observes `cancel_run` on that tool's cancellation signal, and the receipt is canceled. This is the same signal the sandbox uses to stop a process. The new test does not spawn an operating-system child. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-run --test managed_service_tests managed_cancel_reaches_a_blocking_tool` passed.

## 2026-09-26 — History reads recheck membership before return

Scoped recall, scoped transcripts, and context project memory check membership again after the read commits and before the content is returned. A project note write does the same immediately before insert. This does not add a concurrent revocation test. Existing revocation tests still deny the next call. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-memory --lib -- recall_filters_before_limit_revalidates_sources_and_checks_revocation private_project_memory_stays_out_of_other_contexts private_and_team_content_require_participation_not_administration` passed.

## 2026-09-26 — Scoped recall includes revalidated tool results

Authorized recall now returns tool results from the requested context, not only messages. A hit is kept only when the current tool call is still successful and its stored body matches the index. A private tool canary is visible to the private context and absent from team recall. A denied tool call and a deleted tool call are not returned. Revocation still denies the next query. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-memory --lib recall_filters_before_limit_revalidates_sources_and_checks_revocation` passed.

## 2026-09-26 — Private approvals do not become global rules

`get_approval` and approval/egress counts now return the same empty result for a private session as for an unknown id. Remembering an approval installs a global allow rule only for a legacy session. A private shell approval can still be stored, but it does not match later sessions. Legacy approval restore still works. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-memory --lib legacy_readers_cannot_consume_private_history_or_derived_summaries` passed. `cargo test -p tetonic-app --lib granted_approval_is_restored_on_reopen` passed.

## 2026-09-26 — Workspace undo cannot read private file bodies

Legacy file-change reads, workspace undo range, and the workspace mark now include only `legacy-local` sessions. A private change id returns no body, the same as a missing id. A legacy change in the same workspace is still visible. Classification writes and project links on the legacy API require a legacy session. Scoped audit rows can still be written by the owning history path. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-memory --lib -- legacy_readers_cannot_consume_private_history checkpoint_undo_redo_timeline session_data_class_persisted` passed.

## 2026-09-26 — Legacy session lookups hide scoped sessions

`session_workspace`, `session_workspace_root`, `session_status`, and `message_count` now return the same empty result for a private session as for an unknown id. Legacy resume therefore cannot tell those cases apart or read the private workspace. Turn-operation writes require a legacy session. Scoped recall indexing still uses an internal locator, and an authorized principal reads discussion status through `context_discussion_status`, where a missing id and a foreign id are both absent. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-memory --lib -- legacy_readers_cannot_consume_private_history closing_requires_current_access publication_copies` passed. `cargo test -p tetonic-memory --lib recall_filters_before_limit_revalidates_sources_and_checks_revocation` passed.

## 2026-09-26 — Managed submission runs a world attempt

`submit_identity_job_with_context` now drives an agent that carries a world adapter through the existing admission, claim, execution, and finalization owner. One test records a completed world effect in the run receipt. A second test admits with an authority that passes admission and the pre-execution check, then denies the effect: the adapter is not called, and `cancel_run` ends the idle wait with a canceled receipt. The standalone server still starts its own world executor. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-run --test managed_service_tests -- managed_world_attempt_reaches_the_adapter_and_records_completion managed_world_denial_never_reaches_the_adapter_and_cancel_stops_the_wait` passed.

## 2026-09-26 — Managed attempts can run the world executor

`Agent::with_world_adapter` marks an attempt as world work. `ManagedRunService::execute_attempt` then uses `LocalWorldAttemptExecutor` and the same cancellation select as a coding turn. Coding attempts with no world adapter still use `LocalAgentAttemptExecutor`. The standalone server still starts its own world executor instead of admitting a managed run. No managed integration test drives a world adapter through admission yet. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-core --lib agent_is_send_and_sync_without_unsafe_overrides` passed. `cargo check -p tetonic-run --offline` passed.

## 2026-09-26 — World attempts use the same executor trait

`LocalWorldAttemptExecutor` now implements `AgentAttemptExecutor`, the same trait as a coding turn. It stamps one attempt id and refuses a second id before the world adapter is opened. The standalone server starts the world through that executor. Cancellation still ends the perception wait with a canceled outcome. The managed run service does not admit or launch world jobs yet, and this does not retire the server loop. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-runtime --lib world_executor_rejects_a_second_attempt_before_opening_the_world` passed. `cargo check -p tetonic-server --offline` passed.

## 2026-09-26 — Reject unsupported registered shells before inference

Registered submission now refuses `run_shell` before it writes an execution-audit history or contacts inference. Recall, finish, and workspace-jailed file tools remain supported. A host allow list that includes the shell does not start a process. This is not a complete process, credential, and egress matrix, and it does not add a sandboxed shell profile. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib -- shell_is_outside_the_registered_isolation_matrix registered_shell_is_rejected_before_inference` passed.

## 2026-09-26 — Reject preparation when an admission ceiling is full

Registered job preparation now checks the organization, principal, and team held-run ceilings before it writes an execution-audit history or assembles the agent. A full team ceiling returns `TeamCapacityExceeded` and does not create that history. A private run is not charged against the team ceiling. The run-command transaction still decides races where two preparations both observe free capacity. This is not a waiting queue, cumulative token budget, or fair scheduler. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-memory --lib team_ceiling_blocks_a_second_held_run_without_canceling_the_first` passed. `cargo check -p tetonic-app --offline` passed.

## 2026-09-26 — Authorized run event poll

`ContextService::poll_run` reads the next durable lifecycle events only after the same membership check as inspection and replay. An empty batch on a terminal run is caught up. A retention gap is returned as a gap, not invented events. `tetonic control replay-run --follow` repeats that poll until the run is caught up, a gap is reported, or access is denied. Revoking the credential makes the next poll deny without returning event payloads. This is not an unrestricted live fanout and does not stream model tokens. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib poll_run_stops_when_the_credential_is_revoked` passed. `cargo check -p tetonic-cli --offline` passed.

## 2026-09-26 — Membership-checked live session lookup

`ContextService::open_live` verifies the credential and current context membership before returning a process-local live conversation, then checks membership again before the handle is released. A legacy lookup still cannot see a scoped registration. An administrator, a wrong context, and a removed member are all denied, with the same denial as a missing session. Holding the returned handle is not a grant that survives a later membership change. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib scoped_live_lookup_requires_current_membership` passed.

## 2026-09-26 — Server shutdown cancels the world attempt

The standalone world server now binds the agent to an attempt scope and a cancellation gate. Ctrl-C cancels that scope and waits for `run_in_world` to return, instead of dropping the loop. The gate denies the next effect after cancellation. The world loop is still not the managed run service. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-core --lib cancellation_denies_the_next_effect` passed. `cargo check -p tetonic-server --offline` passed.

## 2026-09-26 — Denied world authority does not reach the adapter

`Agent::run_in_world` checks the bound execution gate after manifest and E-stop validation and before `adapter.execute`. A denial drops the action and aborts staged mutations. The world adapter is not called. Hosts that do not bind a gate, including the current standalone server loop, keep the previous manifest and E-stop checks. This does not move that server loop onto the managed runtime. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-core --lib test_run_in_world` passed, including `test_run_in_world_denied_authority_does_not_reach_adapter`.

## 2026-09-26 — Cancel an idle world wait before dispatch

`Agent::run_in_world` now leaves the perception wait when the attempt scope is canceled, instead of blocking until the next world tick. Cancellation is checked again immediately before `adapter.execute`, so a canceled idle wait does not deliver an action. Manifest rejection and E-stop still skip the adapter. This does not move the world loop out of server main, authorize world effects through the managed capability gateway, or retire `run_in_world`. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-core --lib test_run_in_world` passed, including `test_run_in_world_idle_cancel_does_not_dispatch`.

## 2026-09-26 — Noncoding job without a repository

A registered general job whose tools are only `recall` and `finish` now runs through the same managed executor with `workspace_root: None`. File, search, and shell tools are refused before any repository path is opened. Asking the host to allow `read_file` without a workspace returns `WorkspaceUnavailable` and does not start inference. The recall job's inference request and scoped transcript contain a note that was stored only in private history. Operator host settings may omit `workspace`. This does not move the Village loop, extract coding prompts, or prove process isolation. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-tools --lib without_repository_serves_recall_and_refuses_files` and `cargo test -p tetonic-app --lib noncoding_recall_job_runs_without_a_repository` passed.

## 2026-09-26 — Local registered job launch

`tetonic job run` launches a registered job through `launch_registered_job` and the existing managed executor. Operator host settings are a JSON file. The employee request cannot set the workspace, model, tool ceiling, or deadline. The process waits on the same local task as admission. The receipt contains locators and, for the winning launch, a terminal outcome: completed, canceled, limited, or failed. It never reports Running. Invalid host settings fail before admission and produce no receipt. A retry of the same request returns locators only. This is a local command, not a remote setup UI, queue, or isolation matrix. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib launch_reports_a_terminal_outcome_and_rejects_bad_host_settings` passed.

## 2026-09-26 — Team admission ceiling

Schema 45 adds `team_execution_limits` and `run_projections.execution_team_id`. Creating a team records a default ceiling of four concurrent admitted runs. Admission counts held runs for that team inside the existing run-command transaction and returns `TeamCapacityExceeded` when the ceiling is full. Private-context runs are not charged to a team. Lowering the ceiling does not cancel work already admitted. `tetonic control team-execution-limits` reads and compare-and-sets the ceiling. This is concurrency, not cumulative token or money spend, a queue, or a process-isolation guarantee. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-memory --lib -- team_ceiling_blocks_a_second_held_run_without_canceling_the_first v40_upgrade_preserves fresh_open_is_schema_target` passed. `cargo check -p tetonic-cli -p tetonicd -p tetonic-run -p tetonic-app --offline` passed.

## 2026-09-26 — Bind live sessions to an information context

`SessionLiveStore` now records the information context on each registration. Legacy `get`, `contains`, and `remove` only see `legacy-local`. A scoped registration is returned or removed only when the caller names that same context; a wrong context looks the same as a missing ID. Registering the same session ID again conflicts. Process drain still sees every registration because it is not an employee lookup. This does not check membership credentials inside the registry, subscribe to events, or activate a scoped live agent. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib legacy_registry_cannot_read_or_remove_a_scoped_session` passed.

## 2026-09-26 — Team execution privacy canary

A registered team job, using the production runtime and a capturing inference server, recalled with the granted team context. A unique secret stored only in private history was absent from the team inference requests, the team audit transcript, and legacy product events. The same secret was still absent from the private discussion after publication. After an authorized publication into the team discussion, a second team job's recall tool result contained the copy. The private transcript remained one message. This does not authorize live-session lookup, event subscriptions, employee launch transport, org/team budgets, or a noncoding external-tool workload. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-app --lib team_execution_cannot_retrieve_unpublished_private_history` passed.

## 2026-09-26 — Explicit context publication

Schema 44 adds `context_publications`. `publish_context_message` copies one stored user or assistant message into an open destination discussion. The caller cannot supply replacement text. The receipt records publisher, source coordinates, source digest, destination, and time, and does not include the body. The same request returns the original receipt; a reused request that names different source coordinates conflicts. Source and destination membership are both required in the write transaction. A team member who cannot read the private context cannot publish it, and team recall stays empty until publication. After publication, team recall and the destination transcript see the copy, while the private transcript stays one message and is still unreadable to the team. Removing team membership blocks later recall of the copy. This does not authorize live-session lookup, event subscriptions, or a capturing inference provider. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-memory --lib -- publication_copies_into_destination fresh_open_is_schema_target sql_errors_roll_back_every_migration_marker upgrading_preserves_history` passed.

## 2026-09-26 — Scope project notes and digests

Schema 43 adds `project_memory.context_id`. Existing rows become explicit `legacy-local` knowledge; the migration does not infer an owner. Legacy load, notes, status, and consolidation read and write only that context. `load_context_project_memory`, `add_context_project_note`, and `consolidate_context_session` require current content access and keep the result in the named context. A guessed or cross-context session is denied before its text is read. ContextService exposes the same three operations after credential verification. Repository `project.md` remains an untrusted file grant, separate from private or team memory. This does not publish across contexts, authorize live-session lookup, or prove the end-to-end inference canary. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-memory --lib -- project_memory fresh_open_is_schema_target legacy_readers_cannot_consume upgrading_preserves_history sql_errors_roll_back_every_migration_marker project_digest consolidate_skipped project_md_marked` passed.

## 2026-09-26 — Fence legacy briefing workspace scans

`list_recent_sessions_for_workspace` and `recent_touched_paths` previously returned every session and edited path on a workspace, including private and team contexts. Both now require `context_id='legacy-local'`, the same fence already used by recall and recent finish summaries. A private canary path and private session no longer appear in those legacy briefing inputs. This does not scope project digests to an information context, authorize live-session lookup, or prove the end-to-end private-versus-team inference canary. Sprint 1 and Sprint 2 exit criteria remain unmet.

Validation: `cargo test -p tetonic-memory --lib legacy_readers_cannot_consume_private_history_or_derived_summaries` passed.



## 2026-09-25 — First persistence and boundary corrections

MVP-001 and MVP-002 are in progress. MVP-101 has a storage prerequisite implemented; neither the resource service nor organization authorization is complete. No existing fleet caller has been cut over, and no retirement gate is closed.

### Implemented

- Reuse the existing SQLite Store, migration transaction and pre-migration backup machinery. Schema 29 adds organizations and organization-scoped teams. Team ownership is a principal reference, not a grant or authentication assertion.
- Require organization plus team ID for team lookup. Exact creation retries succeed; conflicting attributes fail without overwriting ownership. Concurrent creators cannot both replace the same team. Creation does not activate an agent or charge inference.
- Preserve existing identity records across migration. Existing future-schema checks continue to reject unsupported older writers.
- Correct the IntentCharter verb helper: namespace allowlists reject malformed/unscoped verbs, and path/resource boundaries explicitly reject because this helper cannot enforce them. Real effect-boundary policy and budget integration is still required for D13.

### Validation

Commands run from `engine/`:

| Check | Result |
|---|---|
| Baseline: `cargo test -p tetonic-domain -p tetonic-memory -p tetonic-run --lib` | 30 domain, 69 memory, 9 run tests passed before changes |
| Updated domain tests, in the same combined command | 32 passed |
| Final: `cargo test -p tetonic-memory -p tetonic-run --lib` | 74 memory and 9 run tests passed |
| `cargo check -p tetonic-app` | Passed, including downstream compilation against the changed store |
| `cargo run -p tetonic-arch-gate -- arch` | Passed |

The memory tests cover reopen, scoped keys, conflicting retries, concurrent ownership creation, invalid/orphan resources, upgrade preservation and existing migration crash/recovery scenarios. An intermediate run exposed three downgrade fixtures that leave newer tables behind; the migration was made consistent with the existing idempotent table-creation pattern and all memory tests then passed. This is not evidence of authenticated tenant isolation, end-to-end execution, HA, or a production deployment. Full-workspace tests were not run.

### Contracts used for this slice

The local preview uses one local SQLite authority. It must not be deployed as a shared database file across machines. Organization IDs scope stored resources; trusted caller identity and authorization must be established in a service before exposing these methods through an API. Personal/team context separation cannot be inferred from these tables.

New team resources do not mirror or dual-write the prototype FleetManager maps. They remain unused by product entrypoints until the authorized service and consumer migration are ready. A storage prerequisite is not a second lifecycle authority.

### Next integration work

1. Finish MVP-001 decisions: canonical agent identity and immutable definition revisions, principal/membership model, supported OS/isolation profile, production storage/ownership topology, and numeric capacity/recovery targets.
2. Extend MVP-002 characterization to fake Running/quota behavior, coding/world activation, approval waits and history restore before removing their consumers.
3. Build the authorized resource service and definition/membership persistence for MVP-101; migrate fleet callers through that service into the existing managed execution path.
4. Prove private/team context separation under MVP-102 before exposing shared team execution.
5. Cut over callers and delete prototype authorities only after their replacement behavior passes the retirement gates.

The original inventory and validation report are historical audit evidence. Implementation changes intentionally diverge from their file hashes; do not regenerate that baseline to conceal drift.

## 2026-09-25 — Application resource service boundary

MVP-101 now has an application-composed ResourceService for organization/team creation and reads. It reuses the managed service's SharedStore and refuses composition without storage. An explicitly injected ResourceAuthority must authenticate and authorize each exact action before storage access, including retries. Team ownership comes from the resulting principal; it cannot be supplied in the request. No default allow provider, second database or execution manager was added.

The [service contract](sprints/mvp-1-resources-privacy/resource-service-contract.md) records the trust boundary and pending production requirements. The current authority implementation exists only in tests. Credential verification, durable memberships/grants, administrative audit events, transport exposure and fleet caller migration are still unfinished. This is not a claim of production authentication or end-to-end team isolation. The legacy fleet module description now identifies its in-memory prototype status instead of claiming persistent supervision.

The architecture gate (`cargo run --manifest-path engine/Cargo.toml -p tetonic-arch-gate -- arch`) passed after these changes. Documentation checks resolved 78 local links and passed formatting checks.

Validation: `cargo test --manifest-path engine/Cargo.toml -p tetonic-app --lib resources::tests` passed all four tests after a clean rebuild. Tests exercise application/store composition, persistence across recomposition, ownership conflicts, denied scopes/actions, revocation on reads/retries, and refusal of volatile storage. Focused rustfmt and `git diff --check` passed. Compilation initially failed because the disk was full; user-authorized `cargo clean` removed 100.7 GiB of generated artifacts. The successful rebuild disabled incremental compilation and debug symbols through per-command environment settings; no repository build profile was changed.

## 2026-09-25 — Durable membership authority

Schema 30 adds principals, organization roles and explicit team memberships. Control access is decided in one SQL statement using current enabled state, organization membership and team scope. Missing membership denies; organization removal deletes explicit team memberships; principal disablement overrides all metadata permissions. Upgrade from schema 29 preserves resources without creating implicit grants. The schema-29 migration now checks its marker before insertion so later upgrades can safely reuse the migration chain.

Application::membership_resource_service composes this persistent authority with a required CredentialVerifier. The test verifier proves the service uses a verified stable identity rather than the credential text as ownership, and consults current membership on later requests. A real credential provider, administrative mutation APIs/audit, immutable agent revisions and fleet cutover remain outstanding. Permission and revocation semantics are specified in the service contract linked above.

Storage validation: all 77 tetonic-memory library tests passed, including v29 upgrade, scoped permissions, durable revocation and the existing migration failure/crash matrix. Build commands continue using per-command incremental/debug settings to limit artifact growth.

Application validation: all five `resources::tests` passed. The architecture gate passed. After adding administrator/demotion assertions, the three membership storage tests passed again. Production credential verification is not covered by these tests and is not implemented by this slice.

## 2026-09-25 — Local control credentials

Schema 31 stores credential hashes, audience, issuance/expiry times, revocation and transactional lifecycle events. Application::local_credentials supplies a real local verifier to the membership resource service. Issuing a credential requires an existing enabled principal and grants no role. Per-command provisioning stays internal; no unauthenticated issuance endpoint was added. Existing sha2 moved from the application's dev dependencies to runtime dependencies; UUID v4 is reused for random secrets.

The current service contract specifies lifetime limits, hash-only persistence, audience isolation, independent revocation, redacted Debug output and remaining bootstrap/transport/admin-audit work. The v30 migration now checks its marker, preserving the upgrade chain. Older fixture databases explicitly remove the newer credential tables before simulating their historical schema.

Validation so far: all 80 memory library tests passed, covering time boundaries, wrong audience, disabled principals, restart persistence, independent revocation state, v30 migration preservation and atomic rollback when credential-event writes fail. This does not establish an end-user login experience or complete MVP-101.

Final slice validation: all six application resource tests passed, including real local credential issuance through team creation/read, wrong-audience denial, independent revoke and service recomposition. The architecture gate, focused rustfmt, whitespace and documentation-link checks passed. No HTTP endpoint or external identity provider was tested or added.

## 2026-09-25 — Local operator vertical slice

The existing lokai CLI now exposes `control` commands for one-time bootstrap, local credential issuance/revocation and authenticated team creation/inspection. It composes the same resource, credential and membership implementations from a chosen SharedStore without starting Application's coding runtime. LocalControl is a composition facade, not another lifecycle manager or persistence authority. The [runbook](sprints/mvp-1-resources-privacy/local-control-runbook.md) provides a PowerShell walkthrough and trust limits.

Schema 32 adds administrative events. Bootstrap is serialized in one transaction and refuses any already initialized control principal set. The bootstrap event labels the actor as a local operator without pretending an employee identity was authenticated. Audit failure rolls back all initialization; concurrent bootstrap attempts have one winner. Credential issuance is a separate recoverable step. Existing membership/credential APIs remain internal provisioning doors until authenticated administration is added.

Initial validation: `cargo test --manifest-path engine/Cargo.toml -p tetonic-memory -p lokai-cli` passed the CLI unit suite, 82 memory library tests and three store-concurrency integration tests. A separate CLI-process integration test exercises the usable local path. No network endpoint, remote client or agent activation is implied by these commands.

Final validation: the separate-process `control_cli` test passed, including rejection of volatile storage, missing/revoked credentials and repeated bootstrap. The architecture gate passed. The CLI unit suite contained 112 tests: 110 passed and two were ignored. The local runbook demonstrates the path without writing bearer secrets into command arguments. User documents outside this epic were preserved.

## 2026-09-25 — Retired binary branding correction

Per the product correction, the shipped CLI binary is `tetonic` and the existing stdio daemon is `tetonicd`. `tetonic-server` retains its existing name. CLI help/examples, terminal branding, daemon identification, executable locators, installers, release archives and the local-control runbook now use Tetonic. The release workflow no longer emits duplicate Lokai-named archives. No release was published and no installer was executed against the host.

Legacy Cargo package IDs/source directories (`lokai-cli`, `lokaid`), persisted database/config paths, environment compatibility names and enrollment wire prefixes remain unchanged. These are migration concerns rather than public binary names; renaming them blindly would break stored state or source-path checks. Historical audit inventories remain historical. The generated mock LSP server is test tooling rather than a product binary.

Validation: Cargo metadata exposes `tetonic` and `tetonicd`, with no `lokai`/`lokaid` binary targets. Both binaries built; CLI tests passed (110 passed, two existing ignores), daemon tests passed (48), and the control integration test passed with `CARGO_BIN_EXE_tetonic`. The architecture gate, PowerShell installer AST parse, Git Bash installer syntax check and release/installer reference checks passed. Release YAML was inspected but a YAML parser was unavailable. Packaging was not executed on the cross-platform release matrix.

## 2026-09-25 — Source folder and package branding

The naming correction now includes source folders and Cargo package IDs: `engine/litho/tetonic-cli` (package `tetonic-cli`, binary `tetonic`) and `engine/litho/tetonicd` (package/binary `tetonicd`). The daemon smoke script is `engine/scripts/smoke_tetonicd.py`. Build commands, release/CI selectors, source-path tests, architecture checks, generated-protocol comments and active documentation references were migrated. This supersedes the preceding decision to retain legacy package IDs.

Historical audit inventories retain their original baseline paths/hashes; historical prose is not rewritten into evidence of a newer tree. Documentation source links now resolve to the renamed folders. The `.lokai` malicious-project security fixture and persisted user-data/config names remain legacy-format compatibility cases, not product source package names. The user's checkout directory is outside the versioned source rename.

Validation: renamed CLI/daemon suites, CLI process integration and all 89 architecture-checker unit tests passed; the live architecture gate and epic documentation links passed. All six selected application integration suites that pin CLI, composition, gate, portal and work source paths passed against the renamed folders.

## 2026-09-25 — Authenticated organization membership administration

MVP-101 now exposes organization role grants/removal through the resource service and local `tetonic control set-member` / `remove-member` commands. The actor comes from verified credentials; durable enabled administrator membership is checked again inside the transaction that changes membership and records actor, subject, scope and requested role. Last-enabled-administrator removal/demotion is rejected. Team-creator and platform-only identities cannot administer organization membership. This does not grant execution, tool or private knowledge access.

Trusted local `register-principal` provisions an unprivileged identity with an audit record, without allowing retries to alter an existing principal's enabled or platform-admin fields. The schema remains version 32. Credential revocation remains admission-time: the transaction rechecks actor membership, not the already-verified credential. Remote transport, team-specific membership administration, audit browsing, operator identity/recovery, immutable agent revisions and privacy enforcement remain unfinished.

Validation: 85 memory-store tests, six application resource tests, two separate-process CLI scenarios and the architecture gate passed. Added evidence covers self-escalation/cross-org denial, revoked administrators, last-admin protection, concurrent cross-removal, rollback on audit failure, and grant/create/revoke/deny across CLI processes. All test databases are temporary.

## 2026-09-25 — Explicit team membership administration

Added authenticated `add-team-member` / `remove-team-member` CLI and resource-service operations. Current team owners and organization administrators may manage explicit team metadata membership; ordinary members cannot grant themselves access, and recipients must already belong to the organization. Transactional authority rechecks and audit writes follow the organization administration pattern. Removing explicit membership does not remove independent owner/admin privileges.

Schema 33 adds `team_id` to administrative audit events while preserving earlier records. The previous migration now skips an already-applied marker; legacy downgrade test fixtures remove newer schema artifacts before replay. Existing automatic pre-migration backup, rollback and crash-recovery machinery remains in use.

Validation: all 87 memory tests, six application resource tests, both separate-process CLI scenarios and the architecture gate passed. Evidence covers team scope, nonmember denial, owner removal, transactional audit failure, version-32 upgrade preservation, and explicit grant/read/revoke/deny through real CLI processes. MVP-102 context isolation remains unimplemented; these metadata grants must not be reused as implicit authority to personal knowledge or execution.

## 2026-09-25 — Preserve admitted identity revisions

Managed admission previously overwrote one identity row, while execution compared its admitted identity to that latest row. A later definition revision could invalidate previously admitted work. Schema 34 now retains immutable identity snapshots keyed by identity ID and definition digest, backfills the existing row, and updates the latest projection atomically. Reusing the same key with different attributes is rejected. Managed execution resolves its job-bound revision instead of the latest projection.

This is a migration of the existing managed execution path, not a disconnected resource API. The new application regression admits revision one, persists revision two, verifies revision one reaches inference, then cancels it. Updating a definition is not cancellation; use the explicit cancellation path. Existing execution policy checks remain in place.

This is only part of MVP-101: snapshots are not complete serialized harness/definition payloads, digest generation is still caller-owned, and organization-scoped agent creation, revision selection controls and private/team context isolation remain pending. Migration can preserve only the latest pre-upgrade identity row; older overwritten revisions cannot be reconstructed from this table.

Validation: 88 memory tests (including upgrade/crash/backup checks), all 11 managed-service integration tests, all three application execution regressions and the architecture gate passed. The missing-identity regression initially waited on inference because it still deleted the old projection; it now removes the authoritative revision table and uses a bounded timeout. The regression verifies missing revisions deny before policy/inference, and explicit cancellation still stops work after a newer revision is published.

## 2026-09-25 — Trace privacy cutover across existing context paths

Added the MVP-102 [context isolation cutover](sprints/mvp-1-resources-privacy/context-isolation-cutover.md) from current source inspection. Resume already checks workspace ownership, while recall, briefing summaries and project consolidation use workspace/project boundaries rather than employee/team grants. Live-session lookup, artifact storage and event fanout are trusted internal APIs, not authenticated employee interfaces. The context cache already hashes the full request; its missing privacy input is trusted context/grant state, not merely a session cache key.

The plan now specifies immutable session-context binding, explicit legacy-local migration, scoped retrieval/consolidation, current authorization before cache use, artifact/event controls and adversarial end-to-end canary evidence. It rejects silently turning metadata membership into private-content access. All referenced source file paths were checked. This commit changes the implementation contract only; it does not claim runtime privacy or enable remote access. Provider-specific cached conversation semantics still need a dedicated trace during compiler cutover.

## 2026-09-25 — Durable session context bindings and legacy read isolation

Schema 35 adds information-context records (legacy-local, organization-private and team) and immutable `sessions.context_id`. Existing sessions and new sessions created through the existing local API use `legacy-local`; no employee ownership is guessed. The database rejects unknown context IDs, session rebinding, context mutation and deletion of an in-use context. Existing context ownership references organizations/principals/teams, but record existence is not an authorization grant.

Legacy transcript, resume, recovery-payload and history-list/latest APIs now deny or exclude nonlegacy sessions. Recall FTS and recent finish summaries filter to legacy sessions before returning content. Project consolidation rejects scoped input so future private sessions cannot flow into the existing shared-by-workspace digest. The migration reuses backup/crash/recovery machinery, and older downgrade fixtures now explicitly remove context schema artifacts.

Validation: all 90 memory tests, all 118 application unit tests and the architecture gate passed. A private canary fixture verifies transcript/resume/recall/summary/consolidation exclusion and immutable context binding; upgrade/reopen preserves legacy history. Private fixture construction is trusted test SQL: no public scoped activation API is exposed yet. This is the first connected MVP-102 storage/read-path change, not full privacy: scoped authorization/retrieval, project-knowledge ownership, live sessions, caches, artifacts, streams and publication still require cutover before multiuser execution is enabled.

## 2026-09-25 — Authenticated information-context access

The application now composes `ContextService` from the existing SharedStore and CredentialVerifier; LocalControl exposes the same service. Context creation derives private ownership from the authenticated principal, verifies current enabled organization membership, and permits team contexts only for current team owners/explicit members. Metadata administrator status alone confers no content access. Idempotent creation cannot replace an existing context or reveal an inaccessible conflicting context.

Scoped transcript retrieval verifies current membership and exact session/context binding in one database read transaction. It returns at most 200 messages, preserves chronological order and excludes rolled-back spawn branches. Legacy-local contexts cannot be read through this employee-authorized service. Credential checks occur at admission; membership/content reads share a snapshot, so revocation after the snapshot does not retroactively cancel that already-admitted read.

Validation: focused storage authorization tests, all seven application resource tests, and the architecture gate passed. Tests cover private canary denial for administrators/other employees, explicit team grant/revocation, organization removal, guessed session IDs, rollback exclusion, forged credentials, credential revocation and conflicting context creation. No public scoped session activation or inference path is enabled yet. Session lifecycle, tools/recall, summaries, caches, artifacts and streams still require authorization cutover; this service uses existing history tables rather than a second history store.

## 2026-09-25 — Authenticated scoped discussion history

`ContextService` can now open durable discussion histories and append human messages through the existing sessions/messages tables. Context authorization and mutation share a write transaction. Caller-supplied session IDs cannot rebind an existing session; message retries use a session-scoped client ID and only succeed for identical author/content. Permission is rechecked even on retries. Messages are restricted to user role, 64 KiB, and verified human author attribution rather than an invented agent identity.

Schema 36 adds nullable human-author and client-message identifiers to existing messages, plus a unique session/client-message index. Old messages retain their existing content and unknown historical author; no identity is inferred. Discussions have `mode=discussion`, `status=open`, no model and no workspace grant. Creating one does not activate agents or emit a Running status. Legacy transcript/recall/summary fences continue to apply.

Validation: all 93 memory tests, seven application resource tests and the architecture gate passed. Tests exercise authenticated create/open/append/read, idempotent retries and conflicts, denial for another principal, access removal across reopen, correct human attribution, and rollback of message plus recall indexing when attribution fails. Scoped runtime activation, UI/CLI exposure, close/archive behavior, context-aware tools and end-to-end inference isolation remain outstanding.

## 2026-09-25 — Exercise scoped discussions through the product CLI

`tetonic control context` now exposes private/team context creation, discussion opening, human message submission from a bounded UTF-8 file, and JSON history. Every operation supplies the stdin credential to the same ContextService; no duplicate storage/authorization implementation is introduced. Open/send explicitly report that no agent was activated. History limits are constrained to 1–200 messages.

The local control runbook includes a usable walkthrough and separates discussion history from agent execution. Local database operators remain trusted. Validation: all three CLI integration scenarios and the architecture gate passed. The new separate-process scenario verifies persisted private history, idempotent message retries, no credential/canary disclosure on denied reads, missing credentials, oversized files, organization revocation, team membership requirements and denial when a team context is paired with a private session. Scoped runtime activation and the rest of MVP-102 remain incomplete.

## 2026-09-25 — Authorized context message recall

Added context-scoped message recall in the existing memory store, composed through ContextService and `tetonic control context recall`. Authorization and retrieval share a read snapshot. SQL restricts to the requested context before returning snippets/limits, excludes system messages, and checks the indexed body/role against an existing non-rolled-back source message. The existing index lacks message IDs; this source revalidation prevents deleted/rolled-back entries from being returned without introducing another index. Scoped results do not use global BM25 ranking statistics.

Validation: the focused recall test, all three CLI integration scenarios and the architecture gate passed. Evidence covers private/team canary separation, filtering before a one-result limit, stale-index and rollback exclusion, membership revocation and real CLI search. Query syntax is normalized by the existing FTS tokenizer and input is capped at 4096 bytes; results are capped at 30. The agent tool adapter is not yet switched over, and tool-result recall/project digests are intentionally not represented as authorized by this message-only API. Full scoped execution remains gated on the remaining MVP-102 cutover.

## 2026-09-25 — Bind agent recall tools to authenticated contexts

ContextService can now bind an existing Tools instance to a verified principal/context and its authoritative store. The actual `recall` dispatcher uses scoped message retrieval for that binding, rechecking membership on every invocation. Clones retain the binding, and later legacy `with_memory` configuration cannot replace its database or downgrade it to workspace recall. Model arguments cannot set a principal/context: unknown recall arguments are rejected. Tool descriptions accurately identify scoped message-only recall; returned snippets are marked untrusted and scoped storage errors are sanitized.

This is trusted host composition, not a new model-callable authority. Credential verification is admission-time; the binding checks durable membership thereafter, so credential expiry/revocation does not itself cancel an already-bound execution. Explicit execution lifetime/revocation integration remains pending. Binding recall grants no file/tool capability and does not by itself start scoped inference.

Validation: seven resource tests passed before the final module extraction; the authenticated tool-dispatch test passed again afterward. It exercises real Tools dispatch, model scope spoofing rejection, retained scope through cloning/legacy configuration, and membership removal after binding. The architecture gate initially rejected growth of tools/lib.rs; memory configuration, read-only connection caching and recall dispatch were extracted together into memory.rs, and the gate now passes. Remaining compiler, briefing, artifacts and stream paths are still required before full scoped activation.

## 2026-09-25 — Recheck context compiler membership

ContextService now binds the existing context compiler to a verified principal, information context and exact persisted session. The compiler checks current membership before retrieval, before sealing and before releasing compiled context or registering expansion handles. Expansion checks before provider access and after asynchronous retrieval/scanning. Store checks combine current content membership and session binding in one read snapshot; metadata administrators receive no private-content override. Denials are sanitized.

This is an additional gate, not authorization of arbitrary provider inputs or artifact destinations. Legacy compiler composition remains available for legacy local execution. Checks have snapshot semantics, not atomicity with subsequent membership writes; sealing can persist an artifact before a later denial, so artifact authorization remains required. Credential validity is checked at binding, not continuously. Cache lookup, live model sessions, scoped provider construction and complete runtime activation remain outstanding.

Validation: all 45 context tests and eight application resource tests passed. Tests cover revocation between retrieval and result release, denial before provider reads, exact-session enforcement, private administrator denial and current membership removal against the real store. The architecture gate passed. Broader package verification failed on existing formatting differences in untouched files and Clippy errors in core/tetonic-domain/src/engine_config.rs; it is not reported as passing.

## 2026-09-25 — Durable artifact context ownership

Schema 37 adds immutable artifact-to-information-context bindings, creator attribution and creation timestamps in the existing authoritative database. Trusted writer adapters may register newly created artifacts; this method is not an employee API and must never be used to claim arbitrary existing artifact IDs. Identical retries recheck current membership. Conflicting bindings deny, and SQL updates cannot rebind ownership. Content access checks exact artifact/context ownership and current membership in one read snapshot, without a metadata-administrator override. Existing artifacts are not assigned guessed employee ownership.

Validation: all 95 memory tests passed, including crash recovery and migration rollback. A subsequently added version-36 upgrade test also passed, proving existing private discussion content is preserved; both artifact tests passed together. The architecture gate passed. The prior migration now skips its applied marker, and legacy migration fixtures drop the newer table before replay. Actual artifact reads/writes still use the low-level store: the scoped adapter and compiler composition are the next required cutover, so this commit alone does not enforce artifact privacy.

## 2026-09-25 — Enforce scoped artifact I/O

ContextService now binds the existing ArtifactStore interface to a verified principal and information context. Compiler binding automatically wraps its configured artifact store with this adapter. Writes and sealing check current membership, and sealed artifacts receive immutable ownership through the existing database. Reads and metadata require an exact artifact/context binding; known legacy or foreign IDs do not confer access. Open readers recheck before and after each bounded read, copying bytes into the caller buffer only after authorization succeeds. Writers recheck on every chunk and seal. Backend errors are sanitized. Acceptance and deletion are denied because content membership alone does not authorize those lifecycle operations.

Validation: nine application resource tests and the architecture gate passed. The new real LocalArtifactStore test seals and reads a private canary, denies cross-context and administrator access, rejects an unbound legacy artifact, and revokes membership while a reader/writer remains open. Subsequent reads leave caller buffers unchanged, writes/seal/metadata deny, and acceptance/deletion are rejected.

Limits: the backing artifact store and membership database are a trusted composition pair; raw storage remains an internal operator API. A crash or access loss after sealing but before ownership registration can leave an unbound object requiring lifecycle cleanup, never an implicit readable object. Authorization has read-snapshot semantics, not atomicity across filesystem operations. Credential checks occur at binding; execution cancellation and credential-lifetime integration remain separate work. Provider source grants, live/cached conversations, external artifact endpoints and full scoped runtime activation remain incomplete.

## 2026-09-25 — Fence legacy session termination and close scoped discussions

Tracing live conversation reuse confirmed that execution sessions still use the legacy-local service, while scoped discussions do not enter the live registry. The legacy end_session storage method could nevertheless mutate a scoped session by ID; it now requires legacy scope. ContextService and `tetonic control context close` provide an authorized, transactional discussion close instead. Current membership and exact context/session/mode are checked; identical close retries remain authorized operations. History is preserved, further messages deny, and an open retry cannot falsely report a closed discussion as open.

Validation: all 97 memory tests, all 121 application library tests, three separate-process CLI scenarios and the architecture gate passed. Evidence covers legacy termination denial, administrator/unknown-session denial, preserved content, closed-write rejection and revoked close retries. No model inference or running process is stopped by discussion close. Scoped live execution, resumable discussion lifecycle and authorization of live registry/event use remain outstanding; the cutover document now distinguishes the historical inventory from current progress.

## 2026-09-25 — Retire unused context cache and duplicate persistence helper

Repository-wide Rust reference tracing found ContextCache used only in its four dedicated tests, with no production construction or lookup. The separate persist_pack helper also had no callers; actual compilation persists through pipeline/stage7_seal.rs. Removed that inactive module, its public export and its four feature-only tests. Existing compilation, ownership/revocation, expansion and sealing tests remain. No replacement cache or new authorization subsystem is added for an unused optimization. Future caching requires measured benefit and current authorization on every return; live model conversation reuse remains a separate unresolved boundary.

Validation: all 41 remaining context tests, nine application resource tests and the architecture gate passed. A final Rust reference scan found no remaining ContextCache/persist_pack/cache-module references. This retires 272 lines. Inspection of the surviving seal path also found that it discards the backing artifact ID; retaining an addressable artifact reference is the next connected correction, distinct from cache retirement.

## 2026-09-25 — Preserve context pack storage receipts

The surviving seal path discarded the artifact store's returned ID, leaving only a logical ctx_* pack identity that could not locate the stored object. ContextPack now carries an optional stored_artifact_id receipt after successful sealing, and the domain CompiledContext retains it through the runtime adapter. Logical pack identity remains unchanged. Nonpersisted packs have no receipt. The immutable stored payload itself omits the post-seal receipt, avoiding a self-reference; deserializing old payloads defaults it to None. The receipt is a locator, never an access grant, and scoped readers still authorize it.

The real LocalArtifactStore regression opens the returned receipt, checks payload size and compares the complete stored pack to the returned pack minus its receipt. It also checks nonpersisted behavior and propagation through the domain compiler interface. All 42 context tests and the architecture gate passed. Previously sealed objects whose IDs were discarded are not automatically rediscovered or assigned ownership.

Downstream validation: all nine application resource tests passed after rebuilding the domain/compiler consumers.

## 2026-09-25 — Verify scoped compiler-to-artifact composition

Added an application-level integration test using real credentials/membership, WorkspaceContextProvider, the production filesystem hooks, ContextCompiler, LocalArtifactStore and the scoped storage adapter. A real source-file canary enters compiled evidence and the sealed artifact is retrieved using the returned storage receipt. Another authorized context owned by the same user cannot open that artifact. Removing organization membership blocks subsequent artifact opens, recompilation and expansion of an already-issued handle.

Validation: the integrated test and architecture gate passed. Production filesystem hook construction is now crate-visible so the test uses the actual composition rather than duplicating a test-only filesystem policy. This verifies the formerly separate compiler, ownership and artifact boundaries together; it does not activate employee-scoped inference or prove isolation of tools, live model state, summaries or event subscriptions. Workspace access in this fixture is a trusted host grant, not inferred from context membership.

## 2026-09-25 — Revalidate credentials on bound compiler and artifact operations

Bound compiler membership gates and scoped artifact adapters now re-run their configured CredentialVerifier at every existing authorization boundary and require the same originally bound principal. This closes the admission-only credential lifetime gap for these two adapters: revocation/expiry/verifier failure now deny later use even while membership remains valid. A private, nonserializable binding shares bearer material in host memory without Debug output; it is not written to artifacts or model inputs. Cleanup through writer abandonment remains possible after revocation.

Validation: ten application resource tests and the architecture gate passed. Existing real-store regressions now revoke the credential independently of membership, prove already-bound compiler gates and open artifact readers/writers deny, then issue a new credential and prove access remains valid until membership is removed. The integrated workspace/compiler/artifact test also passes. Credential verification and membership checks remain separate snapshots, not an atomic stop of in-flight I/O. Bound recall tools still use admission-time credential verification; execution-wide cancellation and that synchronous tool boundary remain follow-up work. This entry supersedes the earlier admission-only limitation specifically for compiler and artifact bindings.

## 2026-09-25 — Revalidate credentials in synchronous recall dispatch

Scoped recall now requires a trusted synchronous credential-lifetime checker and invokes it before retrieval and before releasing results. LocalCredentials supplies that checker using the existing credential hash/audience lookup against recall's authoritative read-only store; no raw bearer is retained by Tools and no async runtime is blocked or recursively entered. The originally authenticated principal must still match. Tools clones preserve the checker, and legacy with_memory cannot downgrade the binding. CredentialVerifier adapters without this synchronous capability deny recall binding by default.

Validation: ten application resource tests and the architecture gate passed. The real tool-dispatch regression revokes the original credential while membership remains valid and verifies the existing bound recall denies without returning its private canary. A fresh credential and binding succeed before membership removal, which then separately denies retrieval. Existing clone, scope spoofing and legacy configuration tests continue to pass. This supersedes recall's earlier admission-only credential limitation; verification/retrieval use separate snapshots, so this is not execution-wide atomic cancellation or revocation of content already delivered.

## 2026-09-25 — Guard legacy managed admission against scoped session IDs

Traced the actual application identity_job → ManagedRunService admission/execution path. AdmissionContext carries correlation rather than employee authorization; it previously allowed any supplied session ID. The durable legacy admission path now requires an existing legacy-local session before persisting identity revisions or creating run state. Unknown and scoped IDs return the same sanitized denial. Sessionless and volatile trusted-local paths remain available; no employee-scoped activation is enabled by this change.

Validation: all 12 managed-service integration tests, all 122 application library tests and the architecture gate passed. The new regression attempts admission using a private discussion and an unknown session, verifies identity storage remains untouched, then proves a persisted legacy session can be admitted. The sprint-2 plan records the verified activation boundary and the required principal/context/agent revision/resource/budget binding. Full scoped activation, delegated authority and remaining information boundaries are still outstanding.

## 2026-09-25 — Organization-owned agent registration

Schema 38 adds organization-keyed agent registration linked by foreign key to the existing immutable identity revision table. The ResourceService registration/read methods use verified organization authority; storage rechecks current authority transactionally. Registration stores a versioned harness/configuration JSON envelope (object-only, 64 KiB maximum), computes its SHA-256 digest, generates a stable identity and records creator/time. Identical retries return the same identity; changed definitions conflict rather than silently updating an admitted revision. Identity and resource writes commit together, reusing the existing identity persistence helper inside the transaction.

Registration currently requires organization administration; organization members may read these explicitly organization-owned definitions. Personal/team agent ownership is not inferred. Requested tools/configuration grant no capabilities: identities use unconfigured privilege class and empty effective tool/context subscriptions. Unknown harness configuration is retained as data, not claimed to be executable. Existing coding identities remain legacy and are not reassigned to an organization.

Validation: all 98 memory tests, including migration crash/rollback checks, and the architecture gate passed. New storage evidence covers restart, idempotence, conflicting revisions, non-admin creation denial, cross-org denial, membership removal and rollback of identity insertion if resource insertion fails. Initial registration is implemented; revision update/selection, harness-specific validation, team/private ownership, CLI/UI exposure and managed activation remain outstanding.

Application validation: all 11 resource tests passed, including real credential registration/read, stable retry identity, forged-credential denial and revocation.

## 2026-09-25 — Expose agent registration through the local CLI

`tetonic control agent register` accepts an organization key, harness name and bounded UTF-8 JSON configuration file; `agent get` reads the durable registration. Both use the existing ResourceService and stdin credentials. Output includes the stable identity, definition digest/configuration, unconfigured privilege class and agent_activated=false. UTF-8 BOM files are supported for Windows authoring. The runbook includes a concrete PowerShell walkthrough and explains that harness registration is not validation, tool permission or execution.

Validation: all four separate-process CLI scenarios passed, including the new register/retry/get flow, BOM normalization, persisted identity, conflict preservation, nonobject/oversized input denial, missing credentials, foreign organization access and revocation without configuration/credential disclosure. Architecture checks passed before the final BOM input adjustment. Revision publishing and managed activation remain unfinished.

## 2026-09-25 — Publish and resolve immutable agent definition revisions

Schema 39 moves registered definition payloads into agent_definition_revisions, keyed by the same identity/digest pair as the existing managed identity snapshots. The initial configuration is migrated with creator/time preserved and its redundant registration column is removed. Organization registration remains immutable and continues to select its original default. Publishing a new revision stores its payload and identity snapshot atomically under the existing identity; exact digest lookup applies current organization read authorization. Publishing/retrying never changes admitted work or automatically selects a new registration default. Effective capabilities remain unconfigured.

The existing 98 memory tests passed after migration changes. Three focused registration/revision tests then passed, including the new v38 upgrade preservation test and publish/retry/restart, prior revision retention, unauthorized publication, cross-org denial, membership removal and injected definition-write rollback. The architecture gate passed. CLI revision commands, default selection, harness validation and managed activation remain pending. Historical run identities are unchanged; configuration digests identify the stored envelope bytes, not an executable harness build.

Application validation: all 11 resource tests passed, including authenticated revision publication, explicit digest selection and preservation of the original registration default.

## 2026-09-25 — CLI revision publishing and exact selection

Added `tetonic control agent publish` and optional `agent get --revision <digest>` through the existing authorized resource service. Register/publish share bounded UTF-8/BOM JSON file loading. Publishing retains identity, leaves the original registration default unchanged and reports no activation. Unknown revision lookup fails without fallback. The PowerShell runbook now documents both operations.

Validation: all four CLI process scenarios and the architecture gate passed. The agent scenario now publishes and retries revision two, checks stable identity/different digest, retrieves both complete definitions by exact digest in separate processes, confirms the original default remains selected and rejects an unknown digest. Activation and mutable default-selection policy remain unfinished.

## 2026-09-25 — Reject unsupported coding revisions before execution

Tracing registered-revision activation found an existing policy fallthrough: validate_coding_execution checked tool/step ceilings only when the job digest equaled the compiled coding recipe; a different digest skipped those checks. The coding validator now explicitly rejects unsupported definition revisions and unknown roles. Supported coding definitions retain their existing role, tool and step enforcement. This is harness-specific validation, not a restriction that all future agents must use the coding harness; a registered configuration needs its own explicit compiler/validator before activation.

Validation: all 124 application library tests, all 20 WORK-05 execution-binding integration tests and the architecture gate passed (gate before the added integration fixture). New unit coverage exercises unknown revision/role rejection and retained planner tool/step limits. The production-composed integration test supplies a matching but unsupported identity/job digest and verifies zero model and tool calls. Source tracing also confirms AgentInvocation instructions are runtime input rather than a serialized definition binding; authenticated activation must compile them from the selected revision and effective grants instead of accepting arbitrary caller-built executors. No new activation endpoint is exposed.

## 2026-09-25 — Prepare a general harness from exact registered revisions

ResourceService can now prepare an exact authorized `general` revision into the existing domain AgentInvocation. The compiler verifies the stored envelope bytes against the identity digest, requires schema version 1 and matching harness ownership, and rejects unknown configuration fields. Supported configuration is instructions, optional requested_tools and optional max_steps. Host-supplied preparation ceilings bound steps/input; instructions must be nonempty and requested tool names bounded/unique. The invocation separates stored instructions from current task input and uses empty neutral loop discipline, no coding overlays or empty-tool heuristics, and the existing finish completion convention.

Preparation returns requested tools separately from the unchanged unconfigured identity. It does not construct a tool host, grant capabilities, authorize execution, admit a run or invoke inference. This is the compiler needed by a future governed activation path, not an alternate runtime or completed noncoding execution support.

Validation: all 12 application resource tests and the architecture gate passed. The real-store regression selects an older revision after publication, checks its exact instructions and step bound, verifies requested recall does not appear in effective identity grants, rejects tampered payloads, excessive/zero steps, unknown fields and duplicate tools, and denies preparation after credential revocation. Inference/tool binding, durable execution authorization and runtime activation remain pending.

## 2026-09-25 — Expose the complete invocation to managed execution policy

ExecutionPolicy now receives the actual AgentInvocation instead of only its step count. The manager invokes it before claiming execution or entering the agent loop, allowing product policy to validate instructions, completion behavior and loop discipline against a prepared revision. The existing coding policy consumes the invocation's step limit and retains its revision/role/tool enforcement; this change does not claim that legacy coding instructions are now definition-bound.

Validation: all 126 application library tests, 20 WORK-05 binding tests, the OBS-02 finalization test and the architecture gate passed. The new managed-path regression independently changes instructions, completion tool, discipline, explain mode, empty-tool nudge and step count. A pinned-invocation policy rejects every change with zero inference calls and no execution claim. This is the enforcement interface required for general harness activation, not an activation endpoint or employee execution grant. Durable scoped authorization, effective resource grants and integration of the prepared revision policy remain outstanding.

## 2026-09-25 — Bind general definition conformance to the managed executor

Prepared general revisions now expose read-only accessors and produce an ExecutionPolicy that pins the exact identity, definition digest, input digest and complete invocation. It requires the requested capability set and the exact executable tool set (including the loop's finish capability), rejecting duplicate/extra/missing capabilities, role overlays and undeclared artifacts. This checks definition conformance only: read permission and a matching tool name are not execution or resource authorization. The future activation composition must additionally enforce current principal/context/resource grants and budgets. No employee execution endpoint is exposed.

Validation: all 127 application library tests and the architecture gate passed. The real-store preparation regression checks mismatched instructions, identities, roles, inputs and capability sets. A new integration test registers/prepares an organization-owned general agent, retrieves its existing durable identity and reaches inference through the existing managed admission/claim/executor using the same database and a finish-only host. The provider is a pending test provider; this proves wiring through the existing execution machinery, not real inference quality, completion/finalization, scoped privacy or employee authorization. Updated stale Sprint 1 resource status descriptions to reflect implemented revisions and membership administration.

## 2026-09-25 — Pin the execution validator to each admitted attempt

Tracing the scoped activation boundary found a composition prerequisite: execution selected its validator from the dispatching service handle, while cloned handles share active attempts. ActiveAttempt now retains the validator selected by its admitting handle. Dispatch uses that retained validator, allowing different prepared definitions to share the existing manager registry without dispatch replacing an admitted definition's checks. with_execution_policy selects policy for future admissions only. A pinned callback may still evaluate live grants; this does not freeze authorization results.

Validation: all 128 application library tests, 12 managed-service integration tests and the architecture gate passed. The new regression admits two attempts with distinct rejecting validators, dispatches through a permissive shared handle, and proves each original validator runs with zero inference calls and no execution claim. The missing-identity regression now installs its counting validator before admission, preserving its assertion that missing durable identity denies before policy evaluation. Validators remain trusted process-local code; durable authorization receipts, scoped activation, child delegation authority and restart reconstruction remain outstanding. This is not a persisted employee authorization binding.

## 2026-09-25 — Carry governed scope through existing managed task state

TaskInputBinding now optionally persists principal, organization and information-context attribution as ExecutionScope. Missing fields deserialize as legacy/unscoped; identifiers alone are never grants. Trusted host composition can supply a non-deserializable AuthorizedExecution with an async ExecutionAuthority. Admission requires durable storage, validates scope identifiers and checks current authority before identity/run writes. The active attempt retains that authority; execution compares the durable scope and rechecks authority before its execution claim. Denials expose a fixed message.

Governed child delegation and governed session correlation currently deny explicitly. They cannot fall through to legacy child/session paths while inherited grants/budgets and scoped live-session composition are missing. Existing legacy app callers pass no authorization and retain their existing fences. No employee endpoint, production authority adapter, new run database or alternative lifecycle is introduced. This binding is attribution plus an enforcement hook, not a complete durable grant receipt or approved production activation path. Per-effect checks, budget reservations, scoped history/events, restart authority reconstruction and old-writer compatibility gating remain required before exposure.

Validation: all 129 application library tests, the full tetonic-run unit/integration suite and the architecture gate passed. A subsequently added governed-session denial assertion passed in the focused execution-scope regression. That regression uses a mutable test authority to prove admission denial before identity persistence, scope persistence after reopening storage, denial of unconfigured child delegation, and revocation before claim with zero inference. The test authority is intentionally not evidence of real employee policy evaluation.

## 2026-09-25 — Compose real credentials and context membership into execution authority

ContextService now creates a host-bound execution authority using its existing verifier and SharedStore. The principal comes from verified credentials; every authorization rechecks that credential, exact scope, organization-qualified content membership, the selected registered agent revision and the complete stored identity. A required, separately supplied ExecutionAuthority evaluates execution/resource/budget grants; no default-allow adapter is supplied and content/definition read access cannot substitute for that decision. The manager uses this composition at admission and execution through the previously added hook. Database checks and credential/grant checks have separate snapshot semantics, not atomic revocation of in-flight effects.

The memory content-access query now supports exact organization qualification while preserving existing callers. The registered-general-agent integration test now composes real credentials, a private context and the new authority before reaching inference through the existing manager. Its explicit test grant first denies despite valid content access, then allows; substituted scope, later grant denial and credential revocation reject the bound authority. The grants adapter is still test-supplied: persisted production execution grants, reservations, per-effect authorization, scoped sessions/events and employee activation remain unfinished. This does not claim a complete production authority or automatic cancellation after revocation.

Validation: the updated managed integration test, all 12 application resource tests, the memory context-access regression (including cross-organization denial) and the architecture gate passed.

## 2026-09-25 — Persist explicit scoped job permissions

Schema 40 adds immutable execution grants and transactional issue/revoke audit events in the existing store. Each grant binds an ID, principal/organization/context, exact AgentJobSpec (including revision, input, capabilities, artifacts and recovery key), expiry and revocation state. Authenticated organization administrators may issue/revoke through ResourceService; storage rechecks that authority, subject content access and organization ownership of the registered revision. Exact issuance retries are idempotent, changed or revoked grants cannot be overwritten, and audit failure rolls back mutation. Current context membership, expiry and revocation are checked on use. Schema versioning rejects writers that only support earlier versions; migration fixtures now remove the new dependent tables before replay.

ContextService can compose its real credential/context/identity checks with a stored grant. The existing managed integration now uses this adapter: it denies before issuance, reaches inference after an authenticated grant, and denies subsequent checks after revocation. No test allow callback is used in that path. Grants are job permissions, not single-use admissions, budget reservations, tool resource sandboxes or a guarantee that in-flight work stops. Durable linkage of the chosen grant to run recovery, per-effect checks, budget accounting, scoped sessions/events and an employee activation endpoint remain necessary.

Validation: all 101 memory tests passed, including migration crash recovery and rollback. Grant coverage includes restart, unchanged retries, changed capabilities, unauthorized issuance, expiry, membership removal, revocation, no reactivation and injected audit failure. Application suite and architecture validation are recorded after completion below.

Application validation: all 129 library tests and the architecture gate passed, including the managed execution test using the real persisted grant adapter.

## 2026-09-25 — Persist the selected execution grant with the managed task

TaskInputBinding now retains an optional execution_grant_id alongside ExecutionScope. Stored-grant composition attaches the selected ID, admission validates its shape and writes it through the existing run journal, and execution compares it to the active authorization binding before consulting authority. Custom trusted authorities may omit the stored locator; old snapshots default to no locator. The ID is inspection/recovery attribution, not a bearer credential or an implicit authorization. Recovery still needs to rebuild and revalidate the host authority, not trust this identifier alone.

Validation: all seven application execution regressions, the complete tetonic-run unit/integration suite and the architecture gate passed. The real stored-grant integration reopens the database and resolves the exact grant ID from the task snapshot. Its subsequently extended regression also admits another attempt before revocation, revokes the grant through authenticated ResourceService, and proves manager dispatch denies before claim with no additional inference call. That focused test passed. Per-effect enforcement, budget reservations, scoped session/event composition and employee activation remain outstanding.

## 2026-09-25 — Recheck managed authority inside the agent loop

The existing Agent loop now accepts a neutral async ExecutionGate bound by ManagedRunService from the admitted authority, identity and job. It checks before main and compaction model requests, before processing returned tool calls (including in-loop finish/spawn), and immediately before handing ordinary tool execution to a blocking worker after asynchronous approval/capability waits. Each managed attempt explicitly replaces the binding, including clearing it for legacy execution, so reused agents do not retain a previous attempt's gate. The core knows no organization schema or credential format.

Validation: all eight application execution regressions, 38 core tests and the architecture gate passed. A new managed regression revokes authority inside an inference response that requests a file write, then proves execution fails with the fixed denial message before emitting the tool call and without creating the file. Existing stored-grant integration and pre-claim revocation tests also pass.

Limits: checks do not interrupt already in-flight inference/transport, revoke already-issued output, or make filesystem effects atomic with grant changes. This does not authorize the alternate world/continuous loops, provide stream audience isolation, or complete finalization/commit authority checks. Scoped delegation still denies. Budget accounting, resource-specific controls, governed finalization and employee activation remain unfinished.

## 2026-09-25 — Enforce current authority during successful finalization

Managed finalization now checks the persisted scope/grant binding and current admitted authority before claiming successful finalization, before launching verification, before launching workspace commit, before sealing outputs and after sealing before completion. Denial records PolicyDenied through the existing failure/finish path and delivers a failed terminal result; cleanup and failure recording do not require the revoked execution grant. Legacy unscoped finalization retains its behavior, with persisted binding consistency checked.

Validation: all nine application execution regressions, 12 managed-service integration tests and the architecture gate passed. The new regression proves revocation before finalization prevents verification and commit, and revocation during successful verification prevents the subsequent commit. Both cases produce a durably failed run. Existing cancellation, blocking-worker ownership, commit failure and artifact publication failure tests remain green.

Limits: an already-running verifier/commit is not retroactively canceled or rolled back by these checks. Revocation during sealing may leave an unaccepted object requiring cleanup. Checks do not make authority changes atomic with effects or artifact acceptance; scoped output storage/audience isolation and resource-specific finalization grants remain necessary. Budget enforcement, scoped sessions and employee activation are still unfinished.

## 2026-09-25 — Bind managed final outputs to their information context

After sealing a newly created governed output, ManagedRunService now uses the existing bind_new_context_artifact storage operation to associate it with the admitted principal/context before completing or accepting the result. This is the same immutable ownership primitive used by the scoped artifact writer; the manager remains the trusted creator and lifecycle owner, so content membership is not expanded to permit arbitrary acceptance/deletion. Existing ContextService artifact readers can now retrieve managed outputs under their normal scope and credential checks. Binding failure records a failed run and publishes no accepted artifact receipt.

Validation: all ten application execution regressions, 12 managed-service integration tests and the architecture gate passed. The new finalization regression creates real private contexts and a real stored output, reads its canary through the authorized adapter, denies another context owned by the same person, and denies access after credential revocation. An injected ownership-write failure produces a failed run with no accepted artifact. The subsequently added byte-content assertion passed in the focused test.

Limits: the test composes a trusted test execution authority to isolate finalization/storage behavior; production stored-grant execution remains covered separately. The raw artifact store is an internal operator interface. A crash or denial after sealing can leave an unbound object requiring cleanup. This does not scope event subscribers or complete the employee activation interface, budgets or live-session privacy.

## 2026-09-25 — Complete the governed runtime integration and fence legacy event projection

The registered-general-agent integration now runs through completion instead of stopping at a pending inference call. It uses real registration/revision preparation, credentials, context membership, a stored grant, managed admission/claim, the existing Agent loop, finalization, durable success and scoped output retrieval. A deterministic provider verifies the prepared instructions/task and finish-only catalog and returns a known result. The test reads the result through the scoped artifact adapter, rejects a different context, preserves post-admission grant revocation coverage and rejects artifact reads after credential revocation. This proves runtime composition, not external model quality or an employee-facing launch experience.

Tracing that path identified a legacy fanout gap: ProductRunHooks projected sessionless scoped events into the application-wide sink. ManagedBinding now carries its admitted scope, and that legacy projection declines scoped start/step/terminal events. Approval cleanup remains active. The direct host callback and durable run records remain available to trusted composition; authenticated scoped subscriptions/replay still need implementation. This guard does not claim all telemetry/logging/providers are tenant-isolated.

Validation: all 132 application library tests, the OBS-02 legacy finalization integration and the architecture gate passed. The complete governed integration verifies the unscoped product sink receives no events, while legacy event-delivery tests remain green. Remaining activation work includes budgets, scoped session/history and event delivery, resource isolation and a usable product entry point.

## 2026-09-25 — Authorize inspection of governed run snapshots

ContextService now inspects the existing DurableRunSupervisor snapshot using its own authoritative store. Current credentials and organization-qualified context membership are checked before lookup and again before releasing content. Every task must carry the requested scope; empty, legacy, mixed-context, missing and unauthorized snapshots deny. Source tracing confirms there is no task-scope rebind command: AddTask rejects an existing ID. No new run store or event projection is introduced. Content access remains distinct from execution grants, so a revoked execution grant does not erase authorized result inspection.

`tetonic control inspect-run --org ... --context ... --run ...` exposes this read path with the existing stdin credential handling. The runbook documents that it inspects already-admitted governed runs and does not launch work or provide remote authentication.

Validation: the complete governed execution integration passed with authorized completed-run inspection, wrong-context/missing-run denial, retained inspection after execution-grant revocation and denial after credential revocation. A separate test verifies legacy and mixed-context denial. All four existing CLI process scenarios passed, the new command's help was checked, and the architecture gate passed. Snapshot checks are not atomic with response delivery. Scoped event subscriptions/replay, budgets, live-session isolation and user-facing activation remain unfinished.

## 2026-09-25 — Authorize replay of the existing governed run journal

ContextService now replays existing supervisor events using its sequence cursor and retention-gap contract. It validates the entire run's context and current credential/membership before and after retrieval, so a concurrently added foreign task or revoked access denies the response. Response limits are 1–1000 events. `tetonic control replay-run` uses stdin credentials and prints either events or an explicit gap; no alternate journal or fabricated progress stream is added.

Validation: the complete governed runtime integration passed with two-page cursor progression, wrong-context denial, oversized-page rejection, credential revocation and a real compaction gap. All four existing CLI process scenarios and the architecture gate passed. This is durable lifecycle replay, not token streaming or a live subscription. The supervisor currently loads retained events internally before limiting the response; internal bounded reads remain a scalability improvement. Authorization checks have read-snapshot semantics. Budgets, scoped live conversations and product activation remain unfinished.


## 2026-09-25 — Repair existing reservation capacity accounting before governed budget integration

Static tracing distinguishes the broker's outstanding capacity ledger from cumulative effort/spend accounting: releasing reservations returns their token allowance, and there is no organization/team spend lineage. It must not be presented as durable team budgeting. Reuse inspection found project allocations were added but never reversed, and OverBudget was missing from terminal guards, allowing repeated cleanup to subtract another reservation's capacity or resurrect a terminal record.

The existing ledger now retains project attribution for its active in-memory reservations and uses one release implementation for explicit release, expiry/reconciliation and terminal transitions. That path reverses project usage together with existing scopes. OverBudget is terminal and repeated cleanup preserves its outcome and other active reservations. This adds no alternate ledger or persisted schema. Project attribution is not restored across restart; current broker recovery cancels durable active rows rather than reconstructing this ledger.

Validation: all 88 broker tests passed, including three new regressions covering all terminal outcomes, expiry with project capacity reuse, and duplicate over-budget cleanup while other reservations fill the inference limit. Architecture validation recorded below. Remaining prerequisites include retry identity/envelope validation, recovery error handling and ownership semantics, overflow-safe capacity checks, and durable cumulative effort accounting with organization/team attribution. Governed activation is still incomplete.
Architecture gate and git diff whitespace checks passed.


## 2026-09-25 — Bind active reservation retries to their original scope and request

The existing broker ledger now compares active retries against the original run, task, project, target, speculation flag and complete resource request. Conflicts reject explicitly rather than reusing an unrelated allowance or entering the capacity queue. Exact retries retain reservation ID and expiry. Undispatched expired reservations are released before retry lookup and capacity re-evaluation. The active request binding replaces the preceding project-only attribution map; no second ledger or durable schema was added.

Validation: all 90 broker tests and the architecture gate passed. New regressions exercise seven changed-request dimensions through the admission controller, unchanged retries and expiry at the boundary. These checks are in-memory capacity admission, not durable cumulative spend enforcement or restart identity recovery.


## 2026-09-25 — Connect registered job submission to the existing application run service

Application and DefaultRunService now offer submit_registered_job for trusted host composition. The run service resolves credentials, registration/revision and context/grant authority using its own managed store, constructs the existing StartIdentityJobCommand, and checks the actual tool catalog before admission. PreparedAgentRevision shares identity decoding between job construction and definition validation. The existing managed asynchronous submission gained an AdmissionContext parameter through submit_identity_job_with_context; its legacy entry delegates with the default context. Registry, execution claim, fresh conversation, finalization, artifact ownership, cancellation and completion delivery remain owned by that existing path.

The complete registered-agent integration now uses this application run-service entry for submission instead of hand-composing admission/execution/finalization. Wrong input, grant, context, credential and executor catalog deny before any durable run or inference. The successful run retains authorized output/inspection/replay and revocation assertions. Another submitted run is canceled while inference is pending using the original run service, demonstrating shared ownership and a canceled completion receiver.

Validation: all 133 application library tests and the full tetonic-run suite passed. This is a host API, not an employee CLI/UI launch command. The host still supplies the configured provider and restricted tools; cumulative effort budgeting, activation deduplication, scoped live streams, complete context isolation and actual tool isolation remain unfinished. A recovery ID does not make repeated submission idempotent. No Sprint 1/2 exit or production guarantee is claimed.
Architecture gate and whitespace checks passed for the registered submission integration.


## 2026-09-25 — Assemble registered jobs with existing production controls and scoped audit

Application::submit_registered_job now builds its executor from host-owned settings and the installed typed compute plane. It uses EngineRuntime session assembly, the existing action/capability brokers, Tools workspace/sandbox controls, scoped recall when requested, process admission and the existing workspace finalization driver. An arbitrary provider/Agent is no longer accepted by this application entry. The low-level raw-agent registered helper remains test-only. The prepared identity/grant/definition path and existing managed LocalSet owner are reused. No alternate supervisor, database, queue or effect manager was added.

The product audit writer was extended to bind execution histories to an immutable information context in the existing tables, preserve agent/tool-result attribution, namespace tool call IDs and latch write errors. This latch participates in the existing managed authority checks before subsequent work and finalization. Audit recording can retain terminal evidence after membership revocation; readers still use current context authorization. A managed-binding note links run/task/attempt IDs. The special history mode/status does not represent execution status and cannot be opened as a human discussion or read through legacy history APIs. No schema migration is needed.

Provider and compute-broker references now live in one installed binding, replacing separate mutable fields. Registered assembly accepts the installed brokered variant, including its existing secret scanner; raw host/provider bindings deny. Inference selection for registered jobs currently uses this installed plane rather than the legacy per-session profile mechanism.

Validation: all 135 application library tests passed. New integration coverage uses scripted HTTP responses over a real local socket with the real compute broker, egress guard, runtime and filesystem tools. Read output reaches the next inference request and scoped transcript; write output exists on disk after managed finalization. Missing compute-plane composition and an exceeded host tool ceiling deny before creating runs. Disallowed egress sends no agent request to the test endpoint. Injected tool-audit failure allows no next inference or accepted artifact. Separate audit tests cover scope mismatch, repeated tool IDs across histories and failure latching. Memory/runtime/core and architecture results are recorded below after completion.

Limits: no employee CLI/UI launch command yet; host workspace/tool settings are trusted deployment inputs. Scope-wide budgets, retry deduplication, interactive approvals, live scoped streams, provider-profile selection and comprehensive OS isolation/recovery evidence remain open. Existing policy and resource grants do not imply arbitrary tool safety. Audit failure stops at the next authority boundary and cannot undo completed effects. The new history may remain after later preparation/admission failure; it does not claim Running. Sprint 1/2 exit criteria remain unmet.
Final validation: full tetonic-memory, tetonic-core and tetonic-run test suites passed (including 102 memory and 38 core unit tests plus their integration suites). Architecture gate and git diff whitespace checks passed.

## 2026-09-25 — Enforce host time ceilings through existing managed deadlines

Integration checkpoint e88d248 committed the configured registered executor. Follow-on work reuses TaskInputBinding.deadline, the run deadline projection, managed execution gates, WorkScope and the existing TimedOut failure machinery. Registered host settings now require a 1–86,400-second time ceiling. The deadline starts at submission, is persisted at managed admission and has Unix-second resolution. Legacy callers remain compatible with no deadline. Existing child admission inherits the tighter deadline; governed delegation is still unavailable.

Execution rejects a changed durable deadline, checks expiry at protected action boundaries and interrupts pending inference through the existing cancellation path. A monotonic bound captured at admission prevents backward wall-clock changes from extending active work. Finalization signals cooperative workers and waits for actual worker completion, then records a task timeout rather than accepting a late result. Existing finalization claim/completion gates now reject at the exact deadline. No new supervisor, execution store or timer task was introduced. Artifact publication may drain after an on-time completion; a publication failure must retain recovery evidence rather than be relabeled a timeout.

Focused validation passed: all 16 managed-service tests then present, including four deadline regressions; the configured registered integration now also uses an HTTP endpoint that never responds and verifies a durable TimedOut attempt with no accepted output or active manager binding. A fifth regression directly checks claim/completion rejection at the exact boundary. Broad regression results follow below. Architecture gate and whitespace checks passed.

Limits: this is an operator-owned per-job ceiling, not top-down durable team budgeting or a hard real-time kill guarantee. Noncooperative effects remain owned while draining; completed effects are not reversed. Hung synchronous host/storage operations can delay cleanup. Cumulative tokens/spend, admission concurrency/queues, activation deduplication, product launch transport and isolation/recovery work remain open. Sprint 2 is still in progress.

Final validation: all 135 tetonic-app library tests and the full tetonic-run suite passed (85 tests, including all five new deadline regressions). The broader combined application integration command stopped at cap01_runtime_crate_clean_of_repository_heuristics: its static dependency ban rejects tetonic-secrets, already present in HEAD's runtime Cargo.toml and used by brain.rs redaction. Neither that manifest nor the failing test was changed by this checkpoint. Approval-binding and AUD01 integration suites passed before that failure; later application integration suites were not run by the aborted command. This is recorded baseline debt, not an all-workspace-green claim. Architecture gate passed.

## 2026-09-25 — Deduplicate registered launches in the existing run journal

Registered launches now carry an explicit request_id distinct from recovery_id. A verified principal/organization/key selects a stable run ID; the root task stores the host-computed fingerprint and winning scoped audit locator. Current authority is checked on every retry. Matching requests return an existing receipt, changed jobs/settings/context/grants conflict, and only the original creator admits an attempt. Existing event/projection transactions and command deduplication arbitrate concurrent local managers. No second activation store, supervisor or schema migration was introduced.

Managed submission now owns admission and execution together. Dropping the launch response cannot interrupt the gap between durable admission and local execution. Retrying after a crash or partial admission returns the existing record without resuming or starting duplicate effects. The application response exposes run/task/audit locators plus an execution/completion handle only for the winning launch. Explicit new keys still create distinct jobs. Original deadlines remain immutable across retries.

Focused tests cover a forced two-manager race, conflicting requests, reopened storage, revoked authority, new keys, an injected sequence-2 write failure, and dropping the caller during admission. The configured HTTP/runtime test also exercises concurrent and terminal retries. The first managed test run revealed a legacy ticket-cleanup compatibility issue; cleanup now remains with the legacy caller and moves to the owning submission task for that API. The first expanded application test counted inference-hop runs as agent activations; its assertion now distinguishes these existing run kinds. Final validation results follow below.

Limits: deduplication lasts while run records are retained. Operator deletion removes that history; retention/tombstones remain open. Concurrent preparation can leave an unused audit record. Receipts are locators, not Running status or automatic recovery. Distributed ownership, cumulative token/effort budgets, concurrency/queues, product transport and complete isolation/cutover evidence remain unfinished. Sprint 2 is still active.

The forced race also exposed a deferred SQLite transaction upgrade failing with database-is-locked before the winner became visible. The existing run-command transaction now acquires an Immediate write transaction before reading projection metadata, honoring the configured busy timeout instead of racing a WAL read-to-write upgrade. Twenty forced two-manager races passed after this change. Full domain (32 tests), memory (105 tests including integration) and run (87 tests) suites passed; final application/gate results follow below.
Final validation: all 136 application library tests passed after the transaction fix, along with the architecture gate and whitespace checks. The four tested package suites total 360 passing tests. The previously recorded broader application integration dependency-rule failure was not changed or rerun in this checkpoint.

## 2026-09-25 — Bound registered identity concurrency with durable worker completion

Registered activations now admit one run per stable identity through the existing run-command transaction. An indexed projection in schema 41 makes the check atomic across independent managers sharing SQLite. Matching retries retain their receipt; distinct busy requests return a typed capacity error without creating a run or starting inference. Different identities remain concurrent. This is an incremental identity bound, not completed org/team budgeting or a distributed scheduler.

The run journal now records an attempt's execution_quiesced acknowledgement, validated against its existing lease identity. The manager closes WorkScope, waits for actual workers and records the acknowledgement before dropping ownership or delivering terminal completion. Finalization cleanup moved to its outer owner so early failure paths cannot notify completion while holding a finalization lease. Only a terminal run with every attempt acknowledged returns admission capacity. Cancellation/timeout alone, crashed owners, partial admission and failed acknowledgement writes retain the hold. Existing scoped inspection/replay carries this evidence; no parallel lifecycle store was added.

The upgrade test exposed migration 40 lacking an applied-version guard, which would recreate its tables during v40-to-v41 upgrade; that guard is fixed. Old registered runs lacking quiescence evidence migrate conservatively, even if already terminal. Supported recovery of these ambiguous holds is still required. Upgrade all database writers together. No mixed-version or multi-node ownership guarantee is claimed. Preparation remains unbounded and can leave audit records for rejected admission; queues, cumulative usage, product launch and isolation/cutover work remain open.

Verification covers competing identities/principals on two managers, a noncooperative blocking effect with both retained and dropped finalizers, failed acknowledgement storage and live retry, partial admission after reopening, lease-proof rejection and journal replay. The configured HTTP/runtime test rejects a competing key while the original inference is pending and checks quiescence for success, write, audit failure, egress denial and deadline outcomes. The race fixture initially reused its two-party barrier on a later single admission; that test-only hang was corrected. The historical source-order finalization assertion was updated for the outer completion owner while preserving its cleanup invariant.

Architecture validation required keeping projection migration writes in the existing run_store module. The supervisor also lost its unused duplicate synchronous persist method, retaining the production asynchronous path. No gate allowlist was weakened. Final validation results follow after the remaining checks finish. Sprint 2 remains in progress.

Final validation: domain (32), memory (107 including integration), run (91) and application library (136) suites passed. Both focused application finalization suites passed (34 tests), for 400 distinct passing tests. After the final module cleanup, the capacity migration tests and all 23 managed-service tests passed again. The daemon cargo check, architecture gate and whitespace check passed. The previously documented broader application dependency-rule failure was not changed or rerun. No all-workspace-green claim is made.
