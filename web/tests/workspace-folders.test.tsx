import { afterEach, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { WorkspaceFolders } from '../src/components/team-work/WorkspaceFolders';
import { LocalEngine } from '../src/engine/client';
import type { EngineAgent } from '../src/engine/contracts';

afterEach(() => vi.restoreAllMocks());
function setup(locked = false, unavailable: string[] = []) {
  const client = new LocalEngine('fixture');
  const catalog = {
    revision: 2,
    paths: ['C:/research'],
    configured_paths: ['C:/managed'],
    unavailable_paths: unavailable,
    can_manage: !locked,
  };
  vi.spyOn(client, 'workspaceFolders').mockResolvedValue(catalog);
  const save = vi
    .spyOn(client, 'saveWorkspaceFolders')
    .mockImplementation(async (input) => ({ ...catalog, revision: 3, paths: input.paths }));
  const changed = vi.fn(async () => {});
  render(
    <WorkspaceFolders
      client={client}
      agents={[{ key: 'reader', name: 'Researcher', workspace_root: 'C:/research' } as EngineAgent]}
      onChanged={changed}
    />,
  );
  return { client, save, changed };
}

it('explains affected agents and removes only the chosen approval after confirmation', async () => {
  const { save, changed } = setup();
  await screen.findByText('C:/research');
  expect(screen.getByText(/Used by Researcher/)).toBeTruthy();
  await userEvent.click(screen.getByRole('button', { name: 'Remove C:/research' }));
  expect(save).not.toHaveBeenCalled();
  expect(screen.getByText(/Already running work keeps its access/)).toBeTruthy();
  await userEvent.click(screen.getByRole('button', { name: 'Remove folder' }));
  await waitFor(() => expect(changed).toHaveBeenCalledOnce());
  expect(save.mock.calls[0][0]).toMatchObject({ expected_revision: 2, paths: [] });
  expect(screen.queryByText('C:/research')).toBeNull();
  expect(screen.getByText('C:/managed')).toBeTruthy();
});

it('distinguishes a saved approval from a folder that is currently usable', async () => {
  setup(false, ['C:/research']);
  await screen.findByText(/Unavailable · Reconnect this folder or remove it/);
  expect(screen.getByRole('button', { name: 'Remove C:/research' })).toBeTruthy();
});

it('keeps the same mutation identity on an uncertain retry and can cancel without a write', async () => {
  const { save } = setup();
  await userEvent.click(await screen.findByRole('button', { name: 'Remove C:/research' }));
  await userEvent.click(screen.getByRole('button', { name: 'Keep folder' }));
  expect(save).not.toHaveBeenCalled();
  save.mockRejectedValueOnce(new Error('Connection interrupted. Retry or refresh to confirm.'));
  await userEvent.click(screen.getByRole('button', { name: 'Remove C:/research' }));
  await userEvent.click(screen.getByRole('button', { name: 'Remove folder' }));
  await screen.findByRole('alert');
  await userEvent.click(screen.getByRole('button', { name: 'Remove folder' }));
  await waitFor(() => expect(save).toHaveBeenCalledTimes(2));
  expect(save.mock.calls[0][0]).toEqual(save.mock.calls[1][0]);
});

it('does not expose modification controls for host-managed folders', async () => {
  setup(true);
  await screen.findByText('C:/research');
  expect(screen.queryByRole('button', { name: /Remove/ })).toBeNull();
  expect(screen.queryByRole('button', { name: 'Add a working folder' })).toBeNull();
  expect(screen.getByText(/Disabled by host/)).toBeTruthy();
});

it('does not silently retry stale removal against a new catalog', async () => {
  const { client, save } = setup();
  save.mockRejectedValue(new Error('Folders changed. Refresh before editing.'));
  await userEvent.click(await screen.findByRole('button', { name: 'Remove C:/research' }));
  await userEvent.click(screen.getByRole('button', { name: 'Remove folder' }));
  await screen.findByRole('alert');
  vi.mocked(client.workspaceFolders).mockResolvedValue({
    revision: 3,
    paths: ['C:/research', 'C:/new'],
    configured_paths: ['C:/managed'],
    can_manage: true,
  });
  await userEvent.click(screen.getByRole('button', { name: 'Refresh folders' }));
  await screen.findByText('C:/new');
  expect(screen.queryByRole('group', { name: 'Confirm folder removal' })).toBeNull();
  expect(save).toHaveBeenCalledOnce();
});
