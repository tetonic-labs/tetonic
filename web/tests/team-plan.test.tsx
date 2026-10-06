import { afterEach, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { LocalEngineProvider } from '../src/context/LocalEngineContext';
import { PlanReview } from '../src/components/team-work/PlanReview';
import {
  LocalEngine,
  EngineRequestError,
  type HuddlePlan,
  type PlanView,
  type PlanExecutionView,
  type WorkUsage,
} from '../src/lib/localEngine';

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
      command.action === 'generate'
        ? { ...plan, request_id: command.request_id, status: 'drafting' as const, content: null }
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
      generation:
        command.action === 'generate'
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
    render: () =>
      render(
        <LocalEngineProvider client={client}>
          <PlanReview workId="shape" />
        </LocalEngineProvider>,
      ),
  };
}
afterEach(() => {
  vi.restoreAllMocks();
  sessionStorage.clear();
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
  fireEvent.click(await screen.findByRole('button', { name: 'Start agreed plan' }));
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
  const button = await screen.findByRole('button', { name: 'Start agreed plan' });
  expect(screen.getByText(/Up to 6 minutes for the whole plan/)).toBeTruthy();
  fireEvent.click(button);
  await screen.findByText('The brief changed. Review it again.');
  const first = start.mock.calls[0][1].request_id;
  await waitFor(() =>
    expect(screen.getByRole('button', { name: 'Start agreed plan' })).toHaveProperty(
      'disabled',
      false,
    ),
  );
  fireEvent.click(screen.getByRole('button', { name: 'Start agreed plan' }));
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
  expect(screen.queryByRole('button', { name: 'Start agreed plan' })).toBeNull();
  expect(screen.getByText(/Recorded contributions are kept/)).toBeTruthy();
  fireEvent.click(screen.getByText('What the team was given'));
  expect(screen.getByText('The original brief is unavailable for this saved run.')).toBeTruthy();
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

it('proposes, reviews and agrees to a plan without dispatching assignments', async () => {
  const f = fixture();
  f.render();
  fireEvent.click(await screen.findByRole('button', { name: 'Propose a plan' }));
  fireEvent.click(await screen.findByRole('button', { name: 'Review proposed plan' }));
  await screen.findByRole('heading', { name: 'Compare formats' });
  expect(screen.getByText('After: Compare formats')).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'Agree to this direction' }));
  await screen.findByText('Direction agreed · not started');
  expect(f.update.mock.calls.map((c) => c[1].action)).toEqual(['generate', 'capture', 'agree']);
  expect(f.submit).not.toHaveBeenCalled();
});

it('retains an uncertain operation across closing the overlay and retries its identity', async () => {
  const f = fixture();
  f.update.mockRejectedValueOnce(new Error('Lost response'));
  const first = f.render();
  fireEvent.click(await screen.findByRole('button', { name: 'Propose a plan' }));
  await screen.findByText('Lost response');
  const original = f.update.mock.calls[0][1];
  first.unmount();
  f.render();
  fireEvent.click(await screen.findByRole('button', { name: 'Retry plan operation' }));
  await waitFor(() => expect(f.update).toHaveBeenCalledTimes(2));
  expect(f.update.mock.calls[1][1]).toEqual(original);
  await screen.findByRole('button', { name: 'Review proposed plan' });
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
  f.render();
  fireEvent.click(await screen.findByRole('button', { name: 'Agree to this direction' }));
  await screen.findByText('The engine has not confirmed this operation. Retry the same request.');
  expect(screen.queryByText('Direction agreed · not started')).toBeNull();
  expect(screen.getByRole('button', { name: 'Retry plan operation' })).toBeTruthy();
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
  expect(await screen.findByRole('button', { name: 'Propose a plan' })).toHaveProperty(
    'disabled',
    true,
  );
  expect(f.update).not.toHaveBeenCalled();
  expect(f.submit).not.toHaveBeenCalled();
});
