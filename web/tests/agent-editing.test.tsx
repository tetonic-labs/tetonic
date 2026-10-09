import { afterEach, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { LocalAgentSetup } from '../src/components/work/LocalAgentSetup';
import { LocalEngine } from '../src/engine/client';
import { type EngineAgent } from '../src/engine/contracts';

const existing: EngineAgent = {
  key: 'researcher',
  id: 'stable-identity',
  definition_digest: 'revision-1',
  editable: true,
  name: 'Robin',
  purpose: 'Help with research',
  model: 'local-model',
  provider: 'ollama',
  harness: 'general',
  max_steps: 3,
  max_seconds: 120,
  max_tokens: 4000,
  tools: ['read_file'],
};
const mcp = 'mcp_calendar_pinned_search';
function setup(agent = existing) {
  const client = new LocalEngine('fixture');
  const tools = ['read_file', 'list_dir', 'run_shell', mcp];
  vi.spyOn(client, 'agentCatalog').mockResolvedValue({
    models: ['local-model'],
    harnesses: ['general'],
    tools,
    workspace_root: 'C:/work',
    max_steps: 8,
    max_seconds: 120,
    max_tokens: 4096,
    providers: [{ id: 'openai', name: 'OpenAI', key_saved: true }],
    runtime_profiles: ['ollama', 'openai'].map((provider) => ({
      provider,
      harness: 'general',
      tools,
      tool_restriction: null,
      requires_tool_consent: provider === 'openai',
    })),
    mcp_connections: [
      {
        id: 'calendar',
        name: 'Calendar',
        endpoint: 'http://localhost/mcp',
        status: 'discovered',
        message: 'Ready',
        tools: [
          {
            id: mcp,
            name: 'search',
            description: 'Find availability',
            input_schema: { type: 'object' },
          },
        ],
      },
    ],
  });
  vi.spyOn(client, 'providerModels').mockResolvedValue({
    provider: 'openai',
    models: ['account-model'],
    capabilities_verified: false,
  });
  const create = vi.spyOn(client, 'createAgent');
  const update = vi.spyOn(client, 'updateAgent').mockImplementation(async (old, input) => ({
    ...old,
    ...input,
    definition_digest: 'revision-2',
  }));
  const saved = vi.fn();
  render(
    <LocalAgentSetup
      client={client}
      agent={agent}
      workspace={{
        organization: 'Workspace',
        team_id: 'team',
        team_name: 'Team',
        agent_id: agent.id,
        agent_name: agent.name,
        model: agent.model,
        input_limit: 12000,
        agents: [agent],
        shaping_agent_key: 'the-guide',
        tasks: [],
      }}
      onCreated={saved}
      onBack={vi.fn()}
    />,
  );
  return { create, update, saved };
}
afterEach(() => vi.restoreAllMocks());

it('adds shell and MCP tools without expanding an existing partial file grant, and retries the same save', async () => {
  const f = setup();
  f.update.mockRejectedValueOnce(new Error('Connection interrupted. Try again.'));
  expect(await screen.findByLabelText('Name', { exact: true })).toHaveProperty('value', 'Robin');
  expect(screen.getByLabelText('What will they help with?')).toHaveProperty(
    'value',
    existing.purpose,
  );
  fireEvent.click(screen.getByRole('checkbox', { name: 'Terminal', exact: true }));
  fireEvent.click(screen.getByRole('checkbox', { name: 'Calendar: search' }));
  fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
  await screen.findByText('Connection interrupted. Try again.');
  expect(f.saved).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
  await waitFor(() => expect(f.saved).toHaveBeenCalledOnce());
  const [old, payload] = f.update.mock.calls[0];
  expect(old).toEqual(existing);
  expect(payload.tools).toEqual(['read_file', 'run_shell', mcp]);
  expect(payload.max_steps).toBe(3);
  expect(f.update.mock.calls[1]).toEqual(f.update.mock.calls[0]);
  expect(f.create).not.toHaveBeenCalled();
  expect(f.saved.mock.calls[0][0]).toMatchObject({
    key: existing.key,
    id: existing.id,
    tools: payload.tools,
  });
});

it('retains consent for an unchanged hosted scope and requests it again when adding a tool', async () => {
  const hosted: EngineAgent = {
    ...existing,
    model: 'account-model',
    provider: 'openai',
    hosted_consent: true,
    tool_disclosure: {
      version: 1,
      provider: 'openai',
      endpoint: 'https://api.openai.com/v1/responses',
      tools: ['read_file'],
      workspace: 'C:/work',
    },
  };
  setup(hosted);
  await screen.findByLabelText('Name', { exact: true });
  const consent = screen.getByRole('checkbox', { name: /Allow selected tool inputs and results/ });
  expect(consent).toHaveProperty('checked', true);
  expect(screen.getByRole('button', { name: 'Save changes' })).toHaveProperty('disabled', false);
  fireEvent.click(screen.getByRole('checkbox', { name: 'Calendar: search' }));
  expect(consent).toHaveProperty('checked', false);
  expect(screen.getByRole('button', { name: 'Save changes' })).toHaveProperty('disabled', true);
  fireEvent.click(consent);
  expect(screen.getByRole('button', { name: 'Save changes' })).toHaveProperty('disabled', false);
});

it('lets an agent remove a selected MCP tool that is no longer offered', async () => {
  const f = setup({ ...existing, tools: ['mcp_retired_search'] });
  await screen.findByLabelText('Name', { exact: true });
  expect(screen.getByRole('button', { name: 'Save changes' })).toHaveProperty('disabled', true);
  fireEvent.click(screen.getByRole('checkbox', { name: 'Unavailable MCP tool' }));
  fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
  await waitFor(() => expect(f.update).toHaveBeenCalledOnce());
  expect(f.update.mock.calls[0][1].tools).toEqual([]);
});

it('edits the Guide model with planning disclosure and no execution tool or identity controls', async () => {
  const guide = { ...existing, key: 'the-guide', name: 'The Guide', tools: [] };
  const f = setup(guide);
  await screen.findByRole('combobox', { name: 'Model provider' });
  expect(screen.queryByLabelText('Name', { exact: true })).toBeNull();
  expect(screen.queryByLabelText('What will they help with?')).toBeNull();
  expect(screen.queryByRole('checkbox', { name: 'Terminal', exact: true })).toBeNull();
  expect(screen.queryByRole('checkbox', { name: 'Calendar: search' })).toBeNull();
  fireEvent.change(screen.getByRole('combobox', { name: 'Model provider' }), {
    target: { value: 'openai' },
  });
  await screen.findByRole('option', { name: 'account-model' });
  fireEvent.change(screen.getByLabelText('Model', { exact: true }), {
    target: { value: 'account-model' },
  });
  const save = screen.getByRole('button', { name: 'Save changes' });
  expect(save).toHaveProperty('disabled', true);
  fireEvent.click(
    screen.getByRole('checkbox', { name: /workspace activity summaries.*inspected work results/ }),
  );
  expect(save).toHaveProperty('disabled', false);
  fireEvent.click(save);
  await waitFor(() => expect(f.saved).toHaveBeenCalledOnce());
  expect(f.update.mock.calls[0][0]).toEqual(guide);
  expect(f.update.mock.calls[0][1]).toMatchObject({
    name: guide.name,
    purpose: guide.purpose,
    provider: 'openai',
    model: 'account-model',
    harness: 'general',
    tools: [],
    hosted_consent: true,
    hosted_tools_consent: false,
  });
  expect(f.create).not.toHaveBeenCalled();
});
