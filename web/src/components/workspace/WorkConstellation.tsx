import {
  ArrowUpRight,
  CircleCheck,
  Clock3,
  Flag,
  Network,
  OctagonAlert,
  CirclePause,
  CircleHelp,
} from 'lucide-react';
import { type ProjectView } from '../../lib/projectView';
import { layoutProject } from '../../lib/projectLayout';
import { combinedSignal } from '../../lib/workSignals';
import { WorkStatus } from '../team-work/WorkStatus';

const symbols = {
  done: CircleCheck,
  waiting: Clock3,
  needs_you: Flag,
  blocked: OctagonAlert,
  stopped: CirclePause,
  unknown: CircleHelp,
  working: Network,
};

/** Work is the anchor. Portraits rendered by the map remain unique agent entities;
 * assignment names here describe membership, not an extra concurrent worker. */
export function WorkConstellation({
  project,
  graph,
  selectedStream,
  onStream,
  onAgent,
}: {
  project: ProjectView;
  graph: ReturnType<typeof layoutProject>;
  selectedStream?: string;
  onStream: (id: string) => void;
  onAgent: (id: string) => void;
}) {
  return project.streams.map((stream) => {
    const box = graph.streams[stream.id];
    const signal = combinedSignal(stream.tasks.map((task) => task.status));
    const Icon = stream.role === 'coordination' ? Network : symbols[signal];
    const people = project.people.filter((person) => stream.agents.includes(person.agent.id));
    return (
      <section
        key={stream.id}
        className="constellation-work"
        data-signal={signal}
        data-selected={selectedStream === stream.id}
        data-role={stream.role}
        style={{ left: box.x, top: box.y, width: box.width, height: box.height }}
        aria-label={stream.name}
      >
        <button
          className="constellation-anchor"
          onClick={() => onStream(stream.id)}
          title={stream.name}
          aria-label={`Open ${stream.role === 'coordination' ? 'team coordination for ' : ''}${stream.name}`}
        >
          <span className="constellation-core" aria-hidden="true">
            <span className="constellation-core-ring" />
            <Icon size={25} strokeWidth={1.5} />
          </span>
          <span className="constellation-caption">
            <small>
              {stream.role === 'coordination'
                ? 'Bringing it together'
                : stream.role === 'exploration'
                  ? 'Exploring'
                  : 'Assignment'}
            </small>
            <strong>{stream.role === 'coordination' ? 'Team coordination' : stream.name}</strong>
            <WorkStatus signal={signal} label={stream.stateLabel} />
          </span>
          <ArrowUpRight className="constellation-open" size={18} />
        </button>
        <div className="constellation-assignees">
          {people.length ? (
            people.map(({ agent }) => (
              <button
                key={agent.id}
                onClick={() => onAgent(agent.id)}
                aria-label={`Inspect ${agent.name}, assigned to ${stream.name}`}
              >
                {agent.name}
              </button>
            ))
          ) : (
            <span>{stream.agents.length ? 'Assigned agent unavailable' : 'Unassigned'}</span>
          )}
        </div>
      </section>
    );
  });
}
