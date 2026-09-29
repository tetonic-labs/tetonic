import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { TeamActivityMap } from '../src/components/graph/TeamActivityMap';
import { useOrganizationActivity } from '../src/components/graph/useOrganizationActivity';
import { largeWorkspace } from '../src/store/largeWorkspaces';
import { App } from '../src/App';
import { resourcesFromGraph, type WorkspaceResource } from '../src/lib/toolLibrary';

const org = largeWorkspace('network');
const library = resourcesFromGraph(org.nodes, org.edges, org.teams);
const engineering = org.teams.find((t) => t.name === 'Engineering')!;
const openRoom = vi.fn();
let activity: ReturnType<typeof useOrganizationActivity>;
function Harness({ resources = library }: { resources?: WorkspaceResource[] }) {
  activity = useOrganizationActivity(org.agents, org.nodes, org.edges, org.tracks);
  return (
    <TeamActivityMap
      agents={org.agents}
      teams={org.teams}
      nodes={org.nodes}
      edges={org.edges}
      tracks={org.tracks}
      approvals={[]}
      activity={activity}
      resources={resources}
      scope="all"
      onWork={vi.fn()}
      onTeam={openRoom}
      onAgent={vi.fn()}
      onDestination={vi.fn()}
      onRequest={vi.fn()}
      onAddAgent={vi.fn()}
    />
  );
}
beforeEach(() => {
  vi.stubGlobal('matchMedia', () => ({
    matches: true,
    addEventListener() {},
    removeEventListener() {},
  }));
  openRoom.mockClear();
});
afterEach(() => vi.unstubAllGlobals());
const square = () => screen.getByRole('button', { name: 'Explore Engineering', exact: true });

