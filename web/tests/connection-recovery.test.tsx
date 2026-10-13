import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import {
  acceptConnectionLink,
  connectionDraftScope,
  copyConnectionLink,
} from '../src/engine/connection';
import { connectionProblem, EngineConnectionError } from '../src/engine/failure';
import { LocalEngine } from '../src/engine/client';
import { LocalEngineProvider } from '../src/context/LocalEngineContext';
import { StartupGate } from '../src/components/preload/StartupGate';

const token = 'd'.repeat(64);
const link = () => `${location.origin}/?engine=local#connect=${token}`;
beforeEach(() => {
  sessionStorage.clear();
  history.replaceState(null, '', '/?engine=local');
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.useRealTimers();
  sessionStorage.clear();
  history.replaceState(null, '', '/');
});

it('accepts only a local matching connection without putting its credential in the URL or persistent storage', () => {
  const before = connectionDraftScope();
  expect(acceptConnectionLink(` ${link()} `)).toBe(token);
  expect(location.hash).toBe('');
  expect(sessionStorage.getItem('tetonic_local_session')).toBe(token);
  expect(localStorage.getItem('tetonic_local_session')).toBeNull();
  const after = connectionDraftScope();
  expect(after).not.toBe(before);
  acceptConnectionLink(link());
  expect(connectionDraftScope()).toBe(after);
});

it.each([
  'javascript:alert(1)',
  'https://attacker.example/#connect=secret',
  'http://localhost:65530/?engine=local#connect=secret',
  'http://user:password@localhost/#connect=secret',
  '/?engine=local#connect=secret',
])(
  'rejects an invalid or different destination without replacing the active connection: %s',
  (value) => {
    sessionStorage.setItem('tetonic_local_session', token);
    expect(() => acceptConnectionLink(value)).toThrow();
    expect(sessionStorage.getItem('tetonic_local_session')).toBe(token);
  },
);

it('rejects missing or ambiguous keys without echoing them in an error', () => {
  for (const fragment of ['', '#connect=PRIVATECANARY', `#connect=${token}&connect=${token}`]) {
    const attempt = () => acceptConnectionLink(`${location.origin}/${fragment}`);
    expect(attempt).toThrow();
    try {
      attempt();
    } catch (error) {
      expect(String(error)).not.toContain('PRIVATECANARY');
      expect(String(error)).not.toContain(token);
    }
  }
});

it('copies a link only on request and reports clipboard failure without exposing the credential', async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal('navigator', { clipboard: { writeText } });
  await copyConnectionLink(token);
  expect(writeText).toHaveBeenCalledWith(link());
  writeText.mockRejectedValue(new Error('Clipboard denied'));
  await expect(copyConnectionLink(token)).rejects.toThrow('Clipboard access is unavailable');
});

it('classifies a missing credential and an unreachable engine without making a missing-credential request', async () => {
  const fetch = vi.fn().mockRejectedValue(new TypeError('Failed to fetch'));
  vi.stubGlobal('fetch', fetch);
  await expect(new LocalEngine('').snapshot()).rejects.toMatchObject({ reason: 'missing' });
  expect(fetch).not.toHaveBeenCalled();
  await expect(new LocalEngine(token).snapshot()).rejects.toMatchObject({ reason: 'unreachable' });
});

it('identifies an incompatible error schema even when the version header is missing', async () => {
  vi.stubGlobal(
    'fetch',
    vi.fn().mockResolvedValue(
      new Response(JSON.stringify({ schema_version: 2, error: 'PRIVATECANARY' }), {
        status: 400,
      }),
    ),
  );
  const failure = await new LocalEngine(token).snapshot().catch((error) => error);
  expect(connectionProblem(failure)).toBe('incompatible');
  expect(failure.message).not.toContain('PRIVATECANARY');
});

it('reports deadline expiry but preserves cancellation by its caller', async () => {
  vi.useFakeTimers();
  vi.stubGlobal(
    'fetch',
    vi.fn(
      (_url, options: RequestInit) =>
        new Promise((_resolve, reject) => {
          options.signal?.addEventListener('abort', () =>
            reject(new DOMException('Aborted', 'AbortError')),
          );
        }),
    ),
  );
  const engine = new LocalEngine(token);
  const pending = engine.snapshot().catch((e: unknown) => e);
  await vi.advanceTimersByTimeAsync(10000);
  expect(await pending).toBeInstanceOf(EngineConnectionError);
  expect(await pending).toMatchObject({ reason: 'timeout' });
  const abort = new AbortController();
  const canceled = engine.snapshot(abort.signal).catch((e: unknown) => e);
  abort.abort();
  expect(await canceled).toMatchObject({ name: 'AbortError' });
});

it('connects from the actual entry form in a fresh browser session using Enter', async () => {
  const user = userEvent.setup();
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null);
  vi.stubGlobal(
    'matchMedia',
    vi.fn(() => ({ matches: true, addEventListener() {}, removeEventListener() {} })),
  );
  const fetch = vi.fn(async (url: string, options: RequestInit) => {
    expect((options.headers as Record<string, string>).Authorization).toBe(`Bearer ${token}`);
    const data = url.endsWith('/workspace')
      ? { organization: 'Recovered', tasks: [], agents: [] }
      : url.endsWith('/agent-catalog')
        ? { models: [], harnesses: [] }
        : url.endsWith('/approvals')
          ? { pending_approvals: [], active_stops: [], effort: [] }
          : [];
    return new Response(JSON.stringify(data));
  });
  vi.stubGlobal('fetch', fetch);
  render(
    <LocalEngineProvider>
      <StartupGate>
        <main aria-label="Live workspace" tabIndex={-1}>
          Your real work
        </main>
      </StartupGate>
    </LocalEngineProvider>,
  );
  await screen.findByRole('heading', { name: 'Connect your workspace.' });
  expect(fetch).not.toHaveBeenCalled();
  const input = screen.getByLabelText('Engine connection link');
  fireEvent.change(input, { target: { value: link() } });
  await user.click(input);
  await user.keyboard('{Enter}');
  await screen.findByRole('main', { name: 'Live workspace' });
  expect(location.hash).toBe('');
  expect(screen.queryByLabelText('Engine connection link')).toBeNull();
});

it('explains a rejected credential and accepts a replacement without reloading the page', async () => {
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null);
  vi.stubGlobal(
    'matchMedia',
    vi.fn(() => ({ matches: true, addEventListener() {}, removeEventListener() {} })),
  );
  sessionStorage.setItem('tetonic_local_session', 'a'.repeat(64));
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, options: RequestInit) => {
      if ((options.headers as Record<string, string>).Authorization !== `Bearer ${token}`)
        return new Response(
          JSON.stringify({
            schema_version: 1,
            code: 'unauthenticated',
            error: 'Reconnect.',
            recovery: 'reconnect',
          }),
          { status: 401 },
        );
      return new Response(
        JSON.stringify(url.endsWith('/workspace') ? { tasks: [], agents: [] } : {}),
      );
    }),
  );
  render(
    <LocalEngineProvider>
      <StartupGate>
        <main aria-label="Live workspace">Recovered</main>
      </StartupGate>
    </LocalEngineProvider>,
  );
  await screen.findByText('Your workspace needs a fresh connection link.');
  const input = screen.getByLabelText('Engine connection link');
  fireEvent.change(input, { target: { value: link() } });
  fireEvent.submit(input.closest('form')!);
  await screen.findByRole('main', { name: 'Live workspace' });
});
