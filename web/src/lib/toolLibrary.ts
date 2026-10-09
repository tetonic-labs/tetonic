import type { GraphEdge, GraphNode, Team } from '../types';

export type ResourceKind = 'mcp' | 'tool' | 'storage';
export interface WorkspaceResource {
  id: string;
  name: string;
  description: string;
  kind: ResourceKind;
  source: 'sample' | 'draft';
  catalogId?: string;
  endpoint?: string;
  teamIds: string[];
}
export interface ResourceTemplate {
  id: string;
  name: string;
  description: string;
  kind: 'mcp' | 'tool';
}
// Preview choices, not a registry of verified or installed MCP servers.
export const resourceCatalog: ResourceTemplate[] = [
  {
    id: 'github',
    name: 'GitHub',
    description: 'Bring repositories, issues, and pull requests into the work.',
    kind: 'mcp',
  },
  {
    id: 'docs',
    name: 'Documents',
    description: 'Give teams a connection to their shared documents.',
    kind: 'mcp',
  },
  {
    id: 'linear',
    name: 'Linear',
    description: 'Keep project work close to its issues and milestones.',
    kind: 'mcp',
  },
  {
    id: 'jira',
    name: 'Jira',
    description: 'Connect the work to your issue-tracking workspace.',
    kind: 'mcp',
  },
  {
    id: 'search',
    name: 'Search',
    description: 'Bring a search service into research and exploration.',
    kind: 'mcp',
  },
  {
    id: 'terminal',
    name: 'Terminal',
    description: 'Run commands in a workspace controlled by the runtime.',
    kind: 'tool',
  },
  {
    id: 'files',
    name: 'Files',
    description: 'Read and edit files within a configured workspace.',
    kind: 'tool',
  },
  {
    id: 'build',
    name: 'Build runner',
    description: 'Run builds and checks as part of a workflow.',
    kind: 'tool',
  },
];
export const resourceKindLabel = (kind: ResourceKind) =>
  kind === 'mcp' ? 'MCP connection' : kind === 'storage' ? 'Storage' : 'Runtime tool';
export function resourcesFromGraph(
  nodes: GraphNode[],
  edges: GraphEdge[],
  teams: Team[],
): WorkspaceResource[] {
  return nodes
    .filter((n) => ['tool', 'connector', 'storage'].includes(n.type))
    .map((node) => {
      const text = `${node.name} ${node.label} ${node.metadata?.connectorType || ''}`.toLowerCase();
      const catalogId = text.includes('github')
        ? 'github'
        : text.includes('jira')
          ? 'jira'
          : text.includes('linear')
            ? 'linear'
            : /docs|google|document/.test(text)
              ? 'docs'
              : text.includes('search')
                ? 'search'
                : /cargo|build/.test(text)
                  ? 'build'
                  : /filesystem|fs:/.test(text)
                    ? 'files'
                    : /terminal|shell:/.test(text)
                      ? 'terminal'
                      : undefined;
      const users = new Set(
        edges
          .filter((e) => e.source === node.id || e.target === node.id)
          .map((e) => (e.source === node.id ? e.target : e.source)),
      );
      return {
        id: node.id,
        name: node.type === 'tool' && node.name.includes(':') ? node.label : node.name,
        description:
          node.metadata?.details ||
          (node.label !== 'Sample resource'
            ? node.label
            : 'Shared resource in this example workspace.'),
        kind: node.type === 'connector' ? 'mcp' : node.type === 'storage' ? 'storage' : 'tool',
        source: 'sample',
        catalogId,
        teamIds: teams
          .filter((t) => t.pledgedAgentIds.some((id) => users.has(id)))
          .map((t) => t.id),
      };
    });
}
/** Normalize only the local alias; do not guess a remote destination or MCP path. */
export function normalizeMcpEndpoint(value: string) {
  let address = value.trim();
  if (/^(localhost|127\.0\.0\.1|\[::1\])(?::\d+)?(?:\/|$)/i.test(address)) {
    address = `http://${address}`;
  }
  try {
    const url = new URL(address);
    if (url.protocol === 'http:' && url.hostname === 'localhost') url.hostname = '127.0.0.1';
    return url.toString();
  } catch {
    return address;
  }
}
export function mcpConnectionName(value: string) {
  try {
    return new URL(normalizeMcpEndpoint(value)).host.slice(0, 80);
  } catch {
    return '';
  }
}
export function endpointError(value: string) {
  try {
    const url = new URL(normalizeMcpEndpoint(value));
    if (url.username || url.password || url.search || url.hash)
      return 'Use a server URL without credentials, query parameters, or a fragment.';
    if (
      url.protocol !== 'https:' &&
      !(url.protocol === 'http:' && ['127.0.0.1', '[::1]'].includes(url.hostname))
    )
      return 'Use HTTPS, or HTTP for a local server.';
    if (url.port === '0')
      return 'Use the port number provided by the service; port 0 cannot be connected to.';
    return '';
  } catch {
    return 'Enter a complete server URL, such as https://mcp.example.com/mcp.';
  }
}
