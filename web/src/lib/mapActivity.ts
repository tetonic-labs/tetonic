import { Agent, AgentTrack, GraphEdge, GraphNode, TrackStep } from '../types';
import { teammateName } from './teammates';
export interface Point {
  x: number;
  y: number;
}
export interface Destination {
  id: string;
  name: string;
  kind: GraphNode['type'];
  node: GraphNode;
  point: Point;
}
export interface MapAction {
  agentId: string;
  step: TrackStep;
  targetId: string;
  targetName: string;
  tool?: string;
}
export const WORLD = { width: 1600, height: 1050 };
const places: Point[] = [
  { x: 320, y: 265 },
  { x: 1250, y: 285 },
  { x: 1210, y: 775 },
  { x: 330, y: 795 },
  { x: 815, y: 155 },
  { x: 815, y: 920 },
];
const homes: Point[] = [
  { x: 650, y: 415 },
  { x: 955, y: 580 },
  { x: 690, y: 750 },
  { x: 495, y: 605 },
];
export function homeFor(index: number): Point {
  if (index < homes.length) return homes[index];
  const angle = (index - 4) * 2.4;
  return { x: 800 + Math.cos(angle) * 230, y: 530 + Math.sin(angle) * 240 };
}
export function destinationsFor(
  agents: Agent[],
  nodes: GraphNode[],
  edges: GraphEdge[],
): Destination[] {
  const ids = new Set(agents.map((a) => a.id));
  const targets = new Set(
    edges
      .filter((e) => ids.has(e.source) || ids.has(e.target))
      .flatMap((e) => [e.source, e.target]),
  );
  const ordered = nodes.filter(
    (n) => targets.has(n.id) && ['connector', 'storage', 'tool'].includes(n.type),
  );
  ordered.sort((a, b) => Number(a.type === 'tool') - Number(b.type === 'tool'));
  return ordered.map((node, i) => ({
    id: node.id,
    node,
    kind: node.type,
    name: node.id.startsWith('demo-')
      ? node.name
      : node.metadata?.connectorType === 'github'
        ? 'GitHub'
        : node.metadata?.connectorType === 'jira'
          ? 'Jira'
          : node.metadata?.connectorType === 'google'
            ? 'Documents'
            : node.type === 'tool'
              ? 'Local workspace'
              : 'Memory',
    point: places[i] || { x: 200 + (i % 5) * 290, y: 110 + Math.floor(i / 5) * 220 },
  }));
}
// Only supplied trace actions drive playback, never topology activity flags.
export function actionsFor(
  agents: Agent[],
  destinations: Destination[],
  tracks: Record<string, AgentTrack>,
): MapAction[] {
  const ids = new Set(agents.map((a) => a.id));
  return agents
    .flatMap((agent) =>
      (tracks[agent.id]?.steps || []).flatMap((step) => {
        const destination = destinations.find((d) => d.id === step.targetNodeId);
        if (!step.targetNodeId || (!destination && !ids.has(step.targetNodeId))) return [];
        return [
          {
            agentId: agent.id,
            step,
            targetId: step.targetNodeId,
            targetName:
              destination?.name ||
              teammateName(step.targetNodeId, step.targetNodeName || 'Teammate'),
            tool:
              step.type === 'tool_execution'
                ? step.targetNodeName
                : step.type === 'mcp_query'
                  ? 'MCP'
                  : undefined,
          },
        ];
      }),
    )
    .sort((a, b) => a.step.timestamp.localeCompare(b.step.timestamp));
}
