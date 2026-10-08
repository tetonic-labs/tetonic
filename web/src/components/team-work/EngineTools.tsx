import { McpConnectionEditor } from '../views/McpConnectionEditor';
import { WorkspaceCapabilityLibrary } from '../views/WorkspaceCapabilityLibrary';
import { SkillDetails } from '../views/SkillDetails';
import { useLocalEngine } from '../../context/LocalEngineContext';
import { toolDescription } from '../../lib/agentCapabilities';
import { ToolsView, type ToolkitResource } from '../views/ToolsView';

const internalTools = new Set(['finish', 'dispatch_assignment', 'ask_human']);
const fileTools = new Set(['read_file', 'write_file', 'edit_file', 'list_dir', 'grep', 'glob']);
const mcpConnectionId = (tool: string) => tool.slice(4, tool.lastIndexOf('_'));

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
  const local = tools.filter((id) => !id.startsWith('mcp_') && !id.startsWith('skill_'));
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
    const ids = [
      ...new Set([
        ...connection.tools.map((t) => t.id),
        ...tools.filter((id) => id.startsWith('mcp_') && mcpConnectionId(id) === connection.id),
      ]),
    ];
    resources.push({
      id: `connection:${connection.id}`,
      name: connection.name,
      kind: 'mcp',
      description: 'Tools from a connected service.',
      detail: connection.editable ? (
        <McpConnectionEditor
          key={connection.id}
          connection={connection}
          client={engine.client}
          onChanged={engine.refresh}
        />
      ) : undefined,
      connection,
      tools: describe(ids).map((t) => ({
        ...t,
        name: connection.tools.find((x) => x.id === t.id)?.name || 'Previously selected tool',
      })),
      agents: users(ids),
    });
  }
  for (const skill of catalog?.skills || [])
    resources.push({
      id: skill.id,
      name: skill.name,
      kind: 'skill',
      description: skill.description,
      tools: describe([skill.id]),
      agents: users([skill.id]),
      revoked: !skill.enabled,
      detail: (
        <SkillDetails
          key={skill.id}
          skill={skill}
          client={engine.client}
          onChanged={engine.refresh}
        />
      ),
    });
  const unknownSkills = tools.filter(
    (id) => id.startsWith('skill_') && !catalog?.skills?.some((s) => s.id === id),
  );
  for (const id of unknownSkills)
    resources.push({
      id,
      name: `Unavailable skill ${id.slice(6, 14)}`,
      kind: 'skill',
      description: 'This saved skill is absent from the current workspace library.',
      tools: describe([id]),
      agents: users([id]),
    });
  const missing = tools.filter(
    (id) =>
      id.startsWith('mcp_') &&
      !connectedIds.has(id) &&
      !connections.some((connection) => mcpConnectionId(id) === connection.id),
  );
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
      library={
        <WorkspaceCapabilityLibrary
          mcpSupported={!!catalog?.mcp_management && isConnected}
          client={engine.client}
          supported={ready && Array.isArray(catalog?.skills)}
          onChanged={engine.refresh}
        />
      }
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
