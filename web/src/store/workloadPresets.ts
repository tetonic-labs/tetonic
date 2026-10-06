import type { MissionWithLineage } from '../types/lineage';
import { SAMPLE_LINEAGE_MISSIONS } from './sampleLineageMissions';

export type WorkloadPreset = 'focused' | 'balanced' | 'swarm' | 'live';

export interface WorkloadPresetMeta {
  id: WorkloadPreset;
  label: string;
  itemCount: number;
  description: string;
}

export const WORKLOAD_PRESETS: WorkloadPresetMeta[] = [
  {
    id: 'focused',
    label: 'Focused (3)',
    itemCount: 3,
    description: 'Quiet single-team focus: 1 proposed, 1 in flight, 1 completed deliverable.',
  },
  {
    id: 'balanced',
    label: 'Balanced (6)',
    itemCount: 6,
    description: 'Standard day across archetypes: RFC debate, live coding loop, 2 gates, 2 settled.',
  },
  {
    id: 'swarm',
    label: 'Swarm (16)',
    itemCount: 16,
    description: 'High-density concurrent execution across 5 engineering domains with parallel tools.',
  },
];

// Additional missions to populate the 16-mission Swarm workload
export const SWARM_EXTRA_MISSIONS: MissionWithLineage[] = [
  // ---------------------------------------------------------------------------
  // Proposed in Swarm
  // ---------------------------------------------------------------------------
  {
    id: 'mission-raft-consensus',
    title: 'Quorum Consensus Protocol for Multi-Engine Cluster',
    context: 'Infra',
    status: 'proposed',
    stageLabel: 'Proposed · RFC Scoping',
    leadAgentId: 'agt-architect',
    leadAgentName: 'Ada',
    collaboratorIds: ['agt-builder'],
    startedAt: '1h ago',
    model: 'claude-3-7-sonnet',
    runId: 'run-9104',
    executiveSummary:
      'Design proposal for Raft consensus across distributed local engines to guarantee serializable lineage commit order without split-brain anomalies.',
    whyThisMatters:
      'Prevents conflicting state modifications when multiple co-located engines attempt simultaneous workspace commits.',
    milestones: [
      {
        id: 'm1-raft-rfc',
        number: '01',
        title: 'Consensus Boundary & Term Model',
        summary: 'Specification of Raft leader election and log replication heartbeat interval (50ms).',
        status: 'active',
        timestamp: '1h ago',
      },
      {
        id: 'm2-raft-proto',
        number: '02',
        title: 'gRPC Transport & Quorum Voting',
        summary: 'Prototyping quorum vote serialization over mutual TLS.',
        status: 'pending',
      },
    ],
  },
  {
    id: 'mission-ebpf-trace',
    title: 'Zero-Overhead Linux eBPF Kernel Tracing Probe',
    context: 'Security',
    status: 'proposed',
    stageLabel: 'Proposed · Kernel Probe Design',
    leadAgentId: 'agt-scout',
    leadAgentName: 'Turing',
    collaboratorIds: ['agt-otto'],
    startedAt: '2h ago',
    model: 'qwen3.5:latest',
    runId: 'run-8821',
    executiveSummary:
      'Drafting eBPF kernel kprobes to hook into sys_enter_openat and sys_enter_execve, establishing tamper-proof OS syscall telemetry for untrusted agents.',
    whyThisMatters:
      'Guarantees no subagent can bypass sandbox filesystem auditing even if user-space libraries are compromised.',
    milestones: [
      {
        id: 'm1-ebpf-spec',
        number: '01',
        title: 'Syscall Audit Matrix',
        summary: 'Identified 18 high-risk syscalls requiring kernel ring-buffer intercepts.',
        status: 'active',
        timestamp: '2h ago',
      },
    ],
  },
  {
    id: 'mission-wal-compression',
    title: 'Zstandard Streaming Compression for Write-Ahead Log',
    context: 'Core Systems',
    status: 'proposed',
    stageLabel: 'Proposed · Compression Benchmark',
    leadAgentId: 'agt-builder',
    leadAgentName: 'Barnaby',
    collaboratorIds: ['agt-otto'],
    startedAt: '3h ago',
    model: 'claude-3-7-sonnet',
    runId: 'run-7740',
    executiveSummary:
      'Evaluation of zstd dictionary compression for WAL frames to reduce NVMe write amplification by ~60% under high-frequency agent tool updates.',
    milestones: [
      {
        id: 'm1-wal-bench',
        number: '01',
        title: 'Entropy & Throughput Analysis',
        summary: 'Sampled 100MB of raw lineage frames; zstd level 3 achieved 4.2x ratio at 820MB/s.',
        status: 'active',
        timestamp: '3h ago',
      },
    ],
  },

  // ---------------------------------------------------------------------------
  // In Progress in Swarm
  // ---------------------------------------------------------------------------
  {
    id: 'mission-simd-dispatch',
    title: 'Dynamic AVX-512 / NEON SIMD Dispatch Matrix',
    context: 'Performance',
    status: 'in_progress',
    stageLabel: 'In Progress · Test Pass Running',
    leadAgentId: 'agt-builder',
    leadAgentName: 'Barnaby',
    collaboratorIds: ['agt-evaluator'],
    startedAt: '28m ago',
    model: 'gpt-4o',
    runId: 'run-6411',
    liveElapsedSeconds: 1680,
    liveToolSnippet: 'cargo test --package strata-simd',
    progressPercent: 65,
    executiveSummary:
      'Compiling CPU feature detection routines at engine boot to dynamically dispatch unrolled SIMD vector distance routines without runtime branch penalty.',
    whyThisMatters: 'Enables maximum vector throughput on modern hardware while maintaining scalar fallback safety.',
    milestones: [
      {
        id: 'm1-simd-feat',
        number: '01',
        title: 'Hardware Feature Detection',
        summary: 'CPUID probing implemented for AVX2, AVX-512F, and ARM NEON.',
        status: 'done',
        timestamp: '28m ago',
      },
      {
        id: 'm2-simd-kernels',
        number: '02',
        title: 'Dispatch Table Assembly',
        summary: 'Atomic function pointer swap table passing concurrency test suite.',
        status: 'active',
        timestamp: 'Running now',
        actions: [
          {
            id: 'act-simd-test',
            timestamp: 'Turn 3',
            agentId: 'agt-builder',
            agentName: 'Barnaby',
            actionType: 'test',
            summary: 'Running SIMD kernel verification suite across 10,000 synthetic vector permutations',
            target: 'crates/strata-simd/src/dispatch.rs',
            rawLog: `$ cargo test --package strata-simd -- --nocapture
running 4 tests
test dispatch::test_avx2_detect ... ok
test dispatch::test_avx512_fallback ... ok
test dispatch::test_cosine_accuracy ... ok
test dispatch::test_l2_norm_simd ... ok

test result: ok. 4 passed; 0 failed; finished in 0.42s`,
          },
        ],
      },
    ],
  },
  {
    id: 'mission-zero-copy-rpc',
    title: 'Shared-Memory IPC Rings for Co-Located Companions',
    context: 'Core Systems',
    status: 'in_progress',
    stageLabel: 'In Progress · Verification Pass',
    leadAgentId: 'agt-architect',
    leadAgentName: 'Ada',
    collaboratorIds: ['agt-builder'],
    startedAt: '18m ago',
    model: 'claude-3-7-sonnet',
    runId: 'run-5209',
    liveElapsedSeconds: 1080,
    liveToolSnippet: 'cargo check --bench ipc_ring',
    progressPercent: 40,
    executiveSummary:
      'Replacing TCP localhost sockets with POSIX shared-memory circular ring buffers for inter-agent IPC, cutting latency from 1.2ms to 85μs.',
    milestones: [
      {
        id: 'm1-ipc-proto',
        number: '01',
        title: 'Lock-Free SPSC Ring Buffer',
        summary: 'Single-producer single-consumer ring buffer using atomic head/tail indices.',
        status: 'active',
        timestamp: 'Running now',
        actions: [
          {
            id: 'act-ipc-check',
            timestamp: 'Turn 2',
            agentId: 'agt-architect',
            agentName: 'Ada',
            actionType: 'review',
            summary: 'Verified Acquire/Release memory ordering across atomic cache-line boundaries',
            target: 'engine/ipc/src/ring_buffer.rs',
            rawLog: `$ cargo check --package tetonic-ipc
    Checking tetonic-ipc v0.2.0 (/workspace/engine/ipc)
    Finished dev [unoptimized + debuginfo] target(s) in 0.61s`,
          },
        ],
      },
    ],
  },
  {
    id: 'mission-mem-continuous',
    title: 'Continuous Allocation Profiler for Long-Running Swarms',
    context: 'Benchmarking',
    status: 'in_progress',
    stageLabel: 'In Progress · Active Profile Stream',
    leadAgentId: 'agt-otto',
    leadAgentName: 'Otto',
    collaboratorIds: ['agt-evaluator'],
    startedAt: '12m ago',
    model: 'qwen3.5:latest',
    runId: 'run-4190',
    liveElapsedSeconds: 720,
    liveToolSnippet: 'cargo bench --bench alloc_profile',
    progressPercent: 55,
    executiveSummary:
      'Tracing jemalloc arena allocations across 200,000 synthetic multi-agent iterations to catch subtle heap fragmentation before production release.',
    milestones: [
      {
        id: 'm1-alloc-stream',
        number: '01',
        title: 'Jemalloc Heap Profiling Stream',
        summary: 'Tracking memory RSS deltas every 10k synthetic query operations.',
        status: 'active',
        timestamp: 'Running now',
        actions: [
          {
            id: 'act-alloc-run',
            timestamp: 'Turn 1',
            agentId: 'agt-otto',
            agentName: 'Otto',
            actionType: 'command',
            summary: 'Executing 200k iteration synthetic allocation loop',
            rawLog: `$ cargo bench --bench alloc_profile -- --profile-time=30
Allocating 50,000 test vectors...
[iteration: 50,000]  Active RSS: 414.2 MB  | Fragmentation: 1.02%
[iteration: 100,000] Active RSS: 414.2 MB  | Fragmentation: 1.02%
[iteration: 150,000] Active RSS: 414.3 MB  | Fragmentation: 1.03%
Status: Zero progressive leakage observed.`,
          },
        ],
      },
    ],
  },
  {
    id: 'mission-sandbox-jail',
    title: 'cgroups v2 & Landlock Jail Containment Enforcement',
    context: 'Security',
    status: 'in_progress',
    stageLabel: 'In Progress · Enforcing Policies',
    leadAgentId: 'agt-scout',
    leadAgentName: 'Turing',
    collaboratorIds: ['agt-builder'],
    startedAt: '5m ago',
    model: 'qwen3.5:latest',
    runId: 'run-3810',
    liveElapsedSeconds: 300,
    liveToolSnippet: 'landlock-apply --ruleset strict',
    progressPercent: 20,
    executiveSummary:
      'Locking agent tool execution child processes into Linux Landlock ABI v3 with strict read-only /usr, isolated /tmp, and blocked raw socket access.',
    milestones: [
      {
        id: 'm1-landlock-jail',
        number: '01',
        title: 'Landlock Rule Compilation',
        summary: 'Generating restricted filesystem capability set for agent sub-processes.',
        status: 'active',
        timestamp: 'Running now',
        actions: [
          {
            id: 'act-jail-enforce',
            timestamp: 'Turn 1',
            agentId: 'agt-scout',
            agentName: 'Turing',
            actionType: 'command',
            summary: 'Applied Landlock ABI v3 ruleset: blocked /dev/mem, /etc/shadow, and RAW_SOCKETS',
            rawLog: `$ tetonic-jail enforce --pid 48192 --profile strict_agent
[JAIL] Landlock ABI v3 activated.
[JAIL] Path /etc: READ_ONLY
[JAIL] Path /workspace: READ_WRITE
[JAIL] Net: OUTBOUND_GATED (Port 8443 only via proxy)`,
          },
        ],
      },
    ],
  },

  // ---------------------------------------------------------------------------
  // Needs Attention in Swarm
  // ---------------------------------------------------------------------------
  {
    id: 'mission-data-retention-gate',
    title: 'Purge 90-Day Telemetry Logs from S3 Cold Storage',
    context: 'Data',
    status: 'needs_attention',
    stageLabel: 'Needs Attention · Destructive Gate',
    leadAgentId: 'agt-evaluator',
    leadAgentName: 'Ziggy',
    collaboratorIds: ['agt-scout'],
    startedAt: '34m ago',
    model: 'gpt-4o',
    runId: 'run-2980',
    executiveSummary:
      'Automated retention worker queued batch deletion of 1.4 TB (14,200 objects) of telemetry traces older than 90 days. Irreversible deletion held for Director authorization.',
    whyThisMatters:
      'Prevents irreversible data loss of historical traces required for regulatory compliance unless explicitly sanctioned.',
    error: 'Effect Gate: Irreversible batch deletion requires explicit Director authorization token.',
    milestones: [
      {
        id: 'm1-retention-scan',
        number: '01',
        title: 'Retention Candidate Manifest',
        summary: 'Scanned s3://tetonic-cold-telemetry/2026-Q2/. 14,200 objects ready for purge.',
        status: 'active',
        timestamp: 'Held at gate',
        actions: [
          {
            id: 'act-retention-gate',
            timestamp: 'Turn 2',
            agentId: 'agt-evaluator',
            agentName: 'Ziggy',
            actionType: 'decision',
            summary: 'Outbound batch delete held: Target bucket s3://tetonic-cold-telemetry/ (1.4 TB)',
            rawLog: `[SECURITY BARRIER] Action 's3:DeleteObjects' intercepted by PolicyEngine.
Target: s3://tetonic-cold-telemetry/2026-Q2/* (14,200 objects)
Estimated Reclaim: 1,420 GB NVMe-equivalent cold tier
Status: AWAITING_EXPLICIT_DIRECTOR_SIGN_OFF`,
          },
        ],
      },
    ],
  },

  // ---------------------------------------------------------------------------
  // Completed in Swarm
  // ---------------------------------------------------------------------------
  {
    id: 'mission-tls-cert-rotation',
    title: 'Automate mTLS Key Rotation on 14 Cluster Nodes',
    context: 'Security',
    status: 'settled',
    stageLabel: 'Completed · Verified',
    leadAgentId: 'agt-scout',
    leadAgentName: 'Turing',
    collaboratorIds: ['agt-builder'],
    startedAt: '3h ago',
    completedAt: '2h ago',
    model: 'qwen3.5:latest',
    runId: 'run-1842',
    executiveSummary:
      'Rotated internal TLS root and leaf certificates across 14 distributed engine nodes with zero dropped connections during handshake rollover.',
    milestones: [
      {
        id: 'm1-tls-rotate',
        number: '01',
        title: 'Automated Certificate Renewal',
        summary: 'Generated ed25519 node identities and synchronized certificate chains.',
        status: 'done',
        timestamp: '2h ago',
        verification: {
          suite: 'engine::tls::handshake_tests',
          passed: 28,
          failed: 0,
          benchmark: 'Handshake p99 latency 1.4ms (zero connection drops)',
        },
        artifact: {
          branch: 'sec/cert-rotation-q3',
          commitHash: 'd7481b9',
          filesCount: 6,
          summary: 'Zero-downtime certificate renewal logic deployed.',
        },
      },
    ],
  },
  {
    id: 'mission-hnsw-graph-pruning',
    title: 'Heuristic Graph Pruning in Strata Vector Index',
    context: 'Search',
    status: 'settled',
    stageLabel: 'Completed · Commit f1829e',
    leadAgentId: 'agt-evaluator',
    leadAgentName: 'Ziggy',
    collaboratorIds: ['agt-otto'],
    startedAt: '5h ago',
    completedAt: '3h ago',
    model: 'claude-3-7-sonnet',
    runId: 'run-1104',
    executiveSummary:
      'Implemented degree-bounded heuristic pruning in HNSW layer 0, reclaiming 38% memory overhead with <0.2% recall drop on 1M vectors.',
    milestones: [
      {
        id: 'm1-hnsw-prune',
        number: '01',
        title: 'Heuristic Edge Pruning Algorithm',
        summary: 'Pruned redundant long-range edges conforming to triangle inequality bounding.',
        status: 'done',
        timestamp: '3h ago',
        verification: {
          suite: 'strata_index::pruning_eval',
          passed: 18,
          failed: 0,
          benchmark: 'Memory RSS dropped from 2.1GB to 1.3GB (Recall@10: 98.4%)',
        },
        artifact: {
          branch: 'perf/hnsw-edge-pruning',
          commitHash: 'f1829e2',
          filesCount: 4,
          summary: 'Pruning pass integrated into background WAL compaction.',
        },
      },
    ],
  },
];

