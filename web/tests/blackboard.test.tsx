import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { ProjectBlackboard } from '../src/components/team-work/ProjectBlackboard';
import type { BlackboardThread } from '../src/engine/contracts';
import { LocalEngine } from '../src/engine/client';

const { blackboard } = vi.hoisted(() => ({ blackboard: vi.fn() }));
vi.mock('../src/context/LocalEngineContext', () => ({
  useLocalEngine: () => ({ client: stableClient, isConnected: true }),
}));
const stableClient = { blackboard };
const mira = { agent_id: 'mira', name: 'Mira', work_id: 'compare' };
const sam = { agent_id: 'sam', name: 'Sam', work_id: 'review' };
const root = {
  id: 'root-message',
  author: mira,
  body: 'Please check the attendance assumption.',
  created_at: '2026-10-09T10:00:00Z',
  reply_to: null,
};
const thread: BlackboardThread = {
  id: 'topic',
  title: 'Attendance assumption',
  kind: 'question',
  audience: [mira, sam],
  source_work_id: 'shape',
  root_work_id: 'root',
  resolved: false,
  messages: [root],
  reply_count: 1,
  updated_at: root.created_at,
};
beforeEach(() => {
  blackboard.mockImplementation(async (id?: string) => ({
    threads: [
      {
        ...thread,
        messages: id
          ? [
              root,
              {
                id: 'reply',
                author: sam,
                body: 'Confirmed against the brief.',
                reply_to: root.id,
                created_at: root.created_at,
              },
            ]
          : [root],
      },
    ],
    has_more: false,
  }));
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});
it('shows concise topics, explicit audiences and inline replies without dumping execution logs', async () => {
  render(<ProjectBlackboard projects={[]} entries={[]} onScope={() => {}} />);
  await screen.findByRole('heading', { name: 'Attendance assumption' });
  expect(screen.getByText('Shared with Mira, Sam')).toBeTruthy();
  expect(screen.queryByText('Confirmed against the brief.')).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: '1 reply' }));
  expect(await screen.findByText('Confirmed against the brief.')).toBeTruthy();
  expect(screen.getByText('Replying to Mira')).toBeTruthy();
  expect(blackboard).toHaveBeenCalledWith('topic', 0, expect.any(AbortSignal));
  fireEvent.click(screen.getByRole('button', { name: 'Close thread' }));
  expect(screen.queryByText('Confirmed against the brief.')).toBeNull();
  expect(screen.getByRole('heading', { name: 'Attendance assumption' })).toBeTruthy();
});
it('discloses long messages on demand and treats no replies as nonblocking', async () => {
  blackboard.mockResolvedValue({
    threads: [
      { ...thread, messages: [{ ...root, body: 'Detailed finding. '.repeat(60) }], reply_count: 0 },
    ],
    has_more: false,
  });
  render(<ProjectBlackboard projects={[]} entries={[]} onScope={() => {}} />);
  fireEvent.click(await screen.findByRole('button', { name: 'Read full message' }));
  expect(screen.getByRole('button', { name: 'Show less' }).getAttribute('aria-expanded')).toBe(
    'true',
  );
  fireEvent.click(screen.getByRole('button', { name: 'Show less' }));
  expect(
    screen.getByRole('button', { name: 'Read full message' }).getAttribute('aria-expanded'),
  ).toBe('false');
  fireEvent.click(screen.getByRole('button', { name: 'Open thread' }));
  expect(
    await screen.findByText('No replies yet. This conversation does not hold up other work.'),
  ).toBeTruthy();
});

