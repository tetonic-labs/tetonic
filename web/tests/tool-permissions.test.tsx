import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { LocalAgentSetup } from '../src/components/work/LocalAgentSetup';
import { LocalEngine, type EngineWorkspace } from '../src/lib/localEngine';

const workspace: EngineWorkspace = {
  organization: 'Workspace',
  team_id: 'personal',
  team_name: 'Personal',
  agent_id: 'assistant',
  agent_name: 'Assistant',
  model: 'installed-model',
  input_limit: 12000,
  agents: [],
  tasks: [],
};

function setup(
  tools = ['read_file', 'list_dir', 'grep', 'glob', 'write_file', 'edit_file'],
  hostedReads = false,
  hostedTools = ['read_file', 'list_dir', 'grep', 'glob'],
) {
  const client = new LocalEngine('test');
  vi.spyOn(client, 'providerModels').mockResolvedValue({
    provider: 'openai',
    models: ['gpt-4.1'],
    capabilities_verified: false,
  });
  const catalog = vi.spyOn(client, 'agentCatalog').mockResolvedValue({
    workspace_root: 'C:/approved-work',
    runtime_profiles: hostedReads
      ? [
          { provider: 'ollama', harness: 'general', tools, tool_restriction: null },
          {
            provider: 'openai',
            harness: 'general',
            tools: hostedTools,
            requires_tool_consent: true,
            tool_restriction: 'Writes are not supported.',
          },
        ]
      : undefined,
    models: ['installed-model'],
    harnesses: ['general'],
    tools,
    providers: [{ id: 'openai', name: 'OpenAI', key_saved: true }],
    max_steps: 8,
    max_seconds: 120,
    max_tokens: 4096,
  });
  const create = vi.spyOn(client, 'createAgent').mockImplementation(async (input) => ({
    ...input,
    key: 'saved',
    id: 'saved',
  }));
  render(
    <LocalAgentSetup client={client} workspace={workspace} onCreated={vi.fn()} onBack={vi.fn()} />,
  );
  return { create, catalog };
}

afterEach(() => vi.restoreAllMocks());

