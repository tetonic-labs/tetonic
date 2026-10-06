import { afterEach, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { HumanQuestion } from '../src/components/team-work/HumanQuestion';
import {
  PlanDirectionEditor,
  affectedAssignments,
} from '../src/components/team-work/PlanDirectionEditor';
import {
  EngineRequestError,
  LocalEngine,
  type EngineTask,
  type PlanExecutionView,
  type WorkHumanQuestion,
} from '../src/lib/localEngine';
import { engineAgentToUI } from '../src/lib/engineAdapters';
import { needsHelp } from '../src/lib/workspaceRecords';

const client = new LocalEngine('test');
const refresh = vi.fn(async () => {});
vi.mock('../src/context/LocalEngineContext', () => ({
  useLocalEngine: () => ({ client, isConnected: true, refresh }),
}));
const question: WorkHumanQuestion = {
  id: 'question',
  work_id: 'compare',
  source_work_id: 'source',
  attempt_id: 'attempt',
  content: {
    question: 'Who should this help?',
    why: 'The audience changes the recommendation.',
    options: ['Beginners', 'Experts'],
  },
  deadline: Math.floor(Date.now() / 1000) + 120,
  answer: null,
  response_id: null,
};
const task: EngineTask = {
  id: 'compare',
  input: 'Compare',
  agent_key: 'worker',
  agent_name: 'Mira',
  state: 'waiting_human',
  run_id: 'run',
  sequence: 4,
  messages: [],
  human_questions: [question],
};
const execution: PlanExecutionView = {
  state: 'running',
  root: null,
  error: null,
  directions: [],
  receipt: {
    source_work_id: 'source',
    root_work_id: 'root',
    request_id: 'start',
    revision: 1,
    assignments: ['compare', 'check', 'wrap'].map((key) => ({
      assignment_key: key,
      work_id: key,
      agent_key: 'worker',
      definition_digest: 'digest',
    })),
    content: {
      title: 'Options',
      summary: 'Compare then recommend',
      token_budget: 4000,
      open_questions: [],
      assignments: ['compare', 'check', 'wrap'].map((key) => ({
        key,
        title: key,
        instructions: `Instructions for ${key}`,
        agent_key: 'worker',
        depends_on: key === 'wrap' ? ['compare'] : [],
        tools: [],
        deliverable: 'Result',
        token_budget: 1000,
      })),
    },
  },
  assignments: [
    task,
    { ...task, id: 'check', state: 'completed' },
    { ...task, id: 'wrap', state: 'not_started', human_questions: [] },
  ],
};
afterEach(() => {
  vi.restoreAllMocks();
  sessionStorage.clear();
  refresh.mockClear();
});

it('keeps the same answer command after a lost response, including a remount', async () => {
  const send = vi
    .spyOn(client, 'answerPlanQuestion')
    .mockRejectedValueOnce(new Error('Connection lost'));
  const first = render(<HumanQuestion task={task} question={question} refresh={refresh} />);
  fireEvent.click(screen.getByRole('button', { name: 'Beginners' }));
  fireEvent.click(screen.getByRole('button', { name: 'Send answer' }));
  await screen.findByText('Connection lost');
  const command = send.mock.calls[0][1];
  first.unmount();
  render(<HumanQuestion task={task} question={question} refresh={refresh} />);
  expect(screen.getByRole('textbox', { name: 'Your answer' })).toHaveProperty('value', 'Beginners');
  expect(screen.getByRole('textbox', { name: 'Your answer' })).toHaveProperty('disabled', true);
  send.mockResolvedValueOnce({
    ...question,
    answer: command.answer,
    response_id: command.request_id,
  });
  fireEvent.click(screen.getByRole('button', { name: 'Check this answer again' }));
  await waitFor(() => expect(send).toHaveBeenCalledTimes(2));
  expect(send.mock.calls[1][1]).toEqual(command);
});

it('keeps text after a rejected answer and prevents new answers after the wait ends', async () => {
  vi.spyOn(client, 'answerPlanQuestion').mockRejectedValue(
    new EngineRequestError('Wait ended', 409),
  );
  const view = render(<HumanQuestion task={task} question={question} refresh={refresh} />);
  fireEvent.change(screen.getByRole('textbox'), { target: { value: 'My considered answer' } });
  fireEvent.click(screen.getByRole('button', { name: 'Send answer' }));
  await screen.findByText('Wait ended');
  view.rerender(
    <HumanQuestion task={{ ...task, state: 'canceled' }} question={question} refresh={refresh} />,
  );
  expect(screen.queryByRole('button', { name: 'Send answer' })).toBeNull();
  expect(screen.getByText('My considered answer')).toBeTruthy();
});

it('shows a saved answer without implying that execution completed', () => {
  render(
    <HumanQuestion
      task={task}
      question={{ ...question, answer: 'Beginners', response_id: 'answer' }}
      refresh={refresh}
    />,
  );
  expect(screen.getByText('Your answer to Mira')).toBeTruthy();
  expect(screen.queryByRole('button', { name: 'Send answer' })).toBeNull();
});

it('keeps an acknowledged answer visible even if refreshing the plan fails', async () => {
  vi.spyOn(client, 'answerPlanQuestion').mockImplementation(async (_, command) => ({
    ...question,
    answer: command.answer,
    response_id: command.request_id,
  }));
  const failedRefresh = vi.fn(async () => {
    throw new Error('Refresh unavailable');
  });
  render(<HumanQuestion task={task} question={question} refresh={failedRefresh} />);
  fireEvent.click(screen.getByRole('button', { name: 'Beginners' }));
  fireEvent.click(screen.getByRole('button', { name: 'Send answer' }));
  await screen.findByText('Your answer to Mira');
  expect(screen.queryByRole('button', { name: 'Send answer' })).toBeNull();
  expect(screen.queryByRole('button', { name: 'Check this answer again' })).toBeNull();
});

it('retains unsaved direction when the plan ends while editing', () => {
  const view = render(<PlanDirectionEditor execution={execution} refresh={refresh} />);
  fireEvent.click(screen.getByText('Adjust upcoming work'));
  fireEvent.change(screen.getByRole('combobox'), { target: { value: 'wrap' } });
  fireEvent.change(screen.getByRole('textbox'), { target: { value: 'Keep this thought' } });
  view.rerender(
    <PlanDirectionEditor execution={{ ...execution, state: 'failed' }} refresh={refresh} />,
  );
  expect(screen.getByText('Your unsaved instructions')).toBeTruthy();
  expect(screen.getByText('Keep this thought')).toBeTruthy();
  expect(screen.queryByRole('button', { name: 'Apply to upcoming work' })).toBeNull();
});

it('shows downstream impact and retains completed work; a lost amendment retries exactly', async () => {
  expect(affectedAssignments(execution, 'compare').map((a) => a.key)).toEqual(['compare', 'wrap']);
  const send = vi
    .spyOn(client, 'amendPlanAssignment')
    .mockRejectedValueOnce(new Error('Uncertain response'));
  const view = render(<PlanDirectionEditor execution={execution} refresh={refresh} />);
  fireEvent.click(screen.getByText('Adjust upcoming work'));
  fireEvent.change(screen.getByRole('combobox'), { target: { value: 'wrap' } });
  fireEvent.change(screen.getByRole('textbox'), {
    target: { value: 'Explain this for beginners' },
  });
  expect(screen.getByText(/1 completed contribution stays saved/)).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'Apply to upcoming work' }));
  await screen.findByText('Uncertain response');
  const command = send.mock.calls[0][1];
  view.unmount();
  render(<PlanDirectionEditor execution={execution} refresh={refresh} />);
  fireEvent.click(screen.getByText('Adjust upcoming work'));
  send.mockResolvedValueOnce({
    ...command,
    revision: 1,
    affected_work_ids: ['wrap'],
    retained_work_ids: ['compare', 'check'],
    actor: 'owner',
  });
  fireEvent.click(screen.getByRole('button', { name: 'Check this change again' }));
  await screen.findByText(
    'Direction saved for upcoming work. Completed contributions are retained.',
  );
  expect(send.mock.calls[1][1]).toEqual(command);
});

