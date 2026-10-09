import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { LocalEngine } from '../src/engine/client';
import { type EngineWorkspace, type WorkUsage } from '../src/engine/contracts';
import { LocalEngineProvider } from '../src/context/LocalEngineContext';
import { UsagePanel } from '../src/components/team-work/UsagePanel';

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});
const row: WorkUsage = {
  work_id: 'work',
  title: 'Compare the approaches',
  purpose: 'work',
  budget: {
    token_limit: 1000,
    reserved_tokens: 1000,
    delegated_tokens: 0,
    available_tokens: 0,
    root_work_id: 'work',
  },
  input_tokens: 120,
  output_tokens: 80,
  calls: 2,
  pending_calls: 0,
  unknown_calls: 1,
  held_tokens: 800,
  released_tokens: 0,
  over_limit: false,
};
function setup(rows: WorkUsage[] = [row], missing = false) {
  const client = new LocalEngine('test');
  const data: EngineWorkspace = {
    organization: 'org',
    team_id: 'team',
    team_name: 'Team',
    agent_id: 'a',
    agent_name: 'Mira',
    model: 'test',
    input_limit: 12000,
    agents: [],
    tasks: [],
    usage: rows,
    budget_setting: { revision: 1, token_limit: 4000 },
    budget_max_tokens: 4096,
  };
  if (missing) data.usage = undefined;
  vi.spyOn(client, 'snapshot').mockImplementation(async () => ({ ...data }));
  vi.spyOn(client, 'agentCatalog').mockResolvedValue({
    models: [],
    harnesses: [],
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
  const onWork = vi.fn();
  render(
    <LocalEngineProvider client={client}>
      <UsagePanel onWork={onWork} />
    </LocalEngineProvider>,
  );
  return { client, data, onWork };
}
describe('usage from the real workspace contract', () => {
  it('distinguishes reports, held allowance and unknown usage without inventing a bill', async () => {
    const { onWork } = setup();
    await screen.findByText('Tokens reported');
    expect(screen.getAllByText('≥ 200').length).toBeGreaterThan(0);
    expect(screen.getAllByText('800').length).toBeGreaterThan(0);
    fireEvent.click(screen.getByText('Compare the approaches'));
    expect(screen.getByText(/A request ended without a complete usage report/)).toBeTruthy();
    expect(screen.getByText('Available')).toBeTruthy();
    expect(screen.queryByText(/\$/)).toBeNull();
    fireEvent.click(screen.getByText('Open work →'));
    expect(onWork).toHaveBeenCalledWith('work');
  });
  it('keeps old untracked work distinct from a zero-token completed request', async () => {
    setup([
      {
        ...row,
        work_id: 'legacy',
        title: 'Earlier work',
        budget: null,
        calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        held_tokens: 0,
        unknown_calls: 0,
      },
    ]);
    await screen.findByText('Earlier work');
    expect(screen.getByText('Not tracked')).toBeTruthy();
    fireEvent.click(screen.getByText('Earlier work'));
    expect(screen.getByText('Usage not tracked for this request')).toBeTruthy();
  });
  it('retains the same operation after a lost save response and waits for an exact receipt', async () => {
    const { client, data } = setup([]);
    const save = vi
      .spyOn(client, 'setBudgetSetting')
      .mockRejectedValueOnce(new Error('Reply lost; retry the same request.'))
      .mockImplementation(async (command) => {
        data.budget_setting = {
          revision: command.expected_revision + 1,
          token_limit: command.token_limit,
        };
        return data.budget_setting;
      });
    await screen.findByText('Tokens reported');
    fireEvent.click(screen.getByText('Allowance for new requests'));
    fireEvent.change(screen.getByLabelText('Tokens per request'), { target: { value: '2000' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save allowance' }));
    await screen.findByRole('alert');
    expect(screen.queryByText(/Saved\. This applies/)).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Save allowance' }));
    await screen.findByText(/Saved\. This applies/);
    expect(save.mock.calls[0][0]).toEqual(save.mock.calls[1][0]);
    expect(save.mock.calls[0][0].expected_revision).toBe(1);
    expect(save.mock.calls[0][0].token_limit).toBe(2000);
  });
  it('never presents a missing usage API as zero usage', async () => {
    const { client } = setup([], true);
    await waitFor(() => expect(client.snapshot).toHaveBeenCalled());
    expect(screen.getByText(/Usage reporting is not available/)).toBeTruthy();
    expect(screen.queryByText('Tokens reported')).toBeNull();
  });
});
