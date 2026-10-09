import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { TeamWorkspace } from '../src/components/team-work/TeamWorkspace';
import { LocalEngineProvider } from '../src/context/LocalEngineContext';
import { LocalEngine } from '../src/engine/client';
import { type EngineTask, type EngineWorkspace } from '../src/engine/contracts';

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
  it('keeps execution issues visible without turning the conversation into a control panel', async () => {
    history.replaceState(null, '', `/#shape=${saved.id}`);
    const f = fixture([saved]);
    const content = {
      title: 'Compare our options',
      summary: 'Evaluate both approaches.',
      token_budget: 4000,
      open_questions: [],
      assignments: [
        {
          key: 'compare',
          title: 'Compare',
          agent_key: 'worker',
          instructions: 'Read the supplied notes',
          deliverable: 'Comparison',
          tools: [],
          depends_on: [],
          token_budget: 2000,
        },
      ],
    };
    vi.mocked(f.client.plan).mockResolvedValue({
      plans: [],
      brief_revision: 1,
      generation: null,
      readiness: [],
      execution_available: false,
      execution: {
        state: 'failed',
        error: 'The provider is unavailable. Your work is saved.',
        root: null,
        assignments: [],
        receipt: {
          source_work_id: saved.id,
          request_id: 'start',
          revision: 1,
          root_work_id: 'root',
          content,
          assignments: [
            {
              assignment_key: 'compare',
              work_id: 'child',
              agent_key: 'worker',
              definition_digest: 'pin',
            },
          ],
        },
      },
    });
    const start = vi.spyOn(f.client, 'startPlan');
    f.view();
    const summary = await screen.findByRole('region', { name: 'Team work summary' });
    expect(
      within(summary).getByText('The provider is unavailable. Your work is saved.'),
    ).toBeTruthy();
    expect(screen.queryByRole('region', { name: 'Team execution' })).toBeNull();
    const composer = screen.getByRole('textbox', { name: 'Continue the conversation' });
    fireEvent.change(composer, { target: { value: 'Keep discussing alternatives.' } });
    fireEvent.click(within(summary).getByRole('button', { name: 'Review issue' }));
    expect(screen.getByRole('region', { name: 'Team execution' })).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Back to conversation' }));
    expect(composer).toHaveProperty('value', 'Keep discussing alternatives.');
    expect(start).not.toHaveBeenCalled();
    expect(f.submit).not.toHaveBeenCalled();
  });

  it('retries an unstarted saved message without losing the next thought or creating another turn', async () => {
    const unstarted = { ...saved, state: 'not_started' as const, run_id: null, messages: [] };
    history.replaceState(null, '', `/#shape=${saved.id}`);
    const f = fixture([unstarted]);
    f.submit.mockRejectedValueOnce(new Error('The Guide is still occupied.'));
    f.view();
    const draft = await screen.findByRole('textbox', { name: 'Continue the conversation' });
    fireEvent.change(draft, { target: { value: 'Keep this thought for after your reply.' } });
    expect(screen.getByRole('button', { name: 'Send exploration reply' })).toHaveProperty(
      'disabled',
      true,
    );
    const retry = screen.getByRole('button', { name: 'Retry saved message' });
    fireEvent.click(retry);
    await screen.findByText('The Guide is still occupied.');
    expect(draft).toHaveProperty('value', 'Keep this thought for after your reply.');
    fireEvent.click(retry);
    await waitFor(() => expect(f.submit).toHaveBeenCalledTimes(2));
    expect(f.submit.mock.calls[0]).toEqual([
      saved.id,
      saved.input,
      guide.key,
      undefined,
      'explore',
    ]);
    expect(f.submit.mock.calls[1]).toEqual(f.submit.mock.calls[0]);
    expect(draft).toHaveProperty('value', 'Keep this thought for after your reply.');
  });

  it('keeps discussions and failed Guide replies out of work while retaining searchable conversations', async () => {
    const interrupted = {
      ...saved,
      id: 'interrupted',
      input: 'Another idea',
      state: 'failed' as const,
      error: 'Provider unavailable',
    };
    const f = fixture([saved, interrupted]);
    const start = vi.spyOn(f.client, 'startPlan');
    f.view();
    await screen.findByRole('button', { name: /Continue with the Guide/ });
    expect(screen.queryByRole('button', { name: /Needs you/ })).toBeNull();
    expect(
      within(screen.getByRole('region', { name: 'All projects map' })).queryByRole('button', {
        name: /^Open /,
      }),
    ).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Work', exact: true }));
    expect(screen.queryByText(saved.input)).toBeNull();
    expect(screen.queryByText('Another idea')).toBeNull();
    expect(screen.getByRole('button', { name: 'All 0' })).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Close', exact: true }));
    fireEvent.click(screen.getByRole('button', { name: 'Conversations' }));
    const history = screen.getByRole('dialog', { name: 'Guide conversations' });
    fireEvent.click(
      within(history).getByRole('button', { name: /Help me compare two approaches/ }),
    );
    await screen.findByRole('textbox', { name: 'Continue the conversation' });
    expect(location.hash).toBe(`#shape=${saved.id}`);
    expect(screen.getByText(saved.messages[0].content)).toBeTruthy();
    expect(f.submit).not.toHaveBeenCalled();
    expect(start).not.toHaveBeenCalled();
  });

  it('remembers the chosen conversation after returning to the map and reloading, with its draft and parent', async () => {
    history.replaceState(null, '', `/#shape=${saved.id}`);
    const f = fixture([saved, { ...saved, id: 'newer', input: 'A different idea' }]);
    const page = f.view();
    const message = await screen.findByRole('textbox', { name: 'Continue the conversation' });
    fireEvent.change(message, { target: { value: 'Let’s examine the first option.' } });
    fireEvent.click(screen.getByRole('button', { name: 'Minimize conversation' }));
    page.unmount();
    f.view();
    fireEvent.click(
      await screen.findByRole('button', { name: /Continue with the Guide Help me compare/ }),
    );
    expect(
      await screen.findByRole('textbox', { name: 'Continue the conversation' }),
    ).toHaveProperty('value', 'Let’s examine the first option.');
    fireEvent.click(screen.getByRole('button', { name: 'Send exploration reply' }));
    await waitFor(() =>
      expect(f.submit).toHaveBeenCalledExactlyOnceWith(
        expect.any(String),
        'Let’s examine the first option.',
        guide.key,
        saved.id,
        'explore',
      ),
    );
    expect(screen.queryByRole('button', { name: /Open Help me compare/ })).toBeNull();
  });

  it('starts a separate topic from Guide history without dispatching agents or losing the old discussion', async () => {
    history.replaceState(null, '', `/#shape=${saved.id}`);
    const f = fixture([saved]);
    const start = vi.spyOn(f.client, 'startPlan');
    f.view();
    await screen.findByRole('textbox', { name: 'Continue the conversation' });
    fireEvent.click(screen.getByRole('button', { name: 'Conversations' }));
    fireEvent.click(screen.getByRole('button', { name: 'New conversation' }));
    fireEvent.change(screen.getByRole('textbox', { name: 'What are you working through?' }), {
      target: { value: 'How should I think about this next idea?' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Start exploring with the Guide' }));
    await waitFor(() =>
      expect(f.submit).toHaveBeenCalledExactlyOnceWith(
        expect.any(String),
        'How should I think about this next idea?',
        guide.key,
        undefined,
        'explore',
      ),
    );
    expect(start).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Conversations' }));
    fireEvent.click(screen.getByRole('button', { name: /Help me compare two approaches/ }));
    expect(screen.getByText(saved.messages[0].content)).toBeTruthy();
  });

  it('minimizes a conversation on the map and resumes the same unsent thought', async () => {
    history.replaceState(null, '', '/#shape=saved-exploration');
    const f = fixture([saved]);
    f.view();
    const text = await screen.findByRole('textbox', { name: 'Continue the conversation' });
    fireEvent.change(text, { target: { value: 'I need room to think about this.' } });
    expect(screen.getByRole('region', { name: 'Work conversation' })).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Minimize conversation' }));
    expect(screen.queryByRole('region', { name: 'Work conversation' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: /Continue with the Guide/ }));
    expect(
      await screen.findByRole('textbox', { name: 'Continue the conversation' }),
    ).toHaveProperty('value', 'I need room to think about this.');
    expect(location.hash).toBe('#shape=saved-exploration');
    expect(f.submit).not.toHaveBeenCalled();
  });
  it('starts with the Guide and keeps agent selection optional on an empty map', async () => {
    const f = fixture();
    f.view();
    await screen.findByRole('heading', { name: 'What would you like to move forward?' });
    expect(screen.queryByRole('combobox', { name: 'Assign to agent' })).toBeNull();
    fireEvent.change(screen.getByRole('textbox', { name: 'What would you like to work on?' }), {
      target: { value: 'I am not sure what to bring here yet.' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Send to the Guide' }));
    await waitFor(() =>
      expect(f.submit).toHaveBeenCalledWith(
        expect.any(String),
        'I am not sure what to bring here yet.',
        guide.key,
        undefined,
        'explore',
      ),
    );
    expect(screen.queryByRole('button', { name: 'Prepare a plan' })).toBeNull();
  });
  it('presents a proposal after the open discussion and resolves questions in the same composer', async () => {
    const f = fixture([saved]);
    vi.mocked(f.client.plan).mockResolvedValue({
      plans: [
        {
          work_id: saved.id,
          revision: 3,
          brief_revision: 2,
          request_id: 'proposal',
          generation_id: 'generation',
          status: 'draft',
          created_by: 'owner',
          agreed_by: null,
          agreement_id: null,
          content: {
            title: 'A decision we can use',
            summary: 'Compare the source material and explain the tradeoffs.',
            token_budget: 4000,
            open_questions: ['Which audience is this for?'],
            assignments: [
              {
                key: 'compare',
                title: 'Assess the evidence',
                instructions: 'Use supplied sources.',
                agent_key: 'worker',
                tools: [],
                depends_on: [],
                deliverable: 'A sourced comparison',
                token_budget: 2000,
              },
            ],
          },
        },
      ],
      brief_revision: 2,
      generation: null,
      readiness: ['An assignment needs a working agent.'],
      execution_available: false,
    });
    const start = vi.spyOn(f.client, 'startPlan');
    history.replaceState(null, '', `/#shape=${saved.id}`);
    f.view();
    const planHeading = await screen.findByRole('heading', {
      name: 'A decision we can use',
      level: 3,
    });
    const reply = screen.getByText('What would a useful outcome look like?');
    expect(reply.closest('details')).toBeNull();
    expect(
      reply.compareDocumentPosition(planHeading) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    const composer = screen.getByRole('textbox', { name: 'Continue the conversation' });
    fireEvent.change(composer, { target: { value: 'Keep this thought.' } });
    expect(screen.queryByRole('button', { name: 'Start this plan' })).toBeNull();
    expect(screen.queryByText('Plan history · 1')?.closest('[hidden]')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Review proposal' }));
    fireEvent.click(screen.getByRole('button', { name: 'Work through this with the Guide' }));
    expect(composer).toHaveProperty('value', expect.stringContaining('Keep this thought.'));
    expect(composer).toHaveProperty(
      'value',
      expect.stringContaining('Which audience is this for?'),
    );
    expect(document.activeElement).toBe(composer);
    fireEvent.click(screen.getByRole('button', { name: 'Review proposal' }));
    expect(screen.getByRole('button', { name: 'Start this plan' })).toHaveProperty(
      'disabled',
      true,
    );
    expect(f.submit).not.toHaveBeenCalled();
    expect(start).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Back to conversation' }));
    expect(screen.queryByRole('button', { name: 'Start this plan' })).toBeNull();
    expect(composer).toHaveProperty('value', expect.stringContaining('Keep this thought.'));
  });

  it('returns from a saved Guide setup change to the same discussion with the draft intact', async () => {
    const f = fixture([saved]);
    const update = vi
      .spyOn(f.client, 'updateAgent')
      .mockResolvedValue({ ...guide, definition_digest: 'new-revision' });
    history.replaceState(null, '', `/#shape=${saved.id}`);
    f.view();
    const composer = await screen.findByRole('textbox', { name: 'Continue the conversation' });
    fireEvent.change(composer, { target: { value: 'Keep our direction.' } });
    fireEvent.click(screen.getByRole('button', { name: 'Guide model' }));
    const save = await screen.findByRole('button', { name: /Save/ });
    await waitFor(() => expect(save).toHaveProperty('disabled', false));
    fireEvent.click(save);
    expect(
      await screen.findByRole('textbox', { name: 'Continue the conversation' }),
    ).toHaveProperty('value', 'Keep our direction.');
    expect(update).toHaveBeenCalledTimes(1);
    expect(location.hash).toBe(`#shape=${saved.id}`);
    expect(f.submit).not.toHaveBeenCalled();
  });

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
      'Choose who helps you think and plan. Changes apply to new replies and teams you start.',
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
    fireEvent.click(await screen.findByRole('button', { name: /Continue with the Guide/ }));
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
