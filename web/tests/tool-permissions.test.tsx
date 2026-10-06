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

function setup(tools = ['read_file', 'list_dir', 'grep', 'glob', 'write_file', 'edit_file']) {
  const client = new LocalEngine('test');
  vi.spyOn(client, 'agentCatalog').mockResolvedValue({
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
  return create;
}

afterEach(() => vi.restoreAllMocks());

describe('connected agent permissions', () => {
  it('sends an explicit empty tool grant when no tools are selected', async () => {
    const create = setup();
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

  it('grants only selected file capabilities and clears them for prompt-only providers', async () => {
    const create = setup();
    fireEvent.change(await screen.findByLabelText('Name', { exact: true }), {
      target: { value: 'Reader' },
    });
    await userEvent.click(screen.getByRole('checkbox', { name: 'Read files' }));
    fireEvent.change(screen.getByLabelText('Model provider'), { target: { value: 'openai' } });
    expect(screen.queryByRole('checkbox', { name: 'Read files' })).toBeNull();
    expect(screen.queryByRole('checkbox', { name: 'Write files' })).toBeNull();
    fireEvent.change(screen.getByLabelText('Model', { exact: true }), {
      target: { value: 'gpt-4.1' },
    });
    await userEvent.click(screen.getByRole('checkbox', { name: /Allow this agent/ }));
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
    const create = setup();
    fireEvent.change(await screen.findByLabelText('Name', { exact: true }), {
      target: { value: 'Reader' },
    });
    await userEvent.click(screen.getByRole('checkbox', { name: 'Read files' }));
    await userEvent.click(screen.getByRole('button', { name: /Create agent/ }));
    await waitFor(() => expect(create).toHaveBeenCalledOnce());
    expect(create.mock.calls[0][0].tools).toEqual(['read_file', 'list_dir', 'grep', 'glob']);
  });
});
