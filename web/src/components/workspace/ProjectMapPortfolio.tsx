import { ArrowUpRight, Maximize2 } from 'lucide-react';
import { layoutPortfolio } from '../../lib/projectLayout';
import { projectCounts, type ProjectView } from '../../lib/projectView';
import { combinedSignal } from '../../lib/workSignals';
import { Portrait } from '../ui/Portrait';
import { WorkStatus } from '../team-work/WorkStatus';
import { MapActivity } from './MapActivity';

type Area = ReturnType<typeof layoutPortfolio>['groups'][number];
export function ProjectMapPortfolio({
  groups,
  onProject,
  onArea,
}: {
  groups: Area[];
  onProject: (id: string) => void;
  onArea: (area: Area) => void;
}) {
  return groups.map((group) => (
    <section
      key={group.id}
      className="constellation-area"
      data-tone={group.area?.tone || 'ink'}
      aria-label={group.area?.name || 'Other work'}
      style={{ left: group.x, top: group.y, width: group.width, height: group.height }}
    >
      <button className="constellation-area-heading" onClick={() => onArea(group)}>
        <span className="constellation-area-dot" aria-hidden="true" />
        <strong>{group.area?.name || 'Other work'}</strong>
        <span>
          {group.items.length} {group.items.length === 1 ? 'effort' : 'efforts'}
        </span>
        <Maximize2 size={16} />
      </button>
      {group.items.map((item, index) => (
        <EffortCluster
          key={item.id}
          item={item}
          onOpen={() => onProject(item.id)}
          left={25 + (index % group.columns) * 495}
          top={80 + Math.floor(index / group.columns) * 340}
        />
      ))}
    </section>
  ));
}

function EffortCluster({
  item,
  onOpen,
  left,
  top,
}: {
  item: ProjectView;
  onOpen: () => void;
  left: number;
  top: number;
}) {
  const count = projectCounts(item);
  const statuses = item.streams.flatMap((stream) => stream.tasks.map((task) => task.status));
  const contributions = item.streams.filter((stream) => stream.role !== 'coordination');
  // Each satellite is a recorded assignment. A bounded view avoids painting
  // hundreds of tiny marks; the remainder is explicitly counted, never invented.
  const visible = contributions.slice(0, 10);
  const people = [...new Map(item.people.map((person) => [person.agent.id, person])).values()];
  return (
    <button
      className="constellation-effort"
      data-signal={combinedSignal(statuses)}
      style={{ left, top }}
      onClick={onOpen}
      title={item.title}
      aria-label={`Open effort: ${item.title}`}
    >
      <span className="constellation-effort-figure" aria-hidden="true">
        <svg viewBox="0 0 440 180">
          <ellipse className="constellation-effort-orbit" cx="220" cy="90" rx="118" ry="68" />
          {visible.map((stream, index) => {
            const angle = (index / visible.length) * Math.PI * 2 - Math.PI / 2;
            const x = 220 + Math.cos(angle) * 118,
              y = 90 + Math.sin(angle) * 68;
            return (
              <g
                key={stream.id}
                data-signal={combinedSignal(stream.tasks.map((task) => task.status))}
              >
                <path className="constellation-membership" d={`M220 90 Q${x} 90 ${x} ${y}`} />
                <circle className="constellation-satellite-halo" cx={x} cy={y} r="14" />
                <circle className="constellation-satellite" cx={x} cy={y} r="6" />
              </g>
            );
          })}
        </svg>
        <span className="constellation-effort-people">
          {people.slice(0, 3).map(({ agent }) => (
            <Portrait key={agent.id} agent={agent} size={54} square={false} />
          ))}
          {!people.length && <span className="constellation-unassigned">No agents</span>}
          {people.length > 3 && <span className="constellation-more">+{people.length - 3}</span>}
        </span>
        {contributions.length > 10 && (
          <span className="constellation-more-work">+{contributions.length - 10} assignments</span>
        )}
      </span>
      <strong>
        {item.title}
        <ArrowUpRight size={18} />
      </strong>
      <span className="constellation-effort-state">
        <WorkStatus signal={combinedSignal(statuses)} />
        <MapActivity count={count.working} />
      </span>
      <span className="constellation-effort-meta">
        <span>
          {people.length} {people.length === 1 ? 'agent' : 'agents'}
        </span>
        <i aria-hidden="true">·</i>
        <span>
          {contributions.length} {contributions.length === 1 ? 'assignment' : 'assignments'}
        </span>
        {count.attention > 0 && (
          <>
            <i aria-hidden="true">·</i>
            <span>{count.attention} to review</span>
          </>
        )}
      </span>
    </button>
  );
}
