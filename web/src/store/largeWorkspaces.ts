import { Agent, AgentTrack, GraphEdge, GraphNode, Team } from '../types';
import { mockAgents, mockTeams } from './mockData';

export const workspaceSizes = {
  studio: { name: 'Product studio', agents: 18, teams: 3, resources: 9 },
  company: { name: 'Growing company', agents: 40, teams: 6, resources: 16 },
  network: { name: 'Research network', agents: 80, teams: 10, resources: 24 },
} as const;
export function largeWorkspace(key: keyof typeof workspaceSizes) {
  const size = workspaceSizes[key];
  const names = [
    'Engineering',
    'Research',
    'Design',
    'Operations',
    'Security',
    'Data',
    'Support',
    'Quality',
    'Infrastructure',
    'Documentation',
  ];
  const teams: Team[] = Array.from({ length: size.teams }, (_, i) => ({
    ...mockTeams[0],
    id: `demo-team-${i}`,
    name: names[i],
    tagline: 'Sample workspace',
    pledgedAgentIds: [],
    members: [],
  }));
  const agents: Agent[] = Array.from({ length: size.agents }, (_, i) => {
    const team = teams[i % teams.length];
    const id = `demo-agent-${i}`;
    team.pledgedAgentIds.push(id);
    if (i % 5 === 0) teams[(i + 1) % teams.length].pledgedAgentIds.push(id);
    return {
      ...mockAgents[0],
      id,
      name: `${['Ada', 'Jun', 'Milo', 'Nora', 'Eli', 'Sage'][i % 6]} ${i + 1}`,
      charter: `${team.name} specialist`,
      pledgedTeamId: team.id,
      status: 'idle',
    };
  });
  const nodes: GraphNode[] = Array.from({ length: size.resources }, (_, i) => ({
    id: `demo-resource-${i}`,
    type: i % 3 === 0 ? 'tool' : 'connector',
    name: `${['Terminal', 'GitHub MCP', 'Linear MCP', 'Build runner', 'Docs MCP', 'Search MCP'][i % 6]} ${Math.floor(i / 6) + 1}`,
    label: 'Sample resource',
    category: 'castle',
    x: 0,
    y: 0,
    status: 'idle',
    iconType: 'terminal',
  }));
  const edges: GraphEdge[] = [];
  const tracks: Record<string, AgentTrack> = {};
  agents.forEach((agent, i) => {
    const resource = nodes[Math.floor(i / 3) % nodes.length];
    for (const target of new Set([resource.id, nodes[(i + 1) % nodes.length].id]))
      edges.push({
        id: `${agent.id}-${target}`,
        source: agent.id,
        target,
        type: 'tool_usage',
        isActive: false,
      });
    tracks[agent.id] = {
      agentId: agent.id,
      agentName: agent.name,
      currentStatus: 'Sample',
      totalTokens: 0,
      steps: [
        {
          id: `step-${i}`,
          stepNumber: 1,
          timestamp: new Date(Date.UTC(2026, 8, 28, 12, 0, i % 6)).toISOString(),
          type: resource.type === 'tool' ? 'tool_execution' : 'mcp_query',
          title: `Use ${resource.name}`,
          targetNodeId: resource.id,
          targetNodeName: resource.name,
          status: 'running',
        },
      ],
    };
  });
  return { agents, teams, nodes, edges, tracks };
}
