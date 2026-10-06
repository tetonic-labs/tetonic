import type { AgentCatalog, EngineAgent } from './localEngine';

// Product groupings describe a selection; they never expand a saved grant.
export const agentToolGroups: Record<string, readonly string[]> = {
  read_file: ['read_file', 'list_dir', 'grep', 'glob'],
  write_file: ['write_file', 'edit_file'],
};
const toolDescriptions: Record<string, string> = {
  read_file: 'Read file contents',
  list_dir: 'Browse folders',
  grep: 'Search file contents',
  glob: 'Find files by name',
  write_file: 'Create or replace files',
  edit_file: 'Edit existing files',
  outline: 'Inspect code structure',
  search_code: 'Search code',
};
const internal = new Set(['finish', 'dispatch_assignment', 'ask_human']);

// Match the engine's Ollama alias handling, including registries with a port.
function localModelTag(name: string) {
  return name.split('/').at(-1)?.includes(':') ? name : `${name}:latest`;
}

export function toolDescription(names: readonly string[]) {
  return names
    .filter((name) => !internal.has(name))
    .map((name) => toolDescriptions[name] || name)
    .join(' · ');
}

export function supportedAgentTools(catalog: AgentCatalog, provider: string, harness: string) {
  const profile = catalog.runtime_profiles?.find(
    (p) => p.provider === provider && p.harness === harness,
  );
  return (
    profile?.tools ??
    (provider === 'ollama' && !catalog.runtime_profiles ? catalog.tools || [] : [])
  ).filter((tool) => catalog.tools?.includes(tool));
}

export type AgentSetup = { state: 'configured' | 'needs_setup' | 'unknown'; message: string };

// A view of the last catalog, not authority or proof of live provider access.
// Execution still rechecks credentials, policy, model and folder in the engine.
export function agentSetup(
  agent: EngineAgent,
  catalog: AgentCatalog | null,
  fresh: boolean,
): AgentSetup {
  const needs = (message: string): AgentSetup => ({ state: 'needs_setup', message });
  if (!fresh || !catalog)
    return { state: 'unknown', message: 'Setup information is unavailable or out of date.' };
  if (agent.plan_coordinator)
    return { state: 'configured', message: 'Works from the agreed plan.' };
  const provider = agent.provider || 'ollama';
  const profile = catalog.runtime_profiles?.find(
    (p) => p.provider === provider && p.harness === agent.harness,
  );
  if (!catalog.harnesses.includes(agent.harness) || (catalog.runtime_profiles && !profile))
    return needs('This engine does not support this provider and runtime combination.');
  if (provider !== 'ollama') {
    if (!catalog.providers)
      return { state: 'unknown', message: 'Provider connection status has not been confirmed.' };
    if (!catalog.providers?.find((p) => p.id === provider)?.key_saved)
      return needs('A provider key is needed before assigning work.');
    if (!agent.hosted_consent)
      return needs('Create a replacement agent with permission to send prompts to this provider.');
  }
  const selected = (agent.tools || []).filter((tool) => !internal.has(tool));
  if (selected.length && !catalog.tools)
    return { state: 'unknown', message: 'Tool availability has not been confirmed.' };
  const supported = supportedAgentTools(catalog, provider, agent.harness);
  if (selected.some((tool) => !supported.includes(tool)))
    return needs(
      'Some selected tools are no longer available. Restore host access or create an agent with the available tools.',
    );
  if (
    provider !== 'ollama' &&
    selected.length &&
    (!agent.hosted_workspace ||
      (catalog.workspace_root !== undefined && catalog.workspace_root !== agent.hosted_workspace))
  )
    return needs(
      'The approved folder has changed. Create a replacement agent to approve its current file access.',
    );
  if (provider === 'ollama') {
    if (catalog.local_error) return { state: 'unknown', message: catalog.local_error };
    if (!catalog.models.some((model) => localModelTag(model) === localModelTag(agent.model)))
      return needs(
        'This local model is unavailable. Restore it in Ollama or create an agent with an installed model.',
      );
  }
  return {
    state: 'configured',
    message:
      'Setup matches this engine. Model access and permissions are checked when work starts.',
  };
}
