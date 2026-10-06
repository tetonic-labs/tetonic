import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { LocalEngineProvider } from '../src/context/LocalEngineContext';
import { TeamWorkspace } from '../src/components/team-work/TeamWorkspace';
import {
  EngineRequestError,
  LocalEngine,
  type EngineTask,
  type EngineWorkspace,
} from '../src/lib/localEngine';
import { mergeWorkspace, workRecords } from '../src/lib/workspaceRecords';

const agent = {
  key: 'assistant-key',
  id: 'agent-id',
  name: 'Mira',
  purpose: 'Help explore ideas',
  model: 'installed-model',
  harness: 'general',
  provider: 'ollama',
  max_steps: 6,
  max_seconds: 120,
  max_tokens: 4096,
  tools: [],
};
const base: EngineWorkspace = {
  organization: 'Local test',
  team_id: 'team-id',
  team_name: 'Owner workspace',
  agent_id: agent.id,
  agent_name: agent.name,
  model: agent.model,
  input_limit: 12000,
  agents: [agent],
  tasks: [],
};
const saved: EngineTask = {
  id: 'work-one',
  input: 'Compare the two proposals in detail',
  agent_key: agent.key,
  agent_name: agent.name,
  state: 'completed',
  run_id: 'run-one',
  sequence: 10,
  messages: [{ id: 1, role: 'assistant', content: 'The first proposal meets your requirements.' }],
};
function fixture(tasks: EngineTask[] = []) {
  const client = new LocalEngine('test-token');
  vi.spyOn(client, 'plan').mockResolvedValue({
    plans: [],
    generation: null,
    brief_revision: 1,
    readiness: [],
    execution_available: false,
  });
  const snapshot = vi.spyOn(client, 'snapshot').mockResolvedValue({ ...base, tasks });
  vi.spyOn(client, 'agentCatalog').mockResolvedValue({
    models: [agent.model],
    harnesses: ['general'],
    max_steps: 8,
    max_seconds: 120,
    max_tokens: 4096,
  });
  const items = vi.spyOn(client, 'workItems').mockResolvedValue(
    tasks.map((task) => ({
      id: task.id,
      title: task.input.split('\n')[0],
      status: 'open',
      request_id: task.id,
      version: 1,
    })),
  );
  const approvals = vi
    .spyOn(client, 'approvals')
    .mockResolvedValue({ active_stops: [], pending_approvals: [], effort: [] });
  vi.spyOn(client, 'teams').mockResolvedValue([
    { id: base.team_id, name: base.team_name, org_id: 'local' },
  ]);
  const submit = vi.spyOn(client, 'submit').mockImplementation(async (id, input, key, parent) => ({
    ...saved,
    id,
    input,
    agent_key: key,
    parent_id: parent,
  }));
  const cancel = vi
    .spyOn(client, 'cancel')
    .mockResolvedValue({ ...saved, state: 'canceling', sequence: 11 });
  const view = () =>
    render(
      <LocalEngineProvider client={client}>
        <TeamWorkspace />
      </LocalEngineProvider>,
    );
  return { client, snapshot, items, approvals, submit, cancel, view };
}
afterEach(() => {
  vi.restoreAllMocks();
  sessionStorage.clear();
  window.history.replaceState(null, '', '/');
});

it('saves a brief without launching work and retries an uncertain save with the same identity', async () => {
  const f = fixture([{ ...saved, purpose: 'explore' }]);
  const original = {
    work_id: saved.id,
    revision: 1,
    body: 'We are still comparing options.',
    request_id: 'first-save',
    created_by: 'owner',
  };
  const briefs = vi.spyOn(f.client, 'briefs').mockResolvedValue([original]);
  const save = vi
    .spyOn(f.client, 'saveBrief')
    .mockRejectedValueOnce(new Error('Response lost'))
    .mockImplementation(async (id, request) => ({
      ...original,
      work_id: id,
      revision: 2,
      body: request.body,
      request_id: request.request_id,
    }));
  window.history.replaceState(null, '', `/#shape=${saved.id}`);
  const page = f.view();
  fireEvent.click(await screen.findByText('Saved direction & history'));
  const editor = await screen.findByRole('textbox', { name: 'Current understanding' });
  await waitFor(() => expect(editor).toHaveProperty('value', original.body));
  fireEvent.change(editor, {
    target: { value: 'Choose the limited pilot; investigate access first.' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Save brief' }));
  await screen.findByText('Response lost');
  expect(editor).toHaveProperty('readOnly', true);
  // The save succeeded but its response was lost, and another tab saved again.
  // Reload must still let this tab recover its original receipt, not deadlock.
  briefs.mockResolvedValue([{ ...original, revision: 3, body: 'A later decision' }, original]);
  page.unmount();
  f.view();
  fireEvent.click(await screen.findByText('Saved direction & history'));
  await screen.findByText(/A newer revision is saved/);
  fireEvent.click(screen.getByRole('button', { name: 'Retry save' }));
  await screen.findByText('Saved · revision 2');
  expect(screen.getByText(/A newer revision is saved/)).toBeTruthy();
  expect(screen.getByText('Latest saved brief · revision 3')).toBeTruthy();
  expect(save.mock.calls[1]).toEqual(save.mock.calls[0]);
  expect(f.submit).not.toHaveBeenCalled();
  expect(f.cancel).not.toHaveBeenCalled();
});
it('preserves a conflicting brief draft and requires review before applying it to a newer revision', async () => {
  const f = fixture([{ ...saved, purpose: 'explore' }]);
  const original = {
    work_id: saved.id,
    revision: 1,
    body: 'Initial understanding',
    request_id: 'first-save',
    created_by: 'owner',
  };
  vi.spyOn(f.client, 'briefs')
    .mockResolvedValueOnce([original])
    .mockResolvedValue([{ ...original, revision: 2, body: 'A newer decision' }, original]);
  const save = vi
    .spyOn(f.client, 'saveBrief')
    .mockRejectedValue(new EngineRequestError('Brief changed', 400));
  window.history.replaceState(null, '', `/#shape=${saved.id}`);
  f.view();
  fireEvent.click(await screen.findByText('Saved direction & history'));
  const editor = await screen.findByRole('textbox', { name: 'Current understanding' });
  fireEvent.change(editor, { target: { value: 'My unsaved decision' } });
  fireEvent.click(screen.getByRole('button', { name: 'Save brief' }));
  await screen.findByText(/A newer revision is saved/);
  expect(editor).toHaveProperty('value', 'My unsaved decision');
  expect(screen.getByRole('button', { name: 'Save brief' })).toHaveProperty('disabled', true);
  fireEvent.click(screen.getByRole('button', { name: 'Keep my text against this revision' }));
  expect(screen.getByRole('button', { name: 'Save brief' })).toHaveProperty('disabled', false);
  expect(save).toHaveBeenCalledTimes(1);
});
it('uses actual lineage, preserves complete requests and rejects stale task revisions', () => {
  const reply = { ...saved, id: 'reply', parent_id: saved.id, input: 'Explain why', sequence: 14 };
  const sameAgentOtherConversation = { ...saved, id: 'other', input: 'Unrelated thought' };
  const records = workRecords([saved, reply, sameAgentOtherConversation], []);
  expect(records).toHaveLength(2);
  expect(records.find((record) => record.id === saved.id)?.turns.map((task) => task.id)).toEqual([
    saved.id,
    reply.id,
  ]);
  const merged = mergeWorkspace(
    { ...base, tasks: [reply] },
    { ...base, tasks: [{ ...reply, sequence: 3, state: 'running' }] },
  );
  expect(merged.tasks[0].state).toBe('completed');
});
