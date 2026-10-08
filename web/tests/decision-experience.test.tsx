import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { ApprovalRequests } from '../src/components/team-work/ApprovalRequests';
import { AttentionPanel } from '../src/components/team-work/AttentionPanel';
import { WorkDetails } from '../src/components/team-work/WorkDetails';
import { PlanExecution } from '../src/components/team-work/PlanExecution';
import { HumanQuestion } from '../src/components/team-work/HumanQuestion';
import type {
  EngineTask,
  LocalApproval,
  PlanExecutionView,
  WorkHumanQuestion,
} from '../src/lib/localEngine';

const approval: LocalApproval = {
  org_id: 'org',
  team_id: 'team',
  work_id: 'work',
  approval_id: 'permission',
  proposal_digest: 'exact-command',
  status: 'pending',
  request_id: 'request',
  expires_at: 4102444800,
  proposal: {
    command: 'git status --short',
    working_directory: 'C:/work/research',
    shell: 'pwsh',
    attempt_id: 'attempt',
    call_id: 'call',
    parameter_digest: 'parameters',
    confinement_warnings: ['Filesystem isolation is unavailable on this host.'],
  },
};
const task: EngineTask = {
  id: 'work',
  input: 'Compare the evidence',
  agent_key: 'mira',
  agent_name: 'Mira',
  state: 'running',
  run_id: 'run',
  sequence: 1,
  messages: [],
};
const question: WorkHumanQuestion = {
  id: 'question',
  work_id: task.id,
  source_work_id: 'source',
  attempt_id: 'attempt',
  content: {
    question: 'Which audience matters most?',
    why: 'This changes which evidence I compare.',
    options: ['New customers', 'Existing customers'],
  },
  deadline: 4102444800,
  answer: null,
  response_id: null,
};
const engine = {
  isConnected: true,
  readErrors: {} as Record<string, string>,
  approvals: { pending_approvals: [approval], active_stops: [], effort: [] },
  workspace: { tasks: [task], agents: [] },
  resolveApproval: vi.fn(
    async (_id: string, allow: boolean, _digest: string): Promise<LocalApproval> => ({
      ...approval,
      status: allow ? 'approved' : 'rejected',
    }),
  ),
  refresh: vi.fn(async () => {}),
  client: {
    answerPlanQuestion: vi.fn(
      async (_id: string, command: { answer: string; request_id: string }) => ({
        ...question,
        answer: command.answer,
        response_id: command.request_id,
      }),
    ),
  },
};
vi.mock('../src/context/LocalEngineContext', () => ({ useLocalEngine: () => engine }));
beforeEach(() => {
  engine.isConnected = true;
  engine.readErrors = {};
  engine.approvals.pending_approvals = [approval];
  engine.workspace.tasks = [task];
});
afterEach(() => {
  vi.clearAllMocks();
  vi.useRealTimers();
  sessionStorage.clear();
});

it('puts exact scoped approvals directly beside the work without navigating to the inbox', async () => {
  engine.approvals.pending_approvals = [
    approval,
    {
      ...approval,
      approval_id: 'other',
      work_id: 'another-effort',
      proposal: { ...approval.proposal!, command: 'another command' },
    },
  ];
  render(
    <WorkDetails
      work={{ id: task.id, title: task.input, latest: task, turns: [task] }}
      onAccepted={vi.fn()}
    />,
  );
  expect(screen.getByRole('heading', { name: 'Allow this command?' })).toBeTruthy();
  expect(screen.getByText('git status --short')).toBeTruthy();
  expect(screen.getByText('C:/work/research')).toBeTruthy();
  expect(screen.getByText('Filesystem isolation is unavailable on this host.')).toBeTruthy();
  expect(screen.queryByText('another command')).toBeNull();
  expect(screen.queryByRole('button', { name: 'Open related work' })).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: 'Allow once' }));
  await screen.findByText('Command approved once');
  expect(engine.resolveApproval).toHaveBeenCalledExactlyOnceWith(
    'permission',
    true,
    'exact-command',
  );
});

