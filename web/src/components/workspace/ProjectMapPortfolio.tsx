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
      className="pm-area"
      data-tone={group.area?.tone || 'ink'}
      aria-label={group.area?.name || 'Other work'}
      style={{ left: group.x, top: group.y, width: group.width, height: group.height }}
    >
      <button className="pm-area-heading" onClick={() => onArea(group)}>
        <span>
          <small>
            {group.items.length} {group.items.length === 1 ? 'effort' : 'efforts'}
          </small>
          <strong>{group.area?.name || 'Other work'}</strong>
        </span>
        <Maximize2 size={18} />
      </button>
      {group.items.map((item, index) => (
        <EffortCard
          key={item.id}
          item={item}
          onOpen={() => onProject(item.id)}
          left={25 + (index % group.columns) * 495}
          top={95 + Math.floor(index / group.columns) * 240}
        />
      ))}
    </section>
  ));
}
function EffortCard({
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
  const statuses = item.streams.flatMap((s) => s.tasks.map((t) => t.status));
  const contributions = item.streams
    .filter((s) => s.role !== 'coordination')
    .flatMap((s) => s.tasks);
  const mix = contributions.length ? contributions.map((t) => t.status) : statuses;
  const done = contributions.filter((t) => t.status === 'done').length;
  return (
    <button
      className="pm-project"
      data-signal={combinedSignal(statuses)}
      data-active={count.working > 0}
      style={{ left, top }}
      onClick={onOpen}
      title={item.title}
    >
      <span className="pm-project-team">
        <span>
          {item.kind === 'plan'
            ? 'Team effort'
            : item.kind === 'workspace'
              ? 'Requests & explorations'
              : item.team}
        </span>
        <MapActivity count={count.working} />
        <ArrowUpRight size={18} />
      </span>
      <strong>{item.title}</strong>
      <span className="pm-project-people">
        {item.people.slice(0, 5).map((p) => (
          <Portrait key={p.agent.id} agent={p.agent} size={30} square={false} />
        ))}
        <span>
          {item.people.length} agents · {contributions.length}{' '}
          {item.kind === 'plan' ? 'contributions' : 'requests'}
        </span>
      </span>
      <span className="pm-project-state">
        <span>
          {count.attention
            ? `${count.attention} to review`
            : contributions.length
              ? `${done}/${contributions.length} finished`
              : 'No contributions yet'}
        </span>
        <WorkStatus signal={combinedSignal(statuses)} />
      </span>
      <span className="pm-work-mix" aria-hidden="true">
        {(
          ['done', 'working', 'needs_you', 'blocked', 'waiting', 'stopped', 'unknown'] as const
        ).map((signal) => {
          const count = mix.filter((s) => s === signal).length;
          return count ? (
            <span key={signal} data-signal={signal} style={{ flexGrow: count }} />
          ) : null;
        })}
      </span>
    </button>
  );
}
