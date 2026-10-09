import { afterEach, it, expect, vi } from 'vitest';
import { waitFor, render, fireEvent, screen } from '@testing-library/react';
import { App } from '../src/App';
import { EngineRequestError } from '../src/engine/failure';
import { LocalEngine } from '../src/engine/client';
import { takeConnectionToken } from '../src/engine/connection';
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
  expect(fetch.mock.calls[0][1].headers['X-Tetonic-Api-Version']).toBe('1');
});

it('preserves conflict and recovery information without replaying the mutation', async () => {
  const fetch = vi.fn().mockResolvedValue(
    new Response(
      JSON.stringify({
        schema_version: 1,
        error: 'This team changed.',
        code: 'state_conflict',
        recovery: 'refresh',
        recovery_hint: 'Review the current team before saving again.',
      }),
      { status: 409, headers: { 'x-tetonic-api-version': '1' } },
    ),
  );
  vi.stubGlobal('fetch', fetch);
  const engine = new LocalEngine('token');
  const error = await engine
    .request('/teams', { name: 'Team' })
    .catch((failure: unknown) => failure);
  expect(error).toBeInstanceOf(EngineRequestError);
  expect(error).toMatchObject({
    status: 409,
    code: 'state_conflict',
    recovery: 'refresh',
    message: 'This team changed.',
  });
  expect(fetch).toHaveBeenCalledTimes(1);
});

it('handles legacy, malformed and unsupported-version failures without trusting an arbitrary body', async () => {
  const fetch = vi.fn();
  vi.stubGlobal('fetch', fetch);
  const engine = new LocalEngine('token');
  for (const [body, message] of [
    [{ error: 'Reconnect first.' }, 'Reconnect first.'],
    [null, 'The local engine could not complete this request.'],
    [{ error: { data: 'not text' } }, 'The local engine could not complete this request.'],
    [
      { schema_version: 2, error: 'PRIVATECANARY' },
      'The engine and this app use different API versions. Update the app and reconnect.',
    ],
  ]) {
    fetch.mockResolvedValueOnce(new Response(JSON.stringify(body), { status: 400 }));
    await expect(engine.snapshot()).rejects.toThrow(message as string);
  }
  fetch.mockResolvedValueOnce(
    new Response('{}', { status: 200, headers: { 'x-tetonic-api-version': '2' } }),
  );
  await expect(engine.snapshot()).rejects.toThrow('different API versions');
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
