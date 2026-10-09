import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { LocalAgentSetup } from '../src/components/work/LocalAgentSetup';
import { LocalEngine } from '../src/engine/client';
import { type AgentCatalog, type McpConnection } from '../src/engine/contracts';

const search = {
  id: 'mcp_calendar_pinned_search',
  name: 'search',
  description: 'Find calendar availability',
  input_schema: { type: 'object' },
};
const lookup = {
  ...search,
  id: 'mcp_calendar_pinned_lookup',
  name: 'lookup',
  description: 'Look up an event',
};
const connection: McpConnection = {
  id: 'calendar',
  name: 'Calendar',
  endpoint: 'http://127.0.0.1:8765/mcp',
  status: 'unchecked',
  message: 'Discover tools to check this connection.',
  tools: [],
};
function setup(hosted = false) {
  const client = new LocalEngine('fixture');
  let current = connection;
  const catalog = (): AgentCatalog => ({
    models: ['local-model'],
    harnesses: ['general'],
    tools: current.tools.map((t) => t.id),
    mcp_connections: [current],
    providers: ['openai', 'anthropic', 'google'].map((id) => ({ id, name: id, key_saved: true })),
    runtime_profiles: [
      {
        provider: 'ollama',
        harness: 'general',
        tools: current.tools.map((t) => t.id),
        tool_restriction: null,
      },
      ...['openai', 'anthropic', 'google'].map((provider) => ({
        provider,
        harness: 'general',
        tools: hosted ? current.tools.map((t) => t.id) : [],
        requires_tool_consent: true,
        tool_restriction: hosted ? null : 'MCP tools are not available with this provider.',
      })),
    ],
    max_steps: 4,
    max_seconds: 120,
    max_tokens: 4096,
  });
  vi.spyOn(client, 'agentCatalog').mockImplementation(async () => catalog());
  vi.spyOn(client, 'providerModels').mockImplementation(async (provider) => ({
    provider,
    models: ['account-model'],
    capabilities_verified: false,
  }));
  const discover = vi.spyOn(client, 'discoverMcp').mockImplementation(async () => {
    current = {
      ...connection,
      status: 'discovered',
      message: 'Discovery succeeded.',
      tools: [search, lookup],
    };
    return current;
  });
  const create = vi
    .spyOn(client, 'createAgent')
    .mockImplementation(async (input) => ({ ...input, id: 'new', key: 'new' }));
  render(
    <LocalAgentSetup
      client={client}
      workspace={{
        organization: 'Workspace',
        team_id: 'team',
        team_name: 'Team',
        agent_id: 'default',
        agent_name: 'Assistant',
        model: 'local-model',
        input_limit: 12000,
        agents: [],
        tasks: [],
      }}
      onCreated={vi.fn()}
      onBack={vi.fn()}
    />,
  );
  return {
    create,
    discover,
    setConnection: (value: McpConnection) => {
      current = value;
    },
  };
}
afterEach(() => vi.restoreAllMocks());

