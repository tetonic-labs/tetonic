import { StrictMode } from 'react';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import { FirstUseTutorial } from '../src/components/team-work/FirstUseTutorial';

afterEach(() => vi.restoreAllMocks());

it('waits for startup to finish and does not repeat after a reload, even mid-tour', () => {
  const setup = vi.fn();
  const view = render(<FirstUseTutorial enabled={false} onSetupGuide={setup} />);
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(localStorage.length).toBe(0);
  view.rerender(<FirstUseTutorial enabled onSetupGuide={setup} />);
  expect(screen.getByRole('dialog', { name: 'Your ideas. Your team.' })).toBeTruthy();
  expect(setup).not.toHaveBeenCalled();
  view.unmount();
  render(<FirstUseTutorial enabled onSetupGuide={setup} />);
  expect(screen.queryByRole('dialog')).toBeNull();
});

it('keeps the introduction skippable and dismisses it across reconnects and remounts', () => {
  const setup = vi.fn();
  const view = render(
    <StrictMode>
      <FirstUseTutorial enabled onSetupGuide={setup} />
    </StrictMode>,
  );
  fireEvent.click(screen.getByRole('button', { name: 'Skip tour' }));
  view.rerender(<FirstUseTutorial enabled={false} onSetupGuide={setup} />);
  view.rerender(<FirstUseTutorial enabled onSetupGuide={setup} />);
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(setup).not.toHaveBeenCalled();
});

it('walks forward and back, then hands off to setup only when chosen', () => {
  const setup = vi.fn();
  render(<FirstUseTutorial enabled onSetupGuide={setup} />);
  fireEvent.click(screen.getByRole('button', { name: 'Next' }));
  expect(screen.getByRole('dialog', { name: 'Start with a thought.' })).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'Back' }));
  expect(screen.getByRole('dialog', { name: 'Your ideas. Your team.' })).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'Next' }));
  fireEvent.click(screen.getByRole('button', { name: 'Next' }));
  expect(screen.getByRole('dialog', { name: 'See progress. Stay in control.' })).toBeTruthy();
  expect(setup).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: 'Set up the Guide' }));
  expect(setup).toHaveBeenCalledOnce();
  expect(screen.queryByRole('dialog')).toBeNull();
});

it('traps keyboard focus, supports Escape, and returns focus to the workspace', async () => {
  const user = userEvent.setup();
  render(
    <>
      <main id="main-content" tabIndex={-1}>
        <button>Workspace control</button>
      </main>
      <FirstUseTutorial enabled onSetupGuide={vi.fn()} />
    </>,
  );
  const dialog = screen.getByRole('dialog');
  await waitFor(() => expect(document.activeElement).toBe(within(dialog).getByRole('heading')));
  await user.tab();
  expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Next' }));
  await user.tab();
  expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Skip tour' }));
  await user.keyboard('{Escape}');
  await waitFor(() => expect(document.activeElement).toBe(screen.getByRole('main')));
  expect(screen.queryByRole('dialog')).toBeNull();
});

it('still lets the user enter when browser storage is unavailable', () => {
  vi.spyOn(localStorage, 'getItem').mockImplementation(() => {
    throw new DOMException('Storage is blocked', 'SecurityError');
  });
  render(<FirstUseTutorial enabled onSetupGuide={vi.fn()} />);
  fireEvent.click(screen.getByRole('button', { name: 'Skip tour' }));
  expect(screen.queryByRole('dialog')).toBeNull();
});
