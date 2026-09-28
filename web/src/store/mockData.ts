import { Agent, ApprovalRequest, CastleNode, Team, StreamEvent } from '../types';

export const mockCastleNode: CastleNode = {
  hostname: 'alices-laptop.local',
  castleName: "Alice's Sovereign Castle",
  ip: '127.0.0.1',
  mode: 'sovereign_castle',
  inferenceProvider: {
    name: 'Ollama (Loopback)',
    endpoint: 'http://127.0.0.1:11434',
    model: 'llama3.2:latest',
    status: 'online',
    latencyMs: 38,
    contextTokensLimit: 131072,
  },
  resources: {
    cpuPercent: 18,
    memoryMbUsed: 4210,
    memoryMbTotal: 16384,
    activeProcessesCount: 4,
  },
  policy: {
    requireApprovalForShell: true,
    requireApprovalForFileWrites: true,
    allowRemoteCastleExec: false, // Core Castle Principle: RBAC never gives Susan direct execution access to Alice's castle!
    sandboxingLevel: 'strict',
  },
};

export const mockApprovals: ApprovalRequest[] = [
  {
    id: 'appr-01',
    agentId: 'agt-builder',
    agentName: 'Tectonic-Builder',
    type: 'bash_command',
    title: 'Execute Cargo Test Suite',
    reason:
      'Verify that extracting unit tests into agent_tests.rs preserved all 46 core assertions without regression.',
    payload: 'cargo test -p tetonic-core -- --nocapture',
    status: 'pending',
    requestedAt: new Date(Date.now() - 1000 * 60 * 3).toISOString(),
    expiresInSecs: 240,
    castleOrigin: "Alice's Laptop (Local)",
  },
  {
    id: 'appr-02',
    agentId: 'agt-builder',
    agentName: 'Tectonic-Builder',
    type: 'file_write',
    title: 'Apply Architectural Pruning to agent.rs',
    reason:
      'Remove inline monolithic test modules from agent.rs to trim crate lines from 3,978 down to 2,340 lines (-41%).',
    payload: 'core/tetonic-core/src/agent.rs',
    diff: {
      filepath: 'core/tetonic-core/src/agent.rs',
      summary: '+1 line, -1638 lines',
      oldLines: [
        { num: 2340, text: '// Legacy embedded test harness' },
        { num: 2341, text: '#[cfg(test)]' },
        { num: 2342, text: 'mod tests {' },
        { num: 2343, text: '    use super::*;' },
        { num: 2344, text: '    // [1,634 lines of unit tests]' },
        { num: 2345, text: '}' },
      ],
      newLines: [
        { num: 2340, text: '// Tests separated into dedicated compilation unit' },
        { num: 2341, text: '#[cfg(test)]' },
        { num: 2342, text: '#[path = "agent_tests.rs"]' },
        { num: 2343, text: 'mod agent_tests;' },
      ],
    },
    status: 'pending',
    requestedAt: new Date(Date.now() - 1000 * 60 * 7).toISOString(),
    expiresInSecs: 480,
    castleOrigin: "Alice's Laptop (Local)",
  },
  {
    id: 'appr-03',
    agentId: 'agt-remote-guard',
    agentName: 'LintSentinel',
    type: 'cross_castle_request',
    title: 'Remote Execution Access Request (Susan)',
    reason:
      'Susan (Team Lead) requested to execute a GPU-accelerated AST audit in your castle sandbox.',
    payload: 'remote-exec://castle.susan -> castle.alice [Run: AST-Gpu-Audit]',
    status: 'pending',
    requestedAt: new Date(Date.now() - 1000 * 60 * 12).toISOString(),
    expiresInSecs: 900,
    castleOrigin: "Susan's Workstation (Remote)",
  },
];

export const mockAgents: Agent[] = [
  {
    id: 'agt-builder',
    name: 'Tectonic-Builder',
    charter:
      'Compile, test, and verify Rust crates across the Tetonic engine, enforcing architectural gates and zero-warning builds.',
    model: 'llama3.2:latest',
    status: 'waiting_approval',
    decisionIntervalMs: 2500,
    capabilities: ['fs:read', 'fs:write', 'shell:cargo', 'git:inspect'],
    tokensProcessed: 384102,
    memoryItemsCount: 42,
    isLocalToCastle: true,
    pledgedTeamId: 'team-platform',
    lastActive: 'Just now',
  },
  {
    id: 'agt-sentinel',
    name: 'Castle-Sentinel',
    charter:
      "Private castle daemon guarding Alice's local workstation. Intercepts untrusted remote calls and monitors host memory limits.",
    model: 'qwen2.5-coder:7b',
    status: 'idle',
    decisionIntervalMs: 5000,
    capabilities: ['audit:events', 'castle:guard', 'network:loopback-only'],
    tokensProcessed: 142050,
    memoryItemsCount: 88,
    isLocalToCastle: true,
    pledgedTeamId: undefined, // Private fleet
    lastActive: '5m ago',
  },
  {
    id: 'agt-doc',
    name: 'Doc-Synthesizer',
    charter:
      'Continuously synchronize technical specs, Hugo documentation sites, and ADRs with engine code changes.',
    model: 'llama3.2:latest',
    status: 'thinking',
    decisionIntervalMs: 4000,
    capabilities: ['fs:read', 'hugo:build', 'spec:reconcile'],
    tokensProcessed: 89400,
    memoryItemsCount: 23,
    isLocalToCastle: true,
    pledgedTeamId: 'team-platform',
    lastActive: '1m ago',
  },
  {
    id: 'agt-remote-guard',
    name: 'LintSentinel',
    charter: 'Distributed static analysis bot managed by Susan for repository hygiene.',
    model: 'deepseek-coder:6.7b',
    status: 'waiting_approval',
    decisionIntervalMs: 6000,
    capabilities: ['lint:cargo-clippy', 'ast:inspect'],
    tokensProcessed: 61000,
    memoryItemsCount: 15,
    isLocalToCastle: false,
    pledgedTeamId: 'team-platform',
    lastActive: '12m ago',
  },
];