// Helper to filter/construct missions for a given workload preset
export function getMissionsForPreset(preset: WorkloadPreset): MissionWithLineage[] {
  if (preset === 'live') {
    return [];
  }

  if (preset === 'focused') {
    // 3 items: 1 proposed, 1 in_progress, 1 settled (0 needs attention)
    return [
      SAMPLE_LINEAGE_MISSIONS[0], // mission-vault-auth (Proposed)
      SAMPLE_LINEAGE_MISSIONS[1], // mission-vector-perf (In Progress)
      SAMPLE_LINEAGE_MISSIONS[2], // mission-checkout-resilience (Settled)
    ];
  }

  if (preset === 'balanced') {
    // 6 items: The 6 canonical archetypes
    return [...SAMPLE_LINEAGE_MISSIONS];
  }

  // 'swarm': 16 items
  return [...SAMPLE_LINEAGE_MISSIONS, ...SWARM_EXTRA_MISSIONS];
}

// ---------------------------------------------------------------------------
// Realistic Live Agent Simulation Events
// ---------------------------------------------------------------------------
export interface SimulatedAgentEvent {
  missionId: string;
  agentId: string;
  agentName: string;
  model: string;
  toolName: string;
  actionSummary: string;
  logAppend: string;
  tokenDelta: number;
  newDiff?: {
    target: string;
    diff: string;
  };
}