it('does not silently apply an old edit against a new direction revision', () => {
  const view = render(<PlanDirectionEditor execution={execution} refresh={refresh} />);
  fireEvent.click(screen.getByText('Adjust upcoming work'));
  fireEvent.change(screen.getByRole('combobox'), { target: { value: 'wrap' } });
  fireEvent.change(screen.getByRole('textbox'), { target: { value: 'My direction' } });
  view.rerender(
    <PlanDirectionEditor
      execution={{
        ...execution,
        directions: [
          {
            revision: 1,
            request_id: 'elsewhere',
            assignment_key: 'wrap',
            instructions: 'Someone changed it',
            affected_work_ids: ['wrap'],
            retained_work_ids: ['compare', 'check'],
            actor: 'owner',
          },
        ],
      }}
      refresh={refresh}
    />,
  );
  expect(screen.getByRole('button', { name: 'Apply to upcoming work' })).toHaveProperty(
    'disabled',
    true,
  );
  expect(screen.getByRole('textbox')).toHaveProperty('value', 'My direction');
  fireEvent.click(screen.getByRole('button', { name: 'Use latest direction revision' }));
  expect(screen.getByRole('button', { name: 'Apply to upcoming work' })).toHaveProperty(
    'disabled',
    false,
  );
});

it('projects waiting as attention with a stationary agent, not fabricated activity', () => {
  expect(needsHelp({ id: task.id, title: 'Compare', turns: [task], latest: task })).toBe(true);
  expect(
    engineAgentToUI(
      {
        id: 'worker',
        key: 'worker',
        name: 'Mira',
        purpose: 'Compare',
        model: 'test',
        harness: 'general',
        max_steps: 4,
        max_tokens: 4000,
        max_seconds: 120,
      },
      [task],
    ),
  ).toMatchObject({ status: 'paused', lastActive: 'Needs your input' });
});
