import { describe, expect, it } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App } from '../src/App';
import { teamWorkflows } from '../src/lib/teamRoom';
import type { WorkRecord } from '../src/lib/workEvidence';
import { mockTeams } from '../src/store/mockData';

describe('shared team work', () => {
  it('opens the team record, keeps distinct drafts, and returns to the same map and selection', async () => {
    const user = userEvent.setup();
    const { container } = render(<App initialView="map" />);
    await user.click(screen.getByRole('button', { name: 'Focus Alex', exact: true }));
    const world = container.querySelector('.universe-world') as HTMLElement;
    const before = world.style.transform;
    const entry = screen.getByRole('button', { name: 'Open Platform Core Guild conversation' });
    await user.click(entry);
    const room = screen.getByRole('region', { name: 'Platform Core Guild team room' });
    expect(within(room).getByText('3 people · 3 agents')).toBeTruthy();
    expect(container.querySelector('.map-layer')?.getAttribute('inert')).not.toBeNull();
    await user.type(
      within(room).getByRole('textbox', { name: 'Message Platform Core Guild' }),
      'A thought for everyone',
    );
    await user.click(
      within(room).getByRole('button', { name: 'Get Atlas ready for release', exact: true }),
    );
    const thread = screen.getByRole('complementary', {
      name: 'Work thread: Get Atlas ready for release',
    });
    expect(within(thread).getByRole('heading', { name: 'How we got here' })).toBeTruthy();
    expect(within(thread).getByRole('heading', { name: 'Evidence' })).toBeTruthy();
    await user.type(within(thread).getByRole('textbox'), 'What is the remaining uncertainty?');
    await user.click(within(thread).getByRole('button', { name: 'Send thread reply' }));
    expect(within(thread).getByText('What is the remaining uncertainty?')).toBeTruthy();
    await user.keyboard('{Escape}');
    expect(within(room).getByRole('textbox')).toHaveProperty('value', 'A thought for everyone');
    expect(within(room).queryByText('What is the remaining uncertainty?')).toBeNull();
    await user.click(within(room).getByRole('button', { name: 'Back to map' }));
    await waitFor(() => expect(world.style.transform).toBe(before));
    expect(screen.getByRole('complementary', { name: 'Following Alex' })).toBeTruthy();
    await waitFor(() => expect(document.activeElement).toBe(entry));
    await user.click(entry);
    expect(screen.getByRole('textbox', { name: 'Message Platform Core Guild' })).toHaveProperty(
      'value',
      'A thought for everyone',
    );
    await user.click(
      screen.getByRole('button', { name: 'Get Atlas ready for release', exact: true }),
    );
    expect(screen.getByText('What is the remaining uncertainty?')).toBeTruthy();
  });

  it('branches a thought into a conversation without dispatching work and shares map messages', async () => {
    const user = userEvent.setup();
    render(<App initialView="map" />);
    await user.click(screen.getByRole('button', { name: 'Team conversation', exact: true }));
    await user.type(
      screen.getByRole('textbox', { name: 'Message Platform Core Guild' }),
      'Could we explore a smaller release?{Enter}',
    );
    expect(screen.getAllByText('Could we explore a smaller release?')).toHaveLength(1);
    await user.click(screen.getByRole('button', { name: 'Reply in thread', exact: true }));
    await user.type(screen.getByRole('textbox'), 'Start with the core experience.{Enter}');
    expect(screen.getByText('Start with the core experience.')).toBeTruthy();
    expect(screen.queryByText(/accepted the request/i)).toBeNull();
    await user.keyboard('{Escape}');
    expect(screen.getByRole('button', { name: 'Reply in thread · 1 replies' })).toBeTruthy();
    await user.click(screen.getByRole('button', { name: 'Back to map' }));
    await user.click(screen.getByRole('button', { name: 'Quick message', exact: true }));
    await user.click(screen.getByRole('button', { name: 'Show conversation' }));
    expect(screen.getByRole('log').textContent).toContain('Could we explore a smaller release?');
    expect(screen.getByRole('log').textContent).not.toContain('Start with the core experience.');
  });

  it('keeps the organization clock running while the team reads and preserves a reading position', async () => {
    const user = userEvent.setup();
    const { container } = render(<App initialView="map" />);
    await user.click(screen.getByRole('button', { name: 'Play sample activity' }));
    await user.click(screen.getByRole('button', { name: 'Open Platform Core Guild conversation' }));
    const reading = container.querySelector('.tr-scroll') as HTMLElement;
    reading.scrollTop = 120;
    fireEvent.scroll(reading);
    const time = container.querySelector('[aria-label="Sample time"]')!;
    const before = time.textContent;
    await waitFor(() => expect(time.textContent).not.toBe(before), { timeout: 2500 });
    expect(reading.scrollTop).toBe(120);
    await user.click(screen.getByRole('button', { name: 'Back to map' }));
    expect(screen.getByRole('button', { name: 'Pause sample playback' })).toBeTruthy();
    await user.click(screen.getByRole('button', { name: 'Pause sample playback' }));
  });

  it('keeps room drafts separate between teams and exposes humans without entering an agent', async () => {
    const user = userEvent.setup();
    render(<App initialView="map" />);
    await user.click(screen.getByRole('button', { name: 'Open Platform Core Guild conversation' }));
    await user.type(screen.getByRole('textbox'), 'Platform draft');
    await user.click(screen.getByRole('button', { name: '3 people · 3 agents' }));
    expect(screen.getByText('Susan Vance')).toBeTruthy();
    await user.keyboard('{Escape}');
    const rooms = screen.getByRole('navigation', { name: 'Team conversations' });
    expect(
      within(rooms)
        .getByRole('button', { name: 'Open Platform Core Guild room' })
        .getAttribute('aria-current'),
    ).toBe('page');
    await user.click(within(rooms).getByRole('button', { name: 'Open Personal Fleet room' }));
    expect(screen.getByRole('textbox', { name: 'Message Personal Fleet' })).toHaveProperty(
      'value',
      '',
    );
    await waitFor(() =>
      expect(document.activeElement).toBe(screen.getByRole('heading', { name: 'Personal Fleet' })),
    );
    await user.type(screen.getByRole('textbox'), 'Personal draft');
    expect(
      within(rooms)
        .getByRole('button', { name: 'Open Personal Fleet room' })
        .getAttribute('aria-current'),
    ).toBe('page');
    await user.click(within(rooms).getByRole('button', { name: 'Open Platform Core Guild room' }));
    expect(screen.getByRole('textbox', { name: 'Message Platform Core Guild' })).toHaveProperty(
      'value',
      'Platform draft',
    );
  });

  it('records a decision in the shared work model without overwriting its earlier checkpoint', async () => {
    const user = userEvent.setup();
    render(<App initialView="map" />);
    await user.click(screen.getByRole('button', { name: 'Open Platform Core Guild conversation' }));
    await user.click(
      screen.getByRole('button', { name: 'Get Atlas ready for release', exact: true }),
    );
    const thread = screen.getByRole('complementary', {
      name: 'Work thread: Get Atlas ready for release',
    });
    const choice = within(thread)
      .getAllByRole('button')
      .find((button) => button.parentElement?.className === 'tr-decision')!;
    expect(choice).toBeTruthy();
    const title = choice.querySelector('strong')!.textContent;
    await user.click(choice);
    expect(
      within(thread).getByText('Direction recorded locally. Not yet acknowledged.'),
    ).toBeTruthy();
    await user.keyboard('{Escape}');
    expect(
      screen.getByText(/Core checks pass. Export accessibility needs another day/),
    ).toBeTruthy();
    expect(screen.getByText(title!)).toBeTruthy();
    await user.click(screen.getByRole('button', { name: 'Work', exact: true }));
    expect(
      screen.getByRole('button', { name: 'Open Get Atlas ready for release' }).textContent,
    ).toContain('Direction recorded:');
  });
});

