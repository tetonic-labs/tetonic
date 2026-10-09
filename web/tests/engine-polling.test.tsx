import { afterEach, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { LocalEngineProvider, useLocalEngine } from '../src/context/LocalEngineContext';
import { LocalEngine } from '../src/engine/client';
import type { AgentCatalog, EngineWorkspace } from '../src/engine/contracts';

const catalog: AgentCatalog = {
  models: ['first-model'],
  harnesses: ['general'],
  max_steps: 8,
  max_seconds: 60,
  max_tokens: 4000,
};
const workspace: EngineWorkspace = {
  organization: 'Org',
  team_id: 'team',
  team_name: 'Team',
  agent_id: 'one',
  agent_name: 'Worker',
  model: 'test',
  input_limit: 12000,
  tasks: [],
  agents: [],
};
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((ok, fail) => {
    resolve = ok;
    reject = fail;
  });
  return { promise, resolve, reject };
}
function fixture() {
  const client = new LocalEngine('test');
  vi.spyOn(client, 'snapshot').mockResolvedValue(workspace);
  vi.spyOn(client, 'agentCatalog').mockResolvedValue(catalog);
  vi.spyOn(client, 'workItems').mockResolvedValue([]);
  vi.spyOn(client, 'teams').mockResolvedValue([]);
  vi.spyOn(client, 'approvals').mockResolvedValue({
    active_stops: [],
    pending_approvals: [],
    effort: [],
  });
  return client;
}
function Probe() {
  const engine = useLocalEngine();
  return (
    <>
      <output aria-label="Connection">{engine.isConnected ? 'Connected' : 'Disconnected'}</output>
      <output aria-label="Workspace">{engine.workspace?.organization}</output>
      <output aria-label="Models">{engine.catalog?.models.join(', ')}</output>
      <output aria-label="Read failures">{JSON.stringify(engine.readErrors)}</output>
      <button onClick={() => void engine.refresh()}>Refresh</button>
      <button onClick={engine.reconnect}>Reconnect</button>
    </>
  );
}
async function advance(ms = 0) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  sessionStorage.clear();
  history.replaceState(null, '', '/');
});

it('keeps live work updating while model discovery is still pending', async () => {
  vi.useFakeTimers();
  const client = fixture();
  const pending = deferred<AgentCatalog>();
  vi.mocked(client.agentCatalog).mockReturnValue(pending.promise);
  render(
    <LocalEngineProvider client={client}>
      <Probe />
    </LocalEngineProvider>,
  );
  await advance();
  expect(screen.getByLabelText('Connection').textContent).toBe('Connected');
  vi.mocked(client.snapshot).mockResolvedValue({ ...workspace, organization: 'Updated live work' });
  await advance(1500);
  expect(screen.getByLabelText('Workspace').textContent).toBe('Updated live work');
  expect(client.agentCatalog).toHaveBeenCalledTimes(1);
  await act(async () => {
    pending.resolve(catalog);
  });
  expect(screen.getByLabelText('Models').textContent).toBe('first-model');
});

it('reuses discovery between live polls but immediately reloads after an explicit refresh', async () => {
  vi.useFakeTimers();
  const client = fixture();
  render(
    <LocalEngineProvider client={client}>
      <Probe />
    </LocalEngineProvider>,
  );
  await advance(6000);
  expect(client.snapshot).toHaveBeenCalledTimes(5);
  expect(client.agentCatalog).toHaveBeenCalledTimes(1);
  vi.mocked(client.agentCatalog).mockResolvedValue({ ...catalog, models: ['newly-added-model'] });
  fireEvent.click(screen.getByRole('button', { name: 'Refresh' }));
  await advance();
  expect(client.agentCatalog).toHaveBeenCalledTimes(2);
  expect(screen.getByLabelText('Models').textContent).toBe('newly-added-model');
  await advance(30000);
  expect(client.agentCatalog).toHaveBeenCalledTimes(3);
});

it('retains a catalog failure through live polls and recovers without disconnecting the workspace', async () => {
  vi.useFakeTimers();
  const client = fixture();
  vi.mocked(client.agentCatalog).mockRejectedValueOnce(new Error('Discovery unavailable'));
  render(
    <LocalEngineProvider client={client}>
      <Probe />
    </LocalEngineProvider>,
  );
  await advance(3000);
  expect(screen.getByLabelText('Connection').textContent).toBe('Connected');
  expect(screen.getByLabelText('Read failures').textContent).toContain('Discovery unavailable');
  await advance(3000);
  expect(screen.getByLabelText('Models').textContent).toBe('first-model');
  expect(screen.getByLabelText('Read failures').textContent).toBe('{}');
});

it('ignores an older catalog response superseded by a settings refresh and aborts on close', async () => {
  vi.useFakeTimers();
  const client = fixture();
  const pending = deferred<AgentCatalog>();
  vi.mocked(client.agentCatalog).mockReturnValueOnce(pending.promise);
  const page = render(
    <LocalEngineProvider client={client}>
      <Probe />
    </LocalEngineProvider>,
  );
  await advance();
  const oldSignal = vi.mocked(client.agentCatalog).mock.calls[0][0];
  fireEvent.click(screen.getByRole('button', { name: 'Refresh' }));
  await advance();
  expect(oldSignal?.aborted).toBe(true);
  await act(async () => {
    pending.resolve({ ...catalog, models: ['outdated-model'] });
  });
  expect(screen.getByLabelText('Models').textContent).toBe('first-model');
  page.unmount();
  const reads = vi.mocked(client.agentCatalog).mock.calls.length;
  await advance(60000);
  expect(client.agentCatalog).toHaveBeenCalledTimes(reads);
});

it.each(['resolve', 'reject'] as const)(
  'isolates late catalog %s responses when switching workspace connections',
  async (outcome) => {
    vi.useFakeTimers();
    const pending = deferred<Response>();
    const oldToken = 'a'.repeat(64);
    const newToken = 'b'.repeat(64);
    sessionStorage.setItem('tetonic_local_session', oldToken);
    let oldSignal: AbortSignal | undefined;
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string, options: RequestInit) => {
        const current =
          (options.headers as Record<string, string>).Authorization === `Bearer ${newToken}`;
        if (url.endsWith('/agent-catalog') && !current) {
          oldSignal = options.signal as AbortSignal;
          return pending.promise;
        }
        const value = url.endsWith('/agent-catalog')
          ? { ...catalog, models: ['new-workspace-model'] }
          : url.endsWith('/workspace')
            ? { ...workspace, organization: current ? 'New workspace' : 'Old workspace' }
            : url.endsWith('/approvals')
              ? { active_stops: [], pending_approvals: [], effort: [] }
              : [];
        return new Response(JSON.stringify(value));
      }),
    );
    render(
      <LocalEngineProvider>
        <Probe />
      </LocalEngineProvider>,
    );
    await advance();
    expect(screen.getByLabelText('Workspace').textContent).toBe('Old workspace');
    history.replaceState(null, '', `/#connect=${newToken}`);
    fireEvent(window, new HashChangeEvent('hashchange'));
    await advance();
    expect(oldSignal?.aborted).toBe(true);
    await act(async () => {
      if (outcome === 'resolve')
        pending.resolve(
          new Response(JSON.stringify({ ...catalog, models: ['private-old-model'] })),
        );
      else pending.reject(new Error('Old connection failed'));
    });
    expect(screen.getByLabelText('Workspace').textContent).toBe('New workspace');
    expect(screen.getByLabelText('Models').textContent).toBe('new-workspace-model');
    expect(screen.getByLabelText('Read failures').textContent).toBe('{}');
  },
);