describe('connected agent permissions', () => {
  it('retains a selected terminal when switching to a compatible frontier provider', async () => {
    const { create } = setup(['read_file', 'run_shell'], true, ['read_file', 'run_shell']);
    fireEvent.change(await screen.findByLabelText('Name', { exact: true }), {
      target: { value: 'Operator' },
    });
    await userEvent.click(screen.getByRole('checkbox', { name: 'Terminal' }));
    fireEvent.change(screen.getByLabelText('Model provider'), { target: { value: 'openai' } });
    await screen.findByRole('option', { name: 'gpt-4.1', exact: true });
    expect(screen.getByRole('checkbox', { name: 'Terminal' })).toHaveProperty('checked', true);
    expect(screen.getByText(/Each command appears in Needs you/)).toBeTruthy();
    fireEvent.change(screen.getByLabelText('Model', { exact: true }), {
      target: { value: 'gpt-4.1' },
    });
    await userEvent.click(screen.getByRole('checkbox', { name: /Allow this agent/ }));
    expect(screen.getByRole('button', { name: /Create agent/ })).toHaveProperty('disabled', true);
    await userEvent.click(
      screen.getByRole('checkbox', { name: /Allow selected tool inputs and results/ }),
    );
    await userEvent.click(screen.getByRole('button', { name: /Create agent/ }));
    await waitFor(() => expect(create).toHaveBeenCalledOnce());
    expect(create.mock.calls[0][0]).toMatchObject({
      provider: 'openai',
      tools: ['run_shell'],
      hosted_tools_consent: true,
      expected_workspace_root: 'C:/approved-work',
    });
  });
  it('attaches selected hosted read tools only after explicit folder disclosure consent', async () => {
    const { create } = setup(undefined, true);
    fireEvent.change(await screen.findByLabelText('Name', { exact: true }), {
      target: { value: 'Lab reader' },
    });
    await userEvent.click(screen.getByRole('checkbox', { name: 'Read files' }));
    fireEvent.change(screen.getByLabelText('Model provider'), { target: { value: 'openai' } });
    await screen.findByRole('option', { name: 'gpt-4.1', exact: true });
    fireEvent.change(screen.getByLabelText('Model', { exact: true }), {
      target: { value: 'gpt-4.1' },
    });
    await userEvent.click(screen.getByRole('checkbox', { name: /Allow this agent/ }));
    expect(screen.getByRole('button', { name: /Create agent/ })).toHaveProperty('disabled', true);
    expect(screen.getByRole('checkbox', { name: 'Read files' })).toHaveProperty('checked', true);
    expect(screen.getByRole('checkbox', { name: 'Write files' })).toHaveProperty('disabled', true);
    await userEvent.click(
      screen.getByRole('checkbox', {
        name: /Allow selected tool inputs and results.*C:\/approved-work/,
      }),
    );
    await userEvent.click(screen.getByRole('button', { name: /Create agent/ }));
    await waitFor(() => expect(create).toHaveBeenCalledOnce());
    expect(create.mock.calls[0][0]).toMatchObject({
      provider: 'openai',
      hosted_tools_consent: true,
      expected_workspace_root: 'C:/approved-work',
      tools: ['read_file', 'list_dir', 'grep', 'glob'],
    });
  });
  it('sends an explicit empty tool grant when no tools are selected', async () => {
    const { create } = setup();
    fireEvent.change(await screen.findByLabelText('Name', { exact: true }), {
      target: { value: 'Thinker' },
    });
    expect(screen.queryByRole('checkbox', { name: 'Terminal' })).toBeNull();
    expect(screen.queryByRole('checkbox', { name: 'Recall' })).toBeNull();
    expect(screen.queryByRole('option', { name: 'Coding' })).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: /Create agent/ }));
    await waitFor(() => expect(create).toHaveBeenCalledOnce());
    expect(create.mock.calls[0][0].tools).toEqual([]);
  });

  it('preserves selected tools across provider changes and blocks incompatible creation', async () => {
    const { create } = setup();
    fireEvent.change(await screen.findByLabelText('Name', { exact: true }), {
      target: { value: 'Reader' },
    });
    await userEvent.click(screen.getByRole('checkbox', { name: 'Read files' }));
    fireEvent.change(screen.getByLabelText('Model provider'), { target: { value: 'openai' } });
    await screen.findByRole('option', { name: 'gpt-4.1', exact: true });
    expect(screen.getByRole('checkbox', { name: 'Read files' })).toHaveProperty('checked', true);
    expect(screen.getByRole('checkbox', { name: 'Write files' })).toHaveProperty('disabled', true);
    fireEvent.change(screen.getByLabelText('Model', { exact: true }), {
      target: { value: 'gpt-4.1' },
    });
    await userEvent.click(screen.getByRole('checkbox', { name: /Allow this agent/ }));
    expect(screen.getByRole('alert').textContent).toContain('selected tools are unavailable');
    expect(screen.getByRole('button', { name: /Create agent/ })).toHaveProperty('disabled', true);
    fireEvent.submit(screen.getByRole('button', { name: /Create agent/ }).closest('form')!);
    expect(create).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText('Model provider'), { target: { value: 'ollama' } });
    expect(screen.getByRole('checkbox', { name: 'Read files' })).toHaveProperty('checked', true);
    fireEvent.change(screen.getByLabelText('Model provider'), { target: { value: 'openai' } });
    await screen.findByRole('option', { name: 'gpt-4.1', exact: true });
    fireEvent.change(screen.getByLabelText('Model', { exact: true }), {
      target: { value: 'gpt-4.1' },
    });
    await userEvent.click(screen.getByRole('checkbox', { name: /Allow this agent/ }));
    await userEvent.click(screen.getByRole('checkbox', { name: 'Read files' }));
    await userEvent.click(screen.getByRole('button', { name: /Create agent/ }));
    await waitFor(() => expect(create).toHaveBeenCalledOnce());
    expect(create.mock.calls[0][0]).toMatchObject({ provider: 'openai', tools: [] });
  });

  it('does not offer workspace tools when the host has not granted a folder', async () => {
    setup([]);
    await screen.findByLabelText('Name', { exact: true });
    expect(screen.queryByRole('checkbox', { name: 'Read files' })).toBeNull();
    expect(screen.queryByRole('checkbox', { name: 'Write files' })).toBeNull();
  });

  it('keeps a selected read grant distinct from permission to write', async () => {
    const { create } = setup();
    fireEvent.change(await screen.findByLabelText('Name', { exact: true }), {
      target: { value: 'Reader' },
    });
    await userEvent.click(screen.getByRole('checkbox', { name: 'Read files' }));
    await userEvent.click(screen.getByRole('button', { name: /Create agent/ }));
    await waitFor(() => expect(create).toHaveBeenCalledOnce());
    expect(create.mock.calls[0][0].tools).toEqual(['read_file', 'list_dir', 'grep', 'glob']);
  });

  it('does not expand a selected partial toolkit after the host adds capabilities', async () => {
    const { create, catalog } = setup(['read_file']);
    fireEvent.change(await screen.findByLabelText('Name', { exact: true }), {
      target: { value: 'Reader' },
    });
    await userEvent.click(screen.getByRole('checkbox', { name: 'Read files' }));
    const before = await catalog.mock.results[0].value;
    catalog.mockResolvedValue({ ...before, tools: ['read_file', 'list_dir', 'grep', 'glob'] });
    await userEvent.click(screen.getByRole('button', { name: 'Refresh engine setup' }));
    await waitFor(() => expect(catalog).toHaveBeenCalledTimes(2));
    expect(screen.getByLabelText('Name', { exact: true })).toHaveProperty('value', 'Reader');
    await userEvent.click(screen.getByRole('button', { name: 'Create agent' }));
    await waitFor(() => expect(create).toHaveBeenCalledOnce());
    expect(create.mock.calls[0][0].tools).toEqual(['read_file']);
  });

  it('requires fresh disclosure approval when refreshing reveals a different folder', async () => {
    const { create, catalog } = setup(['read_file'], true);
    fireEvent.change(await screen.findByLabelText('Name', { exact: true }), {
      target: { value: 'Lab reader' },
    });
    fireEvent.change(screen.getByLabelText('Model provider'), { target: { value: 'openai' } });
    await screen.findByRole('option', { name: 'gpt-4.1', exact: true });
    fireEvent.change(screen.getByLabelText('Model', { exact: true }), {
      target: { value: 'gpt-4.1' },
    });
    await userEvent.click(screen.getByRole('checkbox', { name: /Allow this agent/ }));
    await userEvent.click(screen.getByRole('checkbox', { name: 'Read files' }));
    await userEvent.click(
      screen.getByRole('checkbox', { name: /Allow selected tool inputs and results/ }),
    );
    expect(screen.getByRole('button', { name: 'Create agent' })).toHaveProperty('disabled', false);
    const before = await catalog.mock.results[0].value;
    catalog.mockResolvedValue({ ...before, workspace_root: 'C:/different-work' });
    await userEvent.click(screen.getByRole('button', { name: 'Refresh engine setup' }));
    const consent = await screen.findByRole('checkbox', {
      name: /Allow selected tool inputs and results.*C:\/different-work/,
    });
    expect(consent).toHaveProperty('checked', false);
    expect(screen.getByRole('button', { name: 'Create agent' })).toHaveProperty('disabled', true);
    fireEvent.submit(screen.getByRole('button', { name: 'Create agent' }).closest('form')!);
    expect(create).not.toHaveBeenCalled();
    await userEvent.click(consent);
    await userEvent.click(screen.getByRole('button', { name: 'Create agent' }));
    await waitFor(() => expect(create).toHaveBeenCalledOnce());
    expect(create.mock.calls[0][0]).toMatchObject({
      expected_workspace_root: 'C:/different-work',
      tools: ['read_file'],
    });
  });
});