it('keeps the receipt after polling removes the resolved request and never claims the command ran', async () => {
  const view = render(<ApprovalRequests />);
  const allow = screen.getByRole('button', { name: 'Allow once' });
  allow.focus();
  fireEvent.click(allow);
  await screen.findByText('Command approved once');
  expect(document.activeElement).toBe(
    screen.getByRole('heading', { name: 'Command approved once' }),
  );
  engine.approvals.pending_approvals = [];
  view.rerender(<ApprovalRequests />);
  expect(screen.getByText(/not confirmation that the command ran/)).toBeTruthy();
  expect(screen.queryByRole('button', { name: 'Allow once' })).toBeNull();
});

it('rejects mismatched receipts instead of presenting a successful decision', async () => {
  engine.resolveApproval.mockResolvedValueOnce({
    ...approval,
    proposal_digest: 'different-command',
    status: 'approved',
  });
  render(<ApprovalRequests />);
  fireEvent.click(screen.getByRole('button', { name: 'Allow once' }));
  expect(await screen.findByRole('alert')).toHaveProperty(
    'textContent',
    expect.stringContaining('not confirmed'),
  );
  expect(screen.queryByText('Command approved once')).toBeNull();
});

it('expires an open permission and prevents decisions while status is stale', () => {
  vi.useFakeTimers();
  engine.approvals.pending_approvals = [
    { ...approval, expires_at: Math.floor(Date.now() / 1000) + 2 },
  ];
  const view = render(<ApprovalRequests />);
  expect(screen.getByRole('button', { name: 'Allow once' })).toHaveProperty('disabled', false);
  act(() => vi.advanceTimersByTime(2500));
  expect(screen.getByRole('button', { name: 'Allow once' })).toHaveProperty('disabled', true);
  expect(screen.getByText(/permission request has expired/)).toBeTruthy();
  engine.approvals.pending_approvals = [approval];
  engine.readErrors = { Decisions: 'Unavailable' };
  view.rerender(<ApprovalRequests />);
  fireEvent.click(screen.getByRole('button', { name: 'Allow once' }));
  expect(engine.resolveApproval).not.toHaveBeenCalled();
});

it('shows questions immediately and preserves an answer through filtering and resolution', async () => {
  engine.approvals.pending_approvals = [];
  const waiting = { ...task, state: 'waiting_human' as const, human_questions: [question] };
  const record = { id: task.id, title: task.input, latest: waiting, turns: [waiting] };
  const view = render(<AttentionPanel records={[record]} onWork={vi.fn()} />);
  expect(screen.getByRole('heading', { name: question.content.question })).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'New customers' }));
  fireEvent.click(screen.getByRole('button', { name: /Needs a look/ }));
  expect(screen.queryByRole('textbox', { name: 'Your answer' })).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: /Decisions & questions/ }));
  expect(screen.getByRole('textbox', { name: 'Your answer' })).toHaveProperty(
    'value',
    'New customers',
  );
  fireEvent.click(screen.getByRole('button', { name: 'Send answer' }));
  await screen.findByRole('region', { name: 'Answer saved for Mira' });
  expect(screen.getByText('Waiting for an update from Mira.')).toBeTruthy();
  const answered = {
    ...task,
    state: 'waiting_human' as const,
    human_questions: [{ ...question, answer: 'New customers', response_id: 'confirmed' }],
  };
  view.rerender(
    <AttentionPanel
      records={[{ ...record, latest: answered, turns: [answered] }]}
      onWork={vi.fn()}
    />,
  );
  expect(screen.getByText('Nothing is waiting for your input.')).toBeTruthy();
  expect(screen.queryByRole('button', { name: 'Review work' })).toBeNull();
  expect(screen.getByText('Waiting for an update from Mira.')).toBeTruthy();
  view.rerender(
    <AttentionPanel
      records={[
        {
          ...record,
          latest: { ...answered, state: 'running' },
          turns: [{ ...answered, state: 'running' }],
        },
      ]}
      onWork={vi.fn()}
    />,
  );
  expect(screen.getByText('Mira is working again.')).toBeTruthy();
  expect(screen.queryByRole('button', { name: 'Send answer' })).toBeNull();
});

