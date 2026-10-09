import { afterEach, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { LocalEngineProvider } from '../src/context/LocalEngineContext';
import { PlanReview } from '../src/components/team-work/PlanReview';
import { PlanExecution } from '../src/components/team-work/PlanExecution';
import { PlanReadiness } from '../src/components/team-work/PlanHandoff';
import { LocalEngine } from '../src/engine/client';
import { EngineRequestError } from '../src/engine/failure';
import {
  type HuddlePlan,
  type PlanView,
  type PlanExecutionView,
  type WorkUsage,
  type PlanContinuation,
} from '../src/engine/contracts';

const content = {
  title: 'Compare options',
  summary: 'Compare the supplied options and check assumptions.',
  token_budget: 4000,
  open_questions: [],
  assignments: [
    {
      key: 'compare',
      title: 'Compare formats',
      instructions: 'Use supplied evidence.',
      agent_key: 'worker',
      depends_on: [],
      tools: [],
      deliverable: 'A comparison',
      token_budget: 2000,
    },
    {
      key: 'review',
      title: 'Check assumptions',
      instructions: 'Challenge the comparison.',
      agent_key: 'reviewer',
      depends_on: ['compare'],
      tools: [],
      deliverable: 'Open risks',
      token_budget: 2000,
    },
  ],
};
const plan: HuddlePlan = {
  work_id: 'shape',
  revision: 1,
  brief_revision: 2,
  request_id: 'generate',
  generation_id: 'generation',
  status: 'draft',
  content,
  created_by: 'owner',
  agreed_by: null,
  agreement_id: null,
};
function fixture(
  initial: PlanView = {
    plans: [],
    generation: null,
    brief_revision: 2,
    readiness: [],
    execution_available: false,
  },
  usage: WorkUsage[] = [],
) {
  const client = new LocalEngine('test');
  let state = structuredClone(initial);
  vi.spyOn(client, 'snapshot').mockResolvedValue({
    organization: 'Org',
    team_id: 'team',
    team_name: 'Team',
    agent_id: 'one',
    agent_name: 'Worker',
    shaping_agent_key: 'guide',
    model: 'test',
    input_limit: 12000,
    tasks: [],
    usage,
    agents: ['worker', 'reviewer'].map((key) => ({
      key,
      id: key,
      name: key,
      purpose: 'Work',
      model: 'test',
      harness: 'general',
      max_steps: 8,
      max_seconds: 60,
      max_tokens: 4000,
      tools: [],
    })),
  });
  vi.spyOn(client, 'agentCatalog').mockResolvedValue({
    models: ['test'],
    harnesses: ['general'],
    max_steps: 8,
    max_seconds: 60,
    max_tokens: 4000,
  });
  vi.spyOn(client, 'workItems').mockResolvedValue([]);
  vi.spyOn(client, 'teams').mockResolvedValue([]);
  vi.spyOn(client, 'approvals').mockResolvedValue({
    active_stops: [],
    pending_approvals: [],
    effort: [],
  });
  vi.spyOn(client, 'plan').mockImplementation(async () => structuredClone(state));
  const submit = vi.spyOn(client, 'submit');
  const update = vi.spyOn(client, 'updatePlan').mockImplementation(async (_id, command) => {
    const next =
      command.action === 'generate' || command.action === 'prepare'
        ? {
            ...plan,
            revision: command.expected_revision + 1,
            brief_revision:
              command.action === 'prepare'
                ? command.expected_brief_revision + 1
                : command.brief_revision,
            request_id: command.request_id,
            status: 'drafting' as const,
            content: null,
          }
        : command.action === 'capture'
          ? { ...state.plans[0], content, status: 'draft' as const }
          : command.action === 'agree'
            ? {
                ...state.plans[0],
                status: 'agreed' as const,
                agreement_id: command.request_id,
                agreed_by: 'owner',
              }
            : {
                ...plan,
                content: command.content,
                revision: command.expected_revision + 1,
                brief_revision: command.brief_revision,
                request_id: command.request_id,
              };
    state = {
      ...state,
      plans: [next],
      brief_revision: next.brief_revision,
      generation:
        command.action === 'generate' || command.action === 'prepare'
          ? {
              id: 'generation',
              input: 'Host-composed prompt',
              agent_key: 'guide',
              agent_name: 'Guide',
              state: 'completed',
              run_id: 'r',
              sequence: 2,
              messages: [],
            }
          : null,
    };
    return next;
  });
  return {
    client,
    submit,
    update,
    set: (v: Partial<PlanView>) => {
      state = { ...state, ...v };
    },
    render: (suggestion?: string, onAgentSettings?: (key: string) => void) =>
      render(
        <LocalEngineProvider client={client}>
          <PlanReview workId="shape" suggestion={suggestion} onAgentSettings={onAgentSettings} />
        </LocalEngineProvider>,
      ),
  };
}
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  sessionStorage.clear();
  window.history.replaceState(null, '', '/');
});

