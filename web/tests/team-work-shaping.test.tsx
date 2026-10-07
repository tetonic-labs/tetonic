import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { TeamWorkspace } from '../src/components/team-work/TeamWorkspace';
import { LocalEngineProvider } from '../src/context/LocalEngineContext';
import { LocalEngine, type EngineTask, type EngineWorkspace } from '../src/lib/localEngine';

const guide = {
  key: 'configured-guide',
  id: 'guide-id',
  name: 'Our Guide',
  purpose: 'Shape work',
  model: 'test-model',
  harness: 'general',
  provider: 'ollama',
  max_steps: 6,
  max_seconds: 120,
  max_tokens: 4096,
  tools: [],
  editable: true,
  definition_digest: 'guide-revision',
};
const saved: EngineTask = {
  id: 'saved-exploration',
  input: 'Help me compare two approaches.',
  agent_key: guide.key,
  agent_name: guide.name,
  purpose: 'explore',
  state: 'completed',
  run_id: 'run-one',
  sequence: 10,
  messages: [{ id: 1, role: 'assistant', content: 'What would a useful outcome look like?' }],
};
function fixture(tasks: EngineTask[] = []) {
  const client = new LocalEngine('test-token');
  const workspace: EngineWorkspace = {
    organization: 'Test organization',
    team_id: 'owner-team',
    team_name: 'Owner workspace',
    agent_id: guide.id,
    agent_name: guide.name,
    shaping_agent_key: guide.key,
    model: guide.model,
    agents: [guide],
    tasks,
    input_limit: 12000,
  };
  const snapshot = vi.spyOn(client, 'snapshot').mockResolvedValue(workspace);
  vi.spyOn(client, 'agentCatalog').mockResolvedValue({
    models: [guide.model],
    harnesses: ['general'],
    max_steps: 8,
    max_seconds: 120,
    max_tokens: 4096,
  });
  vi.spyOn(client, 'workItems').mockResolvedValue([]);
  vi.spyOn(client, 'teams').mockResolvedValue([]);
  vi.spyOn(client, 'approvals').mockResolvedValue({
    active_stops: [],
    pending_approvals: [],
    effort: [],
  });
  vi.spyOn(client, 'briefs').mockResolvedValue([]);
  vi.spyOn(client, 'plan').mockResolvedValue({
    plans: [],
    generation: null,
    brief_revision: tasks.length ? 1 : 0,
    readiness: [],
    execution_available: false,
  });
  const submit = vi
    .spyOn(client, 'submit')
    .mockImplementation(async (id, input, key, parent, purpose) => ({
      ...saved,
      id,
      input,
      agent_key: key,
      parent_id: parent,
      purpose,
    }));
  const view = () =>
    render(
      <LocalEngineProvider client={client}>
        <TeamWorkspace />
      </LocalEngineProvider>,
    );
  return { client, snapshot, submit, view };
}

afterEach(() => {
  vi.restoreAllMocks();
  sessionStorage.clear();
  history.replaceState(null, '', '/');
});

