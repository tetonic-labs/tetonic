import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { readFileSync, existsSync } from 'node:fs';
import { TeamWorkspace } from '../src/components/team-work/TeamWorkspace';
import { WorkComposer } from '../src/components/team-work/WorkComposer';
import { EngineAgentDetail } from '../src/components/team-work/EngineAgentDetail';
import { LocalEngineProvider } from '../src/context/LocalEngineContext';
import { LocalEngine } from '../src/engine/client';
import { EngineRequestError } from '../src/engine/failure';
import { type EngineTask, type EngineWorkspace, type PlanView } from '../src/engine/contracts';
import { teamWorkspace } from '../src/engine/projections/workspace';
import { layoutProject } from '../src/lib/projectLayout';

const agent = {
  key: 'mira',
  id: 'mira-id',
  definition_digest: 'mira-revision-1',
  editable: true,
  name: 'Mira',
  purpose: 'Understand problems',
  model: 'installed-model',
  provider: 'ollama',
  harness: 'general',
  max_steps: 6,
  max_seconds: 120,
  max_tokens: 4096,
  tools: ['read_file'],
};
const saved: EngineTask = {
  id: 'first',
  input: 'Compare two approaches',
  agent_key: agent.key,
  agent_name: agent.name,
  state: 'completed',
  run_id: 'run-first',
  sequence: 10,
  messages: [{ id: 1, role: 'assistant', content: 'The first approach needs less time.' }],
};

it('groups an executed plan and draws only its recorded assignment dependencies', () => {
  const link = {
    source_work_id: 'shape',
    root_work_id: 'root',
    assignment_key: null,
    title: 'Compare workshops',
    depends_on: [],
  };
  const root = { ...saved, id: 'root', plan: link };
  const compare = {
    ...saved,
    id: 'compare',
    plan: { ...link, assignment_key: 'compare', title: 'Compare formats' },
  };
  const review = {
    ...saved,
    id: 'review',
    plan: {
      ...link,
      assignment_key: 'review',
      title: 'Check assumptions',
      depends_on: ['compare'],
    },
  };
  const result = teamWorkspace(
    {
      organization: 'Our workspace',
      team_id: 'team',
      team_name: 'Team',
      agent_id: agent.id,
      agent_name: agent.name,
      model: agent.model,
      input_limit: 12000,
      agents: [agent],
      tasks: [root, compare, review],
    },
    [],
  );
  expect(result.projects).toHaveLength(1);
  expect(result.projects[0].id).toBe('plan:root');
  expect(result.projects[0].title).toBe('Compare workshops');
  expect(result.projects[0].kind).toBe('plan');
  expect(result.projects[0].area?.name).toBe('Team');
  expect(result.projects[0].streams.find((s) => s.id === 'root')?.role).toBe('coordination');
  expect(result.projects[0].streams.find((s) => s.id === 'compare')?.role).toBe('contribution');
  expect(result.projects[0].streams.find((s) => s.id === 'review')?.dependencies).toEqual([
    { id: 'compare', reason: 'Uses the recorded contribution' },
  ]);
  expect(
    result.entries.filter((e) => e.kind === 'direction').every((e) => e.author === 'Agreed plan'),
  ).toBe(true);
});
function fixture(tasks: EngineTask[] = []) {
  const client = new LocalEngine('test');
  let data: EngineWorkspace = {
    organization: 'Our workspace',
    team_id: 'our-team',
    team_name: 'Our team',
    agent_id: agent.id,
    agent_name: agent.name,
    model: agent.model,
    input_limit: 12000,
    agents: [agent],
    tasks,
  };
  const snapshot = vi.spyOn(client, 'snapshot').mockImplementation(async () => data);
  vi.spyOn(client, 'agentCatalog').mockResolvedValue({
    models: [agent.model],
    harnesses: ['general'],
    tools: ['read_file'],
    mcp_connections: [],
    max_steps: 8,
    max_seconds: 120,
    max_tokens: 4096,
  });
  vi.spyOn(client, 'workItems').mockResolvedValue([]);
  vi.spyOn(client, 'teams').mockResolvedValue([
    { id: data.team_id, name: data.team_name, org_id: 'org' },
  ]);
  const approvals = vi
    .spyOn(client, 'approvals')
    .mockResolvedValue({ active_stops: [], pending_approvals: [], effort: [] });
  const submit = vi
    .spyOn(client, 'submit')
    .mockImplementation(async (id, input, key, parent, purpose) => {
      const task = { ...saved, id, input, agent_key: key, parent_id: parent, purpose };
      data = { ...data, tasks: [...data.tasks, task] };
      return task;
    });
  const cancel = vi.spyOn(client, 'cancel').mockImplementation(async (id) => {
    const task = {
      ...data.tasks.find((t) => t.id === id)!,
      state: 'canceling' as const,
      sequence: 11,
    };
    data = { ...data, tasks: data.tasks.map((t) => (t.id === id ? task : t)) };
    return task;
  });
  const view = () =>
    render(
      <LocalEngineProvider client={client}>
        <TeamWorkspace />
      </LocalEngineProvider>,
    );
  return {
    client,
    snapshot,
    submit,
    cancel,
    approvals,
    view,
    getData: () => data,
    setData: (value: EngineWorkspace) => {
      data = value;
    },
  };
}
afterEach(() => {
  vi.restoreAllMocks();
  sessionStorage.clear();
  history.replaceState(null, '', '/');
});

