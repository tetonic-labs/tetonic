import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { CapabilityPolicyEditor } from '../src/components/views/CapabilityPolicyEditor';
import { LocalEngine } from '../src/engine/client';
import { EngineRequestError } from '../src/engine/failure';
import type { ScopedCapabilityPolicy } from '../src/engine/contracts';

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});
function setup(rows: ScopedCapabilityPolicy[] = []) {
  const client = new LocalEngine('fixture');
  const read = vi.spyOn(client, 'capabilityPolicies').mockResolvedValue(rows);
  const save = vi
    .spyOn(client, 'saveCapabilityPolicy')
    .mockImplementation(async (r) => ({
      scope: r.scope,
      scope_id: r.scope_id,
      revision: r.expected_revision + 1,
      policy: r.policy,
    }));
  render(<CapabilityPolicyEditor client={client} scope="agent" scopeId="robin" />);
  return { save, read };
}
it('shows workspace ceilings while retaining agent choices and sends stable identity', async () => {
  const { save } = setup([
    { scope: 'workspace', scope_id: '', revision: 1, policy: { tier: 'read_only', overrides: {} } },
  ]);
  await waitFor(() =>
    expect((screen.getByLabelText('How much freedom?') as HTMLSelectElement).disabled).toBe(false),
  );
  fireEvent.change(screen.getByLabelText('How much freedom?'), { target: { value: 'automatic' } });
  expect(screen.getAllByText('Blocked · Workspace')).toHaveLength(3);
  fireEvent.click(screen.getByRole('button', { name: 'Customize by capability' }));
  fireEvent.change(screen.getByLabelText('Read files permission'), { target: { value: 'ask' } });
  fireEvent.click(screen.getByRole('button', { name: 'Save permissions' }));
  await waitFor(() =>
    expect(save).toHaveBeenCalledWith(
      expect.objectContaining({
        scope: 'agent',
        scope_id: 'robin',
        expected_revision: 0,
        policy: { tier: 'automatic', overrides: { file_read: 'ask' } },
      }),
    ),
  );
  expect(await screen.findByText(/Saved. These limits/)).toBeTruthy();
});
it('retries the same unconfirmed request and locks edits until confirmation', async () => {
  const { save } = setup();
  save.mockRejectedValueOnce(new Error('Connection lost'));
  await waitFor(() =>
    expect((screen.getByLabelText('How much freedom?') as HTMLSelectElement).disabled).toBe(false),
  );
  fireEvent.change(screen.getByLabelText('How much freedom?'), {
    target: { value: 'review_changes' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Save permissions' }));
  await screen.findByText('Connection lost');
  expect((screen.getByLabelText('How much freedom?') as HTMLSelectElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole('button', { name: 'Retry permission save' }));
  await screen.findByText(/Saved. These limits/);
  expect(save.mock.calls[1][0]).toEqual(save.mock.calls[0][0]);
});
it('requires a reload on revision conflict and never reports an unconfirmed save', async () => {
  const { save, read } = setup();
  save.mockRejectedValueOnce(new EngineRequestError('Permissions changed', 409));
  await waitFor(() =>
    expect((screen.getByLabelText('How much freedom?') as HTMLSelectElement).disabled).toBe(false),
  );
  fireEvent.change(screen.getByLabelText('How much freedom?'), { target: { value: 'read_only' } });
  fireEvent.click(screen.getByRole('button', { name: 'Save permissions' }));
  await screen.findByText('Permissions changed');
  expect(screen.queryByText(/Saved. These limits/)).toBeNull();
  expect((screen.getByLabelText('How much freedom?') as HTMLSelectElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole('button', { name: 'Reload permissions' }));
  await waitFor(() => expect(read).toHaveBeenCalledTimes(2));
});
it('resetting to inherited limits writes a durable null at the current revision', async () => {
  const { save } = setup([
    {
      scope: 'agent',
      scope_id: 'robin',
      revision: 4,
      policy: { tier: 'review_changes', overrides: { shell: 'deny' } },
    },
  ]);
  await waitFor(() =>
    expect((screen.getByLabelText('How much freedom?') as HTMLSelectElement).disabled).toBe(false),
  );
  fireEvent.change(screen.getByLabelText('How much freedom?'), { target: { value: 'inherit' } });
  fireEvent.click(screen.getByRole('button', { name: 'Save permissions' }));
  await waitFor(() =>
    expect(save).toHaveBeenCalledWith(
      expect.objectContaining({ expected_revision: 4, policy: null }),
    ),
  );
});
