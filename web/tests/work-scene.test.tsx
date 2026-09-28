import { expect, it, describe, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { workScene, detailLevel, markerPoint } from '../src/lib/workScene';
import { largeWorkspace } from '../src/store/largeWorkspaces';
import { rushTrace } from '../src/lib/rushTrace';
import { destinationsFor } from '../src/lib/mapActivity';
import { useOrganizationActivity } from '../src/components/graph/useOrganizationActivity';
import { TeamActivityMap } from '../src/components/graph/TeamActivityMap';
import type { MotionExample } from '../src/lib/motionPlayback';

describe('readable activity map', () => {
  it('separates current work, past work and persistent failure from identity', () => {
    const action = {
      id: 'work',
      agentId: 'a',
      targetId: 'tool',
      targetName: 'Tool',
      label: 'Build',
    };
    const trace: MotionExample = {
      id: 'example',
      name: 'Example',
      provenance: 'Test',
      duration: 30,
      events: [
        { id: 's', type: 'start', at: 1, interaction: action },
        { id: 'w', type: 'wait', at: 2, agentId: 'a', interactionId: 'work' },
        { id: 'r', type: 'resume', at: 4, agentId: 'a', interactionId: 'work' },
        { id: 'e', type: 'end', at: 5, agentId: 'a', interactionId: 'work', outcome: 'failed' },
        {
          id: 's2',
          type: 'start',
          at: 9,
          interaction: { ...action, id: 'retry', retryOf: 'work' },
        },
        {
          id: 'e2',
          type: 'end',
          at: 12,
          agentId: 'a',
          interactionId: 'retry',
          outcome: 'completed',
        },
      ],
    };
    expect(workScene(trace, 3).get('a')?.waiting).toBe(true);
    expect(workScene(trace, 7).get('a')?.interaction).toBeUndefined();
    expect(workScene(trace, 10).get('a')?.failure?.interaction.id).toBe('work');
    expect(workScene(trace, 15).get('a')?.failure).toBeUndefined();
    expect(workScene(trace, 15).get('a')?.previous?.outcome).toBe('completed');
  });
  it('keeps all landmarks fixed through the rush and bounds visual activity', () => {
    const data = largeWorkspace('network');
    function TestMap() {
      const activity = useOrganizationActivity(data.agents, data.nodes, data.edges, data.tracks);
      return (
        <TeamActivityMap
          agents={data.agents}
          teams={data.teams}
          nodes={data.nodes}
          edges={data.edges}
          tracks={data.tracks}
          approvals={[]}
          activity={activity}
          onWork={vi.fn()}
          scope="network"
          onAgent={vi.fn()}
          onDestination={vi.fn()}
          onRequest={vi.fn()}
          onAddAgent={vi.fn()}
        />
      );
    }
    const { container } = render(<TestMap />);
    const positions = () =>
      [...container.querySelectorAll<HTMLElement>('[data-entity]')].map((e) => e.style.transform);
    const before = positions();
    fireEvent.click(screen.getByRole('button', { name: 'Motion examples and accessibility' }));
    fireEvent.change(screen.getByRole('combobox', { name: 'Motion example' }), {
      target: { value: 'stress-rush' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Next interaction state' }));
    expect(positions()).toEqual(before);
    expect(container.querySelectorAll('.work-bundle').length).toBeLessThanOrEqual(6);
    expect(container.querySelectorAll('.work-marker')).toHaveLength(0);
    const camera = (container.querySelector('.universe-world') as HTMLElement).style.transform;
    fireEvent.click(screen.getByRole('button', { name: 'Focus Ada 1' }));
    expect(screen.getByRole('complementary', { name: 'Following Ada 1' })).toBeTruthy();
    expect((container.querySelector('.universe-world') as HTMLElement).style.transform).toBe(
      camera,
    );
    expect(container.querySelectorAll('.work-marker').length).toBeLessThanOrEqual(6);
    fireEvent.click(screen.getByRole('button', { name: 'Next interaction state' }));
    expect(screen.getByRole('complementary', { name: 'Following Ada 1' })).toBeTruthy();
    expect(positions()).toEqual(before);
    // A failure remains identifiable while another agent is followed.
    fireEvent.click(screen.getByRole('button', { name: 'Focus Jun 2' }));
    const failed = container.querySelector('[data-entity="demo-agent-0"]')!;
    if (failed.classList.contains('needs-attention'))
      expect(failed.classList.contains('map-dimmed')).toBe(false);
    fireEvent.click(screen.getByRole('button', { name: 'Stop following' }));
    expect(screen.queryByRole('complementary')).toBeNull();
  });
  it('retains all 80 activities even when the view aggregates motion', () => {
    const data = largeWorkspace('network');
    const [trace] = rushTrace(data.agents, destinationsFor(data.agents, data.nodes, data.edges));
    expect([...workScene(trace, 1).values()].filter((w) => w.interaction)).toHaveLength(80);
  });
  it('changes detail continuously and keeps marker travel bounded', () => {
    expect(Math.abs(detailLevel(0.499) - detailLevel(0.501))).toBeLessThan(0.01);
    expect(detailLevel(0.1)).toBe(0);
    expect(detailLevel(1)).toBe(1);
    expect(markerPoint({ x: 0, y: 0 }, { x: 200, y: 100 }, 0)).toEqual({ x: 0, y: 0 });
    const dock = markerPoint({ x: 0, y: 0 }, { x: 200, y: 100 }, 1);
    expect(dock.x).toBe(200);
    expect(dock.y).toBeCloseTo(100);
  });
  it('does not mistake an unrelated success for recovery and preserves independent failures', () => {
    const action = {
      id: 'first',
      agentId: 'a',
      targetId: 'tool',
      targetName: 'Tool',
      label: 'First job',
    };
    const trace: MotionExample = {
      id: 'retry',
      name: 'Retry',
      provenance: 'Test',
      duration: 20,
      events: [
        { id: '1', type: 'start', at: 0, interaction: action },
        { id: '2', type: 'end', at: 1, agentId: 'a', interactionId: 'first', outcome: 'failed' },
        { id: '3', type: 'start', at: 2, interaction: { ...action, id: 'unrelated' } },
        {
          id: '4',
          type: 'end',
          at: 3,
          agentId: 'a',
          interactionId: 'unrelated',
          outcome: 'completed',
        },
        { id: '5', type: 'start', at: 4, interaction: { ...action, id: 'second' } },
        { id: '6', type: 'end', at: 5, agentId: 'a', interactionId: 'second', outcome: 'failed' },
        {
          id: '7',
          type: 'start',
          at: 6,
          interaction: { ...action, id: 'retry', retryOf: 'first', targetId: 'different-tool' },
        },
        { id: '8', type: 'end', at: 7, agentId: 'a', interactionId: 'retry', outcome: 'cancelled' },
        {
          id: '9',
          type: 'start',
          at: 8,
          interaction: { ...action, id: 'retry-again', retryOf: 'first' },
        },
        {
          id: '10',
          type: 'end',
          at: 9,
          agentId: 'a',
          interactionId: 'retry-again',
          outcome: 'completed',
        },
      ],
    };
    expect(
      workScene(trace, 3)
        .get('a')!
        .failures.map((f) => f.interaction.id),
    ).toEqual(['first']);
    expect(workScene(trace, 6).get('a')!.failure!.retryId).toBe('retry');
    expect(workScene(trace, 7).get('a')!.failures).toHaveLength(2);
    expect(workScene(trace, 7).get('a')!.failure!.retryId).toBeUndefined();
    expect(
      workScene(trace, 9)
        .get('a')!
        .failures.map((f) => f.interaction.id),
    ).toEqual(['second']);
  });
});