describe('one connected team workspace', () => {
  it('edits the selected agent from Agents and returns to its saved profile', async () => {
    const f = fixture();
    const create = vi.spyOn(f.client, 'createAgent');
    const update = vi.spyOn(f.client, 'updateAgent').mockImplementation(async (old, input) => {
      const changed = { ...old, ...input, definition_digest: 'mira-revision-2' };
      f.setData({ ...f.getData(), agents: [changed] });
      return changed;
    });
    f.view();
    fireEvent.click(screen.getByRole('button', { name: 'Agents', exact: true }));
    fireEvent.click(
      await screen.findByRole('button', {
        name: /^Mira Understand problems installed-model (Ready|Setup unchecked)$/,
      }),
    );
    const edit = screen.getByRole('button', { name: 'Edit agent' });
    await waitFor(() => expect(edit).toHaveProperty('disabled', false));
    fireEvent.click(edit);
    const name = await screen.findByLabelText('Name', { exact: true });
    expect(name).toHaveProperty('value', 'Mira');
    expect(screen.getByRole('checkbox', { name: 'Read files', exact: true })).toHaveProperty(
      'checked',
      true,
    );
    expect(screen.getByRole('heading', { name: 'MCP connections' })).toBeTruthy();
    fireEvent.change(name, { target: { value: 'Mira updated' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await screen.findByRole('heading', { name: 'Mira updated' });
    expect(screen.getByText('Changes saved. New work will use these settings.')).toBeTruthy();
    expect(update).toHaveBeenCalledExactlyOnceWith(
      agent,
      expect.objectContaining({ name: 'Mira updated', tools: ['read_file'] }),
    );
    expect(create).not.toHaveBeenCalled();
    expect(f.getData().agents).toHaveLength(1);
  });

  it('does not substitute another agent when the selected recipient is unavailable', async () => {
    const f = fixture();
    render(
      <LocalEngineProvider client={f.client}>
        <WorkComposer recipient="missing-agent" onAccepted={vi.fn()} />
      </LocalEngineProvider>,
    );
    await screen.findByRole('option', { name: 'Mira' });
    fireEvent.change(screen.getByLabelText('What would you like to work on?'), {
      target: { value: 'Private research for the selected agent' },
    });
    expect(screen.getByLabelText('Assign to agent')).toHaveProperty('value', 'missing-agent');
    expect(screen.getByRole('button', { name: 'Start work' })).toHaveProperty('disabled', true);
    fireEvent.submit(screen.getByRole('button', { name: 'Start work' }).closest('form')!);
    expect(f.submit).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText('Assign to agent'), { target: { value: agent.key } });
    fireEvent.click(screen.getByRole('button', { name: 'Start work' }));
    await waitFor(() => expect(f.submit).toHaveBeenCalledOnce());
    expect(f.submit.mock.calls[0][2]).toBe(agent.key);
  });

  it('repairs a missing provider key on an existing agent before its first assignment', async () => {
    const f = fixture();
    const hosted = { ...agent, provider: 'openai', hosted_consent: true, tools: [] };
    f.setData({ ...f.getData(), agents: [hosted] });
    let keySaved = false;
    vi.mocked(f.client.agentCatalog).mockImplementation(async () => ({
      models: [],
      harnesses: ['general'],
      tools: [],
      runtime_profiles: [
        { provider: 'openai', harness: 'general', tools: [], tool_restriction: null },
      ],
      providers: [{ id: 'openai', name: 'OpenAI', key_saved: keySaved }],
      max_steps: 8,
      max_seconds: 120,
      max_tokens: 4096,
    }));
    const save = vi.spyOn(f.client, 'saveProviderKey').mockImplementation(async () => {
      keySaved = true;
      return { id: 'openai', name: 'OpenAI', key_saved: true };
    });
    const create = vi.spyOn(f.client, 'createAgent');
    const assign = vi.fn();
    render(
      <LocalEngineProvider client={f.client}>
        <EngineAgentDetail
          profile={hosted}
          created={false}
          records={[]}
          onBack={vi.fn()}
          onWork={vi.fn()}
          onAgent={assign}
        />
      </LocalEngineProvider>,
    );
    const key = await screen.findByLabelText('OpenAI API key');
    const start = screen.getByRole('button', { name: 'Give Mira work' });
    expect(start).toHaveProperty('disabled', true);
    fireEvent.change(key, { target: { value: 'fixture-key-only' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save key securely' }));
    expect(key).toHaveProperty('value', '');
    await waitFor(() => expect(start).toHaveProperty('disabled', false));
    fireEvent.click(start);
    expect(save).toHaveBeenCalledExactlyOnceWith('openai', 'fixture-key-only');
    expect(assign).toHaveBeenCalledExactlyOnceWith(agent.key);
    expect(create).not.toHaveBeenCalled();
    expect(JSON.stringify(sessionStorage)).not.toContain('fixture-key-only');
  });
  it('shows real planning activity at its source work without inventing a second outcome', () => {
    const f = fixture([{ ...saved, purpose: 'explore' }]);
    const data = f.getData();
    data.planning_tasks = [
      {
        ...saved,
        id: 'planning',
        planning_for: saved.id,
        state: 'running',
        messages: [{ id: 8, role: 'assistant', content: 'Actual proposed assignments' }],
      },
    ];
    const result = teamWorkspace(data, []);
    expect(result.records).toHaveLength(1);
    expect(result.projects[0].streams).toHaveLength(1);
    expect(result.projects[0].streams[0].stateLabel).toBe('Preparing a work plan');
    expect(result.projects[0].people[0].doing).toBe('Preparing a work plan');
    expect(result.entries.some((e) => e.content === 'Actual proposed assignments')).toBe(true);
    expect(result.projects[0].streams[0].dependencies).toEqual([]);
  });
  it('makes the root and former team-work URL use the same production entry and retires old shells', () => {
    for (const file of ['index.html', 'dev/team-work/index.html'])
      expect(readFileSync(file, 'utf8')).toContain('src="/src/main.tsx"');
    for (const file of [
      'dev/PreviewApp.tsx',
      'dev/team-work/TeamWorkExample.tsx',
      'src/components/workspace/Workspace.tsx',
      'src/components/work/MissionDeck.tsx',
      'src/components/work/Workroom.tsx',
      'src/components/work/LocalWorkspace.tsx',
    ])
      expect(existsSync(file)).toBe(false);
  });
  it('shows only actual records and groups replies without manufacturing dependencies or destinations', () => {
    const f = fixture([
      saved,
      { ...saved, id: 'reply', parent_id: saved.id, input: 'Explain the risks.' },
    ]);
    const model = teamWorkspace(f.getData(), [], []);
    expect(model.records).toHaveLength(1);
    expect(model.projects[0].streams).toHaveLength(1);
    expect(model.projects[0].streams[0].dependencies).toEqual([]);
    expect(model.projects[0].places).toEqual([]);
    expect(model.projects[0].people[0].destination).toBeUndefined();
    expect(model.entries.map((e) => e.content)).toContain('Explain the risks.');
    expect(model.entries.some((e) => e.content.includes('customer portal'))).toBe(false);
  });
  it('places idle agents separately even when the connected team has no work', () => {
    const f = fixture();
    const data = {
      ...f.getData(),
      agents: Array.from({ length: 12 }, (_, i) => ({
        ...agent,
        key: `agent-${i}`,
        id: `id-${i}`,
      })),
    };
    const layout = layoutProject(teamWorkspace(data, []).projects[0]);
    expect(new Set(layout.people.map((p) => `${p.point.x}:${p.point.y}`)).size).toBe(12);
  });
  it('does not present stopped work as waiting and respects explicit recorded goal grouping', () => {
    const f = fixture([{ ...saved, state: 'canceled' }]);
    const model = teamWorkspace(
      f.getData(),
      [
        {
          id: saved.id,
          title: saved.input,
          status: 'open',
          goal_id: 'goal-one',
          request_id: saved.id,
          version: 1,
        },
      ],
      [],
    );
    expect(model.projects[0].id).toBe('goal:goal-one');
    expect(model.projects[0].streams[0].tasks[0].status).toBe('stopped');
  });
  it('starts real work from the floating composer, restores it by URL, and replies to its recorded parent', async () => {
    const f = fixture();
    const page = f.view();
    const input = screen.getByRole('textbox', { name: 'What would you like to work on?' });
    fireEvent.change(input, { target: { value: saved.input } });
    const send = screen.getByRole('button', { name: 'Start work' });
    await waitFor(() => expect(send).toHaveProperty('disabled', false));
    fireEvent.click(send);
    const detail = await screen.findByRole('region', { name: 'Work details' });
    expect(within(detail).getAllByText(saved.messages[0].content).length).toBeGreaterThan(0);
    const root = f.submit.mock.calls[0][0];
    expect(location.hash).toBe(`#work=${root}`);
    expect(f.submit).toHaveBeenCalledWith(root, saved.input, agent.key, undefined);
    page.unmount();
    f.view();
    await screen.findByRole('region', { name: 'Work details' });
    fireEvent.change(screen.getByRole('textbox', { name: 'Follow up on this work' }), {
      target: { value: 'What are the risks?' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Send follow-up' }));
    await waitFor(() =>
      expect(f.submit).toHaveBeenLastCalledWith(
        expect.any(String),
        'What are the risks?',
        agent.key,
        root,
      ),
    );
    expect(f.submit).toHaveBeenCalledTimes(2);
  });
  it('retains uncertain sends and retries the original agent even after roster changes', async () => {
    const f = fixture();
    f.submit.mockRejectedValueOnce(new Error('Response lost'));
    f.view();
    fireEvent.change(screen.getByRole('textbox', { name: 'What would you like to work on?' }), {
      target: { value: 'Keep this request.' },
    });
    const send = screen.getByRole('button', { name: 'Start work' });
    await waitFor(() => expect(send).toHaveProperty('disabled', false));
    fireEvent.click(send);
    await screen.findByText(/Response lost/);
    f.setData({
      ...f.getData(),
      agents: [{ ...agent, key: 'someone-else', name: 'Someone else' }],
    });
    await screen.findByRole('option', { name: 'Someone else' }, { timeout: 3000 });
    fireEvent.click(screen.getByRole('button', { name: 'Teams', exact: true }));
    fireEvent.click(screen.getByRole('button', { name: 'Close project details' }));
    expect(screen.getByRole('textbox', { name: 'What would you like to work on?' })).toHaveProperty(
      'value',
      'Keep this request.',
    );
    expect(screen.getByLabelText('Assign to agent')).toHaveProperty('value', agent.key);
    fireEvent.click(screen.getByRole('button', { name: 'Retry work request' }));
    await screen.findByRole('region', { name: 'Work details' });
    expect(f.submit.mock.calls[1]).toEqual(f.submit.mock.calls[0]);
  });
  it('keeps validation failures editable and never records rejected work as accepted', async () => {
    const f = fixture();
    f.submit.mockRejectedValue(new EngineRequestError('Too much context', 400));
    f.view();
    const input = screen.getByRole('textbox', { name: 'What would you like to work on?' });
    fireEvent.change(input, { target: { value: 'Request' } });
    const send = screen.getByRole('button', { name: 'Start work' });
    await waitFor(() => expect(send).toHaveProperty('disabled', false));
    fireEvent.click(send);
    await screen.findByText(/Too much context/);
    expect(input).toHaveProperty('readOnly', false);
    expect(screen.queryByRole('region', { name: 'Work details' })).toBeNull();
  });
  it('keeps a pending plan assignment within its coordinator and exposes its context on demand', async () => {
    const f = fixture([
      {
        ...saved,
        state: 'not_started',
        messages: [],
        plan: {
          source_work_id: 'shape',
          root_work_id: 'root',
          assignment_key: 'compare',
          title: 'Compare formats',
          depends_on: [],
        },
      },
    ]);
    history.replaceState(null, '', '/#work=first');
    f.view();
    await screen.findByRole('region', { name: 'Work details' });
    expect(screen.queryByRole('button', { name: 'Retry saved request' })).toBeNull();
    expect(screen.queryByRole('textbox', { name: 'Follow up on this work' })).toBeNull();
    expect(screen.getByText('Shared context received by Mira')).toBeTruthy();
    expect(
      screen
        .getByRole('link', { name: 'Open this team’s plan and contributions' })
        .getAttribute('href'),
    ).toBe('#shape=shape');
    expect(f.submit).not.toHaveBeenCalled();
  });
  it('does not invent completion after a stop request', async () => {
    const f = fixture([{ ...saved, state: 'running', messages: [] }]);
    history.replaceState(null, '', '/#work=first');
    f.view();
    fireEvent.click(await screen.findByRole('button', { name: 'Stop this request' }));
    await screen.findByRole('button', { name: 'Stopping…' });
    expect(f.cancel).toHaveBeenCalledWith(saved.id);
    expect(screen.queryByText('Result ready')).toBeNull();
  });
  it('opens actual team output and tools without imaginary connectors or shared rooms', async () => {
    const f = fixture([
      {
        ...saved,
        messages: [...saved.messages, { id: 2, role: 'tool', content: 'Observed file content.' }],
      },
    ]);
    f.view();
    await screen.findByRole('button', { name: `Open ${saved.input}` });
    fireEvent.click(screen.getByRole('button', { name: 'Blackboard', exact: true }));
    const board = screen.getByRole('log', { name: 'Recorded team output' });
    expect(within(board).getByText('Observed file content.')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Tools & MCPs', exact: true }));
    expect(screen.getByRole('button', { name: 'Manage Files' })).toBeTruthy();
    expect(screen.getByText('No MCP servers are configured on this engine.')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Manage GitHub' })).toBeNull();
  });
  it('filters the live toolkit and opens the exact agent with a saved file grant', async () => {
    const f = fixture();
    vi.mocked(f.client.agentCatalog).mockResolvedValue({
      models: [agent.model],
      harnesses: ['general'],
      tools: ['read_file', 'run_shell'],
      mcp_connections: [],
      max_steps: 8,
      max_seconds: 120,
      max_tokens: 4096,
    });
    f.view();
    await screen.findByRole('button', { name: 'Our team' });
    fireEvent.click(screen.getByRole('button', { name: 'Tools & MCPs', exact: true }));
    fireEvent.change(screen.getByLabelText('Search tools and MCPs'), {
      target: { value: 'commands' },
    });
    expect(screen.getByRole('button', { name: 'Manage Terminal' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Manage Files' })).toBeNull();
    fireEvent.change(screen.getByLabelText('Search tools and MCPs'), { target: { value: '' } });
    fireEvent.click(screen.getByRole('button', { name: 'MCPs', exact: true }));
    expect(screen.queryByRole('button', { name: 'Manage Terminal' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Tools', exact: true }));
    fireEvent.click(screen.getByRole('button', { name: 'Manage Files' }));
    const details = screen.getByRole('complementary', { name: 'Files resource details' });
    expect(within(details).getByText('Read file contents')).toBeTruthy();
    expect(within(details).queryByText('Edit existing files')).toBeNull();
    fireEvent.click(within(details).getByRole('button', { name: 'Mira' }));
    expect(screen.getByRole('heading', { name: 'Mira' })).toBeTruthy();
  });
  it('discovers MCP tools from the restored library and keeps failures visible without granting access', async () => {
    const f = fixture();
    const connection = {
      id: 'library',
      name: 'Library',
      endpoint: 'http://127.0.0.1:8765/mcp',
      status: 'unchecked' as const,
      message: 'Not checked yet',
      tools: [],
    };
    const catalog = {
      models: [agent.model],
      harnesses: ['general'],
      tools: ['read_file'],
      mcp_connections: [connection],
      max_steps: 8,
      max_seconds: 120,
      max_tokens: 4096,
    };
    vi.mocked(f.client.agentCatalog).mockResolvedValue(catalog);
    const create = vi.spyOn(f.client, 'createAgent');
    const discover = vi
      .spyOn(f.client, 'discoverMcp')
      .mockRejectedValueOnce(new Error('Service unavailable'));
    f.view();
    await screen.findByRole('button', { name: 'Our team' });
    fireEvent.click(screen.getByRole('button', { name: 'Tools & MCPs', exact: true }));
    fireEvent.click(screen.getByRole('button', { name: 'Manage Library' }));
    fireEvent.click(screen.getByRole('button', { name: 'Discover Library tools' }));
    expect(await screen.findByRole('alert')).toHaveProperty('textContent', 'Service unavailable');
    const discovered = {
      ...connection,
      status: 'discovered' as const,
      message: '1 read tool discovered',
      tools: [
        {
          id: 'mcp_library_search',
          name: 'search',
          description: 'Search shared knowledge',
          input_schema: {},
        },
      ],
    };
    discover.mockImplementation(async () => {
      vi.mocked(f.client.agentCatalog).mockResolvedValue({
        ...catalog,
        tools: ['read_file', 'mcp_library_search'],
        mcp_connections: [discovered],
      });
      return discovered;
    });
    fireEvent.click(screen.getByRole('button', { name: 'Discover Library tools' }));
    expect(await screen.findByText('search')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Refresh Library tools' })).toBeTruthy();
    expect(screen.queryByRole('alert')).toBeNull();
    expect(discover).toHaveBeenNthCalledWith(1, 'library');
    expect(discover).toHaveBeenNthCalledWith(2, 'library');
    expect(create).not.toHaveBeenCalled();
    expect(screen.getByText('No agent configured with this tool.')).toBeTruthy();
  });
  it('keeps the last engine state visible but stops calling it live after disconnect', async () => {
    const f = fixture([saved]);
    f.view();
    await screen.findByRole('button', { name: `Open ${saved.input}` });
    f.snapshot.mockRejectedValue(new Error('Offline'));
    fireEvent.click(screen.getByRole('button', { name: 'Workspace settings' }));
    fireEvent.click(screen.getByRole('button', { name: 'Reconnect', exact: true }));
    await screen.findByText('Connection lost · showing last recorded state');
    fireEvent.click(screen.getByRole('button', { name: 'Close project details' }));
    expect(screen.getByRole('button', { name: `Open ${saved.input}` })).toBeTruthy();
    fireEvent.change(screen.getByRole('textbox', { name: 'What would you like to work on?' }), {
      target: { value: 'Do not send' },
    });
    expect(screen.getByRole('button', { name: 'Start work' })).toHaveProperty('disabled', true);
  });
  it('requires inspectable effects before approval and sends a scoped decline', async () => {
    const f = fixture();
    const approval = {
      org_id: 'org',
      team_id: 'our-team',
      approval_id: 'approval',
      proposal_digest: 'digest',
      status: 'pending',
      request_id: 'r',
      expires_at: 4102444800,
    };
    f.approvals.mockResolvedValue({ active_stops: [], pending_approvals: [approval], effort: [] });
    const resolve = vi
      .spyOn(f.client, 'resolveApproval')
      .mockResolvedValue({ ...approval, status: 'rejected' });
    f.view();
    fireEvent.click(await screen.findByRole('button', { name: 'Needs you · 1' }));
    expect(screen.getByRole('button', { name: 'Approve unavailable' })).toHaveProperty(
      'disabled',
      true,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Decline request' }));
    await screen.findByText('Request declined');
    expect(resolve).toHaveBeenCalledExactlyOnceWith('approval', false, 'digest');
  });
  it('shows the exact shell action and isolation limits before allowing it once', async () => {
    const f = fixture();
    const approval = {
      org_id: 'org',
      team_id: 'our-team',
      approval_id: 'shell-approval',
      proposal_digest: 'exact-shell-digest',
      status: 'pending',
      request_id: 'shell-request',
      expires_at: 4102444800,
      proposal: {
        command: 'git status --short',
        working_directory: 'C:/work/project',
        shell: 'cmd',
        attempt_id: 'attempt',
        call_id: 'call',
        parameter_digest: 'command-digest',
        confinement_warnings: ['Filesystem isolation is unavailable on this host.'],
      },
    };
    f.approvals.mockResolvedValue({ active_stops: [], pending_approvals: [approval], effort: [] });
    const resolve = vi
      .spyOn(f.client, 'resolveApproval')
      .mockResolvedValue({ ...approval, status: 'approved' });
    f.view();
    fireEvent.click(await screen.findByRole('button', { name: 'Needs you · 1' }));
    expect(screen.getByText('git status --short')).toBeTruthy();
    expect(screen.getByText('C:/work/project')).toBeTruthy();
    expect(screen.getByText('Filesystem isolation is unavailable on this host.')).toBeTruthy();
    expect(resolve).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Allow once' }));
    await screen.findByText('Command approved once');
    expect(resolve).toHaveBeenCalledExactlyOnceWith('shell-approval', true, 'exact-shell-digest');
  });
  it('creates an agent with a partial host toolkit and gives that exact agent its first assignment', async () => {
    const f = fixture();
    const create = vi.spyOn(f.client, 'createAgent').mockImplementation(async (input) => {
      const added = { ...input, key: 'new-agent', id: 'new-id' };
      f.setData({ ...f.getData(), agents: [agent, added] });
      return added;
    });
    f.view();
    fireEvent.click(screen.getByRole('button', { name: 'Agents', exact: true }));
    const add = screen.getByRole('button', { name: 'Create agent' });
    await waitFor(() => expect(add).toHaveProperty('disabled', false));
    fireEvent.click(add);
    fireEvent.change(await screen.findByRole('textbox', { name: 'Name', exact: true }), {
      target: { value: 'June' },
    });
    fireEvent.click(screen.getByRole('checkbox', { name: 'Read files', exact: true }));
    fireEvent.click(screen.getByRole('button', { name: 'Create agent', exact: true }));
    expect(await screen.findByRole('heading', { name: 'June' })).toBeTruthy();
    expect(create).toHaveBeenCalledWith(
      expect.objectContaining({
        name: 'June',
        harness: 'general',
        tools: ['read_file'],
        model: agent.model,
        request_id: expect.any(String),
      }),
    );
    const start = screen.getByRole('button', { name: 'Give June work' });
    await waitFor(() => expect(start).toHaveProperty('disabled', false));
    fireEvent.click(start);
    expect(screen.getByLabelText('Assign to agent')).toHaveProperty('value', 'new-agent');
    fireEvent.change(screen.getByLabelText('What would you like to work on?'), {
      target: { value: 'Read our project notes' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Start work', exact: true }));
    await waitFor(() => expect(f.submit).toHaveBeenCalledOnce());
    expect(f.submit.mock.calls[0][2]).toBe('new-agent');
  });
});

function journeyFixture(waiting = false) {
  const link = {
    source_work_id: 'source',
    root_work_id: 'team-result',
    assignment_key: null,
    title: 'Workshop options',
    depends_on: [],
  };
  const source = {
    ...saved,
    id: 'source',
    purpose: 'explore' as const,
    state: 'not_started' as const,
    input: 'Explore workshops',
    messages: [],
  };
  const root = {
    ...saved,
    id: 'team-result',
    plan: link,
    state: waiting ? ('running' as const) : ('completed' as const),
    messages: waiting
      ? []
      : [
          {
            id: 12,
            role: 'assistant' as const,
            content: 'The team recommends three shorter sessions.',
          },
        ],
  };
  const question = {
    id: 'q',
    work_id: 'contribution',
    source_work_id: 'source',
    attempt_id: 'attempt',
    content: {
      question: 'Who is this for?',
      why: 'The audience changes our comparison.',
      options: ['Beginners', 'Experts'],
    },
    deadline: Math.floor(Date.now() / 1000) + 300,
    answer: null,
    response_id: null,
  };
  const child = {
    ...saved,
    id: 'contribution',
    plan: { ...link, assignment_key: 'compare', title: 'Compare formats' },
    state: waiting ? ('waiting_human' as const) : ('completed' as const),
    human_questions: waiting ? [question] : [],
    messages: waiting
      ? []
      : [
          {
            id: 13,
            role: 'assistant' as const,
            content: 'This is the full comparison contribution.',
          },
        ],
  };
  const f = fixture([source, root, child]);
  const content = {
    title: 'Workshop options',
    summary: 'Compare workshop options.',
    token_budget: 3000,
    open_questions: [],
    assignments: [
      {
        key: 'compare',
        title: 'Compare formats',
        instructions: 'Compare the options.',
        agent_key: agent.key,
        depends_on: [],
        tools: [],
        deliverable: 'A comparison',
        token_budget: 1000,
      },
    ],
  };
  const view: PlanView = {
    plans: [
      {
        work_id: 'source',
        revision: 1,
        brief_revision: 1,
        request_id: 'proposal',
        generation_id: 'generation',
        status: 'agreed',
        content,
        created_by: 'owner',
        agreed_by: 'owner',
        agreement_id: 'agreement',
      },
    ],
    generation: null,
    brief_revision: 1,
    readiness: [],
    execution_available: false,
    execution: {
      receipt: {
        source_work_id: 'source',
        root_work_id: root.id,
        request_id: 'start',
        revision: 1,
        content,
        assignments: [
          {
            assignment_key: 'compare',
            work_id: child.id,
            agent_key: agent.key,
            definition_digest: 'digest',
          },
        ],
      },
      state: root.state,
      root,
      assignments: [child],
      error: null,
    },
  };
  vi.spyOn(f.client, 'plan').mockResolvedValue(view);
  vi.spyOn(f.client, 'briefs').mockResolvedValue([]);
  return { ...f, source, root, child, planView: view, question };
}

it('keeps launched work together without discarding its discussion or inventing a stale alert', () => {
  const f = journeyFixture();
  const model = teamWorkspace(f.getData(), []);
  expect(model.projects).toHaveLength(1);
  expect(model.projects[0].streams.map((s) => s.id)).toEqual(['team-result', 'contribution']);
  expect(model.projects[0].decision).toBeUndefined();
  expect(model.records).toHaveLength(3);
  expect(model.entries.find((e) => e.id === 'source:input')?.projectId).toBe('plan:team-result');
});

it('opens the team result in one click and reads contributions without navigating away', async () => {
  const f = journeyFixture();
  f.view();
  fireEvent.click(await screen.findByRole('button', { name: 'Done Workshop options' }));
  await screen.findByRole('heading', { name: 'Your team’s result' });
  expect(screen.queryByRole('combobox', { name: 'Your discussions' })).toBeNull();
  expect(screen.queryByRole('button', { name: /Needs you/ })).toBeNull();
  expect(screen.queryByRole('tablist')).toBeNull();
  const url = location.hash;
  fireEvent.click(
    within(screen.getByRole('region', { name: 'Team execution' })).getByText('Compare formats', {
      selector: 'strong',
    }),
  );
  expect(screen.getByText('This is the full comparison contribution.')).toBeTruthy();
  expect(location.hash).toBe(url);
  fireEvent.click(screen.getByRole('link', { name: 'Inspect Compare formats' }));
  await screen.findByRole('region', { name: 'Work details' });
  fireEvent.click(screen.getByRole('button', { name: 'Back to previous view' }));
  await screen.findByRole('heading', { name: 'Your team’s result' });
  expect(location.hash).toBe(url);
  expect(f.submit).not.toHaveBeenCalled();
});

it('lets the operator answer a waiting team directly from Needs you', async () => {
  const f = journeyFixture(true);
  const answer = vi
    .spyOn(f.client, 'answerPlanQuestion')
    .mockImplementation(async (_, command) => ({
      ...f.question,
      answer: command.answer,
      response_id: command.request_id,
    }));
  f.view();
  fireEvent.click(await screen.findByRole('button', { name: 'Needs you · 1' }));
  expect(screen.getByText('Who is this for?')).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'Beginners' }));
  fireEvent.click(screen.getByRole('button', { name: 'Send answer' }));
  await screen.findByText('Your answer to Mira');
  expect(answer).toHaveBeenCalledWith(
    'contribution',
    expect.objectContaining({ question_id: 'q', answer: 'Beginners' }),
  );
  expect(f.submit).not.toHaveBeenCalled();
});

it('opens bookmarked team work at its overview and keeps raw coordination inspectable', async () => {
  const f = journeyFixture();
  history.replaceState(null, '', '/#work=team-result');
  f.view();
  await screen.findByRole('heading', { name: 'Your team’s result' });
  fireEvent.click(screen.getByRole('link', { name: 'Inspect coordination activity →' }));
  await screen.findByRole('region', { name: 'Work details' });
  expect(location.hash).toBe('#work=team-result&inspect=1');
  fireEvent.click(screen.getByRole('button', { name: 'Back to previous view' }));
  await screen.findByRole('heading', { name: 'Your team’s result' });
  expect(f.submit).not.toHaveBeenCalled();
});

it('keeps a team result accessible when its original discussion is absent from the snapshot', async () => {
  const f = journeyFixture();
  f.setData({ ...f.getData(), tasks: [f.root, f.child] });
  history.replaceState(null, '', '/#work=team-result');
  f.view();
  await screen.findByRole('heading', { name: 'Your team’s result' });
  expect(screen.getByText('The team recommends three shorter sessions.')).toBeTruthy();
  expect(screen.queryByText('This discussion is unavailable.')).toBeNull();
});

it('starts a new shaping discussion even when earlier team work already exists', async () => {
  const f = journeyFixture();
  f.view();
  await screen.findByRole('button', { name: 'Done Workshop options' });
  fireEvent.click(screen.getByRole('button', { name: 'Shape work together →' }));
  expect(screen.getByRole('textbox', { name: 'What are you working through?' })).toBeTruthy();
  expect(screen.queryByRole('heading', { name: 'Your team’s result' })).toBeNull();
  expect(f.submit).not.toHaveBeenCalled();
});

it('keeps a large work list bounded, searchable, and filterable without hiding the total', async () => {
  const f = fixture(
    Array.from({ length: 48 }, (_, i) => ({
      ...saved,
      id: `effort-${i}`,
      input: `Effort ${i}`,
      state: i % 2 ? 'running' : 'completed',
    })),
  );
  f.view();
  fireEvent.click(await screen.findByRole('button', { name: 'Work', exact: true }));
  const overview = within(screen.getByRole('region', { name: 'Work overview' }));
  await overview.findByText('48 efforts. Keep your attention where it matters.');
  expect(document.querySelectorAll('.work-overview-row')).toHaveLength(30);
  fireEvent.click(overview.getByRole('button', { name: 'Show more · 18 remaining' }));
  expect(document.querySelectorAll('.work-overview-row')).toHaveLength(48);
  fireEvent.click(overview.getByRole('button', { name: 'In progress 24' }));
  expect(document.querySelectorAll('.work-overview-row')).toHaveLength(24);
  fireEvent.change(overview.getByRole('textbox', { name: 'Find work' }), {
    target: { value: 'Effort 47' },
  });
  expect(document.querySelectorAll('.work-overview-row')).toHaveLength(1);
  expect(overview.getByRole('button', { name: /Effort 47 In progress/ })).toBeTruthy();
  expect(overview.queryByRole('button', { name: /Effort 46/ })).toBeNull();
});

it('shows a saved work-team roster with editing and a path to agent management', async () => {
  const f = fixture();
  f.setData({
    ...f.getData(),
    work_teams: [
      {
        id: 'research',
        name: 'Research partners',
        purpose: 'Compare evidence',
        agent_keys: [agent.key],
        revision: 1,
      },
    ],
  });
  f.view();
  await screen.findByRole('button', { name: 'Our team', exact: true });
  fireEvent.click(screen.getByRole('button', { name: 'Teams', exact: true }));
  fireEvent.click(await screen.findByRole('button', { name: /Research partners.*1 agents/ }));
  expect(screen.getByRole('button', { name: 'Edit team' })).toBeTruthy();
  expect(
    within(screen.getByRole('complementary', { name: 'Project details' })).getByText('Mira'),
  ).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'Manage agents' }));
  await screen.findByRole('region', { name: 'Agent roster' });
  expect(screen.getByRole('button', { name: 'Create agent' })).toBeTruthy();
});

it('creates and edits a reusable team from saved agents, then selects it on the map', async () => {
  const f = fixture();
  const jun = { ...agent, key: 'jun', id: 'jun-id', name: 'Jun', purpose: 'Review evidence' };
  const guide = { ...agent, key: 'guide', id: 'guide-id', name: 'The Guide', tools: [] };
  f.setData({
    ...f.getData(),
    work_teams: [],
    shaping_agent_key: 'guide',
    agents: [agent, jun, guide],
  });
  const save = vi.spyOn(f.client, 'saveWorkTeam').mockImplementation(async (r) => {
    const result = {
      id: r.id,
      revision: r.expected_revision + 1,
      name: r.name,
      purpose: r.purpose,
      agent_keys: r.agent_keys,
    };
    f.setData({ ...f.getData(), work_teams: [result] });
    return result;
  });
  const createAgent = vi.spyOn(f.client, 'createAgent');
  f.view();
  fireEvent.click(await screen.findByRole('button', { name: 'Teams', exact: true }));
  const create = await screen.findByRole('button', { name: 'Create team', exact: true });
  await waitFor(() => expect(create).toHaveProperty('disabled', false));
  fireEvent.click(create);
  fireEvent.change(screen.getByLabelText('Team name'), { target: { value: 'Research partners' } });
  fireEvent.click(screen.getByRole('checkbox', { name: /Mira Understand problems/ }));
  fireEvent.click(screen.getByRole('checkbox', { name: /Jun Review evidence/ }));
  expect(screen.queryByRole('checkbox', { name: /The Guide/ })).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: 'Create team', exact: true }));
  await screen.findByRole('button', { name: 'Work with this team' });
  expect(save.mock.calls[0][0]).toMatchObject({
    expected_revision: 0,
    name: 'Research partners',
    agent_keys: ['mira', 'jun'],
  });
  fireEvent.click(screen.getByRole('button', { name: 'Edit team' }));
  fireEvent.change(screen.getByLabelText('Team name'), { target: { value: 'Evidence team' } });
  fireEvent.click(screen.getByRole('checkbox', { name: /Jun Review evidence/ }));
  fireEvent.click(screen.getByRole('button', { name: 'Save team' }));
  await screen.findByRole('heading', { name: 'Evidence team' });
  expect(save.mock.calls[1][0]).toMatchObject({
    id: save.mock.calls[0][0].id,
    expected_revision: 1,
    agent_keys: ['mira'],
  });
  expect(createAgent).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: 'Work with this team' }));
  expect(screen.getByRole('combobox', { name: 'Assign to agent' })).toHaveProperty(
    'value',
    `team:${save.mock.calls[0][0].id}`,
  );
  expect(screen.getByText(/The Guide will coordinate Mira/)).toBeTruthy();
});

it('keeps a team recipient and roster revision across an uncertain send and reopening', async () => {
  const f = fixture();
  const guide = { ...agent, key: 'guide', id: 'guide-id', name: 'The Guide', tools: [] };
  const team = {
    id: 'research',
    revision: 1,
    name: 'Research partners',
    purpose: 'Read evidence',
    agent_keys: ['mira'],
  };
  f.setData({
    ...f.getData(),
    work_teams: [team],
    shaping_agent_key: 'guide',
    agents: [agent, guide],
  });
  const onShape = vi.fn();
  const mount = () =>
    render(
      <LocalEngineProvider client={f.client}>
        <WorkComposer recipient="team:research" onAccepted={() => {}} onShape={onShape} />
      </LocalEngineProvider>,
    );
  f.submit.mockRejectedValueOnce(new Error('Connection dropped'));
  let view = mount();
  fireEvent.change(await screen.findByRole('textbox'), {
    target: { value: 'Compare the evidence' },
  });
  const send = screen.getByRole('button', { name: 'Send to the Guide' });
  await waitFor(() => expect(send).toHaveProperty('disabled', false));
  fireEvent.click(send);
  await screen.findByRole('alert');
  const first = f.submit.mock.calls[0];
  expect(first.slice(2)).toEqual(['guide', undefined, 'explore', { id: 'research', revision: 1 }]);
  view.unmount();
  f.setData({ ...f.getData(), work_teams: [{ ...team, revision: 2, name: 'Changed roster' }] });
  f.submit.mockImplementationOnce(async (id, input, key, parent, purpose) => ({
    ...saved,
    id,
    input,
    agent_key: key,
    parent_id: parent,
    purpose,
    work_team: team,
  }));
  view = mount();
  const retry = await screen.findByRole('button', { name: 'Retry work request' });
  await waitFor(() => expect(retry).toHaveProperty('disabled', false));
  fireEvent.click(retry);
  await waitFor(() => expect(onShape).toHaveBeenCalledWith(first[0]));
  expect(f.submit.mock.calls[1]).toEqual(first);
  view.unmount();
});

it('groups separate plans by their saved team without using the current roster as activity', () => {
  const f = fixture();
  const team = {
    id: 'research',
    revision: 1,
    name: 'Research',
    purpose: 'Evidence',
    agent_keys: ['mira'],
  };
  const tasks = ['one', 'two'].map((id) => ({
    ...saved,
    id,
    work_team: team,
    plan: {
      source_work_id: `source-${id}`,
      root_work_id: id,
      assignment_key: null,
      title: id,
      depends_on: [],
    },
  }));
  const projected = teamWorkspace(
    { ...f.getData(), tasks, work_teams: [{ ...team, revision: 2, agent_keys: ['someone-new'] }] },
    [],
  );
  expect(projected.projects).toHaveLength(2);
  expect(projected.projects.map((p) => p.area?.id)).toEqual(['roster:research', 'roster:research']);
  expect(projected.projects.every((p) => p.people.every((a) => a.agent.id === 'mira'))).toBe(true);
});
