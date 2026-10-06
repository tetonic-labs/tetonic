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
    brief_revision: 0,
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
  it('uses the configured live Guide without passing example projects or assignments', async () => {
    const f = fixture();
    f.view();
    fireEvent.click(screen.getByRole('button', { name: 'Shape work together →', exact: true }));
    const send = screen.getByRole('button', { name: 'Start exploring with the Guide' });
    fireEvent.change(screen.getByRole('textbox', { name: 'What are you working through?' }), {
      target: { value: 'I need to understand our options before deciding.' },
    });
    await waitFor(() => expect(send).toHaveProperty('disabled', false));
    fireEvent.click(send);
    await screen.findByRole('tab', { name: 'Working brief' });
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
    fireEvent.click(screen.getByRole('tab', { name: 'Working brief' }));
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
    f.view();
    fireEvent.click(screen.getByRole('button', { name: 'Shape work together →', exact: true }));
    fireEvent.change(screen.getByRole('textbox', { name: 'What are you working through?' }), {
      target: { value: 'What should we learn first?' },
    });
    const send = screen.getByRole('button', { name: 'Start exploring with the Guide' });
    await waitFor(() => expect(send).toHaveProperty('disabled', false));
    fireEvent.click(send);
    await screen.findByText(/Response lost/);
    fireEvent.click(screen.getByRole('button', { name: 'Close project details' }));
    fireEvent.click(screen.getByRole('button', { name: 'Shape work together →', exact: true }));
    expect(screen.getByRole('textbox', { name: 'What are you working through?' })).toHaveProperty(
      'value',
      'What should we learn first?',
    );
    fireEvent.click(screen.getByRole('button', { name: 'Retry exploration message' }));
    await screen.findByRole('tab', { name: 'Working brief' });
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
