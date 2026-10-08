import { useEffect, useRef } from 'react';
import { motion, useReducedMotion } from 'motion/react';
import {
  ArrowUpRight,
  Flag,
  FileText,
  GitBranch,
  Maximize2,
  Minus,
  Plus,
  TestTube2,
} from 'lucide-react';
import { Portrait } from '../ui/Portrait';
import { useMapCamera } from '../graph/useMapCamera';
import { layoutProject, layoutPortfolio, edgePath } from '../../lib/projectLayout';
import { projectCounts, type ProjectView } from '../../lib/projectView';
import { combinedSignal } from '../../lib/workSignals';
import { WorkStatus } from '../team-work/WorkStatus';
export { projectCounts, projectTasks } from '../../lib/projectView';
export type {
  ProjectView,
  ProjectTask,
  ProjectStream,
  ProjectPerson,
  ProjectPlace,
} from '../../lib/projectView';

export function ProjectMap({
  projects,
  project,
  playing = false,
  selectedStream,
  inspectionWidth = 420,
  onProject,
  onStream,
  onAgent,
  onPlace,
  scope = 'team-work',
}: {
  projects: ProjectView[];
  project?: ProjectView;
  playing?: boolean;
  selectedStream?: string;
  inspectionWidth?: number;
  onProject: (id: string) => void;
  onStream: (id: string) => void;
  onAgent: (id: string) => void;
  onPlace: (id: string) => void;
  scope?: string;
}) {
  const reduced = !!useReducedMotion();
  const inspected = useRef<string | undefined>(undefined);
  const narrowScope = useRef<string | undefined>(undefined);
  const graph = project ? layoutProject(project) : undefined;
  const portfolio = layoutPortfolio(projects);
  const world = graph || portfolio;
  const camera = useMapCamera(
    `${scope}:${project?.id || 'all'}`,
    reduced,
    { width: world.width, height: world.height },
    { x: 0, y: -15, width: world.width, height: world.height + 30 },
    { top: 15, bottom: 15 },
  );
  useEffect(() => {
    const scope = project?.id || 'all';
    if (
      !camera.ready ||
      selectedStream ||
      camera.size.width >= 700 ||
      narrowScope.current === scope
    )
      return;
    narrowScope.current = scope;
    const box = graph ? Object.values(graph.streams)[0] : portfolio.groups[0];
    if (box)
      camera.focus(
        { x: box.x + box.width / 2, y: box.y + box.height / 2 },
        { width: box.width + 60, height: box.height + 45 },
      );
  }, [project?.id, camera.ready]);
  useEffect(() => {
    if (!camera.ready) return;
    if (!selectedStream) {
      if (inspected.current && inspected.current === project?.id) camera.back();
      inspected.current = undefined;
      return;
    }
    const bounds = graph?.streams[selectedStream];
    if (!bounds) return;
    inspected.current = project?.id;
    camera.focus(
      { x: bounds.x + bounds.width / 2, y: bounds.y + bounds.height / 2 },
      { width: bounds.width + 160, height: bounds.height + 80 },
      { right: camera.size.width > 760 ? inspectionWidth + 36 : 0 },
      'project-detail',
      true,
    );
  }, [
    selectedStream,
    camera.ready,
    project?.id,
    camera.size.width,
    camera.size.height,
    inspectionWidth,
  ]);
  return (
    <div
      className="pm-map"
      data-compact={camera.scale < 0.65}
      ref={camera.viewport}
      role="region"
      aria-label={project ? `${project.title} project map` : 'All projects map'}
    >
      <div
        className="universe-canvas pm-canvas"
        tabIndex={0}
        role="group"
        aria-label="Pan and zoom projects"
        onKeyDown={camera.onKeyDown}
        onPointerDown={camera.onPointerDown}
        onPointerMove={camera.onPointerMove}
        onPointerUp={camera.onPointerUp}
        onPointerCancel={camera.onPointerCancel}
        onClickCapture={camera.onClickCapture}
      >
        <div
          className="pm-world"
          style={{
            width: world.width,
            height: world.height,
            transform: `translate(${camera.offset.x}px,${camera.offset.y}px) scale(${camera.scale})`,
          }}
        >
          {project && graph ? (
            <>
              {graph.warnings.length > 0 && (
                <p className="pm-layout-warning">{graph.warnings.join(' ')}</p>
              )}
              <svg
                className="pm-lines"
                width={world.width}
                height={world.height}
                aria-hidden="true"
              >
                <defs>
                  <marker
                    id="pm-arrow"
                    viewBox="0 0 8 8"
                    refX="7"
                    refY="4"
                    markerWidth="6"
                    markerHeight="6"
                    orient="auto-start-reverse"
                  >
                    <path d="M1 1L7 4L1 7" fill="none" stroke="currentColor" />
                  </marker>
                </defs>
                {graph.edges.map((edge, i) => (
                  <g key={`${edge.from}-${edge.to}-${i}`}>
                    <title>
                      {project.streams.find((s) => s.id === edge.from)?.name} →{' '}
                      {project.streams.find((s) => s.id === edge.to)?.name}: {edge.reason}
                    </title>
                    <path
                      className="pm-dependency"
                      data-highlighted={selectedStream === edge.to || selectedStream === edge.from}
                      markerEnd="url(#pm-arrow)"
                      d={edgePath(edge.points)}
                    />
                  </g>
                ))}
              </svg>
              {project.streams.map((stream) => {
                const box = graph.streams[stream.id];
                const done = stream.tasks.filter((t) => t.status === 'done').length;
                const away = project.people.filter(
                  (p) => stream.agents.includes(p.agent.id) && p.destination,
                );
                return (
                  <div
                    className="pm-stream"
                    data-signal={combinedSignal(stream.tasks.map((t) => t.status))}
                    data-selected={selectedStream === stream.id}
                    key={stream.id}
                    style={{ left: box.x, top: box.y, width: box.width, height: box.height }}
                  >
                    <button
                      className="pm-stream-heading"
                      onClick={() => onStream(stream.id)}
                      aria-label={`Open ${stream.name}`}
                    >
                      <span>
                        <small>
                          {stream.stateLabel
                            ? project.people.find((p) => p.agent.id === stream.tasks[0]?.owner)
                                ?.agent.name || 'Unassigned'
                            : `${stream.tasks.length} assignments · ${stream.agents.length} agents`}
                        </small>
                        <strong>{stream.name}</strong>
                      </span>
                      <ArrowUpRight size={20} />
                    </button>
                    <p>{stream.summary}</p>
                    {away.map((p) => {
                      const pos = graph.people.find((a) => a.person.agent.id === p.agent.id)!.home;
                      return (
                        <button
                          className="pm-away"
                          key={p.agent.id}
                          style={{ left: pos.x - box.x - 63, top: pos.y - box.y - 18 }}
                          onClick={() => onAgent(p.agent.id)}
                        >
                          <ArrowUpRight size={16} />
                          <strong>{p.agent.name}</strong>
                          <span>At {project.places.find((s) => s.id === p.destination)?.name}</span>
                        </button>
                      );
                    })}
                    <div className="pm-stream-foot">
                      <WorkStatus
                        signal={combinedSignal(stream.tasks.map((t) => t.status))}
                        label={
                          stream.stateLabel ||
                          `${done} ready · ${stream.tasks.length - done} remaining`
                        }
                      />
                      <span>
                        {stream.tasks.some((t) => t.status === 'needs_you') ? (
                          <>
                            <Flag size={12} />
                            Needs you
                          </>
                        ) : (
                          'Inspect work →'
                        )}
                      </span>
                    </div>
                  </div>
                );
              })}
              {project.places.map((place, i) => {
                const box = graph.places[place.id];
                const Icon =
                  place.kind === 'code' ? GitBranch : place.kind === 'test' ? TestTube2 : FileText;
                const occupants = graph.people.filter((p) => p.person.destination === place.id);
                return (
                  <div
                    className="pm-destination"
                    key={place.id}
                    style={{ left: box.x, top: box.y, width: box.width, height: box.height }}
                  >
                    {i === 0 && <span className="pm-lane-label">Shared destinations</span>}
                    <button
                      className="pm-destination-title"
                      onClick={() => onPlace(place.id)}
                      aria-label={`Inspect ${place.name}`}
                    >
                      <Icon size={22} />
                      <strong>{place.name}</strong>
                      <ArrowUpRight size={15} />
                    </button>
                    <svg
                      className="pm-dock-links"
                      width={box.width}
                      height={box.height}
                      aria-hidden="true"
                    >
                      {occupants.map(({ person, point }) => (
                        <path
                          key={person.agent.id}
                          className="pm-working-link"
                          data-playing={playing && !reduced}
                          d={`M145 53 L${point.x - box.x} ${point.y - box.y - 32}`}
                        />
                      ))}
                    </svg>
                    {!occupants.length && (
                      <p className="pm-destination-empty">No current activity</p>
                    )}
                  </div>
                );
              })}
              {graph.people.map(({ person, point, destination }) => (
                <motion.div
                  key={person.agent.id}
                  className="pm-person-position"
                  initial={false}
                  animate={{ x: point.x, y: point.y }}
                  transition={
                    reduced
                      ? { duration: 0 }
                      : { type: 'spring', stiffness: 240, damping: 22, mass: 0.8 }
                  }
                >
                  <button
                    className="pm-person"
                    onClick={() => onAgent(person.agent.id)}
                    aria-label={`Inspect ${person.agent.name}: ${person.doing}`}
                    data-docked={!!destination}
                  >
                    <span className="pm-portrait">
                      <Portrait agent={person.agent} size={54} square={false} />
                      {person.tool && (
                        <span className="pm-tool" title={person.tool}>
                          {person.tool === 'Tests' ? (
                            <TestTube2 size={12} />
                          ) : person.tool === 'Read' ? (
                            <FileText size={12} />
                          ) : (
                            <GitBranch size={12} />
                          )}
                        </span>
                      )}
                    </span>
                    <strong>{person.agent.name}</strong>
                    <small>{person.doing}</small>
                  </button>
                </motion.div>
              ))}
            </>
          ) : (
            portfolio.groups.map((group) => (
              <section
                key={group.id}
                className="pm-area"
                data-tone={group.area?.tone || 'ink'}
                aria-label={group.area?.name || 'Other work'}
                style={{ left: group.x, top: group.y, width: group.width, height: group.height }}
              >
                <button
                  className="pm-area-heading"
                  onClick={() =>
                    camera.focus(
                      { x: group.x + group.width / 2, y: group.y + group.height / 2 },
                      { width: group.width + 70, height: group.height + 70 },
                    )
                  }
                >
                  <span>
                    <small>
                      Area of work · {new Set(group.items.map((p) => p.team)).size} teams
                    </small>
                    <strong>{group.area?.name || 'Other work'}</strong>
                  </span>
                  <Maximize2 size={18} />
                </button>
                {group.items.map((item, index) => {
                  const count = projectCounts(item);
                  return (
                    <button
                      className="pm-project"
                      data-signal={combinedSignal(
                        item.streams.flatMap((s) => s.tasks.map((t) => t.status)),
                      )}
                      key={item.id}
                      style={{
                        left: 25 + (index % group.columns) * 495,
                        top: 95 + Math.floor(index / group.columns) * 240,
                      }}
                      onClick={() => onProject(item.id)}
                    >
                      <span className="pm-project-team">
                        {item.team}
                        <ArrowUpRight size={19} />
                      </span>
                      <strong>{item.title}</strong>
                      <p>{item.aim}</p>
                      <span className="pm-project-people">
                        {item.people.slice(0, 5).map((p) => (
                          <Portrait key={p.agent.id} agent={p.agent} size={30} square={false} />
                        ))}
                        <span>{item.people.length} agents</span>
                      </span>
                      <span className="pm-project-state">
                        <span>
                          {count.done}/{count.total} contributions done
                        </span>
                        <WorkStatus
                          signal={combinedSignal(
                            item.streams.flatMap((s) => s.tasks.map((t) => t.status)),
                          )}
                        />
                      </span>
                    </button>
                  );
                })}
              </section>
            ))
          )}
        </div>
      </div>
      <div className="pm-zoom" role="group" aria-label="Map controls">
        <button aria-label="Zoom out" onClick={() => camera.zoomBy(1 / 1.2)}>
          <Minus size={17} />
        </button>
        <button aria-label="Fit project map" onClick={camera.fit}>
          <Maximize2 size={17} />
        </button>
        <button aria-label="Zoom in" onClick={() => camera.zoomBy(1.2)}>
          <Plus size={17} />
        </button>
      </div>
    </div>
  );
}
