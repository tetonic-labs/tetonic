import { useEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
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
  Pause,
  Play,
} from 'lucide-react';
import { Portrait } from '../ui/Portrait';
import { useMapCamera } from '../graph/useMapCamera';
import { layoutProject, layoutPortfolio, edgePath } from '../../lib/projectLayout';
import { type ProjectView } from '../../lib/projectView';
import { combinedSignal } from '../../lib/workSignals';
import { WorkStatus } from '../team-work/WorkStatus';
import { MapActivity } from './MapActivity';
import { ProjectMapPortfolio } from './ProjectMapPortfolio';
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
  const [paused, setPaused] = useState(false);
  const [visible, setVisible] = useState(() => !document.hidden);
  useEffect(() => {
    const update = () => setVisible(!document.hidden);
    document.addEventListener('visibilitychange', update);
    return () => document.removeEventListener('visibilitychange', update);
  }, []);
  const animate = playing && !paused && !reduced && visible;
  const inspected = useRef<string | undefined>(undefined);
  const narrowScope = useRef<string | undefined>(undefined);
  const graph = useMemo(() => (project ? layoutProject(project) : undefined), [project]);
  const portfolio = useMemo(() => layoutPortfolio(projects), [projects]);
  const world = graph || portfolio;
  const camera = useMapCamera(
    `${scope}:${project?.id || 'all'}`,
    reduced,
    { width: world.width, height: world.height },
    { x: 0, y: -15, width: world.width, height: world.height + 30 },
    { top: 35, bottom: 15 },
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
    const area = portfolio.groups[0];
    const box = graph
      ? Object.values(graph.streams)[0]
      : area
        ? { x: area.x + 25, y: area.y + 95, width: 470, height: 215 }
        : undefined;
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
      data-compact={camera.scale < 0.7}
      data-motion={animate}
      style={
        {
          '--pm-name-size': `${Math.min(42, Math.max(24, 16 / camera.scale))}px`,
          '--pm-meta-size': `${Math.min(26, Math.max(12, 11 / camera.scale))}px`,
          '--pm-overview-meta-size': `${Math.min(32, Math.max(12, 11 / camera.scale))}px`,
        } as CSSProperties
      }
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
              {graph.bands.map((band) => (
                <div
                  className="pm-work-band"
                  key={band.id}
                  style={{ left: band.x, top: band.y, width: band.width, height: band.height }}
                >
                  <strong>{band.title}</strong>
                  <span>{band.description}</span>
                </div>
              ))}
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
                    data-active={stream.tasks.some((t) => t.status === 'working')}
                    data-role={stream.role}
                    data-selected={selectedStream === stream.id}
                    key={stream.id}
                    style={{ left: box.x, top: box.y, width: box.width, height: box.height }}
                  >
                    <button
                      className="pm-stream-heading"
                      onClick={() => onStream(stream.id)}
                      aria-label={`Open ${stream.role === 'coordination' ? 'team coordination for ' : ''}${stream.name}`}
                      title={stream.name}
                    >
                      <span>
                        <small>
                          {stream.stateLabel
                            ? project.people.find((p) => p.agent.id === stream.tasks[0]?.owner)
                                ?.agent.name || 'Unassigned'
                            : `${stream.tasks.length} assignments · ${stream.agents.length} agents`}
                        </small>
                        <strong>
                          {stream.role === 'coordination' ? 'Team coordination' : stream.name}
                        </strong>
                      </span>
                      <ArrowUpRight size={20} />
                    </button>
                    <p>
                      {stream.role === 'coordination'
                        ? 'Keeps assignments moving and brings the results together.'
                        : stream.summary}
                    </p>
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
                          camera.scale < 0.7
                            ? undefined
                            : stream.stateLabel ||
                              `${done} ready · ${stream.tasks.length - done} remaining`
                        }
                      />
                      <span className="pm-stream-secondary">
                        {stream.tasks.some((t) => t.status === 'needs_you') ? (
                          <>
                            <Flag size={12} />
                            Needs you
                          </>
                        ) : stream.tasks.some((t) => t.status === 'working') ? (
                          <MapActivity
                            count={stream.tasks.filter((t) => t.status === 'working').length}
                          />
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
                          data-playing={
                            animate && ['thinking', 'executing'].includes(person.agent.status)
                          }
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
            <ProjectMapPortfolio
              groups={portfolio.groups}
              onProject={onProject}
              onArea={(group) =>
                camera.focus(
                  { x: group.x + group.width / 2, y: group.y + group.height / 2 },
                  { width: group.width + 70, height: group.height + 70 },
                )
              }
            />
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
        <button
          className="pm-motion-toggle"
          aria-label={paused ? 'Resume activity motion' : 'Pause activity motion'}
          aria-pressed={paused}
          disabled={reduced}
          title={
            reduced
              ? 'Reduced motion is enabled in your system settings'
              : paused
                ? 'Resume activity motion'
                : 'Pause activity motion'
          }
          onClick={() => setPaused(!paused)}
        >
          {paused || reduced ? <Play size={15} /> : <Pause size={15} />}
        </button>
      </div>
      {projects.length > 0 && (
        <div className="pm-reading-key">
          {project?.kind === 'plan'
            ? 'Contributions · dependency links · coordination'
            : project
              ? 'Requests and explorations · open any card to see the work'
              : 'Efforts grouped by team · open a card to see the work inside'}
        </div>
      )}
    </div>
  );
}
