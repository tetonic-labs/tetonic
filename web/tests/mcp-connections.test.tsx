import { afterEach, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { McpConnectionEditor } from '../src/components/views/McpConnectionEditor';
import { LocalAgentSetup } from '../src/components/work/LocalAgentSetup';
import { LocalEngine } from '../src/engine/client';
import { type AgentCatalog, type EngineAgent, type McpConnection } from '../src/engine/contracts';

afterEach(() => vi.restoreAllMocks());
const read = {
  id: 'mcp_service_read',
  name: 'Find availability',
  description: 'Read available times',
  input_schema: { type: 'object' },
  approved: false,
  read_only: true,
};
const initial: McpConnection = {
  id: 'service',
  name: 'Calendar',
  endpoint: 'https://calendar.example/mcp',
  auth: 'bearer',
  revision: 1,
  editable: true,
  enabled: true,
  status: 'unchecked',
  message: 'Saved',
  tools: [],
};
function service() {
  const client = new LocalEngine('fixture');
  let record = initial;
  const save = vi.spyOn(client, 'saveMcpConnection').mockImplementation(async (input) => {
    record = {
      ...record,
      id: input.id,
      name: input.name,
      endpoint: input.endpoint,
      auth: input.auth,
      enabled: input.enabled,
      revision: input.expected_revision + 1,
      status: input.enabled ? 'unchecked' : 'disconnected',
      tools: input.approved_tools?.includes(read.id) ? [{ ...read, approved: true }] : [],
    };
    return record;
  });
  const discover = vi.spyOn(client, 'discoverMcp').mockImplementation(async () => {
    record = {
      ...record,
      status: 'discovered',
      message: 'Checked',
      tools: [{ ...read, approved: record.tools.some((t) => t.id === read.id && t.approved) }],
    };
    return record;
  });
  return { client, save, discover, current: () => record };
}
it.each([
  ['localhost:3102/mcp', 'http://127.0.0.1:3102/mcp', '127.0.0.1:3102'],
  ['http://localhost:3102/mcp', 'http://127.0.0.1:3102/mcp', '127.0.0.1:3102'],
  ['[::1]:3102/mcp', 'http://[::1]:3102/mcp', '[::1]:3102'],
  ['https://calendar.example/mcp', 'https://calendar.example/mcp', 'calendar.example'],
])(
  'connects from just an address (%s) without credentials or tool grants',
  async (address, endpoint, name) => {
    const { client, save, discover } = service();
    render(<McpConnectionEditor client={client} onChanged={async () => {}} />);
    expect(screen.queryByLabelText('Service token')).toBeNull();
    fireEvent.change(screen.getByLabelText('Service address'), { target: { value: address } });
    fireEvent.click(screen.getByRole('button', { name: 'Connect', exact: true }));
    await screen.findByText('Connected', { exact: true });
    expect(save).toHaveBeenCalledOnce();
    expect(save.mock.calls[0][0]).toMatchObject({ endpoint, name, auth: 'none', enabled: true });
    expect(save.mock.calls[0][0].token).toBeUndefined();
    expect(save.mock.calls[0][0].approved_tools).toBeUndefined();
    expect(discover).toHaveBeenCalledOnce();
    expect(screen.getByRole('checkbox', { name: /Find availability/ })).toHaveProperty(
      'checked',
      false,
    );
  },
);

it.each([
  'http://remote.example/mcp',
  'http://localhost.evil.example/mcp',
  'https://user:secret@calendar.example/mcp',
  'localhost:3102/mcp?token=secret',
  'localhost:0/mcp',
])('does not save or discover an invalid address (%s)', async (address) => {
  const { client, save, discover } = service();
  render(<McpConnectionEditor client={client} onChanged={async () => {}} />);
  fireEvent.change(screen.getByLabelText('Service address'), { target: { value: address } });
  fireEvent.click(screen.getByRole('button', { name: 'Connect', exact: true }));
  expect(await screen.findByRole('alert')).toBeTruthy();
  expect(save).not.toHaveBeenCalled();
  expect(discover).not.toHaveBeenCalled();
});

it('discards canceled sign-in changes instead of disabling saved authentication on a later edit', () => {
  const { client, save } = service();
  render(<McpConnectionEditor client={client} connection={initial} onChanged={async () => {}} />);
  fireEvent.click(screen.getByRole('button', { name: 'Edit connection' }));
  fireEvent.change(screen.getByLabelText('Sign-in method'), { target: { value: 'none' } });
  fireEvent.change(screen.getByLabelText('Name (optional)'), {
    target: { value: 'Canceled name' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
  fireEvent.click(screen.getByRole('button', { name: 'Edit connection' }));
  expect(screen.getByLabelText('Sign-in method')).toHaveProperty('value', 'bearer');
  expect(screen.getByLabelText('Name (optional)')).toHaveProperty('value', initial.name);
  expect(screen.getByLabelText('Replacement token (optional)')).toHaveProperty('value', '');
  expect(save).not.toHaveBeenCalled();
});

it('reviews action tools and unannotated tools explicitly without automatically enabling them', async () => {
  const { client, save } = service();
  const action = {
    ...read,
    id: 'mcp_service_move',
    name: 'Move villager',
    read_only: false,
    destructive: false,
  };
  const unknown = {
    ...read,
    id: 'mcp_service_unknown',
    name: 'Unknown operation',
    read_only: undefined,
  };
  render(
    <McpConnectionEditor
      client={client}
      connection={{ ...initial, status: 'discovered', tools: [read, action, unknown] }}
      onChanged={async () => {}}
    />,
  );
  expect(screen.getByText('Available tools')).toBeTruthy();
  expect(screen.getByText('Read-only (reported by service)')).toBeTruthy();
  expect(screen.getByText('Action · can change state')).toBeTruthy();
  expect(screen.getByText('Action · can change state · may be destructive')).toBeTruthy();
  const move = screen.getByRole('checkbox', { name: /Move villager/ });
  expect(move).toHaveProperty('checked', false);
  expect(screen.getByRole('checkbox', { name: /Unknown operation/ })).toHaveProperty(
    'checked',
    false,
  );
  fireEvent.click(move);
  fireEvent.click(screen.getByRole('button', { name: 'Save tool access' }));
  await waitFor(() => expect(save).toHaveBeenCalledOnce());
  expect(save.mock.calls[0][0].approved_tools).toEqual([action.id]);
});
it('connects, reviews access separately, and disconnects without silently selecting an agent tool', async () => {
  const { client, save, discover } = service();
  render(<McpConnectionEditor client={client} onChanged={async () => {}} />);
  fireEvent.click(screen.getByText('Name and sign-in options'));
  fireEvent.change(screen.getByLabelText('Name (optional)'), { target: { value: 'Calendar' } });
  fireEvent.change(screen.getByLabelText('Service address'), {
    target: { value: initial.endpoint },
  });
  fireEvent.change(screen.getByLabelText('Sign-in method'), { target: { value: 'bearer' } });
  fireEvent.change(screen.getByLabelText('Service token'), { target: { value: 'test-token' } });
  fireEvent.click(screen.getByRole('button', { name: 'Connect', exact: true }));
  expect(await screen.findByText('Connected', { exact: true })).toBeTruthy();
  expect(discover).toHaveBeenCalledOnce();
  expect(save.mock.calls[0][0].approved_tools).toBeUndefined();
  expect(
    (screen.getByRole('checkbox', { name: /Find availability/ }) as HTMLInputElement).checked,
  ).toBe(false);
  fireEvent.click(screen.getByRole('checkbox', { name: /Find availability/ }));
  fireEvent.click(screen.getByRole('button', { name: 'Save tool access' }));
  await screen.findByText(/Workspace access saved/);
  expect(save.mock.calls[1][0]).toMatchObject({ approved_tools: [read.id], expected_revision: 1 });
  expect(save.mock.calls[1][0].token).toBeUndefined();
  fireEvent.click(screen.getByRole('button', { name: 'Edit connection' }));
  expect((screen.getByLabelText('Replacement token (optional)') as HTMLInputElement).value).toBe(
    '',
  );
  expect((screen.getByLabelText('Service address') as HTMLInputElement).readOnly).toBe(true);
  fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
  fireEvent.click(screen.getByRole('button', { name: 'Disconnect…' }));
  expect(save).toHaveBeenCalledTimes(2);
  fireEvent.click(screen.getByRole('button', { name: 'Disconnect service' }));
  expect(await screen.findByText('Disconnected', { exact: true })).toBeTruthy();
  expect(save.mock.calls[2][0]).toMatchObject({ enabled: false, expected_revision: 2 });
});

it('keeps a saved connection visible when discovery fails and surfaces an actionable auth failure', async () => {
  const { client, save, discover } = service();
  discover.mockResolvedValue({
    ...initial,
    status: 'unavailable',
    message: 'Service rejected authentication. Update its token.',
  });
  render(<McpConnectionEditor client={client} connection={initial} onChanged={async () => {}} />);
  fireEvent.click(screen.getByRole('button', { name: 'Check connection' }));
  expect(await screen.findByText(/Service rejected authentication/)).toBeTruthy();
  expect(save).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: 'Edit connection' }));
  fireEvent.change(screen.getByLabelText('Replacement token (optional)'), {
    target: { value: 'replacement' },
  });
  save.mockRejectedValue(new Error('Connection changed. Reload its settings before saving.'));
  fireEvent.click(screen.getByRole('button', { name: 'Save & check connection' }));
  expect((await screen.findByRole('alert')).textContent).toContain('Connection changed');
  expect((screen.getByLabelText('Replacement token (optional)') as HTMLInputElement).value).toBe(
    '',
  );
});

it('lets a service request a token after the first connection check without granting tools', async () => {
  const { client, save, discover, current } = service();
  discover.mockImplementationOnce(async () => ({
    ...current(),
    status: 'unavailable',
    message: 'This address requires sign-in. Check that it is the service’s MCP address.',
  }));
  render(<McpConnectionEditor client={client} onChanged={async () => {}} />);
  fireEvent.change(screen.getByLabelText('Service address'), {
    target: { value: initial.endpoint },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Connect', exact: true }));
  await screen.findByText(/This address requires sign-in/);
  expect(save.mock.calls[0][0].auth).toBe('none');
  fireEvent.click(screen.getByRole('button', { name: 'Edit connection' }));
  fireEvent.change(screen.getByLabelText('Sign-in method'), { target: { value: 'bearer' } });
  expect(screen.getByRole('button', { name: 'Save & check connection' })).toHaveProperty(
    'disabled',
    true,
  );
  fireEvent.change(screen.getByLabelText('Service token'), { target: { value: 'test-token' } });
  fireEvent.click(screen.getByRole('button', { name: 'Save & check connection' }));
  await screen.findByText('Connected', { exact: true });
  expect(save.mock.calls[1][0]).toMatchObject({
    auth: 'bearer',
    token: 'test-token',
    expected_revision: 1,
  });
  expect(save.mock.calls[1][0].approved_tools).toBeUndefined();
  expect(screen.getByRole('checkbox', { name: /Find availability/ })).toHaveProperty(
    'checked',
    false,
  );
});

it('adds a connection inside agent editing without submitting or losing the agent draft', async () => {
  const { client, save, current } = service();
  const agent: EngineAgent = {
    id: 'agent',
    key: 'agent',
    name: 'Robin',
    purpose: 'Research',
    model: 'local-model',
    provider: 'ollama',
    harness: 'general',
    tools: [],
    max_steps: 4,
    max_seconds: 120,
    max_tokens: 1024,
    editable: true,
    definition_digest: 'v1',
  };
  vi.spyOn(client, 'agentCatalog').mockImplementation(async (): Promise<AgentCatalog> => {
    const tools = current()
      .tools.filter((t) => t.approved)
      .map((t) => t.id);
    return {
      mcp_management: true,
      skills: [],
      mcp_connections: save.mock.calls.length ? [current()] : [],
      models: ['local-model'],
      harnesses: ['general'],
      tools,
      max_steps: 8,
      max_seconds: 120,
      max_tokens: 4096,
      runtime_profiles: [{ provider: 'ollama', harness: 'general', tools, tool_restriction: null }],
    };
  });
  const update = vi
    .spyOn(client, 'updateAgent')
    .mockImplementation(async (old, input) => ({ ...old, ...input }));
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
        tasks: [],
      }}
      onCreated={vi.fn()}
      onBack={vi.fn()}
    />,
  );
  fireEvent.change(await screen.findByLabelText('Name', { exact: true }), {
    target: { value: 'Robin edited' },
  });
  fireEvent.click(screen.getByText('Add tools, MCPs or skills to the workspace'));
  fireEvent.click(screen.getByRole('button', { name: /Connect a service/ }));
  const editor = within(screen.getByRole('region', { name: 'Connect a service' }));
  fireEvent.click(editor.getByText('Name and sign-in options'));
  await userEvent.type(editor.getByLabelText('Name (optional)'), 'Calendar{Enter}');
  fireEvent.change(editor.getByLabelText('Service address'), {
    target: { value: initial.endpoint },
  });
  fireEvent.change(editor.getByLabelText('Sign-in method'), { target: { value: 'bearer' } });
  fireEvent.change(editor.getByLabelText('Service token'), { target: { value: 'test-token' } });
  expect(update).not.toHaveBeenCalled();
  fireEvent.click(editor.getByRole('button', { name: 'Connect', exact: true }));
  await screen.findByText('Connected', { exact: true });
  fireEvent.click(screen.getByRole('checkbox', { name: /Find availability/ }));
  fireEvent.click(screen.getByRole('button', { name: 'Save tool access' }));
  await waitFor(() => expect(save).toHaveBeenCalledTimes(2));
  await screen.findByText(/Workspace access saved/);
  expect((screen.getByLabelText('Name', { exact: true }) as HTMLInputElement).value).toBe(
    'Robin edited',
  );
  expect(
    (screen.getByRole('checkbox', { name: 'Calendar: Find availability' }) as HTMLInputElement)
      .checked,
  ).toBe(false);
  expect(update).not.toHaveBeenCalled();
});
