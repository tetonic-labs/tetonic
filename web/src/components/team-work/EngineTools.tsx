import { useLocalEngine } from '../../context/LocalEngineContext';
import { toolDescription } from '../../lib/agentCapabilities';
import { ToolsView, type ToolkitResource } from '../views/ToolsView';

const internalTools = new Set(['finish', 'dispatch_assignment', 'ask_human']);
const fileTools = new Set(['read_file', 'write_file', 'edit_file', 'list_dir', 'grep', 'glob']);

export function EngineTools({ onAgent }: { onAgent: (key: string) => void }) {
  const engine = useLocalEngine();
  const { workspace, catalog, readErrors, isConnected } = engine;
  const ready = isConnected && Array.isArray(catalog?.tools) && !readErrors['Agent setup'];
  const tools = [
    ...new Set([
      ...(catalog?.tools || []),
      ...(workspace?.agents.flatMap((a) => a.tools || []) || []),
    ]),
  ].filter((id) => !internalTools.has(id));
  const connections = catalog?.mcp_connections || [];
  const connectedIds = new Set(connections.flatMap((c) => c.tools.map((t) => t.id)));
  const users = (ids: string[]) =>
    (workspace?.agents || [])
      .filter((a) => ids.some((id) => a.tools?.includes(id)))
      .map((a) => ({ key: a.key, name: a.name }));
  const describe = (ids: string[]) =>
    ids.map((id) => ({
      id,
      name: toolDescription([id], catalog),
      available: !!ready && !!catalog?.tools?.includes(id),
    }));
  const local = tools.filter((id) => !id.startsWith('mcp_'));
  const files = local.filter((id) => fileTools.has(id));
  const resources: ToolkitResource[] = [];
  if (files.length)
    resources.push({
      id: 'files',
      name: 'Files',
      kind: 'tool',
      description: 'Read, find, and edit files in the working folder.',
      tools: describe(files),
      agents: users(files),
    });
  for (const id of local.filter((id) => !fileTools.has(id)))
    resources.push({
      id,
      name: id === 'run_shell' ? 'Terminal' : toolDescription([id], catalog),
      kind: 'tool',
      description:
        id === 'run_shell'
          ? 'Run commands and installed command-line tools. Each command needs your approval.'
          : toolDescription([id], catalog),
      tools: describe([id]),
      agents: users([id]),
    });
  for (const connection of connections) {
    const ids = connection.tools.map((t) => t.id);
    resources.push({
      id: `connection:${connection.id}`,
      name: connection.name,
      kind: 'mcp',
      description: 'Tools from a service configured on your engine.',
      connection,
      tools: describe(ids).map((t) => ({
        ...t,
        name: connection.tools.find((x) => x.id === t.id)!.name,
      })),
      agents: users(ids),
    });
  }
  const missing = tools.filter((id) => id.startsWith('mcp_') && !connectedIds.has(id));
  if (missing.length)
    resources.push({
      id: 'unavailable-mcp',
      name: 'Previously selected MCP tools',
      kind: 'mcp',
      description:
        'These agent grants are saved, but their connection is no longer in the current catalog.',
      tools: describe(missing).map((t) => ({ ...t, available: false })),
      agents: users(missing),
    });
  return (
    <ToolsView
      resources={resources}
      ready={ready}
      workspaceRoot={catalog?.workspace_root}
      mcpSupported={Array.isArray(catalog?.mcp_connections)}
      onAgent={onAgent}
      onDiscover={async (id) => {
        await engine.client.discoverMcp(id);
        await engine.refresh();
      }}
    />
  );
}
