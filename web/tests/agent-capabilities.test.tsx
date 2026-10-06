import { describe, expect, it } from 'vitest';
import { agentSetup } from '../src/lib/agentCapabilities';
import type { AgentCatalog, EngineAgent } from '../src/lib/localEngine';

const agent: EngineAgent = {
  key: 'reader',
  id: 'reader',
  name: 'Reader',
  purpose: 'Read notes',
  model: 'account-text-model',
  provider: 'openai',
  harness: 'general',
  hosted_consent: true,
  hosted_workspace: 'C:/work',
  tools: ['read_file'],
  max_steps: 8,
  max_seconds: 120,
  max_tokens: 4096,
};
const catalog: AgentCatalog = {
  models: ['registry:11434/library/local:latest'],
  harnesses: ['general'],
  tools: ['read_file'],
  workspace_root: 'C:/work',
  providers: [{ id: 'openai', name: 'OpenAI', key_saved: true }],
  runtime_profiles: [
    { provider: 'openai', harness: 'general', tools: ['read_file'], tool_restriction: null },
    { provider: 'ollama', harness: 'general', tools: ['read_file'], tool_restriction: null },
  ],
  max_steps: 8,
  max_seconds: 120,
  max_tokens: 4096,
};

describe('agent setup projection', () => {
  it.each<[string, Partial<AgentCatalog>, string]>([
    [
      'missing key',
      { providers: [{ id: 'openai', name: 'OpenAI', key_saved: false }] },
      'provider key',
    ],
    ['removed tool', { tools: [] }, 'tools are no longer available'],
    ['changed folder', { workspace_root: 'C:/elsewhere' }, 'approved folder has changed'],
    ['removed runtime', { runtime_profiles: [] }, 'runtime combination'],
  ])('shows a known setup problem: %s', (_name, changes, message) => {
    expect(agentSetup(agent, { ...catalog, ...changes }, true)).toEqual({
      state: 'needs_setup',
      message: expect.stringContaining(message),
    });
  });

  it('does not represent missing or stale evidence as ready or as a confirmed failure', () => {
    expect(agentSetup(agent, catalog, false).state).toBe('unknown');
    expect(agentSetup(agent, null, true).state).toBe('unknown');
    expect(agentSetup(agent, { ...catalog, providers: undefined }, true).state).toBe('unknown');
    expect(agentSetup(agent, { ...catalog, tools: undefined }, true).state).toBe('unknown');
    expect(
      agentSetup(
        { ...agent, provider: 'ollama' },
        { ...catalog, local_error: 'Ollama offline' },
        true,
      ).state,
    ).toBe('unknown');
  });

  it('accepts only the same installed local tag, including the implicit latest alias', () => {
    const local = { ...agent, provider: 'ollama', model: 'registry:11434/library/local' };
    expect(agentSetup(local, catalog, true).state).toBe('configured');
    expect(agentSetup({ ...local, model: `${local.model}:other` }, catalog, true).state).toBe(
      'needs_setup',
    );
  });

  it('keeps live provider access unverified even when saved setup is compatible', () => {
    expect(agentSetup(agent, catalog, true)).toEqual({
      state: 'configured',
      message: expect.stringContaining('checked when work starts'),
    });
  });
});
