import { afterEach, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { SavedWorkDrafts } from '../src/components/team-work/SavedWorkDrafts';
import type { EngineTask, WorkDraftView } from '../src/engine/contracts';

const state = vi.hoisted(() => ({ isConnected: true, client: { readWorkDraft: vi.fn() } }));
vi.mock('../src/context/LocalEngineContext', () => ({ useLocalEngine: () => state }));
const task: EngineTask = {
  id: 'failed-work',
  input: 'Make the result',
  agent_key: 'one',
  agent_name: 'One',
  state: 'failed',
  run_id: 'run',
  sequence: 4,
  messages: [],
  saved_drafts: [{ artifact_id: 'draft', file_count: 1 }],
};
const files = [
  { path: 'result.txt', operation: 'replace', byte_count: 20000, text_available: true },
];
const listing: WorkDraftView = { artifact_id: 'draft', files, page: null };
afterEach(() => {
  cleanup();
  vi.resetAllMocks();
  state.isConnected = true;
  sessionStorage.clear();
});

it('inspects a saved draft in place, pages long text, and never renders file text as markup', async () => {
  state.client.readWorkDraft.mockImplementation(async (_work, _artifact, file, offset) => ({
    ...listing,
    page:
      file === undefined
        ? null
        : {
            file,
            offset,
            content: offset ? 'last section' : '<script>untrusted content</script>',
            next_offset: offset ? null : 16384,
          },
  }));
  render(<SavedWorkDrafts task={task} />);
  expect(state.client.readWorkDraft).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: 'Review saved draft' }));
  fireEvent.click(await screen.findByRole('button', { name: /result.txt/ }));
  expect(await screen.findByText('<script>untrusted content</script>')).toBeTruthy();
  expect(document.querySelector('script')).toBeNull();
  expect(screen.getByText(/not a verified result/)).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'Next section' }));
  expect(await screen.findByText('last section')).toBeTruthy();
  expect(state.client.readWorkDraft).toHaveBeenLastCalledWith(
    'failed-work',
    'draft',
    0,
    16384,
    expect.any(AbortSignal),
  );
  fireEvent.click(screen.getByRole('button', { name: 'Previous section' }));
  expect(await screen.findByText('<script>untrusted content</script>')).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'Close draft' }));
  expect(screen.queryByText('<script>untrusted content</script>')).toBeNull();
});

it('discards an in-flight read when the engine disconnects', async () => {
  let resolve!: (v: WorkDraftView) => void;
  state.client.readWorkDraft.mockImplementation(
    () =>
      new Promise<WorkDraftView>((done) => {
        resolve = done;
      }),
  );
  const view = render(<SavedWorkDrafts task={task} />);
  fireEvent.click(screen.getByRole('button', { name: 'Review saved draft' }));
  const signal = state.client.readWorkDraft.mock.calls[0][4] as AbortSignal;
  state.isConnected = false;
  view.rerender(<SavedWorkDrafts task={task} />);
  await waitFor(() => expect(signal.aborted).toBe(true));
  await act(async () => {
    resolve(listing);
  });
  expect(screen.queryByText('result.txt')).toBeNull();
  expect(screen.getByRole('button', { name: 'Review saved draft' }).hasAttribute('disabled')).toBe(
    true,
  );
});

it('shows a failed read without claiming a lost draft or offering to replay work', async () => {
  state.client.readWorkDraft
    .mockRejectedValueOnce(new Error('This saved draft is unavailable.'))
    .mockResolvedValue(listing);
  render(<SavedWorkDrafts task={task} />);
  fireEvent.click(screen.getByRole('button', { name: 'Review saved draft' }));
  expect(await screen.findByRole('alert')).toHaveProperty(
    'textContent',
    expect.stringContaining('unavailable'),
  );
  fireEvent.click(screen.getByRole('button', { name: 'Try reading again' }));
  expect(await screen.findByRole('button', { name: /result.txt/ })).toBeTruthy();
  expect(state.client.readWorkDraft).toHaveBeenCalledTimes(2);
});
