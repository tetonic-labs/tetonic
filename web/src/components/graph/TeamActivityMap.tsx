import { useEffect, useMemo, useRef, useState } from 'react';
import type { CSSProperties } from 'react';
import {
  Play,
  Pause,
  RotateCcw,
  Plus,
  Minus,
  Maximize2,
  Terminal,
  Files,
  SkipForward,
  SkipBack,
  MessageCircle,
  X,
  SlidersHorizontal,
  ArrowUpRight,
} from 'lucide-react';
import { Agent, AgentTrack, ApprovalRequest, GraphEdge, GraphNode, Team } from '../../types';
import { teammate } from '../../lib/teammates';
import { destinationsFor } from '../../lib/mapActivity';
import { sampleTime, workStatus } from '../../lib/workEvidence';
import { MapLayout } from '../../lib/mapLayout';
import { detailLevel, markerPoint } from '../../lib/workScene';
import { Portrait } from '../ui/Portrait';
import type { OrganizationActivity } from './useOrganizationActivity';
import { useMapCamera } from './useMapCamera';
interface Props {
  agents: Agent[];
  teams?: Team[];
  nodes: GraphNode[];
  edges: GraphEdge[];
  tracks: Record<string, AgentTrack>;
  approvals: ApprovalRequest[];
  activity: OrganizationActivity;
  onWork: (id: string) => void;
  scope: string;
  onAgent: (id: string) => void;
  onDestination: (id: string) => void;
  onRequest: (id: string) => void;
  onAddAgent: () => void;
}
const emptyTeams: Team[] = [];
export function TeamActivityMap({
  agents,
  teams = emptyTeams,
  nodes,
  edges,
  approvals,
  activity,
  onWork,
  scope,
  onAgent,
  onDestination,
  onRequest,
  onAddAgent,
}: Props) {
  const [pinned, setPinned] = useState<string | null>(null),
    [options, setOptions] = useState(false),
    [reduced, setReduced] = useState(false),
    [lessMotion, setLessMotion] = useState(false);
  useEffect(() => {
    const media = window.matchMedia?.('(prefers-reduced-motion: reduce)');
    const update = () => setReduced(!!media?.matches);
    update();
    media?.addEventListener('change', update);
    return () => media?.removeEventListener('change', update);
  }, []);
  useEffect(() => {
    setOptions(false);
  }, [scope]);
  useEffect(() => {
    if (pinned && !activity.agents.some((a) => a.id === pinned)) setPinned(null);
  }, [activity.agents, pinned]);
  useEffect(() => {
    activity.setReading(options);
    return () => activity.setReading(false);
  }, [options, activity.setReading]);
  const allocator = useRef(new MapLayout());
  const layout = useMemo(
    () =>
      allocator.current.build(
        activity.agents,
        teams,
        destinationsFor(activity.agents, nodes, edges),
        edges,
      ),
    [activity.agents, teams, nodes, edges],
  );
  const visibleIds = new Set(agents.map((a) => a.id));
  const placeIds = new Set(destinationsFor(agents, nodes, edges).map((p) => p.id));
  const destinations = layout.places.filter((p) => placeIds.has(p.id));
  const { example, examples, setExampleId } = activity;
  const noMotion = reduced || lessMotion;
  const motion = activity;
  const visibleGroups = layout.groups.filter((g) => g.agentIds.some((id) => visibleIds.has(id)));
  // Small teams can show their tools alongside their people. Larger scopes open
  // on team landmarks; shared resources keep their organization-wide positions.
  const framingBounds = [
    ...visibleGroups,
    ...(agents.length <= 6
      ? destinations.map((place) => ({
          x: place.point.x - 90,
          y: place.point.y - 70,
          width: 180,
          height: 140,
        }))
      : []),
  ];
  const left = Math.min(...framingBounds.map((g) => g.x)),
    top = Math.min(...framingBounds.map((g) => g.y));
  const scopeBounds =
    scope !== 'all' && visibleGroups.length
      ? {
          x: left - 50,
          y: top - 60,
          width: Math.max(...framingBounds.map((g) => g.x + g.width)) - left + 100,
          height: Math.max(...framingBounds.map((g) => g.y + g.height)) - top + 120,
        }
      : undefined;
  const camera = useMapCamera(scope, noMotion, layout.world, scopeBounds);
  const detail = detailLevel(camera.scale);
  // Preserve screen-space weight for landmarks without enlarging the agent layout.
  const labelScale = Math.max(1, 0.7 / camera.scale);
  const badgeScale = Math.max(1, 0.5 / camera.scale);
  const selectedAgent = activity.agents.find((a) => a.id === pinned),
    selectedWork = pinned ? motion.states.get(pinned) : undefined;
  const workflowId =
    selectedWork?.interaction?.workflowId || selectedWork?.previous?.interaction.workflowId;
  const relevant = new Set<string>(pinned ? [pinned] : []);
  for (const work of motion.states.values())
    if (
      work.id === pinned ||
      (workflowId && work.interaction?.workflowId === workflowId) ||
      work.interaction?.targetId === pinned
    ) {
      relevant.add(work.id);
      if (work.interaction) relevant.add(work.interaction.targetId);
    }
  if (selectedWork?.previous && motion.elapsed - selectedWork.previous.at < 8)
    relevant.add(selectedWork.previous.interaction.targetId);
  for (const failure of selectedWork?.failures || []) relevant.add(failure.interaction.targetId);
  const active = [...motion.states.values()].filter((w) => w.interaction);
  const groupFor = (id: string) => layout.groups.find((g) => g.agentIds.includes(id));
  const summaries = visibleGroups.map((g) => ({
    ...g,
    count: g.agentIds.filter((id) => visibleIds.has(id)).length,
    shared: scope !== 'all' && g.id !== scope,
    working: active.filter((w) => visibleIds.has(w.id) && g.agentIds.includes(w.id)).length,
    attention: g.agentIds.filter(
      (id) =>
        visibleIds.has(id) &&
        (motion.states.get(id)?.waiting ||
          motion.states.get(id)?.failure ||
          approvals.some((r) => r.agentId === id && r.status === 'pending')),
    ).length,
  }));
  const bundles = new Map<
    string,
    {
      from: { x: number; y: number };
      to: { x: number; y: number };
      count: number;
      attention: number;
      label: string;
    }
  >();
  for (const work of active) {
    if (!visibleIds.has(work.id)) continue;
    const group = groupFor(work.id),
      targetGroup = groupFor(work.interaction!.targetId),
      target = layout.points.get(work.interaction!.targetId);
    if (!group || !target || group.id === targetGroup?.id) continue;
    const key = `${group.id}:${targetGroup?.id || work.interaction!.targetId}`,
      old = bundles.get(key);
    if (old) {
      old.count++;
      if (work.waiting) old.attention++;
    } else
      bundles.set(key, {
        from: { x: group.x + group.width / 2, y: group.y + group.height / 2 },
        to: targetGroup
          ? { x: targetGroup.x + targetGroup.width / 2, y: targetGroup.y + targetGroup.height / 2 }
          : target,
        count: 1,
        attention: work.waiting ? 1 : 0,
        label: `${group.name} to ${targetGroup?.name || work.interaction!.targetName}`,
      });
  }
  for (const work of motion.states.values()) {
    const group = groupFor(work.id);
    if (!group || !visibleIds.has(work.id)) continue;
    for (const failure of work.failures) {
      const targetGroup = groupFor(failure.interaction.targetId);
      const to = layout.points.get(failure.interaction.targetId);
      if (!to || targetGroup?.id === group.id) continue;
      const key = `${group.id}:${targetGroup?.id || failure.interaction.targetId}`;
      const existing = bundles.get(key);
      if (existing) existing.attention++;
      else
        bundles.set(key, {
          from: { x: group.x + group.width / 2, y: group.y + group.height / 2 },
          to,
          count: 0,
          attention: 1,
          label: `${group.name} to ${failure.interaction.targetName}`,
        });
    }
  }
  const routePath = (a: { x: number; y: number }, b: { x: number; y: number }) =>
    `M${a.x},${a.y} Q${(a.x + b.x) / 2},${Math.min(a.y, b.y) - 40} ${b.x},${b.y}`;
  const ended = !!example && motion.elapsed >= example.duration;
  const resetOverview = () => {
    setPinned(null);
    camera.fit();
  };
  function clearFocus() {
    const button = camera.viewport.current?.querySelector<HTMLButtonElement>(
      '.person-orb[aria-pressed="true"]',
    );
    setPinned(null);
    button?.focus();
  }
  function step(direction: number) {
    if (!example) return;
    const stops = [
      ...new Set(
        example.events.map((e) =>
          Math.min(example.duration, e.at + (e.type === 'start' ? 1.25 : 0)),
        ),
      ),
      example.duration,
    ].sort((a, b) => a - b);
    const at =
      direction > 0
        ? stops.find((t) => t > motion.elapsed + 0.05)
        : [...stops].reverse().find((t) => t < motion.elapsed - 0.05);
    motion.seek(at ?? (direction > 0 ? example.duration : 0));
  }
  const history = activity.records
    .filter((r) =>
      workflowId ? r.interaction.workflowId === workflowId : r.interaction.agentId === pinned,
    )
    .slice(-3);
  return (
    <div
      className={'universe quiet-map' + (selectedAgent ? ' has-work-focus' : '')}
      ref={camera.viewport}
      data-reduced-motion={noMotion}
      style={
        {
          '--map-detail': detail,
          '--map-summary-detail': Math.min(1, Math.max(0, (camera.scale - 0.075) / 0.018)),
          '--map-pixel': `${1 / camera.scale}px`,
          '--map-badge-scale': badgeScale,
          '--map-status-scale': Math.max(1, 0.28 / camera.scale),
          '--map-resource-scale': Math.max(1, 0.23 / camera.scale),
          '--map-group-fill': `${7 + (1 - detail) * 7}%`,
          '--map-route-opacity': 0.28 + (1 - detail) * 0.25,
        } as CSSProperties
      }
    >
      <div
        className="universe-canvas"
        role="region"
        aria-label="Agent activity map. Drag to explore, scroll to zoom. Arrow keys pan; plus and minus zoom; Home fits the map."
        tabIndex={0}
        onKeyDown={(e) => {
          if (e.key === 'Escape') {
            setPinned(null);
            e.stopPropagation();
          } else camera.onKeyDown(e);
        }}
        onPointerDown={camera.onPointerDown}
        onPointerMove={camera.onPointerMove}
        onPointerUp={camera.onPointerUp}
        onPointerCancel={camera.onPointerCancel}
        style={{
          backgroundSize: `${Math.max(12, 26 * camera.scale)}px ${Math.max(12, 26 * camera.scale)}px`,
          backgroundPosition: `${camera.offset.x}px ${camera.offset.y}px`,
        }}
      >
        <div
          className="universe-world"
          style={{
            width: layout.world.width,
            height: layout.world.height,
            transform: `translate(${camera.offset.x}px,${camera.offset.y}px) scale(${camera.scale})`,
          }}
        >
          {summaries.map((g, i) => (
            <div
              key={g.id}
              className="team-neighborhood"
              style={{
                left: g.x,
                top: g.y,
                width: g.width,
                height: g.height,
                borderRadius: i % 2 ? '44% 38% 42% 32%' : '32% 45% 36% 42%',
              }}
            >
              <button
                className="neighborhood-label"
                onClick={() =>
                  camera.focus(
                    { x: g.x + g.width / 2, y: g.y + g.height / 2 },
                    { width: g.width, height: g.height },
                  )
                }
                aria-label={`Explore ${g.name}`}
                aria-description={`${g.count} ${g.shared ? 'shared contributors' : 'home agents'}, ${g.attention} need review`}
                title={`${g.name}: ${g.count} agents, ${g.working} working${g.attention ? `, ${g.attention} need attention` : ''}`}
                style={{
                  transform: `scale(${labelScale})`,
                  maxWidth: (g.width + 350) / labelScale,
                }}
              >
                <strong>
                  {g.name}
                  {g.shared ? ' · shared' : ''}
                </strong>
                {g.attention > 0 && (
                  <span className="team-attention">! {g.attention} to review</span>
                )}
                <span className="team-routine">
                  {g.count} {g.shared ? 'shared' : 'at home'} · {g.working} active
                </span>
              </button>
            </div>
          ))}
          <svg
            className="universe-routes"
            width={layout.world.width}
            height={layout.world.height}
            aria-hidden="true"
          >
            {!pinned &&
              [...bundles.entries()]
                .sort(
                  (a, b) =>
                    b[1].attention - a[1].attention ||
                    b[1].count - a[1].count ||
                    a[0].localeCompare(b[0]),
                )
                .slice(0, 6)
                .map(([key, b]) => (
                  <g key={key} className={'work-bundle' + (b.attention ? ' bundle-attention' : '')}>
                    <path
                      d={routePath(b.from, b.to)}
                      strokeWidth={1.2 + Math.min(1.3, b.count * 0.2)}
                    />
                    <text
                      transform={`translate(${(b.from.x + b.to.x) / 2},${(b.from.y + b.to.y) / 2 - 24}) scale(${labelScale})`}
                    >
                      {b.attention ? `${b.attention} to review` : `${b.count} active`}
                    </text>
                    <title>
                      {b.label}: {b.count} active interactions; {b.attention} to review. Overview
                      routes are summarized.
                    </title>
                  </g>
                ))}
            {pinned &&
              active
                .filter((w) => relevant.has(w.id))
                .map((w) => {
                  const from = layout.points.get(w.id),
                    to = layout.points.get(w.interaction!.targetId);
                  return from && to ? (
                    <path key={w.id} className="focused-work-route" d={routePath(from, to)} />
                  ) : null;
                })}
            {selectedWork?.previous &&
              motion.elapsed - selectedWork.previous.at < 8 &&
              (() => {
                const a = layout.points.get(pinned!),
                  b = layout.points.get(selectedWork.previous.interaction.targetId);
                return a && b ? <path className="previous-work-route" d={routePath(a, b)} /> : null;
              })()}
            {[
              ...new Set(selectedWork?.failures.map((failure) => failure.interaction.targetId)),
            ].map((id) => {
              const from = layout.points.get(pinned!),
                to = layout.points.get(id);
              return from && to ? (
                <path
                  key={`unresolved-${id}`}
                  className="focused-work-route unresolved-work-route"
                  d={routePath(from, to)}
                />
              ) : null;
            })}
          </svg>
          {pinned &&
            !noMotion &&
            !selectedWork?.interaction &&
            selectedWork?.previous &&
            motion.elapsed - selectedWork.previous.at < 0.65 &&
            (() => {
              const home = layout.points.get(pinned),
                from = layout.points.get(selectedWork.previous.interaction.targetId);
              if (!home || !from) return null;
              const point = markerPoint(
                from,
                home,
                (motion.elapsed - selectedWork.previous.at) / 0.65,
              );
              return (
                <span
                  className="work-marker returning-work"
                  aria-hidden="true"
                  style={{ left: point.x, top: point.y }}
                >
                  <span />
                </span>
              );
            })()}
          {destinations.map((place) => {
            const count = active.filter((w) => w.interaction!.targetId === place.id).length;
            const Icon = place.kind === 'tool' ? Terminal : Files;
            return (
              <div
                key={place.id}
                className={
                  'map-place ' +
                  (count ? 'engaged ' : '') +
                  (pinned && !relevant.has(place.id) ? 'map-dimmed' : '')
                }
                data-entity={place.id}
                style={{
                  transform: `translate(${place.point.x}px,${place.point.y}px) translate(-50%,-50%)`,
                }}
              >
                <button
                  onClick={() => onDestination(place.id)}
                  aria-label={`Inspect ${place.name}`}
                >
                  <span className="place-symbol">
                    <Icon size={30} strokeWidth={1.5} />
                  </span>
                  <strong>{place.name}</strong>
                </button>
                {count > 0 && (
                  <span className="resource-work-count" title={`${count} active interactions`}>
                    {count}
                  </span>
                )}
              </div>
            );
          })}
          {agents.map((agent) => {
            const person = teammate(agent),
              point = layout.points.get(agent.id)!,
              work = motion.states.get(agent.id),
              action = work?.interaction,
              pending = approvals.filter((r) => r.agentId === agent.id && r.status === 'pending'),
              alert = !!pending.length || !!work?.failure || !!work?.waiting;
            const phase = action
                ? work?.waiting
                  ? 'waiting'
                  : motion.elapsed - work!.started < 0.8
                    ? 'approaching'
                    : 'engaged'
                : 'resting',
              label = workStatus(work);
            return (
              <div
                key={agent.id}
                className={
                  'map-person ' +
                  (action ? 'in-action ' : '') +
                  (pinned === agent.id ? 'is-selected ' : '') +
                  (alert ? 'needs-attention ' : '') +
                  (pinned && !relevant.has(agent.id) && !alert ? 'map-dimmed' : '')
                }
                data-entity={agent.id}
                data-phase={phase}
                data-host={action?.targetId || ''}
                style={{ transform: `translate(${point.x}px,${point.y}px) translate(-50%,-50%)` }}
              >
                <button
                  className="person-orb"
                  tabIndex={detail === 0 ? -1 : 0}
                  onClick={() => setPinned(agent.id)}
                  aria-label={`Focus ${person.name}`}
                  aria-pressed={pinned === agent.id}
                  aria-describedby={`motion-${agent.id}`}
                >
                  <Portrait agent={agent} size={82} />
                  <span className="orb-selection" />
                </button>
                <span className="sr-only" id={`motion-${agent.id}`}>
                  {label}
                  {action ? ' in sample playback.' : '.'}
                </span>
                <div className="person-label">
                  <strong title={person.name}>{person.name}</strong>
                  <span className="agent-detail">{label}</span>
                  {(layout.memberships.get(agent.id)?.length || 0) > 1 && (
                    <span
                      className="agent-detail"
                      title={layout.memberships
                        .get(agent.id)!
                        .map((id) => teams.find((t) => t.id === id)?.name)
                        .join(', ')}
                    >
                      Shared across {layout.memberships.get(agent.id)!.length} teams
                    </span>
                  )}
                </div>
                {action && (
                  <span className="agent-work-status" aria-hidden="true">
                    {action.tool === 'Message' ? <MessageCircle size={12} /> : <span />}
                  </span>
                )}
                {pending.length ? (
                  <button
                    className="person-question"
                    aria-label={`Review request from ${person.name}`}
                    onClick={() => onRequest(pending[0].id)}
                  >
                    ?
                  </button>
                ) : alert ? (
                  <button
                    className="person-question"
                    aria-label={`${person.name}: ${work?.waiting ? 'waiting' : 'failed work'}`}
                    onClick={() => setPinned(agent.id)}
                  >
                    {work?.waiting ? 'II' : '!'}
                  </button>
                ) : null}
              </div>
            );
          })}
          {active
            .filter((w) => (pinned ? relevant.has(w.id) : agents.length <= 6))
            .sort(
              (a, b) =>
                Number(b.id === pinned) - Number(a.id === pinned) || a.id.localeCompare(b.id),
            )
            .slice(0, pinned ? 6 : 3)
            .map((work) => {
              const action = work.interaction!,
                from = layout.points.get(work.id),
                to = layout.points.get(action.targetId);
              if (!from || !to) return null;
              const peers = active
                  .filter((w) => w.interaction!.targetId === action.targetId)
                  .sort((a, b) => a.id.localeCompare(b.id)),
                slot = peers.findIndex((w) => w.id === work.id),
                dock = { x: to.x + 75 + (slot % 3) * 23, y: to.y - 45 - Math.floor(slot / 3) * 23 },
                point = markerPoint(
                  from,
                  dock,
                  noMotion ? 1 : (motion.elapsed - work.started) / 0.8,
                );
              return (
                <button
                  key={action.id}
                  className={'work-marker ' + (work.waiting ? 'waiting' : '')}
                  style={{
                    left: point.x,
                    top: point.y,
                    opacity: pinned ? 1 : detail,
                    pointerEvents: !pinned && detail < 0.1 ? 'none' : 'auto',
                  }}
                  tabIndex={!pinned && detail < 0.1 ? -1 : 0}
                  aria-hidden={!pinned && detail === 0}
                  onClick={() => setPinned(work.id)}
                  aria-label={`Follow work by ${teammate(activity.agents.find((a) => a.id === work.id)!).name}: ${action.label}`}
                  title={action.label}
                >
                  {action.tool === 'Message' ? <MessageCircle size={11} /> : <span />}
                </button>
              );
            })}
        </div>
      </div>
      {selectedAgent && (
        <aside className="work-focus" aria-label={`Following ${teammate(selectedAgent).name}`}>
          <button className="work-focus-close" onClick={clearFocus} aria-label="Stop following">
            <X size={15} />
          </button>
          <div className="work-focus-heading">
            <Portrait agent={selectedAgent} size={32} />
            <div>
              <strong>{teammate(selectedAgent).name}</strong>
              <small>Sample · {sampleTime(motion.elapsed)}</small>
            </div>
          </div>
          <p>{workStatus(selectedWork)}</p>
          {selectedWork?.interaction && (
            <span className="work-focus-state">
              {selectedWork.waiting ? 'Waiting at' : 'Working with'}{' '}
              {selectedWork.interaction.targetName}
            </span>
          )}
          {selectedWork?.failure && (
            <p className="work-focus-alert">
              {selectedWork.failure.retryId ? 'Recovery in progress' : 'Unresolved failure'}:{' '}
              {selectedWork.failure.interaction.targetName}
            </p>
          )}
          {history.length > 0 && (
            <ol aria-label="Recent workflow steps">
              {history.map((record) => (
                <li key={record.id}>
                  {record.state} · {sampleTime(record.at)}
                  <span>
                    {record.interaction.label} / {record.interaction.targetName}
                  </span>
                </li>
              ))}
            </ol>
          )}
          <button className="text-action" onClick={() => onWork(selectedAgent.id)}>
            Inspect work <ArrowUpRight size={13} />
          </button>
          <button className="text-action" onClick={() => onAgent(selectedAgent.id)}>
            Open profile <ArrowUpRight size={13} />
          </button>
        </aside>
      )}
      {!agents.length && (
        <div className="map-empty-center">
          <p>Your team starts here.</p>
          <button className="canvas-primary" onClick={onAddAgent}>
            Add an agent <Plus size={17} />
          </button>
        </div>
      )}
      <div className="map-playback" aria-label="Sample activity playback">
        {example ? (
          <>
            <button
              className="play-control"
              aria-label={
                motion.playing
                  ? 'Pause sample playback'
                  : ended
                    ? 'Replay sample activity'
                    : 'Play sample activity'
              }
              onClick={motion.play}
            >
              {motion.playing ? (
                <Pause size={17} />
              ) : ended ? (
                <RotateCcw size={17} />
              ) : (
                <Play size={17} />
              )}
            </button>
            <div className="playback-caption">
              <span>
                {example.id === 'trace' ? 'Sample trace' : 'Motion example'} <i /> Not live
              </span>
              <strong title={example.name}>
                {motion.started ? (ended ? 'Playback ended' : example.name) : 'See how work moves'}
              </strong>
            </div>
            <button
              className="playback-step"
              onClick={() => step(-1)}
              aria-label="Previous interaction state"
              disabled={!motion.started || motion.elapsed <= 0}
            >
              <SkipBack size={16} />
            </button>
            <button
              className="playback-step"
              onClick={() => step(1)}
              aria-label="Next interaction state"
              disabled={ended}
            >
              <SkipForward size={16} />
            </button>
            <button
              className="playback-step"
              aria-label="Motion examples and accessibility"
              aria-expanded={options}
              onClick={() => setOptions(!options)}
            >
              <SlidersHorizontal size={16} />
            </button>
            <span className="sample-clock" aria-label="Sample time">
              {sampleTime(motion.elapsed)}
            </span>
            <span
              className="playback-line"
              style={{ width: `${(motion.elapsed / example.duration) * 100}%` }}
            />
          </>
        ) : (
          <span className="playback-no-events">No recorded activity for this team.</span>
        )}
      </div>
      {options && (
        <div className="motion-options" role="group" aria-label="Motion playback options">
          <label>
            Explore the motion
            <select
              aria-label="Motion example"
              value={example?.id || ''}
              onChange={(e) => {
                setExampleId(e.target.value);
                setOptions(false);
              }}
            >
              {examples.map((e) => (
                <option key={e.id} value={e.id}>
                  {e.name}
                </option>
              ))}
            </select>
          </label>
          <p>{example?.provenance}</p>
          <label className="motion-preference">
            <input
              type="checkbox"
              checked={noMotion}
              disabled={reduced}
              onChange={(e) => setLessMotion(e.target.checked)}
            />
            Reduce motion
          </label>
          {reduced && <small>Using your system preference.</small>}
          <button className="text-action" onClick={() => setOptions(false)}>
            Done
          </button>
        </div>
      )}
      <div className="map-navigation" aria-label="Map controls">
        <button onClick={() => camera.zoomBy(1 / 1.2)} aria-label="Zoom out">
          <Minus size={18} />
        </button>
        <button onClick={resetOverview} aria-label="Fit map">
          <Maximize2 size={17} />
        </button>
        <button onClick={() => camera.zoomBy(1.2)} aria-label="Zoom in">
          <Plus size={18} />
        </button>
      </div>
    </div>
  );
}