it('recovers a failed plan read automatically without submitting or launching work', async () => {
  vi.useFakeTimers();
  const f = fixture({
    plans: [plan],
    generation: null,
    brief_revision: 2,
    readiness: [],
    execution_available: true,
  });
  vi.mocked(f.client.plan).mockRejectedValueOnce(new Error('Temporary read failure'));
  const start = vi.spyOn(f.client, 'startPlan');
  f.render();
  await act(async () => {});
  expect(screen.getByRole('status').textContent).toContain('Retrying');
  await act(async () => {
    await vi.advanceTimersByTimeAsync(2000);
  });
  expect(screen.getByRole('heading', { name: 'Compare formats' })).toBeTruthy();
  expect(screen.queryByText(/Retrying/)).toBeNull();
  expect(f.update).not.toHaveBeenCalled();
  expect(f.submit).not.toHaveBeenCalled();
  expect(start).not.toHaveBeenCalled();
});

it('keeps the last plan visible through a read failure and stops retries when closed', async () => {
  vi.useFakeTimers();
  const f = fixture({
    plans: [plan],
    generation: null,
    brief_revision: 2,
    readiness: [],
    execution_available: true,
  });
  const page = render(
    <LocalEngineProvider client={f.client}>
      <PlanReview workId="shape" conversationActive />
    </LocalEngineProvider>,
  );
  await act(async () => {});
  vi.mocked(f.client.plan).mockRejectedValue(new Error('Temporary read failure'));
  await act(async () => {
    await vi.advanceTimersByTimeAsync(2000);
  });
  expect(screen.getByRole('heading', { name: 'Compare formats' })).toBeTruthy();
  expect(screen.getByRole('status').textContent).toContain('last saved plan');
  page.unmount();
  const reads = vi.mocked(f.client.plan).mock.calls.length;
  await act(async () => {
    await vi.advanceTimersByTimeAsync(30000);
  });
  expect(f.client.plan).toHaveBeenCalledTimes(reads);
});

it('backs off repeated failed reads and allows an immediate manual refresh', async () => {
  vi.useFakeTimers();
  const f = fixture();
  vi.mocked(f.client.plan).mockRejectedValue(new Error('Offline'));
  f.render();
  await act(async () => {});
  expect(f.client.plan).toHaveBeenCalledTimes(1);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(2000);
  });
  expect(f.client.plan).toHaveBeenCalledTimes(2);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(2000);
  });
  expect(f.client.plan).toHaveBeenCalledTimes(2);
  fireEvent.click(screen.getByRole('button', { name: 'Retry plan refresh' }));
  await act(async () => {});
  expect(f.client.plan).toHaveBeenCalledTimes(3);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(8000);
  });
  expect(f.client.plan).toHaveBeenCalledTimes(4);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(14999);
  });
  expect(f.client.plan).toHaveBeenCalledTimes(4);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1);
  });
  expect(f.client.plan).toHaveBeenCalledTimes(5);
});

