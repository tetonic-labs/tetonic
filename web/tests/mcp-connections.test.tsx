import { afterEach, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { McpConnectionEditor } from '../src/components/views/McpConnectionEditor';
import { LocalAgentSetup } from '../src/components/work/LocalAgentSetup';
import {
  LocalEngine,
  type AgentCatalog,
  type EngineAgent,
  type McpConnection,
} from '../src/lib/localEngine';

afterEach(() => vi.restoreAllMocks());
const read = {
  id: 'mcp_service_read',
  name: 'Find availability',
  description: 'Read available times',
  input_schema: { type: 'object' },
  approved: false,
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
it('connects, reviews access separately, and disconnects without silently selecting an agent tool', async () => {
  const { client, save, discover } = service();
  render(<McpConnectionEditor client={client} onChanged={async () => {}} />);
  fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Calendar' } });
  fireEvent.change(screen.getByLabelText('MCP endpoint'), { target: { value: initial.endpoint } });
  fireEvent.change(screen.getByLabelText('Service token'), { target: { value: 'test-token' } });
  fireEvent.click(screen.getByRole('button', { name: 'Connect & review tools' }));
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
  expect((screen.getByLabelText('MCP endpoint') as HTMLInputElement).readOnly).toBe(true);
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
  await userEvent.type(editor.getByLabelText('Name'), 'Calendar{Enter}');
  fireEvent.change(editor.getByLabelText('MCP endpoint'), { target: { value: initial.endpoint } });
  fireEvent.change(editor.getByLabelText('Service token'), { target: { value: 'test-token' } });
  expect(update).not.toHaveBeenCalled();
  fireEvent.click(editor.getByRole('button', { name: 'Connect & review tools' }));
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
