import type { MissionWithLineage } from '../types/lineage';

export const SAMPLE_LINEAGE_MISSIONS: MissionWithLineage[] = [
  // ---------------------------------------------------------------------------
  // 1. INCOMPLETE / PROPOSED: Security Vault Migration
  // ---------------------------------------------------------------------------
  {
    id: 'mission-vault-auth',
    title: 'Migrate Auth Tokens to OS Credential Vault',
    context: 'Security',
    status: 'proposed',
    stageLabel: 'Proposed · Architectural Debate',
    leadAgentId: 'agt-scout',
    leadAgentName: 'Turing',
    collaboratorIds: ['agt-builder'],
    startedAt: '15m ago',
    executiveSummary:
      'Security audit detected plaintext API keys in local developer config files. Scoped proposal to eliminate all file-based secrets and migrate inference credentials into the native OS Secure Vault (Windows Credential Manager / macOS Keychain).',
    whyThisMatters:
      'Prevents accidental git commits of production tokens and protects hardware from rogue process extraction.',
    milestones: [
      {
        id: 'm1-intent',
        number: '01',
        title: 'Security Directive & Threat Model',
        summary:
          'Audit detected 2 unencrypted API tokens residing in ~/.tetonic/config.toml. Mandated zero disk plaintext.',
        status: 'done',
        timestamp: '15m ago',
      },
      {
        id: 'm2-debate',
        number: '02',
        title: 'Architectural Debate: OS Keyring vs. Encrypted SQLite',
        summary:
          'Turing and Barnaby debated storage primitives. Turing advocates native OS Keyring for hardware TPM backing; Barnaby raised cross-platform headless daemon concerns.',
        status: 'active',
        timestamp: 'Active right now',
        tradeoffs: [
          {
            option: 'Option A: Native OS Credential Vault (wincred / macOS Keychain / SecretService)',
            pros: 'Hardware TPM encryption, zero plaintext on disk, standard OS security policy.',
            cons: 'Requires platform-specific C-FFI bindings; headless Linux CI requires SecretService daemon.',
            chosen: true,
          },
          {
            option: 'Option B: Encrypted SQLite DB (SQLCipher) with user passphrase',
            pros: 'Single unified Rust codebase across all operating systems without platform quirks.',
            cons: 'Requires human entering passphrase every startup; risk of passphrase forgotten.',
            chosen: false,
          },
        ],
      },
      {
        id: 'm3-execution',
        number: '03',
        title: 'Rust Assembly & Atmos Credential Provider',
        summary:
          'Scoped implementation in engine/atmos/src/credentials.rs and tetonic-cli credential prompt.',
        status: 'pending',
      },
      {
        id: 'm4-verification',
        number: '04',
        title: 'Memory Heap Dump & Penetration Audit',
        summary:
          'Automated heap scan to verify key bytes are zeroized upon drop and never written to swap.',
        status: 'pending',
      },
    ],
  },

  // ---------------------------------------------------------------------------
  // 2. IN PROGRESS: Vector Index Latency Optimization
  // ---------------------------------------------------------------------------
  {
    id: 'mission-vector-perf',
    title: 'Optimize Vector Index Latency under Concurrency',
    context: 'Performance',
    status: 'in_progress',
    stageLabel: 'In Progress · Step 3 of 4 Running',
    leadAgentId: 'agt-evaluator',
    leadAgentName: 'Ziggy',
    collaboratorIds: ['agt-scout', 'agt-builder'],
    startedAt: '42m ago',
    executiveSummary:
      'Multi-agent concurrent queries currently cause vector search latency spikes up to 85ms on large embeddings. Active implementation of an HNSW (Hierarchical Navigable Small World) index with 8-bit scalar quantization to achieve sub-15ms p99 query latency.',
    whyThisMatters:
      'When 6 companions search shared memory at once, vector lookup cannot become the bottleneck.',
    milestones: [
      {
        id: 'm1-intent',
        number: '01',
        title: 'Performance Benchmark Contract',
        summary:
          'Target: p99 latency <15ms under 100 simultaneous concurrent worker queries over 50,000 vectors.',
        status: 'done',
        timestamp: '42m ago',
      },
      {
        id: 'm2-debate',
        number: '02',
        title: 'Algorithm Selection: HNSW vs. IVFFlat',
        summary:
          'Evaluated Flat L2 brute-force vs. IVFFlat vs. HNSW. Selected HNSW with cosine metric for 98.4% recall at 10x throughput.',
        status: 'done',
        timestamp: '35m ago',
      },
      {
        id: 'm3-execution',
        number: '03',
        title: 'Implementation in Strata Memory Layer',
        summary:
          'Ziggy created engine/strata/src/hnsw_graph.rs (+184 lines). Otto currently generating SIMD AVX2 dot-product acceleration.',
        status: 'active',
        timestamp: 'In flight now',
        actions: [
          {
            id: 'act-1',
            timestamp: '01:22 AM',
            agentId: 'agt-evaluator',
            agentName: 'Ziggy',
            actionType: 'edit',
            summary: 'Created HNSW graph structure with multi-layer skip lists',
            target: 'engine/strata/src/hnsw_graph.rs',
            diff: `@@ -0,0 +18,12 @@
+pub struct HnswIndex<M: DistanceMetric> {
+    pub layers: Vec<LayerGraph>,
+    pub entry_point: NodeId,
+    pub max_connections: usize,
+    pub ef_construction: usize,
+    metric: PhantomData<M>,
+}
+impl<M: DistanceMetric> HnswIndex<M> {
+    pub fn search_knn(&self, query: &[f32], k: usize) -> Vec<SearchResult> {
+        // SIMD-accelerated nearest neighbor traversal
+    }
+}`,
          },
          {
            id: 'act-2',
            timestamp: '01:38 AM',
            agentId: 'agt-scout',
            agentName: 'Otto',
            actionType: 'command',
            summary: 'Running micro-benchmarks on 10,000 vectors via cargo bench',
            target: 'engine/tooling/tetonic-bench',
            rawLog: `running 3 benchmarks
benchmark hnsw_search_10k_dim1536 ... bench:   11,420,180 ns/iter (+/- 410,200)
benchmark flat_l2_10k_dim1536     ... bench:   84,190,040 ns/iter (+/- 1,820,000)
Speedup: 7.37x faster with 98.7% recall retention.`,
          },
        ],
      },
      {
        id: 'm4-verification',
        number: '04',
        title: 'Stress Verification Suite',
        summary:
          'Will execute 50,000 synthetic embeddings with simulated thread contention across 8 worker cores.',
        status: 'pending',
      },
    ],
  },

  // ---------------------------------------------------------------------------
  // 3. FINISHED / SETTLED: Checkout Reliability & Error Resilience
  // ---------------------------------------------------------------------------
  {
    id: 'mission-checkout-recovery',
    title: 'Restore Checkout Reliability & Error Resilience',
    context: 'Operations',
    status: 'settled',
    stageLabel: 'Settled & Verified · 14/14 Tests Passed',
    leadAgentId: 'agt-builder',
    leadAgentName: 'Barnaby',
    collaboratorIds: ['agt-architect', 'agt-evaluator'],
    startedAt: '3h ago',
    completedAt: '28m ago',
    executiveSummary:
      'Remediated intermittent 504 gateway timeouts caused by upstream payment packet drops during burst traffic. Implemented exponential backoff with full jitter and a persistent SQLite local retry spool. 100% success rate verified across 1,000 simulated stress requests with 0 customer-visible drops.',
    whyThisMatters:
      'Payment requests are mission-critical. A brief network blip must never lose a customer transaction or fail without retry.',
    milestones: [
      {
        id: 'm1-intent',
        number: '01',
        title: 'Incident Detection & Threshold Breach',
        summary:
          'Error rate rose from 0.2% to 8.1% following upstream provider outage. System triggered an automated mission.',
        status: 'done',
        timestamp: '3h ago',
      },
      {
        id: 'm2-debate',
        number: '02',
        title: 'Strategy Formulation: Full Rollback vs. Localized Retry Spool',
        summary:
          'Barnaby analyzed gateway TCP traces. The team chose to keep the current release active and deploy a localized spool with exponential backoff rather than a disruptive full rollback.',
        status: 'done',
        timestamp: '2h 15m ago',
        tradeoffs: [
          {
            option: 'Strategy A: Roll back entire release to previous version',
            pros: 'Safe and instant; returns codebase to yesterday’s state.',
            cons: 'Rolls back 4 unrelated feature additions and ignores the upstream packet loss issue.',
            chosen: false,
          },
          {
            option: 'Strategy B: Implement exponential retry with jitter + SQLite spool',
            pros: 'Directly solves upstream packet loss, zero transaction drops, keeps new features.',
            cons: 'Requires 30 lines of resilient retry logic and unit test coverage.',
            chosen: true,
          },
        ],
      },
      {
        id: 'm3-execution',
        number: '03',
        title: 'Atomic Code Execution & Spool Implementation',
        summary:
          'Modified engine/strata/src/backup.rs, schema.rs, and service.rs in an atomic OS transaction.',
        status: 'done',
        timestamp: '1h 10m ago',
        actions: [
          {
            id: 'act-b1',
            timestamp: '02:04 AM',
            agentId: 'agt-builder',
            agentName: 'Barnaby',
            actionType: 'read',
            summary: 'Inspected upstream HTTP transport retry policy',
            target: 'engine/mantle/tetonic-run/src/service.rs',
          },
          {
            id: 'act-b2',
            timestamp: '02:18 AM',
            agentId: 'agt-builder',
            agentName: 'Barnaby',
            actionType: 'edit',
            summary: 'Added exponential backoff with decorrelated full jitter algorithm',
            target: 'engine/mantle/tetonic-run/src/service.rs',
            diff: `@@ -142,6 +142,18 @@
+pub async fn execute_with_resilient_retry<F, Fut, T>(mut op: F) -> Result<T, ServiceError>
+where
+    F: FnMut() -> Fut,
+    Fut: Future<Output = Result<T, ServiceError>>,
+{
+    let mut attempts = 0;
+    let mut delay = Duration::from_millis(50);
+    loop {
+        match op().await {
+            Ok(val) => return Ok(val),
+            Err(e) if e.is_retryable() && attempts < 5 => {
+                attempts += 1;
+                let jitter = rand::random::<f64>() * 0.5 + 0.5;
+                tokio::time::sleep(delay.mul_f64(jitter)).await;
+                delay = (delay * 2).min(Duration::from_secs(3));
+            }
+            Err(final_err) => return Err(final_err),
+        }
+    }
+}`,
            rawLog: `$ cargo test --package tetonic-run --lib service::retry_tests
   Compiling tetonic-run v0.1.0 (C:/tetonic/engine/mantle/tetonic-run)
    Finished test [unoptimized + debuginfo] target(s) in 0.44s
     Running unittests src/lib.rs (target/debug/deps/tetonic_run-2b99)
test service::retry_tests::test_single_packet_drop_recovery ... ok
test service::retry_tests::test_decorrelated_jitter_distribution ... ok
test service::retry_tests::test_sqlite_spool_flush_on_reconnect ... ok
test result: ok. 14 passed; 0 failed; finished in 0.88s`,
          },
          {
            id: 'act-b3',
            timestamp: '02:35 AM',
            agentId: 'agt-architect',
            agentName: 'Ada',
            actionType: 'review',
            summary: 'Code review completed: verified lock contention and thread safety',
            target: 'engine/mantle/tetonic-run/src/service.rs',
          },
        ],
      },
      {
        id: 'm4-verification',
        number: '04',
        title: 'Verification & Synthetic Stress Testing',
        summary:
          '14/14 unit tests passed. Injected 1,000 simulated socket resets; 100% recovered with 0 dropped events.',
        status: 'done',
        timestamp: '45m ago',
        verification: {
          suite: 'tetonic-run::service::retry_tests',
          passed: 14,
          failed: 0,
          benchmark: 'p95: 14.2ms, p99: 18.6ms across 1,000 fault-injected operations',
          details: `running 14 tests
test service::retry_tests::test_single_packet_drop_recovery ... ok
test service::retry_tests::test_decorrelated_jitter_distribution ... ok
test service::retry_tests::test_sqlite_spool_flush_on_reconnect ... ok
test service::retry_tests::test_max_retry_threshold_exhaustion ... ok
test service::retry_tests::test_concurrent_spool_contention ... ok
test result: ok. 14 passed; 0 failed; 0 ignored; finished in 1.42s`,
        },
      },
      {
        id: 'm5-artifact',
        number: '05',
        title: 'Settled Artifact & Git Commit Recorded',
        summary:
          'Committed to git branch fix/checkout-resilience (commit hash a3f89e1). Sealed into Strata memory ledger.',
        status: 'done',
        timestamp: '28m ago',
        artifact: {
          branch: 'fix/checkout-resilience',
          commitHash: 'a3f89e17b8',
          filesCount: 3,
          summary: '3 files changed, +42 insertions, -6 deletions',
        },
      },
    ],
  },

  // ---------------------------------------------------------------------------
  // 4. FAST SINGLE-TURN TOOL RUN (Otto · Direct Benchmark Receipt, NO Phase Tabs)
  // ---------------------------------------------------------------------------
  {
    id: 'mission-mem-profile',
    title: 'Profile Memory Allocation Spikes under 50k Vector Insertions',
    context: 'Performance',
    status: 'settled',
    stageLabel: 'Delivered · 1 Tool Step',
    leadAgentId: 'agt-otto',
    leadAgentName: 'Otto',
    collaboratorIds: [],
    model: 'qwen3.5:latest',
    runId: 'run-5e229a4f-5531',
    startedAt: '14m ago',
    completedAt: '14m ago',
    executiveSummary:
      'Single-turn micro-benchmark on vector heap allocation. Executed cargo bench memory_allocations over 50,000 synthetic 1536-dim embeddings. Maximum heap allocation remained strictly bounded to 38.4 MB with 0 page faults and 0 memory leaks detected.',
    milestones: [
      {
        id: 'm1-bench',
        number: '01',
        title: 'Micro-Benchmark Execution',
        summary: 'Ran cargo bench -p tetonic-bench --bench memory_allocations in sandbox.',
        status: 'done',
        actions: [
          {
            id: 'act-bench-1',
            timestamp: '03:12 AM',
            agentId: 'agt-otto',
            agentName: 'Otto',
            actionType: 'command',
            summary: 'Executed cargo bench memory_allocations in isolated cgroup sandbox',
            target: 'engine/tooling/tetonic-bench',
            rawLog: `$ cargo bench --package tetonic-bench --bench memory_allocations
   Compiling tetonic-bench v0.1.0 (C:/tetonic/engine/tooling/tetonic-bench)
    Finished bench [optimized] target(s) in 1.12s
     Running benches/memory_allocations.rs (target/release/deps/memory_allocations-9d8a)
Gnuplot not found, using plotters backend
memory_profile/50k_insertions:
  time:   [82.140 ms 82.418 ms 82.711 ms]
  allocs: 50,000 calls (0 reallocations)
  peak:   38.42 MB rss (limit: 128 MB)
  leaks:  0 bytes unreleased upon drop
Benchmark complete: memory footprint safely within director threshold.`,
          },
        ],
      },
    ],
  },

  // ---------------------------------------------------------------------------
  // 5. GATED ACTION / NEEDS ATTENTION (Turing · Network Effect Gate Pending)
  // ---------------------------------------------------------------------------
  {
    id: 'mission-remote-sync-gate',
    title: 'Sync Revocation List from External Node 10.0.4.12',
    context: 'Security',
    status: 'needs_attention',
    stageLabel: 'Needs Attention · Gated Action',
    leadAgentId: 'agt-scout',
    leadAgentName: 'Turing',
    collaboratorIds: [],
    model: 'qwen3.5:latest',
    runId: 'run-9c1e482a-9912',
    startedAt: '8m ago',
    executiveSummary:
      'Agent Turing attempted an outbound TCP socket to remote node 10.0.4.12:8443 to pull signed certificate revocation lists. Execution halted by engine network effect gate.',
    whyThisMatters:
      'Outbound network connections from sandboxed workers require human director sign-off to prevent unauthorized telemetry or data exfiltration.',
    milestones: [
      {
        id: 'm1-gate',
        number: '01',
        title: 'Effect Gate Hold',
        summary:
          'Harness intercepted outbound socket request: -> castle.us-east-vault [Run: sync_revocations:10.0.4.12]. Waiting for director authorization.',
        status: 'active',
        actions: [
          {
            id: 'act-gate-1',
            timestamp: '03:18 AM',
            agentId: 'agt-scout',
            agentName: 'Turing',
            actionType: 'command',
            summary: 'Requested outbound socket to remote node 10.0.4.12:8443',
            target: 'network:guard',
            rawLog: `[SECURITY POLICY GATE]
Effect Class: cross_castle_request
Target: 10.0.4.12:8443 (castle.us-east-vault)
Payload Digest: SHA256(sync_revocations_v2_bundle)
Policy: Default deny for unlisted external CIDRs.
Action: Execution suspended. Awaiting Director Authorize / Reject decision.`,
          },
        ],
      },
    ],
  },

  // ---------------------------------------------------------------------------
  // 6. FAILED EXECUTION / NEEDS ATTENTION (Ziggy · Compiler Panic & SIGILL)
  // ---------------------------------------------------------------------------
  {
    id: 'mission-simd-compile-failure',
    title: 'Compile AVX-512 Distance Kernel for Strata Index',
    context: 'Core Indexing',
    status: 'needs_attention',
    stageLabel: 'Failed · SIGILL',
    leadAgentId: 'agt-evaluator',
    leadAgentName: 'Ziggy',
    collaboratorIds: [],
    model: 'gpt-4o',
    runId: 'run-3f88d120-7741',
    startedAt: '19m ago',
    executiveSummary:
      'SIMD kernel compilation failed during sandbox verification. Target CPU architecture does not support AVX-512 Foundation instructions.',
    error: 'SIGILL: Illegal instruction on host CPU architecture (AVX-512F unsupported by target hardware)',
    milestones: [
      {
        id: 'm1-fail',
        number: '01',
        title: 'JIT Compilation Failure',
        summary:
          'Rust SIMD compilation aborted with SIGILL code 132. Target hardware lacks AVX-512F vector registers.',
        status: 'active',
        actions: [
          {
            id: 'act-fail-1',
            timestamp: '03:07 AM',
            agentId: 'agt-evaluator',
            agentName: 'Ziggy',
            actionType: 'command',
            summary: 'Executed cargo build --features avx512',
            target: 'engine/strata/src/simd_kernel.rs',
            rawLog: `$ cargo build --package tetonic-strata --features avx512
   Compiling tetonic-strata v0.1.0 (C:/tetonic/engine/strata/tetonic-memory)
error: signal 4 (SIGILL): illegal instruction
   --> engine/strata/src/simd_kernel.rs:48:9
    |
48  |         _mm512_fmadd_ps(a, b, acc);
    |         ^^^^^^^^^^^^^^^^^^^^^^^^^^ instruction not supported on this host CPU
    |
    = note: host CPU 'x86-64-v3' does not advertise 'avx512f' feature flag
    = help: compile with '--features avx2' for broad x86-64 hardware compatibility
error: could not compile 'tetonic-strata' due to 1 previous error`,
          },
        ],
      },
    ],
  },
];