export const mockTeams: Team[] = [
  {
    id: 'personal',
    name: 'Personal Fleet',
    tagline:
      'Your private castle. Agents here never share compute or data outside your local machine.',
    isPersonal: true,
    members: [
      {
        id: 'usr-alice',
        name: 'Alice Miller',
        handle: '@alice',
        avatarUrl: '',
        role: 'owner',
        isCurrentUser: true,
      },
    ],
    pledgedAgentIds: ['agt-sentinel'],
    createdAt: '2026-09-01T00:00:00Z',
  },
  {
    id: 'team-platform',
    name: 'Platform Core Guild',
    tagline: 'Engine architecture, micro-benchmarks, and distributed execution runtime.',
    isPersonal: false,
    members: [
      {
        id: 'usr-alice',
        name: 'Alice Miller',
        handle: '@alice',
        avatarUrl: '',
        role: 'owner',
        isCurrentUser: true,
      },
      {
        id: 'usr-susan',
        name: 'Susan Vance',
        handle: '@susan_lead',
        avatarUrl: '',
        role: 'lead',
        isCurrentUser: false,
      },
      {
        id: 'usr-marcus',
        name: 'Marcus Chen',
        handle: '@mchen',
        avatarUrl: '',
        role: 'member',
        isCurrentUser: false,
      },
    ],
    pledgedAgentIds: ['agt-builder', 'agt-doc', 'agt-remote-guard'],
    createdAt: '2026-09-15T00:00:00Z',
  },
];

export const mockStreamEvents: StreamEvent[] = [
  {
    id: 'ev-1',
    timestamp: '19:42:01',
    agentId: 'agt-builder',
    agentName: 'Tectonic-Builder',
    type: 'thought',
    content:
      'Observing workspace changes. The monolithic core/tetonic-core/src/agent.rs was pruned by 1,638 lines. I need to run cargo test to verify that the extracted agent_tests.rs suite executes all 46 test cases cleanly.',
    metadata: { tokenCount: 48 },
  },
  {
    id: 'ev-2',
    timestamp: '19:42:04',
    agentId: 'agt-builder',
    agentName: 'Tectonic-Builder',
    type: 'approval_badge',
    content: 'Requested human approval for: cargo test -p tetonic-core -- --nocapture',
  },
  {
    id: 'ev-3',
    timestamp: '19:42:15',
    agentId: 'agt-builder',
    agentName: 'Tectonic-Builder',
    type: 'tool_call',
    content: 'Running command in workspace sandbox...',
    metadata: {
      toolName: 'shell:cargo',
    },
  },
  {
    id: 'ev-4',
    timestamp: '19:42:22',
    agentId: 'agt-builder',
    agentName: 'Tectonic-Builder',
    type: 'tool_result',
    content: `running 46 tests
test agent_tests::test_agent_instantiation ... ok
test agent_tests::test_agent_budget_discipline ... ok
test agent_tests::test_scope_cancellation_gate ... ok
test agent_tests::test_turn_attestation_verification ... ok
test agent_tests::test_tool_execution_sandboxing ... ok
[... 41 more tests ...]
test result: ok. 46 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`,
    metadata: {
      toolName: 'shell:cargo',
      durationMs: 7120,
      exitCode: 0,
    },
  },
  {
    id: 'ev-5',
    timestamp: '19:42:25',
    agentId: 'agt-builder',
    agentName: 'Tectonic-Builder',
    type: 'message',
    content: `All **46 unit tests** passed with zero regressions.

The architectural separation is successful:
- \`agent.rs\` is down from **3,978 lines to 2,340 lines** (-41%).
- Test fixtures now live cleanly in \`core/tetonic-core/src/agent_tests.rs\`.
- Next step: Ready to build and serve the Axum REST endpoints in \`tetonic-server\`.`,
  },
];
