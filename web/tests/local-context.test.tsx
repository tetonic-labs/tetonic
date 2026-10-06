import { afterEach, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { LocalEngineProvider, useLocalEngine } from '../src/context/LocalEngineContext';
import { LocalEngine } from '../src/lib/localEngine';

afterEach(() => vi.restoreAllMocks());

it('creates work with a UUID accepted by the existing engine contract', async () => {
  vi.spyOn(LocalEngine.prototype, 'snapshot').mockResolvedValue({
    organization: 'Local',
    team_id: 'team',
    team_name: 'Personal',
    agent_id: 'agent',
    agent_name: 'Local assistant',
    model: 'local-model',
    input_limit: 12000,
    agents: [],
    tasks: [],
  });
  vi.spyOn(LocalEngine.prototype, 'agentCatalog').mockResolvedValue({
    models: [],
    harnesses: ['general'],
    max_steps: 8,
    max_seconds: 120,
    max_tokens: 4096,
  });
  vi.spyOn(LocalEngine.prototype, 'workItems').mockResolvedValue([]);
  vi.spyOn(LocalEngine.prototype, 'approvals').mockResolvedValue({
    active_stops: [],
    pending_approvals: [],
    effort: [],
  });
  vi.spyOn(LocalEngine.prototype, 'digest').mockResolvedValue({
    summary: '',
    total_work_items: 0,
    completed_items: 0,
    active_items: 0,
    pending_approvals_count: 0,
    highlights: [],
  });
  vi.spyOn(LocalEngine.prototype, 'teams').mockResolvedValue([]);
  const create = vi
    .spyOn(LocalEngine.prototype, 'createWorkItem')
    .mockImplementation(async (input) => {
      expect(input.id).toMatch(
        /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
      );
      return { ...input, status: 'open', version: 1, request_id: input.id };
    });
  function Probe() {
    const engine = useLocalEngine();
    return (
      <button
        disabled={!engine.isConnected}
        onClick={() => void engine.createWorkItem('Compare supplied documents')}
      >
        Create work
      </button>
    );
  }
  render(
    <LocalEngineProvider>
      <Probe />
    </LocalEngineProvider>,
  );
  await waitFor(() => expect(screen.getByRole('button')).toHaveProperty('disabled', false));
  await userEvent.click(screen.getByRole('button', { name: 'Create work' }));
  expect(create).toHaveBeenCalledOnce();
  expect(create.mock.calls[0][0].title).toBe('Compare supplied documents');
});