it('removes an externally resolved request without offering the stale action again', () => {
  const view = render(<ApprovalRequests />);
  engine.approvals.pending_approvals = [];
  view.rerender(<ApprovalRequests />);
  expect(screen.queryByRole('button', { name: 'Allow once' })).toBeNull();
  expect(engine.resolveApproval).not.toHaveBeenCalled();
});

it('shows only a plan root and its own assignment approvals in the team view', () => {
  const execution: PlanExecutionView = {
    state: 'running',
    root: { ...task, id: 'root' },
    assignments: [task],
    error: null,
    receipt: {
      source_work_id: 'source',
      root_work_id: 'root',
      request_id: 'start',
      revision: 1,
      assignments: [
        {
          assignment_key: 'compare',
          work_id: task.id,
          agent_key: task.agent_key,
          definition_digest: 'agent-revision',
        },
      ],
      content: {
        title: 'Team research',
        summary: 'Compare evidence',
        token_budget: 4000,
        open_questions: [],
        assignments: [
          {
            key: 'compare',
            title: 'Compare',
            instructions: 'Compare evidence',
            agent_key: task.agent_key,
            depends_on: [],
            tools: [],
            deliverable: 'Comparison',
            token_budget: 2000,
          },
        ],
      },
    },
  };
  engine.approvals.pending_approvals = [
    approval,
    {
      ...approval,
      approval_id: 'unrelated',
      work_id: 'unrelated',
      proposal: { ...approval.proposal!, command: 'unrelated command' },
    },
  ];
  render(
    <PlanExecution
      workId="source"
      view={{
        plans: [],
        generation: null,
        brief_revision: 1,
        execution,
        readiness: [],
        execution_available: true,
      }}
      refresh={engine.refresh}
    />,
  );
  expect(screen.getByText('git status --short')).toBeTruthy();
  expect(screen.queryByText('unrelated command')).toBeNull();
});

it('does not send a second answer while the first response is pending', async () => {
  let finish!: (value: WorkHumanQuestion) => void;
  engine.client.answerPlanQuestion.mockImplementationOnce(
    async () =>
      new Promise<WorkHumanQuestion>((resolve) => {
        finish = resolve;
      }),
  );
  render(
    <HumanQuestion
      task={{ ...task, state: 'waiting_human' }}
      question={question}
      refresh={engine.refresh}
    />,
  );
  fireEvent.click(screen.getByRole('button', { name: 'New customers' }));
  screen.getByRole('button', { name: 'Send answer' }).focus();
  fireEvent.click(screen.getByRole('button', { name: 'Send answer' }));
  fireEvent.click(screen.getByRole('button', { name: 'Sending…' }));
  expect(engine.client.answerPlanQuestion).toHaveBeenCalledTimes(1);
  const command = engine.client.answerPlanQuestion.mock.calls[0][1];
  await act(async () =>
    finish({ ...question, answer: command.answer, response_id: command.request_id }),
  );
  await waitFor(() => expect(screen.queryByRole('button', { name: 'Send answer' })).toBeNull());
  expect(document.activeElement).toBe(screen.getByRole('status'));
});

it('does not pull focus back when a person moves away while a decision is pending', async () => {
  let finish!: (receipt: LocalApproval) => void;
  engine.resolveApproval.mockImplementationOnce(
    () =>
      new Promise<LocalApproval>((resolve) => {
        finish = resolve;
      }),
  );
  render(
    <>
      <ApprovalRequests />
      <button>Another request</button>
    </>,
  );
  screen.getByRole('button', { name: 'Allow once' }).focus();
  fireEvent.click(screen.getByRole('button', { name: 'Allow once' }));
  const next = screen.getByRole('button', { name: 'Another request' });
  next.focus();
  await act(async () => finish({ ...approval, status: 'approved' }));
  expect(document.activeElement).toBe(next);
});
