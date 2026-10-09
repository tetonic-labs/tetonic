import { act, renderHook } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { useWorkspaceDraft } from '../src/components/workspace/useWorkspaceDraft';
import type { EngineTask } from '../src/engine/contracts';

afterEach(() => sessionStorage.clear());
afterEach(() => vi.restoreAllMocks());

const accepted = { id: 'accepted', state: 'running', messages: [] } as unknown as EngineTask;

it.each(['accepted', 'failed'])(
  'preserves another conversation draft when a late send is %s after navigation',
  async (outcome) => {
    let resolve!: (task: EngineTask) => void;
    let reject!: (error: Error) => void;
    const submit = vi.fn(
      () =>
        new Promise<EngineTask>((done, fail) => {
          resolve = done;
          reject = fail;
        }),
    );
    const first = renderHook(() => useWorkspaceDraft('navigation'));
    act(() => first.result.current.edit('conversation-a', 'Send this thought.'));
    let sending!: Promise<void>;
    act(() => {
      sending = first.result.current.send('conversation-a', 'guide', undefined, submit, vi.fn());
    });
    first.unmount();

    const second = renderHook(() => useWorkspaceDraft('navigation'));
    act(() => second.result.current.edit('conversation-b', 'Keep my next idea.'));
    await act(async () => {
      if (outcome === 'accepted') resolve(accepted);
      else reject(new Error('Connection interrupted'));
      await sending;
    });
    expect(second.result.current.drafts['conversation-a'].pending !== undefined).toBe(
      outcome === 'failed',
    );
    expect(second.result.current.drafts['conversation-b'].text).toBe('Keep my next idea.');
    second.unmount();

    const reopened = renderHook(() => useWorkspaceDraft('navigation'));
    expect(reopened.result.current.drafts['conversation-b']?.text).toBe('Keep my next idea.');
    if (outcome === 'accepted')
      expect(reopened.result.current.drafts['conversation-a']).toEqual({ text: '' });
    else
      expect(reopened.result.current.drafts['conversation-a'].error).toBe('Connection interrupted');
  },
);

it('ignores an older failure after a retry was accepted and the user started a new draft', async () => {
  let reject!: (error: Error) => void;
  const submit = vi.fn(() => new Promise<EngineTask>((_, fail) => (reject = fail)));
  const first = renderHook(() => useWorkspaceDraft('retry'));
  act(() => first.result.current.edit('conversation', 'Original message.'));
  let sending!: Promise<void>;
  act(() => {
    sending = first.result.current.send('conversation', 'guide', undefined, submit, vi.fn());
  });
  const requestId = first.result.current.drafts.conversation.pending!.id;
  first.unmount();

  const reopened = renderHook(() => useWorkspaceDraft('retry'));
  const retry = vi.fn().mockResolvedValue(accepted);
  await act(async () => {
    await reopened.result.current.send('conversation', 'guide', undefined, retry, vi.fn());
  });
  expect(retry).toHaveBeenCalledExactlyOnceWith(
    'Original message.',
    'guide',
    undefined,
    requestId,
    undefined,
  );
  act(() => reopened.result.current.edit('conversation', 'A new thought after acceptance.'));
  await act(async () => {
    reject(new Error('Old request timed out'));
    await sending;
  });
  expect(reopened.result.current.drafts.conversation).toEqual({
    text: 'A new thought after acceptance.',
  });
  reopened.unmount();
  const restored = renderHook(() => useWorkspaceDraft('retry'));
  expect(restored.result.current.drafts.conversation).toEqual({
    text: 'A new thought after acceptance.',
  });
});

it('keeps draft updates isolated to their engine connection scope', () => {
  const first = renderHook(() => useWorkspaceDraft('engine-one'));
  const second = renderHook(() => useWorkspaceDraft('engine-two'));
  act(() => first.result.current.edit('conversation', 'First workspace thought.'));
  expect(second.result.current.drafts.conversation).toBeUndefined();
  act(() => second.result.current.edit('conversation', 'Second workspace thought.'));
  expect(first.result.current.drafts.conversation.text).toBe('First workspace thought.');
});

it('still opens accepted work when the original send settles before a reopened retry', async () => {
  let acceptOriginal!: (task: EngineTask) => void;
  let acceptRetry!: (task: EngineTask) => void;
  const first = renderHook(() => useWorkspaceDraft('overlapping-retry'));
  act(() => first.result.current.edit('conversation', 'Send once.'));
  let original!: Promise<void>;
  act(() => {
    original = first.result.current.send(
      'conversation',
      'guide',
      undefined,
      () =>
        new Promise((resolve) => {
          acceptOriginal = resolve;
        }),
      vi.fn(),
    );
  });
  first.unmount();
  const reopened = renderHook(() => useWorkspaceDraft('overlapping-retry'));
  const onAccepted = vi.fn();
  let retried!: Promise<void>;
  act(() => {
    retried = reopened.result.current.send(
      'conversation',
      'guide',
      undefined,
      () =>
        new Promise((resolve) => {
          acceptRetry = resolve;
        }),
      onAccepted,
    );
  });
  await act(async () => {
    acceptOriginal(accepted);
    await original;
  });
  await act(async () => {
    acceptRetry(accepted);
    await retried;
  });
  expect(onAccepted).toHaveBeenCalledExactlyOnceWith(accepted);
  expect(reopened.result.current.drafts.conversation).toEqual({ text: '' });
});

it('sends the current text when storage fails instead of reverting to an older saved draft', async () => {
  const writer = renderHook(() => useWorkspaceDraft('storage-failure'));
  act(() => writer.result.current.edit('conversation', 'Old text.'));
  vi.spyOn(Object.getPrototypeOf(sessionStorage), 'setItem').mockImplementation(() => {
    throw new Error('Storage full');
  });
  act(() => writer.result.current.edit('conversation', 'The thought I actually want to send.'));
  expect(writer.result.current.storageWarning).toBe(true);
  const submit = vi.fn().mockResolvedValue(accepted);
  await act(async () => {
    await writer.result.current.send('conversation', 'guide', undefined, submit, vi.fn());
  });
  expect(submit).toHaveBeenCalledWith(
    'The thought I actually want to send.',
    'guide',
    undefined,
    expect.any(String),
    undefined,
  );
  expect(writer.result.current.drafts.conversation.text).toBe('');
});