describe('shared workflow projection', () => {
  const record = (
    id: string,
    state: WorkRecord['state'],
    agentId = 'agt-builder',
    targetId = 'agt-sentinel',
  ): WorkRecord => ({
    id,
    at: 1,
    state,
    interaction: {
      id: 'operation',
      workflowId: 'release',
      agentId,
      targetId,
      targetName: 'Sam',
      label: 'Release / Review the plan',
    },
  });
  it('keeps one workflow and one identity across both team records', () => {
    const teams = mockTeams.map((team) =>
      team.id === 'personal'
        ? { ...team, pledgedAgentIds: [...team.pledgedAgentIds, 'agt-builder'] }
        : team,
    );
    const flows = teamWorkflows(
      [record('begin', 'started'), record('end', 'completed')],
      teams,
      'demo',
    );
    expect(flows).toHaveLength(1);
    expect(flows[0].teamIds).toEqual(expect.arrayContaining(['personal', 'team-platform']));
    expect(flows[0].agentIds).toHaveLength(2);
    expect(flows[0].operations).toBe(1);
    expect(flows[0].state).toBe('settled');
    expect(teamWorkflows([record('begin', 'started')], teams, 'other')[0].id).not.toBe(flows[0].id);
  });
  it('retains failure emphasis during a retry and clears it only on completion', () => {
    const failed = record('failure', 'failed');
    const retry = {
      ...record('retry-start', 'started'),
      interaction: { ...failed.interaction, id: 'retry', retryOf: 'operation' },
    };
    expect(teamWorkflows([failed, retry], mockTeams, 'demo')[0].state).toBe('blocked');
    expect(
      teamWorkflows(
        [failed, retry, { ...retry, id: 'retry-end', state: 'completed' }],
        mockTeams,
        'demo',
      )[0].state,
    ).toBe('settled');
  });
  it('does not invent a workflow from unrelated simultaneous operations', () => {
    const first = record('first', 'started');
    first.interaction.workflowId = undefined;
    const second = {
      ...first,
      id: 'second',
      interaction: { ...first.interaction, id: 'unrelated' },
    };
    expect(teamWorkflows([first, second], mockTeams, 'demo')).toHaveLength(2);
  });
});
