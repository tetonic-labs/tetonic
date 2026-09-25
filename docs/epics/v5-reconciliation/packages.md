# Workspace disposition inventory

Generated from manifests and tracked paths. Dispositions are planning recommendations, not proof of dead code.

| Package | Files / Rust | Proposed disposition | Target responsibility | Declared dependents (including tests) |
|---|---:|---|---|---|
| `tetonic-egress` | 6 / 4 | keep-integrate | Outbound authorization; not an OS-wide firewall | tetonic-app, tetonic-arch-gate, tetonic-enroll, tetonic-fabric-client, tetonic-inference, tetonic-node, tetonic-runtime, tetonic-server |
| `tetonic-fabric-client` | 23 / 21 | reshape | Retain live transport; retire legacy wire only after migration | tetonic-app, tetonic-node |
| `tetonic-fabric-protocol` | 31 / 30 | keep-generalize | Worker delivery contracts; distinguish inference from agent execution | tetonic-app, tetonic-broker, tetonic-fabric-client, tetonic-inference, tetonic-node |
| `tetonic-inference` | 24 / 21 | keep-integrate | Provider adapters behind broker and egress | tetonic-app, tetonic-arch-gate, tetonic-broker, tetonic-capacity, tetonic-core, tetonic-eval, tetonic-fabric-client, tetonic-node, tetonic-orchestrator, tetonic-runtime, tetonic-server |
| `tetonic-rpc` | 9 / 7 | adapt | Local RPC compatibility; not authenticated remote control API | lokaid, tetonic-arch-gate |
| `tetonic-core` | 20 / 18 | reshape | Built-in harness; remove universal coding/world lifecycle assumptions | tetonic-app, tetonic-arch-gate, tetonic-bench, tetonic-eval, tetonic-orchestrator, tetonic-run, tetonic-runtime, tetonic-server |
| `tetonic-domain` | 33 / 31 | reshape | Shared identity, execution, authorization and adapter contracts | tetonic-app, tetonic-arch-gate, tetonic-artifact, tetonic-broker, tetonic-context, tetonic-core, tetonic-eval, tetonic-fabric-client, tetonic-fabric-protocol, tetonic-index, tetonic-inference, tetonic-memory, tetonic-node, tetonic-orchestrator, tetonic-policy, tetonic-run, tetonic-runtime, tetonic-sandbox, tetonic-secrets, tetonic-server, tetonic-tools, tetonic-transaction |
| `tetonic-policy` | 11 / 9 | keep-integrate | Execution policy; add trusted tenant and principal context | tetonic-app, tetonic-arch-gate, tetonic-core, tetonic-eval, tetonic-inference, tetonic-memory, tetonic-orchestrator, tetonic-run, tetonic-runtime, tetonic-tools |
| `tetonic-runtime` | 19 / 16 | reshape | Worker assembly and capability services; separate optional harness strategies | tetonic-app, tetonic-arch-gate, tetonic-eval, tetonic-orchestrator, tetonic-run, tetonic-server |
| `tetonic-sandbox` | 31 / 28 | keep-integrate | Process isolation with explicit supported guarantees | tetonic-app, tetonic-eval, tetonic-node, tetonic-tools |
| `tetonic-secrets` | 12 / 10 | keep-integrate | Credential handling and outbound scanning | tetonic-app, tetonic-broker, tetonic-context, tetonic-eval, tetonic-node, tetonic-runtime, tetonic-sandbox, tetonic-telemetry, tetonic-tools |
| `tetonic-telemetry` | 15 / 13 | keep-integrate | Correlated and redacted operational telemetry | tetonic-app, tetonic-broker, tetonic-core, tetonic-eval, tetonic-fabric-client, tetonic-inference, tetonic-memory, tetonic-rpc, tetonic-tools, tetonic-transaction |
| `tetonic-transaction` | 21 / 19 | isolate | Workspace mutation facility for coding capabilities | tetonic-app, tetonic-arch-gate, tetonic-core, tetonic-fabric-client, tetonic-orchestrator, tetonic-runtime, tetonic-tools |
| `lokai-cli` | 41 / 39 | reshape | Tetonic client; retain compatibility until command migration | none |
| `lokaid` | 42 / 31 | retire-after-cutover | Compatibility transport to common services; retire independent assembly | none |
| `tetonic-app` | 88 / 85 | reshape | Control services and composition; extract coding product behavior | lokai-cli, lokaid, tetonic-eval |
| `tetonic-lsp` | 15 / 13 | isolate | Optional coding capability | tetonic-app |
| `tetonic-tools` | 25 / 21 | isolate | Coding tool pack; adapt to generic capability boundary | tetonic-app, tetonic-arch-gate, tetonic-bench, tetonic-core, tetonic-orchestrator, tetonic-runtime |
| `tetonic-broker` | 36 / 34 | keep-integrate | Compute admission and budget accounting; not agent registry | tetonic-app, tetonic-eval |
| `tetonic-capacity` | 22 / 21 | isolate | Optional inference capacity optimization | tetonic-app, tetonic-node |
| `tetonic-enroll` | 12 / 10 | keep-integrate | Node trust/bootstrap; distinct from employee authorization | tetonic-app, tetonic-fabric-client, tetonic-node |
| `tetonic-node` | 22 / 19 | reshape | Existing inference worker transport; replace detached role registry | tetonic-app, tetonic-enroll |
| `tetonic-orchestrator` | 22 / 20 | split | Move fleet domain to control services; keep coding strategy optional | tetonic-app, tetonic-eval |
| `tetonic-run` | 36 / 35 | keep-generalize | Authoritative durable run/task/attempt state and managed execution | tetonic-app, tetonic-broker, tetonic-fabric-client |
| `tetonic-server` | 7 / 5 | replace-composition | General server bootstrap; migrate world harness out of main | none |
| `tetonic-artifact` | 9 / 7 | keep-integrate | Scoped artifact access and provenance | tetonic-app, tetonic-arch-gate, tetonic-context, tetonic-eval, tetonic-fabric-client, tetonic-run, tetonic-runtime |
| `tetonic-context` | 19 / 17 | isolate | Context compilation; coding assumptions behind capability/harness boundary | tetonic-app, tetonic-orchestrator, tetonic-runtime |
| `tetonic-index` | 14 / 12 | isolate | Optional repository indexing capability | tetonic-app, tetonic-bench, tetonic-orchestrator |
| `tetonic-memory` | 29 / 26 | split-logically | Separate platform persistence from agent memory interfaces | tetonic-app, tetonic-arch-gate, tetonic-broker, tetonic-capacity, tetonic-eval, tetonic-fabric-client, tetonic-node, tetonic-orchestrator, tetonic-run, tetonic-runtime, tetonic-tools |
| `tetonic-arch-gate` | 37 / 14 | update | Preserve invariants; replace obsolete structural rules | none |
| `tetonic-bench` | 3 / 1 | keep | Performance measurement; add runtime workload baselines | none |
| `tetonic-eval` | 26 / 22 | keep-expand | Retain evaluations; add lifecycle and tenant isolation scenarios | tetonic-app |
