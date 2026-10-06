import { afterEach, it, expect, vi } from 'vitest';
import { waitFor, render, fireEvent, screen } from '@testing-library/react';
import { App } from '../src/App';
import { LocalEngine, takeConnectionToken } from '../src/lib/localEngine';
const base = { tasks: [] };
afterEach(() => {
  vi.unstubAllGlobals();
  sessionStorage.clear();
  history.replaceState(null, '', '/');
});
it('removes the connection secret from the URL and sends it only as an authorization header', async () => {
  const token = 'a'.repeat(64);
  window.history.replaceState(null, '', `/?engine=local#connect=${token}`);
  const fetch = vi.fn().mockResolvedValue({ ok: true, json: async () => base });
  vi.stubGlobal('fetch', fetch);
  const engine = new LocalEngine(takeConnectionToken());
  expect(window.location.hash).toBe('');
  await engine.snapshot();
  await waitFor(() => expect(fetch).toHaveBeenCalled());
  expect(fetch.mock.calls[0][0]).toBe('/api/local/workspace');
  expect(fetch.mock.calls[0][1].headers.Authorization).toBe(`Bearer ${token}`);
});

it('rotates the current UI connection without retaining another workspace’s work', async () => {
  sessionStorage.setItem('tetonic_local_session', 'a'.repeat(64));
  const fetch = vi.fn().mockImplementation(async (url: string, options: RequestInit) => {
    const current =
      (options.headers as Record<string, string>).Authorization === `Bearer ${'b'.repeat(64)}`;
    const name = current ? 'New workspace request' : 'Old workspace request';
    const value = url.endsWith('/workspace')
      ? {
          organization: current ? 'New organization' : 'Old organization',
          team_id: 'team',
          team_name: 'Team',
          agent_id: 'a',
          agent_name: 'Assistant',
          model: 'test',
          input_limit: 12000,
          agents: [],
          tasks: [
            {
              id: current ? 'new' : 'old',
              input: name,
              agent_key: 'a',
              agent_name: 'Assistant',
              state: 'completed',
              run_id: 'run',
              sequence: 1,
              messages: [],
            },
          ],
        }
      : url.endsWith('/agent-catalog')
        ? { models: [], harnesses: [], max_steps: 1, max_seconds: 1, max_tokens: 1 }
        : url.endsWith('/approvals')
          ? { active_stops: [], pending_approvals: [], effort: [] }
          : [];
    return { ok: true, json: async () => value };
  });
  vi.stubGlobal('fetch', fetch);
  render(<App />);
  await screen.findByRole('button', { name: 'Open Old workspace request' });
  history.replaceState(null, '', `/#connect=${'b'.repeat(64)}`);
  fireEvent(window, new HashChangeEvent('hashchange'));
  await screen.findByRole('button', { name: 'Open New workspace request' });
  expect(screen.queryByRole('button', { name: 'Open Old workspace request' })).toBeNull();
  expect(location.hash).toBe('');
});
