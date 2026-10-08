import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { ProjectMap } from '../src/components/workspace/ProjectMap';
import { layoutProject } from '../src/lib/projectLayout';
import type { ProjectStream, ProjectView } from '../src/lib/projectView';

const motionPreference = vi.hoisted(() => ({ reduced: false }));
vi.mock('motion/react', async (importOriginal) => ({
  ...(await importOriginal<object>()),
  useReducedMotion: () => motionPreference.reduced,
}));
const stream = (
  id: string,
  status: 'working' | 'done' | 'blocked' = 'working',
  dependencies: string[] = [],
): ProjectStream => ({
  id,
  name: id,
  summary: '',
  agents: [],
  role: 'contribution',
  tasks: [{ id, title: id, owner: '', status, detail: '' }],
  dependencies: dependencies.map((id) => ({ id, reason: 'Uses this contribution' })),
});
const project = (streams: ProjectStream[]): ProjectView => ({
  id: 'effort',
  title: 'Launch research',
  kind: 'plan',
  team: 'Studio',
  aim: '',
  update: '',
  people: [],
  places: [],
  streams,
});
const props = { onProject: vi.fn(), onStream: vi.fn(), onAgent: vi.fn(), onPlace: vi.fn() };
afterEach(() => {
  motionPreference.reduced = false;
  vi.restoreAllMocks();
});

describe('truthful work activity', () => {
  it('shows ongoing work even when another contribution needs attention, without claiming completion', () => {
    const running = project([
      stream('research'),
      stream('review', 'blocked'),
      { ...stream('root', 'done'), role: 'coordination' },
    ]);
    const completed = {
      ...project([stream('finished', 'done')]),
      id: 'finished',
      title: 'Published result',
    };
    render(<ProjectMap {...props} projects={[running, completed]} playing />);
    const card = screen.getByRole('button', { name: /Launch research/ });
    expect(within(card).getByText('1 running')).toBeTruthy();
    expect(within(card).getByText('Blocked')).toBeTruthy();
    expect(within(card).queryByText('Done')).toBeNull();
    expect(
      within(screen.getByRole('button', { name: /Published result/ })).queryByText(/running/),
    ).toBeNull();
  });
  it('pauses cues, stops them on disconnection and resumes only when enabled', () => {
    const data = [project([stream('research')])];
    const view = render(<ProjectMap {...props} projects={data} playing />);
    const map = screen.getByRole('region', { name: 'All projects map' });
    expect(map.getAttribute('data-motion')).toBe('true');
    fireEvent.click(screen.getByRole('button', { name: 'Pause activity motion' }));
    expect(map.getAttribute('data-motion')).toBe('false');
    fireEvent.click(screen.getByRole('button', { name: 'Resume activity motion' }));
    expect(map.getAttribute('data-motion')).toBe('true');
    view.rerender(<ProjectMap {...props} projects={data} playing={false} />);
    expect(map.getAttribute('data-motion')).toBe('false');
    expect(screen.getByText('1 running')).toBeTruthy(); // Last observed state remains readable.
  });
  it('keeps static state visible with reduced motion and while the document is hidden', () => {
    motionPreference.reduced = true;
    const data = [project([stream('research')])];
    const view = render(<ProjectMap {...props} projects={data} playing />);
    expect(
      screen.getByRole('region', { name: 'All projects map' }).getAttribute('data-motion'),
    ).toBe('false');
    expect(
      screen.getByRole('button', { name: 'Pause activity motion' }).hasAttribute('disabled'),
    ).toBe(true);
    motionPreference.reduced = false;
    view.rerender(<ProjectMap {...props} projects={data} playing />);
    const hidden = vi.spyOn(document, 'hidden', 'get').mockReturnValue(true);
    fireEvent(document, new Event('visibilitychange'));
    expect(
      screen.getByRole('region', { name: 'All projects map' }).getAttribute('data-motion'),
    ).toBe('false');
    hidden.mockReturnValue(false);
    fireEvent(document, new Event('visibilitychange'));
    expect(
      screen.getByRole('region', { name: 'All projects map' }).getAttribute('data-motion'),
    ).toBe('true');
  });
});

describe('recorded work structure', () => {
  it('groups independent contributions, their follow-up, and coordination without adding edges', () => {
    const data = project([
      { ...stream('coordinator', 'working', ['review']), role: 'coordination' },
      stream('research'),
      stream('analysis'),
      stream('review', 'working', ['research', 'analysis']),
    ]);
    const layout = layoutProject(data);
    expect(layout.bands.map((b) => b.ids)).toEqual([
      ['research', 'analysis'],
      ['review'],
      ['coordinator'],
    ]);
    expect(layout.streams.research.y).toBe(layout.streams.analysis.y);
    expect(layout.streams.review.x).toBeGreaterThan(layout.streams.analysis.x);
    expect(layout.streams.coordinator.x).toBeGreaterThan(layout.streams.review.x);
    expect(layout.edges.map((e) => [e.from, e.to])).toEqual([
      ['review', 'coordinator'],
      ['research', 'review'],
      ['analysis', 'review'],
    ]);
    const complete = {
      ...data,
      streams: data.streams.map((s) => ({
        ...s,
        tasks: s.tasks.map((t) => ({ ...t, status: 'done' as const })),
      })),
    };
    expect(layoutProject(complete).streams).toEqual(layout.streams);
  });
  it('labels missing dependency groups for review instead of calling them independent', () => {
    const layout = layoutProject(project([stream('review', 'working', ['missing'])]));
    expect(layout.bands[0].title).toBe('Dependencies need review');
    expect(layout.edges).toHaveLength(0);
  });
});