describe('shaping in the team-work map', () => {
  it('points a hosted Guide with a removed key to settings without submitting phantom work', async () => {
    const f = fixture([saved]);
    const current = await f.client.snapshot();
    f.snapshot.mockResolvedValue({
      ...current,
      agents: [{ ...guide, provider: 'openai', hosted_consent: true }],
    });
    vi.mocked(f.client.agentCatalog).mockResolvedValue({
      models: [],
      harnesses: ['general'],
      tools: [],
      max_steps: 8,
      max_seconds: 120,
      max_tokens: 4096,
      providers: [{ id: 'openai', name: 'OpenAI', key_saved: false }],
    });
    history.replaceState(null, '', `/#shape=${saved.id}`);
    f.view();
    fireEvent.change(await screen.findByRole('textbox', { name: 'Continue the conversation' }), {
      target: { value: 'Continue this plan' },
    });
    await screen.findByText(/A provider key is needed.*Open Guide model/);
    expect(screen.getByRole('button', { name: 'Send exploration reply' })).toHaveProperty(
      'disabled',
      true,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Guide model' }));
    await screen.findByRole('combobox', { name: 'Model provider' });
    expect(f.submit).not.toHaveBeenCalled();
  });
  it('opens Guide settings directly from a conversation and returns without losing its draft', async () => {
    const f = fixture([saved]);
    history.replaceState(null, '', `/#shape=${saved.id}`);
    f.view();
    fireEvent.change(await screen.findByRole('textbox', { name: 'Continue the conversation' }), {
      target: { value: 'Keep this unfinished thought.' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Guide model' }));
    await screen.findByText(
      'Choose who helps you think and plan. Changes apply to the next reply.',
    );
    expect(screen.queryByRole('textbox', { name: 'Name', exact: true })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Back to previous view' }));
    expect(
      await screen.findByRole('textbox', { name: 'Continue the conversation' }),
    ).toHaveProperty('value', 'Keep this unfinished thought.');
    expect(f.submit).not.toHaveBeenCalled();
  });
  it('uses the configured live Guide without passing example projects or assignments', async () => {
    const f = fixture();
    f.view();
    const send = await screen.findByRole('button', { name: 'Send to the Guide' });
    fireEvent.change(screen.getByRole('textbox', { name: 'What would you like to work on?' }), {
      target: { value: 'I need to understand our options before deciding.' },
    });
    await waitFor(() => expect(send).toHaveProperty('disabled', false));
    fireEvent.click(send);
    await screen.findByRole('textbox', { name: 'Continue the conversation' });
    expect(screen.queryByRole('tablist')).toBeNull();
    expect(f.submit).toHaveBeenCalledExactlyOnceWith(
      expect.any(String),
      'I need to understand our options before deciding.',
      guide.key,
      undefined,
      'explore',
    );
    expect(screen.getByText('What would a useful outcome look like?')).toBeTruthy();
    expect(screen.queryByText('· sample map')).toBeNull();
    expect(location.hash).toBe(`#shape=${f.submit.mock.calls[0][0]}`);
  });

  it('resumes a saved discussion and saves its brief without dispatching sample agents', async () => {
    const f = fixture([saved]);
    const original = {
      work_id: saved.id,
      revision: 1,
      body: 'Compare the two approaches.',
      request_id: 'first-save',
      created_by: 'owner',
    };
    vi.mocked(f.client.briefs).mockResolvedValue([original]);
    const save = vi.spyOn(f.client, 'saveBrief').mockImplementation(async (id, request) => ({
      ...original,
      work_id: id,
      revision: 2,
      body: request.body,
      request_id: request.request_id,
    }));
    f.view();
    fireEvent.click(await screen.findByRole('button', { name: `Open ${saved.input}` }));
    fireEvent.click(await screen.findByText('Saved direction & history'));
    const editor = await screen.findByRole('textbox', { name: 'Current understanding' });
    await waitFor(() => expect(editor).toHaveProperty('value', original.body));
    fireEvent.change(editor, {
      target: { value: 'Start with a two-hour pilot. Access is unresolved.' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save brief' }));
    await waitFor(() =>
      expect(save).toHaveBeenCalledExactlyOnceWith(saved.id, {
        request_id: expect.any(String),
        expected_revision: 1,
        body: 'Start with a two-hour pilot. Access is unresolved.',
      }),
    );
    expect(f.submit).not.toHaveBeenCalled();
  });

  it('retains an uncertain send across closing and reopening, then retries the same identity', async () => {
    const f = fixture();
    f.submit.mockRejectedValueOnce(new Error('Response lost'));
    const page = f.view();
    const send = await screen.findByRole('button', { name: 'Send to the Guide' });
    fireEvent.change(screen.getByRole('textbox', { name: 'What would you like to work on?' }), {
      target: { value: 'What should we learn first?' },
    });
    await waitFor(() => expect(send).toHaveProperty('disabled', false));
    fireEvent.click(send);
    await screen.findByText(/Response lost/);
    page.unmount();
    f.view();
    const retry = await screen.findByRole('button', { name: 'Retry work request' });
    await waitFor(() => expect(retry).toHaveProperty('disabled', false));
    expect(screen.getByRole('textbox', { name: 'What would you like to work on?' })).toHaveProperty(
      'value',
      'What should we learn first?',
    );
    fireEvent.click(retry);
    await screen.findByRole('textbox', { name: 'Continue the conversation' });
    expect(f.submit).toHaveBeenCalledTimes(2);
    expect(f.submit.mock.calls[1]).toEqual(f.submit.mock.calls[0]);
  });

  it('keeps disconnected drafting local and does not manufacture a reply', async () => {
    const f = fixture();
    f.snapshot.mockRejectedValue(new Error('Engine unreachable'));
    f.view();
    fireEvent.click(screen.getByRole('button', { name: 'Shape work together →', exact: true }));
    await screen.findByText('Engine disconnected');
    fireEvent.change(screen.getByRole('textbox', { name: 'What are you working through?' }), {
      target: { value: 'Keep this thought until connected.' },
    });
    expect(screen.getByRole('button', { name: 'Start exploring with the Guide' })).toHaveProperty(
      'disabled',
      true,
    );
    expect(f.submit).not.toHaveBeenCalled();
    expect(screen.queryByText('What would a useful outcome look like?')).toBeNull();
  });
});
