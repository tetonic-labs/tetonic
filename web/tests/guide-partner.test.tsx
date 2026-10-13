import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { GuideUpdates } from '../src/components/team-work/GuideUpdates';
import { GuideContext } from '../src/components/team-work/GuideContext';
import { GuideAttentionSettings } from '../src/components/team-work/GuideAttentionSettings';
import { EngineRequestError } from '../src/engine/failure';
import type { GuideAttentionView } from '../src/engine/contracts';

const state = vi.hoisted(() => ({
  client: {
    seeGuideUpdate: vi.fn(),
    guideContext: vi.fn(),
    focusGuide: vi.fn(),
    saveGuideAttention: vi.fn(),
  },
  refresh: vi.fn(),
  guideAttention: null as GuideAttentionView | null,
  isConnected: true,
  readErrors: {} as Record<string, string>,
}));
vi.mock('../src/context/LocalEngineContext', () => ({ useLocalEngine: () => state }));
beforeEach(() => {
  vi.clearAllMocks();
  state.isConnected = true;
  state.readErrors = {};
  state.guideAttention = {
    partial: false,
    settings: { revision: 1, enabled: false, daily_tokens: 0, assessment_tokens: 0 },
    reserved_today: 0,
    items: [
      {
        signal: {
          key: 'incident',
          work_id: 'work-1',
          signature: 'one',
          kind: 'human_request',
          title: 'Venue access',
          detail: 'Needs the owner’s access decision.',
        },
        state: 'reviewed',
        assessment_id: 'assessment',
        update: 'A useful concise introduction. ' + 'Evidence detail. '.repeat(80),
        seen: false,
        observed_at: '2026-10-13T12:00:00Z',
      },
    ],
  };
});
afterEach(cleanup);

it('keeps background updates secondary, truncates details, and marks read without resolving work', async () => {
  const onWork = vi.fn();
  render(<GuideUpdates compact onWork={onWork} />);
  expect(screen.queryByText('Venue access')).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: /Updates · 1 new/ }));
  expect(screen.getByText('Venue access')).toBeTruthy();
  expect(screen.getByText(/Automatic Guide assessments are off/)).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'Read more' }));
  expect(screen.getByRole('button', { name: 'Show less' })).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'Inspect work' }));
  expect(onWork).toHaveBeenCalledWith('work-1');
  fireEvent.click(screen.getByRole('button', { name: 'Mark read' }));
  await waitFor(() => expect(state.client.seeGuideUpdate).toHaveBeenCalledWith('incident'));
  expect(state.guideAttention?.items[0].state).toBe('reviewed');
});

it('keeps a changed subject inspectable and requires a fresh choice after a revision conflict', async () => {
  const subject = {
    id: 'subject',
    revision: 1,
    title: 'Reading group',
    understanding: 'We rejected the workshop.',
    source_turns: ['first'],
    work_ids: [],
  };
  const initial = {
    focus: { revision: 1, subject_id: 'subject' },
    subject,
    subjects: [subject],
    partial: false,
  };
  state.client.guideContext.mockResolvedValue(initial);
  state.client.focusGuide
    .mockRejectedValueOnce(new EngineRequestError('The discussion changed. Choose again.', 409))
    .mockResolvedValueOnce({});
  render(<GuideContext conversationId="first" sequence={1} active={false} />);
  fireEvent.click(await screen.findByRole('button', { name: /Thinking about Reading group/ }));
  expect(screen.getByText('We rejected the workshop.')).toBeTruthy();
  state.client.guideContext.mockResolvedValue({
    ...initial,
    focus: { revision: 2, subject_id: 'subject' },
  });
  fireEvent.change(screen.getByRole('combobox', { name: 'Discussion subject' }), {
    target: { value: '' },
  });
  await screen.findByText('The discussion changed. Choose again.');
  fireEvent.change(screen.getByRole('combobox', { name: 'Discussion subject' }), {
    target: { value: '' },
  });
  await waitFor(() => expect(state.client.focusGuide).toHaveBeenCalledTimes(2));
  expect(state.client.focusGuide.mock.calls[0][1]).toBe(1);
  expect(state.client.focusGuide.mock.calls[1][1]).toBe(2);
  expect(state.client.focusGuide.mock.calls[0][3]).not.toBe(
    state.client.focusGuide.mock.calls[1][3],
  );
});

it('does not enable assessments until the owner saves and preserves their allowance draft while data refreshes', async () => {
  const view = render(<GuideAttentionSettings />);
  fireEvent.click(screen.getByRole('switch', { name: 'Automatic Guide updates' }));
  fireEvent.change(screen.getByRole('spinbutton', { name: 'Daily assessment allowance' }), {
    target: { value: '12000' },
  });
  expect(state.client.saveGuideAttention).not.toHaveBeenCalled();
  state.guideAttention!.reserved_today = 4000;
  view.rerender(<GuideAttentionSettings />);
  expect(
    (screen.getByRole('spinbutton', { name: 'Daily assessment allowance' }) as HTMLInputElement)
      .value,
  ).toBe('12000');
  fireEvent.click(screen.getByRole('button', { name: 'Save Guide settings' }));
  await waitFor(() =>
    expect(state.client.saveGuideAttention).toHaveBeenCalledWith(
      expect.objectContaining({
        revision: 1,
        enabled: true,
        daily_tokens: 12000,
        assessment_tokens: 8000,
      }),
      expect.any(String),
    ),
  );
});