it('does not erase or replay an unconfirmed plan edit when a background read succeeds', async () => {
  vi.useFakeTimers();
  const f = fixture({
    plans: [plan],
    generation: null,
    brief_revision: 2,
    readiness: [],
    execution_available: true,
  });
  f.update.mockRejectedValue(new Error('Save response was lost'));
  const page = f.render();
  await act(async () => {});
  fireEvent.click(screen.getByRole('button', { name: 'Adjust plan' }));
  fireEvent.change(screen.getByRole('textbox', { name: 'Outcome' }), {
    target: { value: 'My unsaved direction' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Save plan revision' }));
  await act(async () => {});
  page.rerender(
    <LocalEngineProvider client={f.client}>
      <PlanReview workId="shape" conversationActive />
    </LocalEngineProvider>,
  );
  await act(async () => {
    await vi.advanceTimersByTimeAsync(2000);
  });
  expect(screen.getByRole('alert').textContent).toBe('Save response was lost');
  expect(screen.getByRole('textbox', { name: 'Outcome' })).toHaveProperty(
    'value',
    'My unsaved direction',
  );
  expect(f.update).toHaveBeenCalledTimes(1);
  expect(screen.getByRole('button', { name: 'Retry plan operation' })).toBeTruthy();
});

it('offers the specific agent access and workspace connection fixes without granting or starting work', async () => {
  const view: PlanView = {
    plans: [
      {
        ...plan,
        content: {
          ...content,
          assignments: [{ ...content.assignments[0], tools: ['mcp_calendar_read'] }],
        },
      },
    ],
    brief_revision: 2,
    generation: null,
    readiness: ['The assignment needs access to calendar information.'],
    execution_available: false,
  };
  const f = fixture(view);
  const settings = vi.fn();
  const tools = vi.fn();
  const discuss = vi.fn();
  const start = vi.spyOn(f.client, 'startPlan');
  render(
    <LocalEngineProvider client={f.client}>
      <PlanReadiness view={view} onAgentSettings={settings} onTools={tools} onDiscuss={discuss} />
    </LocalEngineProvider>,
  );
  fireEvent.click(await screen.findByRole('button', { name: 'Review worker’s access' }));
  expect(settings).toHaveBeenCalledWith('worker');
  fireEvent.click(screen.getByRole('button', { name: 'Add tools or a connection' }));
  expect(tools).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole('button', { name: 'Work through this with the Guide' }));
  expect(discuss).toHaveBeenCalledWith(expect.stringContaining(view.readiness[0]));
  expect(f.submit).not.toHaveBeenCalled();
  expect(start).not.toHaveBeenCalled();
});

const continuation: PlanContinuation = {
  source_work_id: 'shape',
  root_work_id: 'old-root',
  continuation_work_id: 'follow-up',
  request_id: 'prepare',
  created_by: 'owner',
  retained: [{ work_id: 'compare', title: 'Compare formats' }],
  review_before_repeat: [{ work_id: 'review', title: 'Check assumptions' }],
};

it('prepares only a reviewable continuation and reuses an uncertain command after reopening', async () => {
  const f = fixture({
    plans: [{ ...plan, status: 'agreed' }],
    generation: null,
    brief_revision: 2,
    readiness: [],
    execution_available: false,
    execution: {
      receipt: {
        source_work_id: 'shape',
        root_work_id: 'old-root',
        request_id: 'old-start',
        revision: 1,
        content,
        assignments: [],
      },
      state: 'failed',
      root: null,
      assignments: [],
      error: 'A worker failed',
    },
    recovery: { available: true, reason: null, retained_count: 1, unfinished_count: 1 },
  });
  const start = vi.spyOn(f.client, 'startPlan');
  const prepare = vi
    .spyOn(f.client, 'continuePlan')
    .mockRejectedValueOnce(new Error('Proposal response lost'))
    .mockResolvedValueOnce(continuation);
  const page = f.render();
  const button = await screen.findByRole('button', { name: 'Review unfinished work' });
  fireEvent.click(button);
  fireEvent.click(button);
  await screen.findByText('Proposal response lost');
  expect(prepare).toHaveBeenCalledTimes(1);
  const request = prepare.mock.calls[0][1];
  expect(request.expected_root_work_id).toBe('old-root');
  page.unmount();
  f.render();
  fireEvent.click(await screen.findByRole('button', { name: 'Check this proposal again' }));
  await screen.findByRole('link', { name: 'Open continuation →' });
  expect(prepare.mock.calls[1][1]).toEqual(request);
  await waitFor(() => expect(window.location.hash).toBe('#shape=follow-up'));
  expect(start).not.toHaveBeenCalled();
  expect(f.update).not.toHaveBeenCalled();
  expect(f.submit).not.toHaveBeenCalled();
});

it('explains a stopped-run prerequisite without offering a duplicate dispatch', async () => {
  const f = fixture({
    plans: [plan],
    generation: null,
    brief_revision: 2,
    readiness: [],
    execution_available: false,
    execution: {
      receipt: {
        source_work_id: 'shape',
        root_work_id: 'old-root',
        request_id: 'old',
        revision: 1,
        content,
        assignments: [],
      },
      state: 'recovery_required',
      root: null,
      assignments: [],
      error: null,
    },
    recovery: {
      available: false,
      reason: 'The earlier run has not fully stopped.',
      retained_count: 1,
      unfinished_count: 1,
    },
  });
  f.render();
  await screen.findByText('The earlier run has not fully stopped.');
  expect(screen.getByRole('button', { name: 'Review unfinished work' })).toHaveProperty(
    'disabled',
    true,
  );
  expect(f.submit).not.toHaveBeenCalled();
});

it('requires review of earlier tool actions, resets on plan edits, and retains it for uncertain starts', async () => {
  const view: PlanView = {
    plans: [plan],
    generation: null,
    brief_revision: 2,
    readiness: [],
    execution_available: true,
    continuation_from: continuation,
  };
  const f = fixture(view);
  const start = vi.spyOn(f.client, 'startPlan').mockRejectedValue(new Error('Start reply lost'));
  const element = (v: PlanView) => (
    <LocalEngineProvider client={f.client}>
      <PlanExecution workId="follow-up" view={v} refresh={async () => {}} />
    </LocalEngineProvider>
  );
  const page = render(element(view));
  await waitFor(() => expect(screen.getByRole('checkbox')).toHaveProperty('disabled', false));
  expect(screen.getByRole('button', { name: 'Start this plan' })).toHaveProperty('disabled', true);
  expect(screen.getByRole('link', { name: 'Inspect Check assumptions' }).getAttribute('href')).toBe(
    '#work=review&inspect=1',
  );
  fireEvent.click(screen.getByRole('checkbox'));
  await waitFor(() =>
    expect(screen.getByRole('button', { name: 'Start this plan' })).toHaveProperty(
      'disabled',
      false,
    ),
  );
  const revised = {
    ...view,
    plans: [{ ...plan, work_id: 'follow-up', revision: 2, status: 'agreed' as const }],
  };
  page.rerender(element(revised));
  expect(screen.getByRole('checkbox')).toHaveProperty('checked', false);
  expect(screen.getByRole('button', { name: 'Start this plan' })).toHaveProperty('disabled', true);
  fireEvent.click(screen.getByRole('checkbox'));
  fireEvent.click(screen.getByRole('button', { name: 'Start this plan' }));
  await screen.findByText('Start reply lost');
  expect(start.mock.calls[0][1]).toMatchObject({ revision: 2, reviewed_previous_actions: true });
  page.unmount();
  render(element(revised));
  fireEvent.click(await screen.findByRole('button', { name: 'Check this start again' }));
  await waitFor(() => expect(start).toHaveBeenCalledTimes(2));
  expect(start.mock.calls[1][1]).toEqual(start.mock.calls[0][1]);
});

it('reviews the coordination destination and retains consent with an uncertain start', async () => {
  const coordinator = { provider: 'openai', model: 'reviewed-model' };
  const f = fixture({
    plans: [plan],
    generation: null,
    brief_revision: 2,
    readiness: [],
    execution_available: true,
    coordinator,
  });
  const start = vi.spyOn(f.client, 'startPlan').mockRejectedValue(new Error('Response lost'));
  const settings = vi.fn();
  const page = f.render(undefined, settings);
  const button = await screen.findByRole('button', { name: 'Start this plan' });
  expect(button).toHaveProperty('disabled', true);
  fireEvent.click(screen.getByRole('button', { name: 'Change model' }));
  expect(settings).toHaveBeenCalledWith('guide');
  fireEvent.click(screen.getByRole('checkbox', { name: /Allow the shared brief/ }));
  fireEvent.click(button);
  await screen.findByText('Response lost');
  const request = start.mock.calls[0][1];
  expect(request).toMatchObject({ coordinator, hosted_coordination_consent: true });
  expect(f.update.mock.calls[0][1]).toEqual({
    action: 'agree',
    request_id: request.request_id,
    revision: 1,
  });
  page.unmount();
  // Another tab may change the Guide after an uncertain response. Retrying must
  // neither display the new destination nor silently send it the old consent.
  f.set({ coordinator: { provider: 'google', model: 'later-model' } });
  f.render();
  fireEvent.click(await screen.findByRole('button', { name: 'Check this start again' }));
  await waitFor(() => expect(start).toHaveBeenCalledTimes(2));
  expect(start.mock.calls[1][1]).toEqual(request);
  expect(screen.getByText(/Coordination: reviewed-model/)).toBeTruthy();
  expect(screen.queryByText(/Coordination: later-model/)).toBeNull();
});

it('requires fresh consent when the coordination model changes before starting', async () => {
  const view: PlanView = {
    plans: [plan],
    generation: null,
    brief_revision: 2,
    readiness: [],
    execution_available: true,
    coordinator: { provider: 'openai', model: 'first' },
  };
  const f = fixture(view);
  const element = (v: PlanView) => (
    <LocalEngineProvider client={f.client}>
      <PlanExecution workId="shape" view={v} refresh={async () => {}} />
    </LocalEngineProvider>
  );
  const page = render(element(view));
  await waitFor(() => expect(screen.getByRole('checkbox')).toHaveProperty('disabled', false));
  fireEvent.click(screen.getByRole('checkbox'));
  await waitFor(() =>
    expect(screen.getByRole('button', { name: 'Start this plan' })).toHaveProperty(
      'disabled',
      false,
    ),
  );
  page.rerender(element({ ...view, coordinator: { provider: 'anthropic', model: 'second' } }));
  expect(screen.getByRole('checkbox')).toHaveProperty('checked', false);
  expect(screen.getByRole('button', { name: 'Start this plan' })).toHaveProperty('disabled', true);
});

it('takes a blocked plan directly to the agent that needs setup', async () => {
  const issue = { agent_key: 'reviewer', message: 'Reviewer needs its provider key.' };
  const f = fixture({
    plans: [plan],
    generation: null,
    brief_revision: 2,
    readiness: [issue.message],
    setup_issues: [issue],
    execution_available: false,
    coordinator: { provider: 'ollama', model: 'local-model' },
  });
  const settings = vi.fn();
  f.render(undefined, settings);
  fireEvent.click(await screen.findByRole('button', { name: 'Review agent setup' }));
  expect(settings).toHaveBeenCalledWith('reviewer');
  expect(screen.getByRole('button', { name: 'Start this plan' })).toHaveProperty('disabled', true);
  expect(f.submit).not.toHaveBeenCalled();
});

it('shows a proposal saved by the Guide when its reply finishes, without a prepare click or launch', async () => {
  const f = fixture();
  const start = vi.spyOn(f.client, 'startPlan');
  const element = (active: boolean) => (
    <LocalEngineProvider client={f.client}>
      <PlanReview workId="shape" conversationActive={active} />
    </LocalEngineProvider>
  );
  const page = render(element(true));
  await waitFor(() => expect(f.client.plan).toHaveBeenCalled());
  expect(screen.queryByRole('button', { name: 'Prepare a plan' })).toBeNull();
  f.set({ plans: [plan], execution_available: true });
  page.rerender(element(false));
  await screen.findByRole('heading', { name: 'Compare formats' });
  expect(screen.getByRole('button', { name: 'Start this plan' })).toHaveProperty('disabled', false);
  expect(f.update).not.toHaveBeenCalled();
  expect(start).not.toHaveBeenCalled();
  expect(f.submit).not.toHaveBeenCalled();
});

it('keeps unsaved owner edits when a Guide reply saves a newer proposal', async () => {
  const f = fixture({
    plans: [plan],
    generation: null,
    brief_revision: 2,
    readiness: [],
    execution_available: true,
  });
  const element = (active: boolean) => (
    <LocalEngineProvider client={f.client}>
      <PlanReview workId="shape" conversationActive={active} />
    </LocalEngineProvider>
  );
  const page = render(element(false));
  fireEvent.click(await screen.findByRole('button', { name: 'Adjust plan' }));
  fireEvent.change(screen.getByRole('textbox', { name: 'Outcome' }), {
    target: { value: 'Keep my unsaved direction' },
  });
  page.rerender(element(true));
  f.set({
    plans: [
      {
        ...plan,
        revision: 2,
        brief_revision: 3,
        content: { ...content, summary: 'The new proposal' },
      },
    ],
    brief_revision: 3,
  });
  page.rerender(element(false));
  await screen.findByText(/A newer plan or brief is saved/);
  expect(screen.getByRole('textbox', { name: 'Outcome' })).toHaveProperty(
    'value',
    'Keep my unsaved direction',
  );
  expect(screen.getByRole('button', { name: 'Save plan revision' })).toHaveProperty(
    'disabled',
    true,
  );
  expect(f.update).not.toHaveBeenCalled();
  expect(f.submit).not.toHaveBeenCalled();
});

it('starts the agreed plan through its own engine door and reuses an uncertain request', async () => {
  const f = fixture({
    plans: [{ ...plan, status: 'agreed' }],
    generation: null,
    brief_revision: 2,
    readiness: [],
    execution_available: true,
  });
  const start = vi
    .spyOn(f.client, 'startPlan')
    .mockRejectedValueOnce(new Error('Start response lost'));
  const first = f.render();
  fireEvent.click(await screen.findByRole('button', { name: 'Start this plan' }));
  await screen.findByText('Start response lost');
  const request = start.mock.calls[0][1];
  first.unmount();
  const execution: PlanExecutionView = {
    receipt: {
      source_work_id: 'shape',
      request_id: request.request_id,
      revision: 1,
      root_work_id: 'root',
      content,
      assignments: content.assignments.map((a) => ({
        assignment_key: a.key,
        work_id: a.key,
        agent_key: a.agent_key,
        definition_digest: 'pinned',
      })),
    },
    state: 'running',
    root: {
      id: 'root',
      input: 'Shared brief',
      agent_key: 'coordinator',
      agent_name: 'Coordinator',
      state: 'running',
      run_id: 'run',
      sequence: 1,
      messages: [],
    },
    assignments: [],
    error: null,
  };
  start.mockImplementationOnce(async () => {
    f.set({ execution, execution_available: false });
    return execution;
  });
  f.render();
  fireEvent.click(await screen.findByRole('button', { name: 'Check this start again' }));
  await screen.findByRole('heading', { name: 'Your team at work' });
  expect(start.mock.calls[1][1]).toEqual(request);
  expect(f.submit).not.toHaveBeenCalled();
  fireEvent.click(screen.getByText('Compare formats'));
  expect(screen.getByRole('link', { name: 'Inspect Compare formats' }).getAttribute('href')).toBe(
    '#work=compare',
  );
  expect(screen.getByRole('button', { name: 'Stop this plan' })).toBeTruthy();
});

it('allows a corrected start after an explicit rejection and shows the whole-plan deadline', async () => {
  const f = fixture({
    plans: [{ ...plan, status: 'agreed' }],
    generation: null,
    brief_revision: 2,
    readiness: [],
    execution_available: true,
    execution_max_seconds: 360,
  });
  const start = vi
    .spyOn(f.client, 'startPlan')
    .mockRejectedValue(new EngineRequestError('The brief changed. Review it again.', 400));
  f.render();
  const button = await screen.findByRole('button', { name: 'Start this plan' });
  expect(screen.getByText(/Up to 6 minutes for the whole plan/)).toBeTruthy();
  fireEvent.click(button);
  await screen.findByText('The brief changed. Review it again.');
  const first = start.mock.calls[0][1].request_id;
  await waitFor(() =>
    expect(screen.getByRole('button', { name: 'Start this plan' })).toHaveProperty(
      'disabled',
      false,
    ),
  );
  fireEvent.click(screen.getByRole('button', { name: 'Start this plan' }));
  await waitFor(() => expect(start).toHaveBeenCalledTimes(2));
  expect(start.mock.calls[1][1].request_id).not.toBe(first);
});

it('does not present a partial coordinator answer as a completed team result', async () => {
  const f = fixture({
    plans: [{ ...plan, status: 'agreed' }],
    generation: null,
    brief_revision: 2,
    readiness: [],
    execution_available: false,
    execution: {
      receipt: {
        source_work_id: 'shape',
        request_id: 'start',
        revision: 1,
        root_work_id: 'root',
        content,
        assignments: [
          {
            assignment_key: 'compare',
            work_id: 'compare',
            agent_key: 'worker',
            definition_digest: 'pinned',
          },
        ],
      },
      state: 'failed',
      root: {
        id: 'root',
        input: 'Shared brief',
        agent_key: 'coordinator',
        agent_name: 'Coordinator',
        state: 'failed',
        run_id: 'run',
        sequence: 1,
        messages: [{ id: 1, role: 'assistant', content: 'UNVERIFIED_COMPLETION' }],
      },
      assignments: [],
      error: 'A contribution did not complete.',
    },
  });
  f.render();
  await screen.findByText('A contribution did not complete.');
  expect(screen.queryByText('UNVERIFIED_COMPLETION')).toBeNull();
  expect(screen.queryByRole('button', { name: 'Start this plan' })).toBeNull();
  expect(screen.getByText(/Recorded contributions are kept/)).toBeTruthy();
  fireEvent.click(screen.getByText('What the team was given'));
  expect(screen.getByText('The original brief is unavailable for this saved run.')).toBeTruthy();
});

it('distinguishes active workers, dependency waits, and execution without a recorded result', async () => {
  const execution: PlanExecutionView = {
    receipt: {
      source_work_id: 'shape',
      request_id: 'start',
      revision: 1,
      root_work_id: 'root',
      content,
      assignments: content.assignments.map((a) => ({
        assignment_key: a.key,
        work_id: a.key,
        agent_key: a.agent_key,
        definition_digest: 'pinned',
      })),
    },
    state: 'running',
    root: null,
    error: null,
    assignments: content.assignments.map((a) => ({
      id: a.key,
      input: a.instructions,
      agent_key: a.agent_key,
      agent_name: a.agent_key,
      state: a.depends_on.length ? 'not_started' : 'running',
      run_id: null,
      sequence: 1,
      messages: [],
    })),
  };
  const view: PlanView = {
    plans: [plan],
    generation: null,
    brief_revision: 2,
    readiness: [],
    execution_available: false,
    execution,
  };
  const f = fixture(view);
  const element = () => (
    <LocalEngineProvider client={f.client}>
      <PlanExecution workId="shape" view={view} refresh={async () => {}} />
    </LocalEngineProvider>
  );
  const page = render(element());
  await screen.findByText('Working now: worker.');
  expect(screen.getByText('Waiting for Compare formats')).toBeTruthy();
  execution.assignments[0].state = 'completed';
  page.rerender(element());
  expect(screen.queryByText('Waiting for Compare formats')).toBeNull();
  expect(screen.getByText('Waiting to start')).toBeTruthy();
  execution.assignments.forEach((task) => {
    task.state = 'running';
  });
  page.rerender(element());
  expect(screen.getByText('Working now: worker · reviewer.')).toBeTruthy();
  execution.state = 'completed';
  execution.assignments.forEach((task) => {
    task.state = 'completed';
  });
  page.rerender(element());
  expect(screen.getByText(/Execution finished · no combined response recorded/)).toBeTruthy();
  expect(screen.queryByRole('heading', { name: 'Your team’s result' })).toBeNull();
  expect(screen.queryByText(/Working now:/)).toBeNull();
});

it('shows the exact execution brief beside the result even after the working brief changes', async () => {
  const f = fixture({
    plans: [{ ...plan, status: 'agreed', brief_revision: 3 }],
    generation: null,
    brief_revision: 3,
    readiness: [],
    execution_available: false,
    execution: {
      receipt: {
        source_work_id: 'shape',
        request_id: 'start',
        revision: 1,
        brief_revision: 2,
        brief: '## Source S1\nThe original supplied evidence.',
        root_work_id: 'root',
        content,
        assignments: [],
      },
      state: 'completed',
      assignments: [],
      error: null,
      root: {
        id: 'root',
        input: 'Host-composed context',
        agent_key: 'coordinator',
        agent_name: 'Coordinator',
        state: 'completed',
        run_id: 'run',
        sequence: 2,
        messages: [{ id: 1, role: 'assistant', content: 'A source-backed recommendation.' }],
      },
    },
  });
  f.render();
  await screen.findByText('A source-backed recommendation.');
  const disclosure = screen.getByText('What the team was given').closest('details')!;
  expect(disclosure.open).toBe(false);
  fireEvent.click(screen.getByText('What the team was given'));
  expect(screen.getByText(/Brief 2, saved when this team started/)).toBeTruthy();
  expect(screen.getByRole('heading', { name: 'Source S1' })).toBeTruthy();
  expect(screen.getByText('The original supplied evidence.')).toBeTruthy();
  expect(f.submit).not.toHaveBeenCalled();
  expect(f.update).not.toHaveBeenCalled();
});

it('explains exhausted coordination even while the plan total has unused allowance', async () => {
  const scopedContent = {
    ...content,
    token_budget: 11096,
    assignments: content.assignments.map((a) => ({ ...a, token_budget: 3500 })),
  };
  const row = (
    work_id: string,
    used: number,
    token_limit: number,
    delegated_tokens: number,
  ): WorkUsage => ({
    work_id,
    title: work_id,
    purpose: 'work',
    budget: {
      token_limit,
      delegated_tokens,
      reserved_tokens: used,
      available_tokens: 0,
      root_work_id: 'root',
    },
    input_tokens: used,
    output_tokens: 0,
    calls: 1,
    pending_calls: 0,
    unknown_calls: 0,
    held_tokens: 0,
    released_tokens: 0,
    over_limit: used > token_limit - delegated_tokens,
  });
  const f = fixture(
    {
      plans: [{ ...plan, content: scopedContent, status: 'agreed' }],
      generation: null,
      brief_revision: 2,
      readiness: [],
      execution_available: false,
      execution: {
        receipt: {
          source_work_id: 'shape',
          request_id: 'start',
          revision: 1,
          root_work_id: 'root',
          content: scopedContent,
          assignments: scopedContent.assignments.map((a) => ({
            assignment_key: a.key,
            work_id: a.key,
            agent_key: a.agent_key,
            definition_digest: 'pinned',
          })),
        },
        state: 'failed',
        assignments: [],
        error: 'Work stopped at its token allowance.',
        root: {
          id: 'root',
          input: 'Shared brief',
          agent_key: 'coordinator',
          agent_name: 'Coordinator',
          state: 'failed',
          run_id: 'run',
          sequence: 2,
          messages: [],
        },
      },
    },
    [row('root', 4681, 11096, 7000), row('compare', 1596, 3500, 0), row('review', 1983, 3500, 0)],
  );
  f.render();
  await screen.findByText(/Coordination used 4,681 tokens against its 4,096 allowance/);
  expect(screen.getByText(/8,260 \/ 11,096 tokens reported/)).toBeTruthy();
  expect(screen.getByText(/4,681 reported \/ 4,096 allowed/)).toBeTruthy();
  expect(screen.getByText(/1,596 reported \/ 3,500 allowed/)).toBeTruthy();
  expect(f.submit).not.toHaveBeenCalled();
});

it('prepares and captures a proposal inline without dispatching assignments', async () => {
  const f = fixture();
  f.render();
  fireEvent.click(await screen.findByRole('button', { name: 'Plan work from this discussion' }));
  fireEvent.click(screen.getByRole('button', { name: 'Prepare a plan' }));
  await screen.findByRole('heading', { name: 'Compare formats' });
  expect(screen.getByText('After: Compare formats')).toBeTruthy();
  expect(f.update.mock.calls.map((c) => c[1].action)).toEqual(['generate', 'capture']);
  expect(screen.getByRole('button', { name: 'Start this plan' })).toBeTruthy();
  expect(f.submit).not.toHaveBeenCalled();
});

it('retains an uncertain operation across closing the overlay and retries its identity', async () => {
  const f = fixture();
  f.update.mockRejectedValueOnce(new Error('Lost response'));
  const first = f.render();
  fireEvent.click(await screen.findByRole('button', { name: 'Plan work from this discussion' }));
  fireEvent.click(screen.getByRole('button', { name: 'Prepare a plan' }));
  await screen.findByText('Lost response');
  const original = f.update.mock.calls[0][1];
  first.unmount();
  f.render();
  fireEvent.click(await screen.findByRole('button', { name: 'Retry plan operation' }));
  await waitFor(() => expect(f.update.mock.calls.length).toBeGreaterThanOrEqual(2));
  expect(f.update.mock.calls[1][1]).toEqual(original);
  await screen.findByRole('heading', { name: 'Compare formats' });
});

it('keeps edits and blocks stale agreement until current brief and plan are reviewed', async () => {
  const f = fixture({
    plans: [plan],
    generation: null,
    brief_revision: 2,
    readiness: [],
    execution_available: false,
  });
  const first = f.render();
  fireEvent.click(await screen.findByRole('button', { name: 'Adjust plan' }));
  fireEvent.change(screen.getByRole('textbox', { name: 'Outcome' }), {
    target: { value: 'My changed outcome' },
  });
  first.unmount();
  f.set({ brief_revision: 3 });
  f.render();
  await waitFor(() =>
    expect(screen.getByRole('textbox', { name: 'Outcome' })).toHaveProperty(
      'value',
      'My changed outcome',
    ),
  );
  expect(screen.getByRole('button', { name: 'Save plan revision' })).toHaveProperty(
    'disabled',
    true,
  );
  fireEvent.click(screen.getByRole('button', { name: 'Apply edits against current revisions' }));
  fireEvent.click(screen.getByRole('button', { name: 'Save plan revision' }));
  await screen.findByText('Plan 2 · brief 3');
  expect(f.update.mock.calls[0][1]).toMatchObject({
    action: 'revise',
    expected_revision: 1,
    brief_revision: 3,
    content: { title: 'My changed outcome' },
  });
  expect(f.submit).not.toHaveBeenCalled();
});

it('does not treat a mismatched agreement receipt as success', async () => {
  const f = fixture({
    plans: [plan],
    generation: null,
    brief_revision: 2,
    readiness: [],
    execution_available: false,
  });
  f.update.mockResolvedValueOnce({ ...plan, status: 'agreed', agreement_id: 'wrong' });
  const start = vi.spyOn(f.client, 'startPlan');
  f.render();
  fireEvent.click(await screen.findByRole('button', { name: 'Start this plan' }));
  await screen.findByText('Agreement was not confirmed. Retry the same start.');
  expect(screen.queryByText('Direction agreed · not started')).toBeNull();
  expect(screen.getByRole('button', { name: 'Check this start again' })).toBeTruthy();
  expect(start).not.toHaveBeenCalled();
});

it('requires a saved brief and performs no planning on read', async () => {
  const f = fixture({
    plans: [],
    generation: null,
    brief_revision: 0,
    readiness: [],
    execution_available: false,
  });
  f.render();
  expect(screen.queryByRole('button', { name: 'Prepare a plan' })).toBeNull();
  fireEvent.click(await screen.findByRole('button', { name: 'Plan work from this discussion' }));
  expect(screen.getByRole('button', { name: 'Prepare a plan' })).toHaveProperty('disabled', true);
  expect(f.update).not.toHaveBeenCalled();
  expect(f.submit).not.toHaveBeenCalled();
});

it('carries reviewed direction into planning and agrees and dispatches with one recoverable start', async () => {
  const f = fixture({
    plans: [],
    generation: null,
    brief_revision: 0,
    readiness: [],
    execution_available: false,
  });
  const start = vi
    .spyOn(f.client, 'startPlan')
    .mockRejectedValueOnce(new Error('Start response lost'));
  const first = f.render('Investigate the supplied options with two independent contributors.');
  fireEvent.click(await screen.findByRole('button', { name: 'Plan work from this discussion' }));
  expect(screen.getByRole('textbox', { name: 'Direction for the team' })).toBeTruthy();
  fireEvent.change(screen.getByRole('textbox', { name: 'Direction for the team' }), {
    target: {
      value: 'Use only the supplied evidence. Compare options and independently check constraints.',
    },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Prepare a plan' }));
  await screen.findByRole('heading', { name: 'Compare formats' });
  expect(f.update.mock.calls[0][1]).toMatchObject({
    action: 'prepare',
    expected_revision: 0,
    expected_brief_revision: 0,
    body: 'Use only the supplied evidence. Compare options and independently check constraints.',
  });
  expect(start).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: 'Start this plan' }));
  await screen.findByText('Start response lost');
  const request = start.mock.calls[0][1];
  expect(f.update.mock.calls.map((c) => c[1].action)).toEqual(['prepare', 'capture', 'agree']);
  expect(f.update.mock.calls[2][1]).toEqual({ action: 'agree', ...request });
  first.unmount();
  start.mockImplementationOnce(async () => {
    const execution: PlanExecutionView = {
      receipt: {
        source_work_id: 'shape',
        ...request,
        root_work_id: 'root',
        content,
        assignments: content.assignments.map((a) => ({
          assignment_key: a.key,
          work_id: a.key,
          agent_key: a.agent_key,
          definition_digest: 'saved-agent-revision',
        })),
      },
      state: 'running',
      root: null,
      assignments: [],
      error: null,
    };
    f.set({ execution });
    return execution;
  });
  f.render();
  fireEvent.click(await screen.findByRole('button', { name: 'Check this start again' }));
  await screen.findByRole('heading', { name: 'Your team at work' });
  expect(start.mock.calls[1][1]).toEqual(request);
  expect(f.update.mock.calls.filter((c) => c[1].action === 'agree')).toHaveLength(1);
  expect(f.submit).not.toHaveBeenCalled();
});

it('keeps an unconfirmed prepared direction pinned across closing and reopening', async () => {
  const f = fixture({
    plans: [],
    generation: null,
    brief_revision: 0,
    readiness: [],
    execution_available: false,
  });
  f.update.mockRejectedValueOnce(new Error('Preparation response lost'));
  const first = f.render('First direction');
  fireEvent.click(await screen.findByRole('button', { name: 'Plan work from this discussion' }));
  fireEvent.click(screen.getByRole('button', { name: 'Prepare a plan' }));
  await screen.findByText('Preparation response lost');
  const original = f.update.mock.calls[0][1];
  first.unmount();
  f.render('A later response must not overwrite the pending operation');
  fireEvent.click(await screen.findByRole('button', { name: 'Retry plan operation' }));
  await screen.findByRole('heading', { name: 'Compare formats' });
  expect(f.update.mock.calls[1][1]).toEqual(original);
});
