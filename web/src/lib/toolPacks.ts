import type { AgentCatalog, ToolPack, ToolPackSelection } from '../engine/contracts';

export function packRequest(
  packs: ToolPack[],
  selected: string[],
): { tools: string[]; toolPackSelection?: ToolPackSelection } {
  if (!packs.length) return { tools: selected };
  const members = new Set(packs.flatMap((p) => p.tools));
  return {
    tools: selected.filter((t) => !members.has(t)),
    toolPackSelection: {
      packs: packs.map(({ id, revision }) => ({ id, revision })),
      excluded_tools: [...members].filter((t) => !selected.includes(t)),
    },
  };
}

/** Keep individual additions, exclusions and overlapping packs during explicit updates. */
export function changePack(packs: ToolPack[], tools: string[], id: string, replacement?: ToolPack) {
  const before = new Set(packs.flatMap((p) => p.tools));
  const excluded = new Set([...before].filter((t) => !tools.includes(t)));
  const next = [...packs.filter((p) => p.id !== id), ...(replacement ? [replacement] : [])];
  return {
    packs: next,
    tools: [
      ...new Set([
        ...tools.filter((t) => !before.has(t)),
        ...next.flatMap((p) => p.tools).filter((t) => !excluded.has(t)),
      ]),
    ],
  };
}

export function packToolLabel(id: string, catalog: AgentCatalog): string {
  for (const connection of catalog.mcp_connections || []) {
    const tool = connection.tools.find((t) => t.id === id);
    if (tool) return `${connection.name} · ${tool.name}`;
  }
  const skill = catalog.skills?.find((s) => s.id === id);
  if (skill) return `${skill.name} · Skill`;
  return (
    (
      {
        read_file: 'Read a file',
        list_dir: 'List a folder',
        grep: 'Search file contents',
        glob: 'Find files',
        write_file: 'Write a file',
        edit_file: 'Edit a file',
        run_shell: 'Run a command',
        blackboard: 'Team blackboard',
        recall: 'Recall context',
      } as Record<string, string>
    )[id] || id
  );
}
