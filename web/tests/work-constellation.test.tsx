import { describe, expect, it, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { ProjectMapPortfolio } from '../src/components/workspace/ProjectMapPortfolio';
import { WorkConstellation } from '../src/components/workspace/WorkConstellation';
import { layoutProject, layoutPortfolio } from '../src/lib/projectLayout';
import { type ProjectView } from '../src/lib/projectView';
import { exampleProjects } from './fixtures/example';

function sharedAgentProject(): ProjectView {
  const original = exampleProjects(0, false)[0];
  const person = { ...original.people[0], destination: undefined, tool: undefined };
  return {
    ...original,
    people: [person],
    places: [],
    streams: ['waiting', 'working'].map((status, index) => ({
      id: `assignment-${index}`,
      name: `Assignment ${index}`,
      summary: '',
      role: 'contribution',
      agents: [person.agent.id],
      tasks: [
        {
          id: `task-${index}`,
          title: `Task ${index}`,
          owner: person.agent.id,
          status: status as 'waiting' | 'working',
          detail: '',
        },
      ],
    })),
  };
}

describe('constellation workspace', () => {
  it('keeps assignment and agent inspection separate and preserves recorded status', () => {
    const project = sharedAgentProject();
    const onStream = vi.fn(),
      onAgent = vi.fn();
    render(
      <WorkConstellation
        project={project}
        graph={layoutProject(project)}
        onStream={onStream}
        onAgent={onAgent}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Open Assignment 1' }));
    expect(onStream).toHaveBeenCalledWith('assignment-1');
    expect(onAgent).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Inspect Mira, assigned to Assignment 1' }));
    expect(onAgent).toHaveBeenCalledWith(project.people[0].agent.id);
    expect(screen.getByText('Waiting')).toBeTruthy();
    expect(screen.getByText('In progress')).toBeTruthy();
  });

  it('keeps one portrait entity for an agent shared by assignments and docks it at actual active work', () => {
    const project = sharedAgentProject();
    const layout = layoutProject(project);
    expect(layout.people).toHaveLength(1);
    const home = layout.people[0].home;
    const active = layout.streams['assignment-1'];
    expect(home.x).toBeGreaterThan(active.x);
    expect(home.x).toBeLessThan(active.x + active.width);
    const reordered = layoutProject({ ...project, streams: [...project.streams].reverse() });
    expect(reordered.people[0].home.x).toBeGreaterThan(reordered.streams['assignment-1'].x);
    expect(reordered.people[0].home.x).toBeLessThan(
      reordered.streams['assignment-1'].x + active.width,
    );
  });

  it('distinguishes unavailable membership from an unassigned task', () => {
    const project = sharedAgentProject();
    project.people = [];
    project.streams[0].agents = [];
    render(
      <WorkConstellation
        project={project}
        graph={layoutProject(project)}
        onStream={vi.fn()}
        onAgent={vi.fn()}
      />,
    );
    expect(screen.getByText('Unassigned')).toBeTruthy();
    expect(screen.getByText('Assigned agent unavailable')).toBeTruthy();
  });

  it('exposes effort and area navigation with truthful counts when the visual cluster is capped', () => {
    const project = sharedAgentProject();
    project.streams = Array.from({ length: 17 }, (_, index) => ({
      ...project.streams[0],
      id: `stream-${index}`,
      role: index === 16 ? 'coordination' : 'contribution',
    }));
    const onProject = vi.fn(),
      onArea = vi.fn();
    const groups = layoutPortfolio([project]).groups;
    render(<ProjectMapPortfolio groups={groups} onProject={onProject} onArea={onArea} />);
    expect(screen.getByText('+6 assignments')).toBeTruthy();
    expect(screen.getByText('16 assignments')).toBeTruthy();
    expect(screen.getByText('1 agent')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: `Open effort: ${project.title}` }));
    expect(onProject).toHaveBeenCalledWith(project.id);
    fireEvent.click(screen.getByRole('button', { name: /Customer experience 1 effort/ }));
    expect(onArea).toHaveBeenCalledWith(groups[0]);
  });
});