describe('connected MCP tool selection', () => {
  it.each(['openai', 'anthropic', 'google'])(
    'preserves MCP selection for %s and requires consent for its exact data scope',
    async (provider) => {
      const f = setup(true);
      await userEvent.click(await screen.findByRole('button', { name: 'Discover Calendar tools' }));
      await userEvent.click(await screen.findByRole('checkbox', { name: 'Calendar: search' }));
      fireEvent.change(screen.getByLabelText('Name', { exact: true }), {
        target: { value: 'Calendar researcher' },
      });
      fireEvent.change(screen.getByLabelText('Model provider'), { target: { value: provider } });
      await screen.findByRole('option', { name: 'account-model', exact: true });
      fireEvent.change(screen.getByLabelText('Model', { exact: true }), {
        target: { value: 'account-model' },
      });
      await userEvent.click(screen.getByRole('checkbox', { name: /Allow this agent/ }));
      expect(screen.getByRole('checkbox', { name: 'Calendar: search' })).toHaveProperty(
        'checked',
        true,
      );
      expect(screen.getByRole('button', { name: 'Create agent' })).toHaveProperty('disabled', true);
      await userEvent.click(
        screen.getByRole('checkbox', { name: /Allow selected tool inputs and results/ }),
      );
      expect(screen.getByRole('button', { name: 'Create agent' })).toHaveProperty(
        'disabled',
        false,
      );
      await userEvent.click(screen.getByRole('checkbox', { name: 'Calendar: lookup' }));
      expect(
        screen.getByRole('checkbox', { name: /Allow selected tool inputs and results/ }),
      ).toHaveProperty('checked', false);
      expect(screen.getByRole('button', { name: 'Create agent' })).toHaveProperty('disabled', true);
      await userEvent.click(
        screen.getByRole('checkbox', { name: /Allow selected tool inputs and results/ }),
      );
      await userEvent.click(screen.getByRole('button', { name: 'Create agent' }));
      await waitFor(() => expect(f.create).toHaveBeenCalledOnce());
      expect(f.create.mock.calls[0][0]).toMatchObject({
        provider,
        tools: [search.id, lookup.id],
        hosted_tools_consent: true,
      });
    },
  );
  it('discovers an operator-configured service and attaches only the individual selected tool', async () => {
    const f = setup();
    fireEvent.change(await screen.findByLabelText('Name', { exact: true }), {
      target: { value: 'Calendar researcher' },
    });
    expect(screen.queryByRole('checkbox', { name: 'Calendar: search' })).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: 'Discover Calendar tools' }));
    await userEvent.click(await screen.findByRole('checkbox', { name: 'Calendar: search' }));
    expect(f.discover).toHaveBeenCalledExactlyOnceWith('calendar');
    expect(screen.getByRole('checkbox', { name: 'Calendar: lookup' })).toHaveProperty(
      'checked',
      false,
    );
    await userEvent.click(screen.getByRole('button', { name: 'Create agent' }));
    await waitFor(() => expect(f.create).toHaveBeenCalledOnce());
    expect(f.create.mock.calls[0][0]).toMatchObject({ tools: [search.id], provider: 'ollama' });
    expect(f.create.mock.calls[0][0]).not.toHaveProperty('endpoint');
  });
  it('keeps selected MCP tools visible and blocks an incompatible provider rather than removing them', async () => {
    const f = setup();
    await userEvent.click(await screen.findByRole('button', { name: 'Discover Calendar tools' }));
    await userEvent.click(await screen.findByRole('checkbox', { name: 'Calendar: search' }));
    fireEvent.change(screen.getByLabelText('Name', { exact: true }), {
      target: { value: 'Researcher' },
    });
    fireEvent.change(screen.getByLabelText('Model provider'), { target: { value: 'openai' } });
    await screen.findByRole('option', { name: 'account-model', exact: true });
    fireEvent.change(screen.getByLabelText('Model', { exact: true }), {
      target: { value: 'account-model' },
    });
    await userEvent.click(screen.getByRole('checkbox', { name: /Allow this agent/ }));
    expect(screen.getByRole('checkbox', { name: 'Calendar: search' })).toHaveProperty(
      'checked',
      true,
    );
    expect(screen.getByRole('button', { name: 'Create agent' })).toHaveProperty('disabled', true);
    fireEvent.submit(screen.getByRole('button', { name: 'Create agent' }).closest('form')!);
    expect(f.create).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole('checkbox', { name: 'Calendar: search' }));
    await userEvent.click(screen.getByRole('button', { name: 'Create agent' }));
    await waitFor(() => expect(f.create).toHaveBeenCalledOnce());
    expect(f.create.mock.calls[0][0].tools).toEqual([]);
  });
  it('keeps a removed tool removable and does not substitute the server’s changed version', async () => {
    const f = setup();
    await userEvent.click(await screen.findByRole('button', { name: 'Discover Calendar tools' }));
    await userEvent.click(await screen.findByRole('checkbox', { name: 'Calendar: search' }));
    fireEvent.change(screen.getByLabelText('Name', { exact: true }), {
      target: { value: 'Researcher' },
    });
    f.setConnection({
      ...connection,
      status: 'unavailable',
      message: 'Server unavailable',
      tools: [],
    });
    await userEvent.click(screen.getByRole('button', { name: 'Refresh engine setup' }));
    await screen.findByText('Server unavailable');
    const old = screen.getByRole('checkbox', { name: 'Calendar: search' });
    expect(old).toHaveProperty('checked', true);
    expect(old).toHaveProperty('disabled', false);
    expect(screen.getByRole('button', { name: 'Create agent' })).toHaveProperty('disabled', true);
    await userEvent.click(old);
    await userEvent.click(screen.getByRole('button', { name: 'Create agent' }));
    await waitFor(() => expect(f.create).toHaveBeenCalledOnce());
    expect(f.create.mock.calls[0][0].tools).toEqual([]);
  });
  it('shows discovery failures without making tools appear connected', async () => {
    const f = setup();
    f.discover.mockRejectedValue(new Error('Connection could not be checked'));
    await userEvent.click(await screen.findByRole('button', { name: 'Discover Calendar tools' }));
    await screen.findByRole('alert');
    expect(screen.queryByRole('checkbox', { name: 'Calendar: search' })).toBeNull();
    expect(f.create).not.toHaveBeenCalled();
  });
});