describe('team map lens', () => {
  it('keeps a busy team visible when its agents work with remote destinations', () => {
    const { container } = render(<Harness />);
    const data = org.teams.find((t) => t.name === 'Data')!;
    const rush = activity.examples.find((e) => e.name.startsWith('All-hands rush'))!;
    act(() => activity.setExampleId(rush.id));
    fireEvent.click(screen.getByRole('button', { name: 'Explore Data', exact: true }));
    for (const time of [2, 6, 12, 20, 34, 52]) {
      act(() => activity.seek(time));
      for (const id of data.pledgedAgentIds) {
        const entity = container.querySelector<HTMLElement>(`[data-entity="${id}"]`)!;
        const target =
          activity.states.get(id)?.interaction?.targetId ||
          activity.states.get(id)?.previous?.interaction.targetId;
        if (target && data.pledgedAgentIds.includes(target)) continue;
        const home = entity.dataset.home!.split(',').map(Number);
        const position = entity.style.transform.match(/translate\(([-\d.]+)px,([-\d.]+)px\)/)!;
        expect(
          Math.hypot(Number(position[1]) - home[0], Number(position[2]) - home[1]),
        ).toBeLessThanOrEqual(20.01);
      }
    }
  });
  it('widens a filtered map into the organization without interrupting the requested zoom', () => {
    const { container } = render(<App initialView="map" />);
    expect(screen.queryByRole('button', { name: 'Focus Sam', exact: true })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Explore Platform Core Guild' }));
    expect(container.querySelector('.living-map')?.getAttribute('data-team-blend')).toBe('1.000');
    fireEvent.click(screen.getByRole('button', { name: 'Back to organization' }));
    expect(screen.getByRole('button', { name: 'All teams', exact: true })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Focus Sam', exact: true })).toBeTruthy();
  });

  it('restores the correct team and resources when revisiting an earlier camera view', () => {
    const { container } = render(<Harness />);
    fireEvent.click(square());
    const world = container.querySelector<HTMLElement>('.universe-world')!;
    const engineeringView = world.style.transform;
    fireEvent.click(screen.getByRole('button', { name: 'Fit map' }));
    fireEvent.click(screen.getByRole('button', { name: 'Explore Research', exact: true }));
    fireEvent.click(screen.getByRole('button', { name: 'Previous map view' }));
    fireEvent.click(screen.getByRole('button', { name: 'Previous map view' }));
    expect(world.style.transform).toBe(engineeringView);
    expect(screen.getByRole('complementary', { name: 'Engineering tools and MCPs' })).toBeTruthy();
    expect(screen.queryByRole('complementary', { name: 'Research tools and MCPs' })).toBeNull();
  });
  it('zooms from the square, isolates the team, and shows only its current resource assignments', () => {
    const { container } = render(<Harness />);
    const world = container.querySelector<HTMLElement>('.universe-world')!;
    const before = world.style.transform;
    fireEvent.click(square());
    expect(world.style.transform).not.toBe(before);
    expect(openRoom).not.toHaveBeenCalled();
    expect(container.querySelector('.living-map')?.getAttribute('data-team-blend')).toBe('1.000');
    const others = [...container.querySelectorAll<HTMLElement>('.team-neighborhood')].filter(
      (g) => g.dataset.team !== engineering.id,
    );
    expect(others.every((g) => Number(g.style.opacity) < 0.1 && g.hasAttribute('inert'))).toBe(
      true,
    );
    const resources = screen.getByRole('complementary', { name: 'Engineering tools and MCPs' });
    const assigned = library.filter((r) => r.teamIds.includes(engineering.id));
    expect(
      [...resources.querySelectorAll<HTMLElement>('[data-resource]')].map(
        (r) => r.dataset.resource,
      ),
    ).toEqual(
      [
        ...assigned.filter((r) => r.kind === 'mcp'),
        ...assigned.filter((r) => r.kind !== 'mcp'),
      ].map((r) => r.id),
    );
    expect(assigned.length).toBeLessThan(library.length);
  });

  it('fades continuously back to the organization on wheel out without moving homes or restarting work', () => {
    const { container } = render(<Harness />);
    const homes = [...container.querySelectorAll('[data-home]')].map((n) =>
      n.getAttribute('data-home'),
    );
    act(() => activity.seek(24));
    const time = activity.elapsed;
    fireEvent.click(square());
    const map = container.querySelector('.universe-canvas')!;
    const root = container.querySelector('.living-map')!;
    fireEvent.wheel(map, { deltaY: 90, clientX: 500, clientY: 350 });
    expect(Number(root.getAttribute('data-team-blend'))).toBeGreaterThan(0);
    expect(Number(root.getAttribute('data-team-blend'))).toBeLessThan(1);
    for (let i = 0; i < 8; i++) fireEvent.wheel(map, { deltaY: 150, clientX: 500, clientY: 350 });
    expect(root.getAttribute('data-team-blend')).toBe('0.000');
    expect(screen.queryByRole('complementary', { name: 'Engineering tools and MCPs' })).toBeNull();
    expect(
      [...container.querySelectorAll<HTMLElement>('.team-neighborhood')].every(
        (g) => g.style.opacity === '1',
      ),
    ).toBe(true);
    expect(
      [...container.querySelectorAll('[data-home]')].map((n) => n.getAttribute('data-home')),
    ).toEqual(homes);
    expect(activity.elapsed).toBe(time);
  });

  it('uses the editable library rather than old graph edges, including empty assignments and setup drafts', () => {
    const view = render(<Harness />);
    fireEvent.click(square());
    const world = view.container.querySelector<HTMLElement>('.universe-world')!;
    const position = world.style.transform;
    view.rerender(<Harness resources={[]} />);
    expect(screen.getByText('No tools assigned yet.')).toBeTruthy();
    const draft: WorkspaceResource = {
      id: 'draft-custom',
      name: 'An intentionally long MCP connection name for the engineering team',
      kind: 'mcp',
      description: '',
      source: 'draft',
      teamIds: [engineering.id],
    };
    view.rerender(<Harness resources={[draft]} />);
    const shelf = screen.getByRole('complementary', { name: 'Engineering tools and MCPs' });
    expect(within(shelf).getByText(draft.name)).toBeTruthy();
    expect(within(shelf).getByText('Setup draft')).toBeTruthy();
    expect(world.style.transform).toBe(position);
    expect(view.container.querySelector('[data-entity="draft-custom"]')).toBeNull();
  });

  it('supports keyboard entry, separate conversation access, and Back without losing the close view', async () => {
    const user = userEvent.setup();
    const { container } = render(<Harness />);
    square().focus();
    await user.keyboard('{Enter}');
    const world = container.querySelector<HTMLElement>('.universe-world')!;
    const close = world.style.transform;
    fireEvent.click(screen.getByRole('button', { name: 'Open Engineering conversation' }));
    expect(openRoom).toHaveBeenCalledOnce();
    act(() => openRoom.mock.calls[0][1]());
    expect(world.style.transform).toBe(close);
    fireEvent.click(screen.getByRole('button', { name: 'Back to organization' }));
    expect(container.querySelector('.living-map')?.getAttribute('data-team-blend')).toBe('0.000');
    fireEvent.click(screen.getByRole('button', { name: 'Previous map view' }));
    expect(world.style.transform).toBe(close);
    expect(screen.getByRole('complementary', { name: 'Engineering tools and MCPs' })).toBeTruthy();
  });
});
