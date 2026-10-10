import { afterEach, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { LocalAgentSetup } from '../src/components/work/LocalAgentSetup';
import { LocalEngine } from '../src/engine/client';
import type { AgentCatalog, EngineWorkspace } from '../src/engine/contracts';

afterEach(() => vi.restoreAllMocks());

function setup(hosted = false, manageable = true, empty = false) {
  const client = new LocalEngine('fixture');
  const configured = empty ? [] : ['C:/work'];
  let folders = [...configured];
  const catalog = (): AgentCatalog => ({
    folder_management: manageable,
    workspace_root: empty ? null : 'C:/work',
    workspace_folders: folders,
    models: ['local-model'],
    harnesses: ['general'],
    tools: folders.length ? ['read_file'] : [],
    max_steps: 16,
    max_seconds: 300,
    max_tokens: 4096,
    providers: [{ id: 'openai', name: 'OpenAI', key_saved: true }],
    runtime_profiles: ['ollama', 'openai'].map((provider) => ({
      provider,
      harness: 'general',
      tools: folders.length ? ['read_file'] : [],
      requires_tool_consent: provider !== 'ollama',
      tool_restriction: null,
    })),
  });
  vi.spyOn(client, 'agentCatalog').mockImplementation(async () => catalog());
  vi.spyOn(client, 'providerModels').mockResolvedValue({
    provider: 'openai',
    models: ['test-model'],
    capabilities_verified: false,
  });
  vi.spyOn(client, 'workspaceFolders').mockResolvedValue({
    revision: 0,
    paths: [],
    configured_paths: configured,
    can_manage: manageable,
  });
  const inspect = vi.spyOn(client, 'inspectFolder').mockResolvedValue({ path: 'C:/research' });
  const save = vi.spyOn(client, 'saveWorkspaceFolders').mockImplementation(async (input) => {
    folders = [...configured, ...input.paths];
    return { revision: 1, paths: input.paths, configured_paths: configured, can_manage: true };
  });
  const create = vi
    .spyOn(client, 'createAgent')
    .mockImplementation(async (input) => ({ ...input, key: 'saved', id: 'saved' }));
  const workspace: EngineWorkspace = {
    organization: 'Workspace',
    team_id: 'team',
    team_name: 'Team',
    agent_id: 'assistant',
    agent_name: 'Assistant',
    model: hosted ? '' : 'local-model',
    input_limit: 12000,
    agents: [],
    tasks: [],
  };
  render(
    <LocalAgentSetup client={client} workspace={workspace} onCreated={vi.fn()} onBack={vi.fn()} />,
  );
  return { client, inspect, save, create };
}

async function openFolderSetup() {
  fireEvent.change(await screen.findByLabelText('Name', { exact: true }), {
    target: { value: 'My research agent' },
  });
  await userEvent.click(screen.getByRole('checkbox', { name: 'Read files' }));
  await userEvent.click(screen.getByRole('button', { name: 'Add a working folder' }));
  await userEvent.type(
    screen.getByLabelText('Folder on the engine computer'),
    'C:/research{Enter}',
  );
  await screen.findByText('Use this folder?');
}

it('can add the first folder before file tools become available', async () => {
  const { create } = setup(false, true, true);
  await screen.findByLabelText('Name', { exact: true });
  expect(screen.queryByRole('checkbox', { name: 'Read files' })).toBeNull();
  await userEvent.click(screen.getByRole('button', { name: 'Add a working folder' }));
  await userEvent.type(
    screen.getByLabelText('Folder on the engine computer'),
    'C:/research{Enter}',
  );
  await userEvent.click(await screen.findByRole('button', { name: 'Add and select folder' }));
  expect(await screen.findByRole('checkbox', { name: 'Read files' })).toHaveProperty(
    'checked',
    false,
  );
  expect(screen.getByRole('combobox', { name: 'Working folder' })).toHaveProperty(
    'value',
    'C:/research',
  );
  expect(create).not.toHaveBeenCalled();
});

it('checks then adds a folder in place and only grants it when the agent is saved', async () => {
  const { inspect, save, create } = setup();
  await openFolderSetup();
  expect(inspect).toHaveBeenCalledWith('C:/research');
  expect(save).not.toHaveBeenCalled();
  expect(create).not.toHaveBeenCalled();
  expect(screen.getByRole('button', { name: 'Create agent' })).toHaveProperty('disabled', true);
  await userEvent.click(screen.getByRole('button', { name: 'Add and select folder' }));
  await waitFor(() =>
    expect(screen.getByRole('combobox', { name: 'Working folder' })).toHaveProperty(
      'value',
      'C:/research',
    ),
  );
  expect(screen.getByLabelText('Name', { exact: true })).toHaveProperty(
    'value',
    'My research agent',
  );
  expect(create).not.toHaveBeenCalled();
  await userEvent.click(screen.getByRole('button', { name: 'Create agent' }));
  await waitFor(() => expect(create).toHaveBeenCalledOnce());
  expect(create.mock.calls[0][0]).toMatchObject({
    workspace_root: 'C:/research',
    tools: ['read_file'],
  });
});

it('preserves retry identity after an uncertain save and retains the draft on cancel', async () => {
  const { save, create } = setup();
  save.mockRejectedValueOnce(new Error('Connection interrupted. Retry to confirm.'));
  await openFolderSetup();
  await userEvent.click(screen.getByRole('button', { name: 'Add and select folder' }));
  await screen.findByRole('alert');
  await userEvent.click(screen.getByRole('button', { name: 'Add and select folder' }));
  await waitFor(() => expect(save).toHaveBeenCalledTimes(2));
  expect(save.mock.calls[0][0]).toEqual(save.mock.calls[1][0]);
  await screen.findByRole('button', { name: 'Add a working folder' });
  await userEvent.click(screen.getByRole('button', { name: 'Add a working folder' }));
  await userEvent.click(screen.getByRole('button', { name: 'Cancel folder setup' }));
  expect(screen.getByLabelText('Name', { exact: true })).toHaveProperty(
    'value',
    'My research agent',
  );
  expect(create).not.toHaveBeenCalled();
});

it('requires renewed hosted disclosure after registering and selecting a different folder', async () => {
  const { create } = setup(true);
  fireEvent.change(await screen.findByLabelText('Name', { exact: true }), {
    target: { value: 'Reader' },
  });
  fireEvent.change(screen.getByLabelText('Model provider'), { target: { value: 'openai' } });
  await screen.findByRole('option', { name: 'test-model', exact: true });
  fireEvent.change(screen.getByLabelText('Model', { exact: true }), {
    target: { value: 'test-model' },
  });
  await userEvent.click(screen.getByRole('checkbox', { name: /Allow this agent/ }));
  await userEvent.click(screen.getByRole('checkbox', { name: 'Read files' }));
  await userEvent.click(
    screen.getByRole('checkbox', { name: /Allow selected tool inputs and results/ }),
  );
  await userEvent.click(screen.getByRole('button', { name: 'Add a working folder' }));
  await userEvent.type(
    screen.getByLabelText('Folder on the engine computer'),
    'C:/research{Enter}',
  );
  await userEvent.click(await screen.findByRole('button', { name: 'Add and select folder' }));
  await screen.findByRole('button', { name: 'Add a working folder' });
  expect(
    screen.getByRole('checkbox', { name: /Allow selected tool inputs and results/ }),
  ).toHaveProperty('checked', false);
  expect(screen.getByRole('button', { name: 'Create agent' })).toHaveProperty('disabled', true);
  await userEvent.click(
    screen.getByRole('checkbox', { name: /Allow selected tool inputs and results/ }),
  );
  await userEvent.click(screen.getByRole('button', { name: 'Create agent' }));
  await waitFor(() => expect(create).toHaveBeenCalledOnce());
  expect(create.mock.calls[0][0]).toMatchObject({
    workspace_root: 'C:/research',
    expected_workspace_root: 'C:/research',
    hosted_tools_consent: true,
  });
});

it('respects a host-managed catalog and does not offer an ineffective add action', async () => {
  setup(false, false);
  await screen.findByLabelText('Name', { exact: true });
  await userEvent.click(screen.getByRole('checkbox', { name: 'Read files' }));
  expect(screen.queryByRole('button', { name: 'Add a working folder' })).toBeNull();
  expect(screen.getByText(/The host operator manages available folders/)).toBeTruthy();
});