it('shows persisted emoji counts and lets the operator inspect who reacted, including replies', async () => {
  blackboard.mockImplementation(async (id?: string) => ({
    threads: [
      {
        ...thread,
        messages: [
          { ...root, reactions: [{ emoji: '👍', agents: id ? [mira, sam] : [mira] }] },
          ...(id
            ? [
                {
                  id: 'reply',
                  author: sam,
                  body: 'A detailed review. '.repeat(80),
                  created_at: root.created_at,
                  reply_to: root.id,
                  reactions: [{ emoji: '💡', agents: [mira] }],
                },
              ]
            : []),
        ],
      },
    ],
    has_more: false,
  }));
  render(<ProjectBlackboard projects={[]} entries={[]} onScope={() => {}} />);
  const reaction = await screen.findByRole('button', { name: 'Like · 1: Mira' });
  fireEvent.click(reaction);
  expect(
    within(screen.getByRole('list', { name: 'Agents who reacted with Like' })).getByText('Mira'),
  ).toBeTruthy();
  fireEvent.click(reaction);
  expect(screen.queryByRole('list', { name: 'Agents who reacted with Like' })).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: '1 reply' }));
  // Opening a thread refreshes root reactions as well as displaying its replies.
  await screen.findByRole('button', { name: 'Like · 2: Mira, Sam' });
  fireEvent.click(screen.getByRole('button', { name: 'Idea · 1: Mira' }));
  expect(screen.getByRole('list', { name: 'Agents who reacted with Idea' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Read full message' })).toBeTruthy();
  expect(blackboard.mock.calls.every(([, offset]) => offset === 0)).toBe(true);
});

it('collapses rendered content by height and remeasures when the available width changes', async () => {
  let height = 240;
  const measure = vi
    .spyOn(HTMLElement.prototype, 'getBoundingClientRect')
    .mockImplementation(function () {
      return { height: this.classList.contains('bb-body-inner') ? height : 0 } as DOMRect;
    });
  let resized: () => void = () => {};
  const disconnect = vi.fn();
  vi.stubGlobal(
    'ResizeObserver',
    class {
      constructor(callback: () => void) {
        resized = callback;
      }
      observe() {}
      disconnect = disconnect;
    },
  );
  const view = render(<ProjectBlackboard projects={[]} entries={[]} onScope={() => {}} />);
  const expand = await screen.findByRole('button', { name: 'Read full message' });
  const body = document.getElementById(expand.getAttribute('aria-controls')!)!;
  expect(body.dataset.collapsed).toBe('true');
  fireEvent.click(expand);
  expect(body.dataset.collapsed).toBe('false');
  height = 60;
  act(() => resized());
  expect(screen.queryByRole('button', { name: 'Show less' })).toBeNull();
  expect(body.dataset.collapsed).toBe('false');
  view.unmount();
  expect(disconnect).toHaveBeenCalled();
  measure.mockRestore();
});
it('does not present transcripts as agent conversations when the endpoint is unavailable', async () => {
  blackboard.mockRejectedValue(
    new Error('This engine needs an update for Blackboard conversations.'),
  );
  render(<ProjectBlackboard projects={[]} entries={[]} onScope={() => {}} />);
  await screen.findByRole('alert');
  expect(screen.queryByText('No conversations here yet')).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: 'Execution records' }));
  expect(screen.getByRole('log', { name: 'Recorded team output' })).toBeTruthy();
});
it('cancels an in-flight thread read when the inspector closes', async () => {
  const view = render(<ProjectBlackboard projects={[]} entries={[]} onScope={() => {}} />);
  await screen.findByRole('heading', { name: thread.title });
  blackboard.mockImplementation(() => new Promise(() => {}));
  fireEvent.click(screen.getByRole('button', { name: '1 reply' }));
  await waitFor(() => expect(blackboard).toHaveBeenCalledWith('topic', 0, expect.any(AbortSignal)));
  const signal = blackboard.mock.calls.at(-1)![2] as AbortSignal;
  view.unmount();
  expect(signal.aborted).toBe(true);
});

it('filters work on the server before paging and resets paging when the work scope changes', async () => {
  blackboard.mockResolvedValue({ threads: [thread], has_more: true });
  const view = render(
    <ProjectBlackboard projects={[]} entries={[]} projectId="plan:root" onScope={() => {}} />,
  );
  await screen.findByRole('heading', { name: thread.title });
  expect(blackboard).toHaveBeenCalledWith(undefined, 0, expect.any(AbortSignal), ['root']);
  fireEvent.click(screen.getByRole('button', { name: 'Older topics' }));
  await waitFor(() =>
    expect(blackboard).toHaveBeenCalledWith(undefined, 20, expect.any(AbortSignal), ['root']),
  );
  blackboard.mockResolvedValue({ threads: [], has_more: false });
  view.rerender(
    <ProjectBlackboard projects={[]} entries={[]} projectId="plan:other" onScope={() => {}} />,
  );
  await screen.findByText('No conversations here yet');
  expect(blackboard).toHaveBeenCalledWith(undefined, 0, expect.any(AbortSignal), ['other']);
  expect(screen.queryByRole('heading', { name: thread.title })).toBeNull();
});

it('sends scoped inspection as an authenticated read query and preserves an empty scope', async () => {
  const fetch = vi
    .fn()
    .mockResolvedValue(new Response(JSON.stringify({ threads: [], has_more: false })));
  vi.stubGlobal('fetch', fetch);
  await new LocalEngine('test-token').blackboard(undefined, 20, undefined, []);
  expect(fetch).toHaveBeenCalledWith(
    '/api/local/blackboard/query',
    expect.objectContaining({
      method: 'POST',
      body: JSON.stringify({ offset: 20, work_ids: [] }),
      headers: expect.objectContaining({ Authorization: 'Bearer test-token' }),
    }),
  );
});
