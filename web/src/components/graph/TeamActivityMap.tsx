import { useEffect, useMemo, useState } from 'react';
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
  Plug,
  SkipForward,
  SkipBack,
  MessageCircle,
  X,
  SlidersHorizontal,
  ArrowUpRight,
  ArrowLeft,
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
import { useLocalMapMotion } from './useLocalMapMotion';
import { localEntities, LOCAL_AGENT_LIMIT, mapSignal } from '../../lib/localMapMotion';
import { TeamInspection } from './TeamInspection';
import { TeamMapResources } from './TeamMapResources';
import { resourcesFromGraph, type WorkspaceResource } from '../../lib/toolLibrary';
interface Props {
  agents: Agent[];
  teams?: Team[];
  nodes: GraphNode[];
  edges: GraphEdge[];
  tracks: Record<string, AgentTrack>;
  approvals: ApprovalRequest[];
  activity: OrganizationActivity;
  onWork: (id: string) => void;
  onTeam?: (id: string, restore: () => void) => void;
  scope: string;
  onAgent: (id: string) => void;
  onDestination: (id: string) => void;
  onRequest: (id: string) => void;
  onAddAgent: () => void;
  onTools?: (teamId?: string, resourceId?: string) => void;
  resources?: WorkspaceResource[];
  onFocusedTeam?: (teamId: string | null) => void;
  onExploreTeam?: () => void;
  visible?: boolean;
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
  onTeam,
  scope,
  onAgent,
  onDestination,
  onRequest,
  onAddAgent,
  onTools,
  resources,
  onFocusedTeam,
  onExploreTeam,
  visible = true,
}: Props) {
  const [pinned, setPinned] = useState<string | null>(null),
    [options, setOptions] = useState(false),
    [reduced, setReduced] = useState(false),
    [lessMotion, setLessMotion] = useState(false);
  const [hoveredTeam, setHoveredTeam] = useState<string | null>(null);
  const [inquiry, setInquiry] = useState<string | null>(null);
  useEffect(() => {
    const media = window.matchMedia?.('(prefers-reduced-motion: reduce)');
    const update = () => setReduced(!!media?.matches);
    update();
    media?.addEventListener('change', update);
    return () => media?.removeEventListener('change', update);
  }, []);
  useEffect(() => {
    setOptions(false);
    setInquiry(null);
  }, [scope]);
  useEffect(() => {
    if (pinned && !activity.agents.some((a) => a.id === pinned)) setPinned(null);
  }, [activity.agents, pinned]);
  useEffect(() => {
    activity.setReading(options);
    return () => activity.setReading(false);
  }, [options, activity.setReading]);
  const [allocator] = useState(
    () =>
      new MapLayout(
        window.innerWidth /
          Math.max(240, window.innerHeight - (window.innerWidth < 700 ? 420 : 290)),
      ),
  );
  const layout = useMemo(
    () =>
      allocator.build(
        activity.agents,
        teams,
        destinationsFor(activity.agents, nodes, edges),
        edges,
      ),
    [allocator, activity.agents, teams, nodes, edges],
  );
  const contextualIds = new Set<string>();
  for (const agent of agents) {
    const work = activity.states.get(agent.id);
    if (work?.interaction) contextualIds.add(work.interaction.targetId);
    if (work?.previous && activity.elapsed - work.previous.at < 8)
      contextualIds.add(work.previous.interaction.targetId);
  }
  const shownAgents = activity.agents.filter(
    (a) => agents.some((member) => member.id === a.id) || contextualIds.has(a.id),
  );
  const visibleIds = new Set(shownAgents.map((a) => a.id));
  const placeIds = new Set(destinationsFor(agents, nodes, edges).map((p) => p.id));
  const destinations = layout.places.filter((p) => placeIds.has(p.id) || contextualIds.has(p.id));
  const { example, examples, setExampleId } = activity;
  useEffect(() => {
    setInquiry(null);
  }, [example?.id]);
  const noMotion = reduced || lessMotion;
  const motion = activity;
  const visibleGroups = layout.groups.filter((g) => g.agentIds.some((id) => visibleIds.has(id)));
  // Small teams can show their tools alongside their people. Larger scopes open
  // on team landmarks; shared resources keep their organization-wide positions.
  const framingBounds = [
    ...visibleGroups,
    ...(agents.length <= 6
      ? destinations
          .filter((place) => placeIds.has(place.id))
          .map((place) => ({
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
  const focusedTeam = camera.context || null;
  // The lens is driven by camera distance, so wheel, pinch, Back and Home all
  // reverse the same transition without changing layout homes or playback.
  const [focusRanges, setFocusRanges] = useState<Record<string, { from: number; to: number }>>({});
  const focusRange = focusRanges[focusedTeam || ''] || { from: 0, to: 1 };
  const focusProgress = focusedTeam
    ? Math.min(
        1,
        Math.max(
          0,
          (camera.scale - focusRange.from) / Math.max(0.01, focusRange.to - focusRange.from),
        ),
      )
    : 0;
  const teamBlend = focusProgress * focusProgress * (3 - 2 * focusProgress);
  const contextualTeam = teamBlend > 0.5 ? focusedTeam : null;
  useEffect(() => {
    onFocusedTeam?.(contextualTeam);
  }, [contextualTeam, onFocusedTeam]);
  const focusMembers = new Set([
    ...(teams.find((t) => t.id === focusedTeam)?.pledgedAgentIds || []),
    ...(layout.groups.find((g) => g.id === focusedTeam)?.agentIds || []),
  ]);
  const library = useMemo(
    () => resources || resourcesFromGraph(nodes, edges, teams),
    [resources, nodes, edges, teams],
  );
  const teamResources = library.filter((resource) => resource.teamIds.includes(focusedTeam || ''));
  const memberOpacity = (id: string) => (focusMembers.has(id) ? 1 : 1 - teamBlend * 0.96);
  const [mapLevel, setMapLevel] = useState<'overview' | 'teams' | 'local'>('overview');
  useEffect(() => {
    setMapLevel((previous) => {
      if (camera.scale < 0.24) return 'overview';
      if (camera.scale > 0.84) return 'local';
      if (previous === 'overview' && camera.scale < 0.3) return previous;
      if (previous === 'local' && camera.scale > 0.76) return previous;
      return 'teams';
    });
  }, [camera.scale]);
  const detail = detailLevel(camera.scale);
  // Move the heading above its landmark before the portraits emerge beneath it.
  const landmarkDetail = Math.min(1, Math.max(0, (camera.scale - 0.12) / 0.05));
  const portraitDetail = Math.min(1, Math.max(0, (camera.scale - 0.18) / 0.12));
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
  const center = {
    x: (camera.size.width / 2 - camera.offset.x) / camera.scale,
    y: (camera.size.height / 2 - camera.offset.y) / camera.scale,
  };
  const nearest = [...visibleGroups].sort(
    (a, b) =>
      Math.hypot(a.x + a.width / 2 - center.x, a.y + a.height / 2 - center.y) -
      Math.hypot(b.x + b.width / 2 - center.x, b.y + b.height / 2 - center.y),
  )[0];
  const focusedGroup = visibleGroups.find((g) => g.id === focusedTeam);
  const focusedInView =
    focusedGroup &&
    Math.abs(focusedGroup.x + focusedGroup.width / 2 - center.x) * camera.scale <
      camera.size.width / 2 &&
    Math.abs(focusedGroup.y + focusedGroup.height / 2 - center.y) * camera.scale <
      camera.size.height / 2;
  const localGroup = groupFor(pinned || '') || (focusedInView ? focusedGroup : nearest);
  const actorIds = useMemo(() => {
    const members = localGroup?.agentIds.filter((id) => visibleIds.has(id)) || [];
    return [...new Set([...(pinned ? [pinned] : []), ...members])].slice(0, LOCAL_AGENT_LIMIT);
  }, [localGroup?.id, localGroup?.agentIds.join('|'), pinned, scope]);
  const allAgentIds = useMemo(() => new Set(activity.agents.map((a) => a.id)), [activity.agents]);
  const entityNames = useMemo(
    () =>
      new Map([
        ...activity.agents.map((a) => [a.id, teammate(a).name] as const),
        ...layout.places.map((p) => [p.id, p.name] as const),
      ]),
    [activity.agents, layout],
  );
  const physicalEntities = useMemo(
    () => localEntities(layout.points, actorIds, motion.states, allAgentIds, entityNames),
    [layout, actorIds, motion.states, allAgentIds, entityNames],
  );
  const localFrames = useLocalMapMotion(physicalEntities, activity, noMotion, visible);
  const frameById = new Map(localFrames.map((f) => [f.id, f]));
  const dockingIds = new Set(localFrames.filter((f) => f.interaction).map((f) => f.id));
  const blendProgress = Math.min(1, Math.max(0, (camera.scale - 0.5) / 0.34));
  const localBlend = Math.max(teamBlend, blendProgress * blendProgress * (3 - 2 * blendProgress));
  const pointFor = (id: string, blend = localBlend) => {
    const home = layout.points.get(id),
      live = frameById.get(id)?.position;
    if (!home || !live) return home;
    const work = motion.states.get(id);
    const target =
      frameById.get(id)?.interaction?.targetId ||
      work?.interaction?.targetId ||
      work?.previous?.interaction.targetId;
    const targetHome = target ? layout.points.get(target) : undefined;
    const localTarget =
      targetHome &&
      focusedGroup &&
      focusMembers.has(target!) &&
      targetHome.x >= focusedGroup.x &&
      targetHome.x <= focusedGroup.x + focusedGroup.width &&
      targetHome.y >= focusedGroup.y &&
      targetHome.y <= focusedGroup.y + focusedGroup.height;
    const distance = Math.hypot(live.x - home.x, live.y - home.y);
    // Remote work stays legible beside the team resource shelf. Project its
    // travel into the agent's home cell; the underlying solver keeps running.
    const remoteScale =
      teamBlend && focusMembers.has(id) && !localTarget && distance > 20
        ? 1 - teamBlend + (teamBlend * 20) / distance
        : 1;
    return {
      x: home.x + (live.x - home.x) * blend * remoteScale,
      y: home.y + (live.y - home.y) * blend * remoteScale,
    };
  };
  const explore = (id: string) => {
    const group = visibleGroups.find((g) => g.id === id);
    if (!group) return;
    if (scope !== 'all' && onExploreTeam) {
      camera.preserveNextScopeChange();
      onExploreTeam();
    }
    setPinned(null);
    setInquiry(null);
    const bounds = { width: group.width + 120, height: group.height + 200 };
    const space = camera.size.width < 700 ? { bottom: 142 } : { right: 290 };
    const closeScale = camera.focusScale(bounds, space);
    setFocusRanges((ranges) => ({
      ...ranges,
      [id]: {
        from: Math.min(camera.organizationScale * 1.22, closeScale * 0.55),
        to: closeScale * 0.94,
      },
    }));
    camera.focus(
      { x: group.x + group.width / 2, y: group.y + group.height / 2 },
      bounds,
      space,
      id,
    );
  };
  const followActivity = (id: string) => {
    const bounds = { width: camera.size.width < 700 ? 340 : 580, height: 470 };
    const progress = Math.min(1, Math.max(0, (camera.focusScale(bounds) - 0.5) / 0.34));
    // Center the representation at the destination zoom, including an existing dock.
    const point = pointFor(id, progress * progress * (3 - 2 * progress));
    if (point) camera.focus(point, bounds, undefined, focusedTeam || undefined);
  };
  const zoom = (factor: number) => {
    const point = pinned ? pointFor(pinned) : undefined;
    const screen = point && {
      x: point.x * camera.scale + camera.offset.x,
      y: point.y * camera.scale + camera.offset.y,
    };
    camera.zoomBy(
      factor,
      screen &&
        screen.x > 0 &&
        screen.x < camera.size.width &&
        screen.y > 0 &&
        screen.y < camera.size.height
        ? screen
        : undefined,
    );
  };
  const summaries = visibleGroups.map((g) => ({
    ...g,
    count: g.agentIds.filter((id) => visibleIds.has(id)).length,
    shared: scope !== 'all' && g.id !== scope,
    working: active.filter((w) => visibleIds.has(w.id) && g.agentIds.includes(w.id)).length,
    requests: g.agentIds.filter(
      (id) =>
        visibleIds.has(id) && approvals.some((r) => r.agentId === id && r.status === 'pending'),
    ).length,
    failures: g.agentIds.filter((id) => visibleIds.has(id) && !!motion.states.get(id)?.failure)
      .length,
    waiting: g.agentIds.filter((id) => visibleIds.has(id) && !!motion.states.get(id)?.waiting)
      .length,
    attention: g.agentIds.filter(
      (id) =>
        visibleIds.has(id) &&
        (motion.states.get(id)?.failure ||
          approvals.some((r) => r.agentId === id && r.status === 'pending')),
    ).length,
  }));
  const bundles = new Map<
    string,
    {
      from: { x: number; y: number };
      to: { x: number; y: number };
      count: number;
      moving: number;
      attention: number;
      groups: string[];
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
      if (!work.waiting) old.moving++;
    } else
      bundles.set(key, {
        from: { x: group.x + group.width / 2, y: group.y + group.height / 2 },
        to: targetGroup
          ? { x: targetGroup.x + targetGroup.width / 2, y: targetGroup.y + targetGroup.height / 2 }
          : target,
        count: 1,
        moving: work.waiting ? 0 : 1,
        attention: 0,
        groups: [group.id, targetGroup?.id || ''],
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
          moving: 0,
          attention: 1,
          groups: [group.id, targetGroup?.id || ''],
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
      className={
        'universe quiet-map living-map' + (selectedAgent || inquiry ? ' has-work-focus' : '')
      }
      ref={camera.viewport}
      data-reduced-motion={noMotion}
      data-map-level={teamBlend > 0.98 ? 'local' : mapLevel}
      data-team-focus={teamBlend > 0.02 ? focusedTeam : undefined}
      data-team-blend={teamBlend.toFixed(3)}
      style={
        {
          '--map-detail': detail,
          '--map-portrait-detail': portraitDetail,
          '--map-name-opacity': Math.min(1, Math.max(0, (camera.scale - 0.2) / 0.12)),
          '--map-work-detail': Math.min(1, Math.max(0, (camera.scale - 0.65) / 0.2)),
          '--map-local-detail': localBlend,
          '--map-name-scale': Math.max(1, 0.65 / camera.scale),
          '--map-summary-detail': Math.min(1, Math.max(0, (camera.scale - 0.075) / 0.018)),
          '--map-pixel': `${1 / camera.scale}px`,
          '--map-badge-scale': badgeScale,
          '--map-status-scale': Math.max(1, 0.28 / camera.scale),
          '--map-resource-scale': Math.max(1, 0.23 / camera.scale),
          '--map-group-fill': `${7 + (1 - detail) * 7}%`,
          '--map-route-opacity': 0.28 + (1 - detail) * 0.25,
          '--team-focus-blend': teamBlend,
        } as CSSProperties
      }
    >
      <div
        className="universe-canvas"
        role="region"
        aria-label="Agent activity map. Drag to explore, scroll to zoom. Arrow keys pan; plus and minus zoom; Home fits the map; Alt and left arrow returns to the previous view."
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
        onClickCapture={camera.onClickCapture}
        onLostPointerCapture={camera.onPointerCancel}
        onDragStart={(event) => event.preventDefault()}
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
          {summaries.map((g) => (
            <div
              key={g.id}
              className="team-neighborhood"
              data-team={g.id}
              onClick={(event) => {
                if (!(event.target as Element).closest('button')) explore(g.id);
              }}
              inert={focusedTeam !== g.id && teamBlend > 0.95}
              data-highlighted={hoveredTeam === g.id || focusedTeam === g.id || inquiry === g.id}
              onPointerEnter={() => setHoveredTeam(g.id)}
              onPointerLeave={() => setHoveredTeam(null)}
              onFocusCapture={() => setHoveredTeam(g.id)}
              onBlurCapture={(e) => {
                if (!e.currentTarget.contains(e.relatedTarget as Node)) setHoveredTeam(null);
              }}
              style={{
                left: g.x,
                top: g.y,
                width: g.width,
                height: g.height,
                borderRadius: '5% 5% 18% 5%',
                opacity: focusedTeam === g.id ? 1 : 1 - teamBlend * 0.96,
              }}
            >
              <button
                className="team-zoom-target"
                onClick={() => explore(g.id)}
                aria-label={`Explore ${g.name}`}
              />
              <div
                className="neighborhood-heading"
                style={{
                  transform: `scale(${labelScale})`,
                  bottom: `calc(${landmarkDetail * 100}% + 5 * var(--map-pixel))`,
                  maxWidth: (g.width + 320) / labelScale,
                }}
              >
                <button
                  className="neighborhood-label"
                  onClick={() => explore(g.id)}
                  aria-label={`Zoom into ${g.name}`}
                  aria-description={`${g.count} ${g.shared ? 'shared contributors' : 'home agents'}, ${g.attention} need review`}
                  title={`${g.name}: ${g.count} agents, ${g.working} working${g.attention ? `, ${g.attention} need attention` : ''}`}
                >
                  <strong>
                    {g.name}
                    {g.shared ? ' · shared' : ''}
                    <ArrowUpRight size={13} className="team-conversation-hint" />
                  </strong>
                  {g.requests > 0 && (
                    <span className="team-attention" data-signal="input">
                      ◆ {g.requests} needs you
                    </span>
                  )}
                  {g.failures > 0 && (
                    <span className="team-attention" data-signal="blocked">
                      ■ {g.failures} blocked
                    </span>
                  )}
                  <span className="team-routine">
                    {landmarkDetail < 0.5
                      ? g.working
                        ? `${g.working - g.waiting} working${g.waiting ? ` · ${g.waiting} waiting` : ''}`
                        : `${g.count} agents`
                      : `${g.count} ${g.shared ? 'shared' : 'agents'} · ${g.working - g.waiting} working${g.waiting ? ` · ${g.waiting} waiting` : ''}`}
                  </span>
                </button>
                {teams.some((team) => team.id === g.id) && (
                  <button
                    className="team-open-chat"
                    onClick={() => onTeam?.(g.id, () => {})}
                    aria-label={`Open ${g.name} conversation`}
                  >
                    <MessageCircle size={12} /> Conversation
                  </button>
                )}
                {!(mapLevel === 'overview' && teamBlend < 0.98) && (
                  <button
                    className="team-ask"
                    onClick={() => setInquiry(g.id)}
                    aria-label={`Ask about ${g.name}`}
                  >
                    Ask about team <MessageCircle size={12} />
                  </button>
                )}
              </div>
              {mapLevel === 'overview' && teamBlend < 0.98 && (
                <button
                  className="team-ask-overview"
                  aria-label={`Ask about ${g.name}`}
                  title={`Ask about ${g.name}`}
                  style={{ transform: `scale(${badgeScale})` }}
                  onClick={() => setInquiry(g.id)}
                >
                  <MessageCircle size={16} />
                </button>
              )}
            </div>
          ))}
          <svg
            className="universe-routes"
            style={{ opacity: 1 - teamBlend * 0.88 }}
            width={layout.world.width}
            height={layout.world.height}
            aria-hidden="true"
          >
            {!pinned &&
              [...bundles.entries()]
                .sort(
                  (a, b) =>
                    Number(!!hoveredTeam && b[1].groups.includes(hoveredTeam)) -
                      Number(!!hoveredTeam && a[1].groups.includes(hoveredTeam)) ||
                    b[1].attention - a[1].attention ||
                    b[1].count - a[1].count ||
                    a[0].localeCompare(b[0]),
                )
                .slice(0, 6)
                .map(([key, b]) => (
                  <g
                    key={key}
                    className={'work-bundle' + (b.attention ? ' bundle-attention' : '')}
                    data-highlighted={!!hoveredTeam && b.groups.includes(hoveredTeam)}
                    data-muted={!!hoveredTeam && !b.groups.includes(hoveredTeam) && !b.attention}
                    data-playing={
                      visible && b.moving > 0 && !noMotion && motion.playing && !activity.reading
                    }
                  >
                    <path
                      d={routePath(b.from, b.to)}
                      strokeWidth={1.2 + Math.min(1.3, b.count * 0.2)}
                    />
                    <text
                      transform={`translate(${(b.from.x + b.to.x) / 2},${(b.from.y + b.to.y) / 2 - 24}) scale(${labelScale})`}
                    >
                      {b.attention
                        ? `${b.attention} blocked`
                        : b.moving
                          ? `${b.count} active`
                          : `${b.count} waiting`}
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
                  const from = pointFor(w.id),
                    to = pointFor(w.interaction!.targetId);
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
          <svg
            className="local-orbits"
            width={layout.world.width}
            height={layout.world.height}
            aria-hidden="true"
            style={{ opacity: localBlend }}
          >
            {localFrames
              .filter((f) => actorIds.includes(f.id) && f.host && f.radius && f.interaction)
              .map((f) => {
                const host = pointFor(f.interaction!.targetId);
                return host ? (
                  <circle
                    key={f.id}
                    cx={host.x}
                    cy={host.y}
                    r={f.radius}
                    style={{
                      opacity:
                        f.attachment *
                        0.3 *
                        (focusMembers.has(f.interaction!.targetId) ? 1 : 1 - teamBlend),
                    }}
                  />
                ) : null;
              })}
          </svg>
          {pinned &&
            !noMotion &&
            (!actorIds.includes(pinned) || localBlend < 0.1) &&
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
          {scope === 'all' &&
            layout.resourceAreas.map((area) => (
              <div
                key={area.id}
                className="map-resource-area"
                inert={teamBlend > 0.95}
                style={{
                  left: area.x,
                  top: area.y,
                  width: area.width,
                  height: area.height,
                  opacity: 1 - teamBlend,
                }}
              >
                <button
                  onClick={() => onTools?.()}
                  aria-label={`Manage ${area.name}`}
                  style={{ transform: `scale(${labelScale})` }}
                >
                  {area.name} <span>{area.count}</span>
                  <ArrowUpRight size={14} />
                </button>
              </div>
            ))}
          {destinations.map((place) => {
            const count = active.filter((w) => w.interaction!.targetId === place.id).length;
            const point = pointFor(place.id) || place.point;
            const Icon =
              place.kind === 'tool' ? Terminal : place.kind === 'connector' ? Plug : Files;
            return (
              <div
                key={place.id}
                className={
                  'map-place ' +
                  (count ? 'engaged ' : '') +
                  (pinned && !relevant.has(place.id) ? 'map-dimmed' : '')
                }
                data-entity={place.id}
                inert={teamBlend > 0.95}
                style={{
                  transform: `translate(${point.x}px,${point.y}px) translate(-50%,-50%)`,
                  opacity: (mapLevel === 'overview' ? 0.6 : 1) * (1 - teamBlend),
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
          {shownAgents.map((agent) => {
            const person = teammate(agent),
              point = pointFor(agent.id)!,
              work = motion.states.get(agent.id),
              action = work?.interaction,
              pending = approvals.filter((r) => r.agentId === agent.id && r.status === 'pending'),
              alert = !!pending.length || !!work?.failure;
            const signal = mapSignal(work, pending.length > 0);
            const phase =
                localBlend > 0.5 && frameById.has(agent.id)
                  ? frameById.get(agent.id)!.phase
                  : action
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
                data-home={`${layout.points.get(agent.id)!.x},${layout.points.get(agent.id)!.y}`}
                data-physical-position={
                  frameById.has(agent.id)
                    ? `${frameById.get(agent.id)!.position.x},${frameById.get(agent.id)!.position.y}`
                    : undefined
                }
                data-phase={phase}
                data-signal={signal}
                data-host={action?.targetId || ''}
                inert={!focusMembers.has(agent.id) && teamBlend > 0.95}
                style={{
                  transform: `translate(${point.x}px,${point.y}px) translate(-50%,-50%)`,
                  opacity:
                    memberOpacity(agent.id) *
                    (pinned === agent.id ? 1 : portraitDetail) *
                    (pinned && !relevant.has(agent.id) && !alert ? 0.24 : 1),
                }}
              >
                <button
                  className="person-orb"
                  tabIndex={detail === 0 ? -1 : 0}
                  onClick={() => setPinned(agent.id)}
                  aria-label={`Focus ${person.name}`}
                  aria-pressed={pinned === agent.id}
                  aria-describedby={`motion-${agent.id}`}
                >
                  <Portrait agent={agent} size={82} square />
                  <span className="orb-selection" />
                </button>
                <span className="sr-only" id={`motion-${agent.id}`}>
                  {label}
                  {action ? ' in sample playback.' : '.'}
                </span>
                <div className="person-label">
                  <strong title={person.name}>{person.name}</strong>
                  {(action || pinned === agent.id) && <span className="agent-detail">{label}</span>}
                  {(layout.memberships.get(agent.id)?.length || 0) > 1 &&
                    (!action || pinned === agent.id) && (
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
                    aria-label={`${person.name}: failed work`}
                    onClick={() => setPinned(agent.id)}
                  >
                    !
                  </button>
                ) : null}
              </div>
            );
          })}
          {active
            .filter((w) => !dockingIds.has(w.id) || localBlend < 0.95)
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
                    opacity:
                      memberOpacity(work.id) *
                      (1 - teamBlend) *
                      (pinned ? 1 : detail) *
                      (dockingIds.has(work.id) ? 1 - localBlend : 1),
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
      {focusedGroup && (
        <div
          className="team-map-resource-lens"
          style={{ opacity: teamBlend }}
          inert={teamBlend < 0.15}
          aria-hidden={teamBlend < 0.15}
        >
          <TeamMapResources
            name={focusedGroup.name}
            resources={teamResources}
            memberIds={focusMembers}
            activity={activity}
            onManage={() => onTools?.(focusedGroup.id)}
            onInspect={(resource) =>
              resource.source === 'draft'
                ? onTools?.(focusedGroup.id, resource.id)
                : onDestination(resource.id)
            }
          />
        </div>
      )}
      {inquiry && (
        <TeamInspection
          key={inquiry}
          group={layout.groups.find((g) => g.id === inquiry)}
          agents={activity.agents}
          approvals={approvals}
          activity={activity}
          onClose={() => setInquiry(null)}
          onFocus={setPinned}
        />
      )}
      {selectedAgent && !inquiry && (
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
          <p>{selectedWork?.interaction?.label || workStatus(selectedWork)}</p>
          <button className="text-action" onClick={() => followActivity(selectedAgent.id)}>
            Move closer <ArrowUpRight size={13} />
          </button>
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
        <button onClick={camera.back} disabled={!camera.canGoBack} aria-label="Previous map view">
          <ArrowLeft size={17} />
        </button>
        <button onClick={() => zoom(1 / 1.2)} aria-label="Zoom out">
          <Minus size={18} />
        </button>
        <button onClick={resetOverview} aria-label="Fit map">
          <Maximize2 size={17} />
        </button>
        <button onClick={() => zoom(1.2)} aria-label="Zoom in">
          <Plus size={18} />
        </button>
      </div>
      <div className="map-distance" aria-live="polite">
        {teamBlend > 0.5 && focusedGroup ? (
          <>
            <span>{focusedGroup.name}</span>
            <button onClick={resetOverview}>Back to organization</button>
          </>
        ) : mapLevel === 'local' && localGroup ? (
          <>
            <span>
              {localGroup.name} · {actorIds.length} in local motion
              {localGroup.agentIds.length > actorIds.length
                ? ` / ${localGroup.agentIds.length} agents`
                : ''}
            </span>
            <button onClick={() => setInquiry(localGroup.id)}>Ask about team</button>
          </>
        ) : (
          <span>{detail > 0 ? 'Teams & people' : 'Organization overview'}</span>
        )}
      </div>
    </div>
  );
}
