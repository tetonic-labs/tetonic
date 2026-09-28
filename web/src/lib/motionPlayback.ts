import { Agent } from '../types';
import { teammate } from './teammates';
import { Destination, homeFor, MapAction } from './mapActivity';
import { Entity, GraphMotion, Interaction, InteractionEvent } from './graphMotion';
import { stressTraces } from './stressTraces';
import { organizationTraces } from './organizationTraces';
import { rushTrace } from './rushTrace';

export interface MotionExample {
  id: string;
  name: string;
  provenance: string;
  events: InteractionEvent[];
  duration: number;
}
export const entitiesFor = (agents: Agent[], places: Destination[]): Entity[] => [
  ...places.map((p) => ({ id: p.id, kind: 'destination' as const, home: p.point })),
  ...agents.map((a, i) => ({ id: a.id, kind: 'agent' as const, home: homeFor(i) })),
];
export function examplesFor(
  agents: Agent[],
  places: Destination[],
  actions: MapAction[],
): MotionExample[] {
  const examples: MotionExample[] = [];
  if (actions.length) {
    const events: InteractionEvent[] = [];
    actions.forEach((action, i) => {
      const interaction: Interaction = {
        id: `trace-${action.agentId}-${action.step.id}`,
        agentId: action.agentId,
        targetId: action.targetId,
        targetName: action.targetName,
        tool: action.tool,
        label: action.step.title,
      };
      events.push({ id: `${interaction.id}-start`, at: i * 4, type: 'start', interaction });
      if (action.step.status === 'success' || action.step.status === 'failed')
        events.push({
          id: `${interaction.id}-end`,
          at: i * 4 + 2.8,
          type: 'end',
          agentId: action.agentId,
          interactionId: interaction.id,
          outcome: action.step.status === 'success' ? 'completed' : 'failed',
        });
      if (action.step.status === 'pending')
        events.push({
          id: `${interaction.id}-wait`,
          at: i * 4 + 0.05,
          type: 'wait',
          agentId: action.agentId,
          interactionId: interaction.id,
        });
    });
    examples.push({
      id: 'trace',
      name: 'Sample trace',
      provenance: 'Fixture trace · timing compressed · unfinished work stays attached',
      events: events.sort((a, b) => a.at - b.at),
      duration: actions.length * 4 + 1,
    });
  }
  const first = agents[0],
    second = agents[1],
    third = agents[2],
    place = places[0],
    next = places.find((p) => p.kind === 'tool') || places[1];
  if (!first || !place) return examples;
  const make = (
    id: string,
    name: string,
    build: (
      start: (
        at: number,
        key: string,
        agentId: string,
        targetId: string,
        targetName: string,
        tool?: string,
      ) => Interaction,
      end: (
        at: number,
        action: Interaction,
        outcome?: 'completed' | 'failed' | 'cancelled',
      ) => void,
      events: InteractionEvent[],
    ) => void,
  ) => {
    const events: InteractionEvent[] = [];
    const start = (
      at: number,
      key: string,
      agentId: string,
      targetId: string,
      targetName: string,
      tool = 'MCP',
    ) => {
      const interaction = {
        id: `${id}-${key}`,
        agentId,
        targetId,
        targetName,
        tool,
        label: `Motion example: ${name}`,
      };
      events.push({ id: `${interaction.id}-start`, at, type: 'start', interaction });
      return interaction;
    };
    const end = (
      at: number,
      interaction: Interaction,
      outcome: 'completed' | 'failed' | 'cancelled' = 'completed',
    ) => {
      events.push({
        id: `${interaction.id}-end`,
        at,
        type: 'end',
        agentId: interaction.agentId,
        interactionId: interaction.id,
        outcome,
      });
    };
    build(start, end, events);
    events.sort((a, b) => a.at - b.at);
    examples.push({
      id,
      name,
      provenance: 'Synthetic motion example · no engine work is running',
      events,
      duration: Math.max(...events.map((e) => e.at)) + 2.8,
    });
  };
  make('flow', 'Dock, work & release', (start, end) => {
    const a = start(0.2, 'first', first.id, place.id, place.name);
    end(3, a);
    if (next) {
      const b = start(3.1, 'next', first.id, next.id, next.name, 'Terminal');
      end(6.2, b);
    }
    if (second) {
      const c = start(6.4, 'handoff', first.id, second.id, teammate(second).name, 'Message');
      end(9.5, c);
    }
  });
  if (second)
    make('shared', 'A shared orbit', (start, end, events) => {
      const a = start(0.2, 'one', first.id, place.id, place.name),
        b = start(1, 'two', second.id, place.id, place.name);
      if (third) {
        const c = start(1.8, 'three', third.id, place.id, place.name);
        end(6, c);
      }
      events.push(
        { id: 'shared-wait', at: 3, type: 'wait', agentId: first.id, interactionId: a.id },
        { id: 'shared-resume', at: 4.5, type: 'resume', agentId: first.id, interactionId: a.id },
      );
      end(5, a);
      end(7.5, b);
    });
  make('interrupt', 'Failure, cancellation & redirect', (start, end) => {
    const a = start(0.2, 'cancel', first.id, place.id, place.name);
    end(0.43, a, 'cancelled');
    const b = start(2.1, 'failed', first.id, place.id, place.name);
    end(4.8, b, 'failed');
    if (next) {
      start(5.7, 'redirect-from', first.id, place.id, place.name);
      const c = start(6, 'redirect-to', first.id, next.id, next.name, 'Terminal');
      end(9, c);
    }
  });
  return [
    ...examples,
    ...rushTrace(agents, places),
    ...stressTraces(agents, places),
    ...organizationTraces(agents, places),
  ];
}

// This adapter schedules fixture events; the physics layer knows nothing about
// traces or elapsed playback time. A future authenticated event stream can dispatch
// the same start/end/wait/resume contract without replacing the motion system.
export class MotionPlayback {
  world: GraphMotion;
  elapsed = 0;
  cursor = 0;
  constructor(
    readonly entities: Entity[],
    readonly example?: MotionExample,
  ) {
    this.world = new GraphMotion(entities);
  }
  reset() {
    this.world = new GraphMotion(this.entities);
    this.cursor = 0;
    this.elapsed = 0;
  }
  advance(seconds: number, play: boolean, reduced = false) {
    if (!play || !this.example) {
      this.world.advance(seconds, reduced);
      return;
    }
    const target = Math.min(this.example.duration, this.elapsed + seconds);
    while (
      this.cursor < this.example.events.length &&
      this.example.events[this.cursor].at <= target
    ) {
      const event = this.example.events[this.cursor++];
      this.world.advance(Math.max(0, event.at - this.elapsed), reduced);
      this.elapsed = event.at;
      this.world.dispatch(event);
    }
    this.world.advance(Math.max(0, target - this.elapsed), reduced);
    this.elapsed = target;
  }
  seek(time: number) {
    this.reset();
    const target = Math.max(0, Math.min(this.example?.duration || 0, time));
    while (this.elapsed + 0.008 < target)
      this.advance(Math.min(1 / 60, target - this.elapsed), true);
    this.advance(target - this.elapsed, true);
    this.world.settle();
  }
}
