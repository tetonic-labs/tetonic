import { describe, it, expect, vi } from 'vitest';
import { render, screen, within, waitFor, fireEvent } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App } from '../src/App';
import { AgentsView } from '../src/components/views/AgentsView';
import { mockTeams } from '../src/store/mockData';
import { saveAgentImage } from '../src/lib/agentImages';

describe('map workspace', () => {
  it('centers the docked agent, rather than its overview home, when moving closer', async () => {
    const user = userEvent.setup();
    const { container } = render(<App initialView="map" />);
    await user.click(screen.getByRole('button', { name: 'Motion examples and accessibility' }));
    await user.selectOptions(screen.getByRole('combobox', { name: 'Motion example' }), 'shared');
    for (let i = 0; i < 3; i++)
      await user.click(screen.getByRole('button', { name: 'Next interaction state' }));
    await user.click(screen.getByRole('button', { name: 'Focus Alex', exact: true }));
    await user.click(screen.getByRole('button', { name: 'Move closer', exact: true }));
    await waitFor(() => {
      const world = (
        container.querySelector('.universe-world') as HTMLElement
      ).style.transform.match(/translate\(([-\d.]+)px,([-\d.]+)px\) scale\(([-\d.]+)\)/)!;
      const actor = (
        container.querySelector('[data-entity="agt-builder"]') as HTMLElement
      ).style.transform.match(/translate\(([-\d.]+)px,([-\d.]+)px\)/)!;
      expect(Number(world[1]) + Number(actor[1]) * Number(world[3])).toBeCloseTo(720, 0);
      expect(Number(world[2]) + Number(actor[2]) * Number(world[3])).toBeCloseTo(445, 0);
    });
  });
  it('keeps a custom image in its square map portrait', () => {
    const image = 'data:image/png;base64,iVBORw0KGgo=';
    saveAgentImage('agt-builder', image);
    render(<App initialView="map" />);
    const portrait = screen
      .getByRole('button', { name: 'Focus Alex', exact: true })
      .querySelector('img')!;
    expect(portrait.src).toBe(image);
    expect(portrait.style.borderRadius).toBe('7px');
    expect(portrait.style.objectFit).toBe('cover');
  });
  it('keeps a team inquiry scoped and the camera steady while activity continues', async () => {
    const user = userEvent.setup();
    const { container } = render(<App initialView="map" />);
    await user.click(screen.getByRole('button', { name: 'Teams', exact: true }));
    await user.click(screen.getByRole('button', { name: 'View all teams' }));
    await user.click(screen.getByRole('button', { name: 'Motion examples and accessibility' }));
    await user.selectOptions(screen.getByRole('combobox', { name: 'Motion example' }), 'shared');
    await user.click(screen.getByRole('button', { name: 'Play sample activity' }));
    const transform = (container.querySelector('.universe-world') as HTMLElement).style.transform;
    await user.click(
      screen.getByRole('button', { name: 'Ask about Platform Core Guild', exact: true }),
    );
    const inspector = screen.getByRole('complementary', {
      name: 'Asking about Platform Core Guild',
    });
    const input = within(inspector).getByRole('textbox');
    expect(document.activeElement).toBe(input);
    await user.type(input, 'What needs attention?');
    for (const neighborhood of container.querySelectorAll('.team-neighborhood'))
      fireEvent.pointerEnter(neighborhood);
    expect(within(inspector).getByRole('textbox').getAttribute('aria-label')).toBe(
      'Ask about Platform Core Guild',
    );
    expect((container.querySelector('.universe-world') as HTMLElement).style.transform).toBe(
      transform,
    );
    await user.click(
      within(inspector).getByRole('button', { name: 'Ask about recorded activity' }),
    );
    expect(inspector.textContent).toContain('explicit requests for human input');
    await waitFor(() => expect(screen.getByLabelText('Sample time').textContent).not.toBe('0:00'), {
      timeout: 2200,
    });
    expect(screen.getByRole('button', { name: 'Pause sample playback' })).toBeTruthy();
    expect(inspector.textContent).toContain('Recorded sample · 0:00');
    await user.click(screen.getByRole('button', { name: 'Pause sample playback' }));
  });
  it('opens on the scoped map and exposes preview provenance on demand', async () => {
    const user = userEvent.setup();
    render(<App initialView="map" />);
    expect(screen.getByRole('region', { name: /Agent activity map/ })).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Focus Sam', exact: true })).toBeNull();
    await user.click(screen.getByText('Preview', { exact: true }));
    expect(screen.getByText(/No engine is connected/)).toBeTruthy();
  });
  it('retains separate agent drafts, addresses messages correctly, and restores focus', async () => {
    const user = userEvent.setup();
    render(<App initialView="map" />);
    const trigger = screen.getByRole('button', { name: 'Focus Alex', exact: true });
    await user.click(trigger);
    const profileTrigger = screen.getByRole('button', { name: 'Open profile', exact: true });
    await user.click(profileTrigger);
    await user.type(screen.getByRole('textbox', { name: 'Message Alex' }), 'Alex only');
    await user.keyboard('{Escape}');
    await waitFor(() => expect(document.activeElement).toBe(profileTrigger));
    await user.click(screen.getByRole('button', { name: 'Focus Robin', exact: true }));
    await user.click(screen.getByRole('button', { name: 'Open profile', exact: true }));
    expect(
      (screen.getByRole('textbox', { name: 'Message Robin' }) as HTMLTextAreaElement).value,
    ).toBe('');
    await user.type(screen.getByRole('textbox', { name: 'Message Robin' }), 'Robin only');
    await user.click(screen.getByRole('button', { name: 'Send preview message' }));
    await user.click(screen.getByRole('button', { name: 'Activity', exact: true }));
    expect(screen.getByText('Robin only')).toBeTruthy();
    await user.selectOptions(screen.getByRole('combobox', { name: 'Agent' }), 'agt-builder');
    expect(screen.queryByText('Robin only')).toBeNull();
    await user.keyboard('{Escape}');
    await user.click(trigger);
    await user.click(screen.getByRole('button', { name: 'Open profile', exact: true }));
    expect(
      (screen.getByRole('textbox', { name: 'Message Alex' }) as HTMLTextAreaElement).value,
    ).toBe('Alex only');
  });
  it('decides only the requested escalation and requires explicit next-request navigation', async () => {
    const user = userEvent.setup();
    render(<App initialView="map" />);
    await user.click(screen.getByRole('button', { name: 'Review request from Casey' }));
    expect(screen.getByRole('heading', { name: 'Allow work on your machine?' })).toBeTruthy();
    await user.click(screen.getByRole('button', { name: 'Decline', exact: true }));
    expect(screen.getByRole('heading', { name: 'Declined.' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Approve this request' })).toBeNull();
    await user.click(screen.getByRole('button', { name: 'See decision' }));
    expect(screen.getByText('Decision: rejected')).toBeTruthy();
    await user.keyboard('{Escape}');
    expect(screen.getByRole('button', { name: 'Needs you, 4 items' })).toBeTruthy();
  });
  it('creates a team and adds an existing agent through explicit membership controls', async () => {
    const user = userEvent.setup();
    render(<App initialView="map" />);
    await user.click(screen.getByRole('button', { name: 'Teams', exact: true }));
    await user.click(screen.getAllByRole('button', { name: 'Create a team', exact: true })[0]);
    await user.type(screen.getByRole('textbox', { name: 'Team name' }), 'Research');
    await user.click(screen.getByRole('button', { name: 'Create team', exact: true }));
    await user.click(screen.getByRole('button', { name: 'Add agent to Research' }));
    const add = screen.getByRole('button', { name: 'Add Sam to Research' });
    await user.click(add);
    expect(add).toHaveProperty('disabled', true);
    await user.keyboard('{Escape}');
    expect(screen.getByRole('button', { name: 'Focus Sam', exact: true })).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Focus Alex', exact: true })).toBeNull();
  });
  it('shows the simple creation form when no agents exist', async () => {
    const user = userEvent.setup(),
      create = vi.fn();
    render(
      <AgentsView
        agents={[]}
        teams={mockTeams}
        currentTeamId="team-platform"
        onCreate={create}
        onPledgeAgent={vi.fn()}
        onInspect={vi.fn()}
      />,
    );
    await user.type(screen.getByRole('textbox', { name: 'Name', exact: true }), 'June');
    await user.type(screen.getByRole('textbox', { name: 'What will they help with?' }), 'Research');
    await user.click(screen.getByRole('button', { name: 'Create agent', exact: true }));
    expect(create).toHaveBeenCalledExactlyOnceWith('June', 'Research', 'team-platform');
  });
  it('preserves camera placement while opening and closing an inspector', async () => {
    const user = userEvent.setup();
    const { container } = render(<App initialView="map" />);
    const world = container.querySelector('.universe-world') as HTMLElement;
    const initialScale = Number(world.style.transform.split('scale(')[1]?.replace(')', ''));
    await user.click(screen.getByRole('button', { name: 'Zoom in', exact: true }));
    await waitFor(() =>
      expect(Number(world.style.transform.split('scale(')[1]?.replace(')', ''))).toBeCloseTo(
        initialScale * 1.2,
        9,
      ),
    );
    const trigger = screen.getByRole('button', { name: 'Inspect GitHub', exact: true });
    await user.click(trigger);
    const transform = world.style.transform;
    expect(screen.getByRole('dialog', { name: 'GitHub MCP Connector' })).toBeTruthy();
    await user.keyboard('{Escape}');
    await waitFor(() => expect(document.activeElement).toBe(trigger));
    expect(world.style.transform).toBe(transform);
  });
  it('supports synthetic playback, explicit release states and reduced motion', async () => {
    const user = userEvent.setup();
    const { container } = render(<App initialView="map" />);
    await user.click(screen.getByRole('button', { name: 'Motion examples and accessibility' }));
    await user.selectOptions(screen.getByRole('combobox', { name: 'Motion example' }), 'flow');
    await user.click(screen.getByRole('button', { name: 'Next interaction state' }));
    const actor = container.querySelector('[data-entity="agt-builder"]')!;
    expect(actor.getAttribute('data-phase')).toBe('engaged');
    await user.click(screen.getByRole('button', { name: 'Next interaction state' }));
    expect(actor.getAttribute('data-host')).toBe('');
    await user.click(screen.getByRole('button', { name: 'Motion examples and accessibility' }));
    await user.click(screen.getByRole('checkbox', { name: 'Reduce motion' }));
    expect(container.querySelector('.universe')?.getAttribute('data-reduced-motion')).toBe('true');
    expect(
      within(screen.getByRole('group', { name: 'Motion playback options' })).getByText(
        /Synthetic motion example/,
      ),
    ).toBeTruthy();
  });
  it('keeps floating chat drafts and conversations separate without resetting motion', async () => {
    const user = userEvent.setup();
    const { container } = render(<App initialView="map" />);
    await user.click(screen.getByRole('button', { name: 'Motion examples and accessibility' }));
    await user.selectOptions(screen.getByRole('combobox', { name: 'Motion example' }), 'flow');
    await user.click(screen.getByRole('button', { name: 'Next interaction state' }));
    const actor = container.querySelector('[data-entity="agt-builder"]')!;
    expect(actor.getAttribute('data-phase')).toBe('engaged');
    await user.type(
      screen.getByRole('textbox', { name: 'Message the team on the map' }),
      'Team thought',
    );
    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Message recipient' }),
      'agt-doc',
    );
    await user.type(
      screen.getByRole('textbox', { name: 'Message Robin on the map' }),
      'Robin thought{Enter}',
    );
    expect(screen.getByRole('log', { name: 'Conversation with Robin' }).textContent).toContain(
      'Robin thought',
    );
    expect(actor.getAttribute('data-phase')).toBe('engaged');
    await user.selectOptions(screen.getByRole('combobox', { name: 'Message recipient' }), 'team');
    expect(
      (screen.getByRole('textbox', { name: 'Message the team on the map' }) as HTMLTextAreaElement)
        .value,
    ).toBe('Team thought');
    expect(
      screen.getByRole('log', { name: 'Conversation with the team' }).textContent,
    ).not.toContain('Robin thought');
    await user.click(screen.getByRole('button', { name: 'Send map message' }));
    expect(screen.getByRole('log', { name: 'Conversation with the team' }).textContent).toContain(
      'Team thought',
    );
    expect(screen.getByText('Saved here. No engine is connected.')).toBeTruthy();
  });
  it('connects failures to global attention and the same agent evidence', async () => {
    const user = userEvent.setup();
    render(<App initialView="map" />);
    await user.click(screen.getByRole('button', { name: 'Motion examples and accessibility' }));
    await user.selectOptions(screen.getByRole('combobox', { name: 'Motion example' }), 'interrupt');
    // Step until the first recorded failure in the deterministic interruption sample.
    for (let i = 0; i < 6 && !screen.queryByRole('button', { name: /failed work/ }); i++)
      await user.click(screen.getByRole('button', { name: 'Next interaction state' }));
    await user.click(screen.getByRole('button', { name: /Needs you/ }));
    const review = screen.getByRole('region', { name: 'Work to review' });
    await user.click(within(review).getByRole('button', { name: /Unresolved failure/ }));
    const summary = screen.getByRole('region', { name: 'Current sample work' });
    expect(summary.textContent).toContain('Unresolved failure');
    expect(screen.getByRole('list', { name: 'Agent activity' }).textContent).toContain('Failed:');
    expect(screen.queryByText('No events in this view')).toBeNull();
    await user.click(screen.getByRole('button', { name: 'Back to attention' }));
    expect(screen.getByRole('region', { name: 'Work to review' })).toBeTruthy();
  });
  it('keeps playback, selection and camera when switching away from a team and back', async () => {
    const user = userEvent.setup();
    const { container } = render(<App initialView="map" />);
    await user.click(screen.getByRole('button', { name: 'Motion examples and accessibility' }));
    await user.selectOptions(screen.getByRole('combobox', { name: 'Motion example' }), 'flow');
    await user.click(screen.getByRole('button', { name: 'Next interaction state' }));
    await user.click(screen.getByRole('button', { name: 'Focus Alex', exact: true }));
    const world = container.querySelector('.universe-world') as HTMLElement;
    const transform = world.style.transform;
    const time = screen.getByLabelText('Sample time').textContent;
    const actor = container.querySelector('[data-entity="agt-builder"]') as HTMLElement;
    const home = actor.dataset.home;
    const physicalPosition = actor.dataset.physicalPosition;
    await user.click(screen.getByRole('button', { name: 'Teams', exact: true }));
    await user.click(screen.getByRole('button', { name: 'View all teams' }));
    expect(screen.getByLabelText('Sample time').textContent).toBe(time);
    expect(screen.getByRole('complementary', { name: 'Following Alex' })).toBeTruthy();
    // Semantic zoom blends the representation, not the actual home or paused physics.
    expect(actor.dataset.home).toBe(home);
    expect(actor.dataset.physicalPosition).toBe(physicalPosition);
    await user.click(screen.getByRole('button', { name: 'Teams', exact: true }));
    await user.click(screen.getByRole('button', { name: 'Open Platform Core Guild map' }));
    expect(world.style.transform).toBe(transform);
    expect(screen.getByLabelText('Sample time').textContent).toBe(time);
    expect(container.querySelector('[data-entity="agt-builder"]')?.getAttribute('data-phase')).toBe(
      'engaged',
    );
  });
});
