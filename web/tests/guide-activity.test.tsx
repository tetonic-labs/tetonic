import { fireEvent, render, screen } from '@testing-library/react';
import { expect, it } from 'vitest';
import { GuideActivity } from '../src/components/team-work/GuideActivity';

it('shows real operations and their receipts instead of a timed sequence of invented phases', () => {
  const view = render(<GuideActivity activities={[]} active connected />);
  expect(screen.getByRole('status').textContent).toContain('Thinking it through');
  expect(screen.queryByText(/Checking agents/)).toBeNull();
  view.rerender(
    <GuideActivity
      activities={[{ id: '1', operation: 'resources', state: 'requested' }]}
      active
      connected
    />,
  );
  expect(screen.getByText('Checking agents, tools and budgets…')).toBeTruthy();
  expect(screen.queryByText('Checked agents, tools and budgets')).toBeNull();
  view.rerender(
    <GuideActivity
      activities={[
        { id: '1', operation: 'resources', state: 'completed' },
        { id: '2', operation: 'propose', state: 'requested' },
      ]}
      active
      connected
    />,
  );
  expect(screen.queryByText('Checked agents, tools and budgets')).toBeNull();
  expect(screen.getByText('Saving the team proposal…')).toBeTruthy();
  expect(screen.queryByText('Saved a team proposal for review')).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: 'Show 2 actions' }));
  expect(screen.getByText('Checked agents, tools and budgets')).toBeTruthy();
  view.rerender(
    <GuideActivity
      activities={[
        { id: '1', operation: 'resources', state: 'completed' },
        { id: '2', operation: 'propose', state: 'completed' },
      ]}
      active={false}
      connected
    />,
  );
  expect(screen.getByText('Saved a team proposal for review')).toBeTruthy();
  expect(screen.queryByRole('status')).toBeNull();
});

it('never reports a failed, interrupted or disconnected operation as successful or still animating', () => {
  const view = render(
    <GuideActivity
      activities={[{ id: '1', operation: 'resources', state: 'requested' }]}
      active
      connected={false}
    />,
  );
  expect(screen.getByText('Connection lost · check not confirmed')).toBeTruthy();
  expect(view.container.querySelector('.guide-activity-dots')).toBeNull();
  view.rerender(
    <GuideActivity
      activities={[{ id: '1', operation: 'propose', state: 'failed' }]}
      active={false}
      connected
    />,
  );
  expect(screen.getByText('Could not save the team proposal')).toBeTruthy();
  view.rerender(
    <GuideActivity
      activities={[{ id: '1', operation: 'propose', state: 'requested' }]}
      active={false}
      connected
    />,
  );
  expect(screen.getByText('Saving the team proposal · not confirmed')).toBeTruthy();
  expect(view.container.querySelector('.guide-activity-dots')).toBeNull();
});