export const LIVE_SIMULATION_EVENTS: SimulatedAgentEvent[] = [
  {
    missionId: 'mission-vector-perf',
    agentId: 'agt-evaluator',
    agentName: 'Ziggy',
    model: 'claude-3-7-sonnet',
    toolName: 'cargo bench --bench hnsw_search',
    actionSummary: 'Evaluating concurrent query latency under 100 worker load',
    logAppend: `[+3.1s] Benchmarking hnsw_search/10000: Iteration 4/10: p50=8.1ms, p99=14.4ms... Analyzing L1D misses.`,
    tokenDelta: 48,
  },
  {
    missionId: 'mission-vector-perf',
    agentId: 'agt-evaluator',
    agentName: 'Ziggy',
    model: 'claude-3-7-sonnet',
    toolName: 'perf stat -e cache-misses',
    actionSummary: 'Profiling CPU cache misses in HnswNode::distance inner loop',
    logAppend: `[+6.2s] perf: 14.8% L1D miss rate. Inlining _mm256_prefetch on node neighbor buffer.`,
    tokenDelta: 36,
  },
  {
    missionId: 'mission-vector-perf',
    agentId: 'agt-evaluator',
    agentName: 'Ziggy',
    model: 'claude-3-7-sonnet',
    toolName: 'git diff -U2 engine/strata/src/hnsw.rs',
    actionSummary: 'Applied AVX2 prefetch intrinsic and aligned struct layout to 64 bytes',
    logAppend: `[+9.4s] Applied unified diff to engine/strata/src/hnsw.rs (+14 lines / -3 lines). Zero borrow conflicts.`,
    tokenDelta: 62,
    newDiff: {
      target: 'engine/strata/src/hnsw.rs',
      diff: `@@ -242,5 +242,10 @@ impl HnswIndex {
+    // Prefetch candidate node neighbor list into L1 cache
+    unsafe {
+        core::arch::x86_64::_mm_prefetch(
+            candidate.neighbors.as_ptr() as *const i8,
+            core::arch::x86_64::_MM_HINT_T0,
+        );
+    }
     let dist = distance_simd(&query, &candidate.vector);`,
    },
  },
  {
    missionId: 'mission-simd-dispatch',
    agentId: 'agt-builder',
    agentName: 'Barnaby',
    model: 'gpt-4o',
    toolName: 'cargo check --package strata-simd',
    actionSummary: 'Verifying fallback path when AVX-512 target feature flag is absent',
    logAppend: `[+12.8s] Checking strata-simd v0.2.1... Finished dev [unoptimized] target(s) in 0.42s`,
    tokenDelta: 54,
  },
  {
    missionId: 'mission-zero-copy-rpc',
    agentId: 'agt-architect',
    agentName: 'Ada',
    model: 'claude-3-7-sonnet',
    toolName: 'cargo test --bench ipc_ring',
    actionSummary: 'Simulating 1,000,000 zero-copy message transfers across shared ring',
    logAppend: `[+15.2s] Test ring_buffer::spsc_throughput: 1,000,000 messages transferred in 84.1ms (11.8M msg/sec).`,
    tokenDelta: 42,
  },
  {
    missionId: 'mission-mem-continuous',
    agentId: 'agt-otto',
    agentName: 'Otto',
    model: 'qwen3.5:latest',
    toolName: 'cargo bench --bench alloc_profile',
    actionSummary: 'Sampling active RSS memory watermark at 150k iteration mark',
    logAppend: `[+18.5s] Memory RSS: 414.2 MB (delta: +0.00MB). Jemalloc dirty pages: 4. No leak detected.`,
    tokenDelta: 28,
  },
];
