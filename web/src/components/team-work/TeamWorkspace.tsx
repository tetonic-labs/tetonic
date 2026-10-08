import { useEffect, useMemo, useRef, useState } from 'react';
import { ArrowLeft, ChevronRight, Layers, Settings2, X } from 'lucide-react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import { connectionDraftScope } from '../../lib/localEngine';
import { teamWorkspace } from '../../lib/teamWorkspace';
import { attentionItems } from '../../lib/attentionItems';
import {
  projectWorkContext,
  type WorkContextQuery,
  type WorkContextSource,
} from '../../lib/workContext';
import { ProjectMap } from '../workspace/ProjectMap';
import { LiveShaping } from './LiveShaping';
import { WorkShelf } from './WorkShelf';
import { workJourneys } from '../../lib/workJourneys';
import { WorkComposer } from './WorkComposer';
import { WorkDetails } from './WorkDetails';
import { TeamPanels } from './TeamPanels';
import { EngineTools } from './EngineTools';
import { UsagePanel } from './UsagePanel';
import { ProjectBlackboard } from './ProjectBlackboard';
import { WorkContextPanel } from './WorkContextPanel';
import './team-work.css';
import './engine-workspace.css';
import './work-journey.css';
import './operator-experience.css';

type Panel = {
  kind:
    | 'work'
    | 'agents'
    | 'teams'
    | 'attention'
    | 'settings'
    | 'blackboard'
    | 'context'
    | 'tools'
    | 'usage'
    | 'shaping'
    | 'detail';
  id?: string;
  inspect?: boolean;
  edit?: boolean;
} | null;
function route(): { panel: Panel; project?: string } {
  const hash = new URLSearchParams(location.hash.slice(1));
  return {
    panel: hash.has('shape')
      ? { kind: 'shaping', id: hash.get('shape')! }
      : hash.has('work')
        ? { kind: 'detail', id: hash.get('work')!, inspect: hash.has('inspect') }
        : null,
    project: hash.get('project') || undefined,
  };
}
export function TeamWorkspace() {
  useLocalEngine();
  return <ConnectedTeamWorkspace key={connectionDraftScope()} />;
}
function ConnectedTeamWorkspace() {
  const engine = useLocalEngine();
  const { workspace, workItems, isConnected, isConnecting, readErrors, lastUpdated } = engine;
  const { records, projects, entries } = useMemo(
    () => teamWorkspace(workspace, workItems, engine.approvals?.pending_approvals),
    [workspace, workItems, engine.approvals],
  );
  const [projectId, setProjectId] = useState(route().project);
  const [panel, setPanel] = useState<Panel>(route().panel);
  const [trail, setTrail] = useState<NonNullable<Panel>[]>([]);
  const [recipient, setRecipient] = useState<string>();
  const [query, setQuery] = useState<WorkContextQuery>({});
  const [boardScope, setBoardScope] = useState<string>();
  const [boardHighlight, setBoardHighlight] = useState<string>();
  const [dark, setDark] = useState(false);
  const origin = useRef<HTMLElement | null>(null);
  const openedTeam = useRef(false);
  const body = useRef<HTMLDivElement>(null);
  const project = projects.find((p) => p.id === projectId);
  const selected =
    panel?.kind === 'detail'
      ? records.find((r) => r.id === panel.id || r.turns.some((t) => t.id === panel.id))
      : undefined;
  const launchedSources = new Set(
    records.flatMap((r) => (r.latest?.plan ? [r.latest.plan.source_work_id] : [])),
  );
  const attentionRecords = records.filter((r) => !launchedSources.has(r.id));
  const focusSource =
    panel?.kind === 'shaping'
      ? panel.id
      : selected?.latest?.plan && !selected.latest.plan.assignment_key && !panel?.inspect
        ? selected.latest.plan.source_work_id
        : selected?.latest?.purpose === 'explore'
          ? selected.id
          : undefined;
  const journey = panel?.kind === 'shaping' || !!focusSource;
  const focusRoot = records.find(
    (r) =>
      r.latest?.plan &&
      r.latest.plan.source_work_id === focusSource &&
      !r.latest.plan.assignment_key,
  );
  const focusRecord = focusRoot || records.find((r) => r.id === focusSource);
  const attention = attentionItems(
    attentionRecords,
    engine.approvals?.pending_approvals || [],
  ).total;
  const visibleRecords = project
    ? records.filter((record) => project.streams.some((stream) => stream.id === record.id))
    : records;
  const working =
    visibleRecords.filter(
      (r) => r.latest && ['starting', 'running', 'canceling'].includes(r.latest.state),
    ).length +
    (workspace?.planning_tasks || []).filter(
      (t) =>
        ['starting', 'running', 'canceling'].includes(t.state) &&
        (!project || visibleRecords.some((r) => r.id === t.planning_for)),
    ).length;
  const context = projectWorkContext(projects, entries, {
    origin: 'engine',
    revision: lastUpdated
      ? `Updated ${new Date(lastUpdated).toLocaleTimeString()}`
      : 'Not connected',
    observedAt: lastUpdated ? new Date(lastUpdated).toISOString() : '',
    completeness: 'partial',
  });
  function url(hash = '') {
    history.pushState(null, '', `${location.pathname}${location.search}${hash}`);
  }
  function open(next: NonNullable<Panel>) {
    if (!panel) origin.current = document.activeElement as HTMLElement;
    if (
      panel &&
      (panel.kind !== next.kind || panel.id !== next.id || panel.inspect !== next.inspect)
    )
      setTrail((old) => [...old, panel]);
    setPanel(next);
  }
  function choose(id?: string) {
    setProjectId(id);
    setTrail([]);
    setPanel(null);
    setBoardScope(id);
    url(id ? `#project=${encodeURIComponent(id)}` : '');
  }
  function close() {
    setTrail([]);
    setPanel(null);
    url(projectId ? `#project=${encodeURIComponent(projectId)}` : '');
    requestAnimationFrame(() =>
      (origin.current?.isConnected
        ? origin.current
        : document.getElementById('main-content')
      )?.focus({ preventScroll: true }),
    );
  }
  function back() {
    const previous = trail.at(-1);
    if (!previous) {
      close();
      return;
    }
    setTrail((old) => old.slice(0, -1));
    setPanel(previous);
    url(
      previous.id && ['detail', 'shaping'].includes(previous.kind)
        ? `#${previous.kind === 'shaping' ? 'shape' : 'work'}=${encodeURIComponent(previous.id)}${previous.inspect ? '&inspect=1' : ''}`
        : projectId
          ? `#project=${encodeURIComponent(projectId)}`
          : '',
    );
  }
  function showWork(id: string, inspect = false) {
    const record = records.find((r) => r.id === id || r.turns.some((t) => t.id === id));
    const root = record?.id || id;
    const owner = projects.find((p) => p.streams.some((s) => s.id === root));
    if (owner) setProjectId(owner.id);
    const source =
      record?.latest?.plan && !record.latest.plan.assignment_key && !inspect
        ? record.latest.plan.source_work_id
        : record?.latest?.purpose === 'explore'
          ? root
          : undefined;
    if (source) {
      url(`#shape=${encodeURIComponent(source)}`);
      open({ kind: 'shaping', id: source });
    } else {
      url(`#work=${encodeURIComponent(root)}${inspect ? '&inspect=1' : ''}`);
      open({ kind: 'detail', id: root, inspect });
    }
  }
  function shape(id?: string) {
    url(id ? `#shape=${encodeURIComponent(id)}` : '');
    open({ kind: 'shaping', id });
  }
  function readSource(source: WorkContextSource) {
    setProjectId(source.projectId);
    if (source.target.kind === 'stream') showWork(source.target.id);
    else if (source.target.kind === 'agent') open({ kind: 'agents', id: source.target.id });
    else {
      setBoardScope(source.projectId);
      setBoardHighlight(source.target.id);
      open({ kind: 'blackboard' });
    }
  }
  useEffect(() => {
    document.title = 'Tetonic · Team workspace';
    const restore = () => {
      const value = route();
      setTrail([]);
      setPanel(value.panel);
      setProjectId(value.project);
    };
    window.addEventListener('popstate', restore);
    window.addEventListener('hashchange', restore);
    return () => {
      window.removeEventListener('popstate', restore);
      window.removeEventListener('hashchange', restore);
    };
  }, []);
  useEffect(() => {
    if (!workspace || openedTeam.current) return;
    openedTeam.current = true;
    if (!route().project && !route().panel && projects.length === 1) setProjectId(projects[0].id);
  }, [workspace, projects]);
  useEffect(() => {
    const owner = projects.find((p) =>
      p.streams.some((s) => s.id === (focusRoot?.id || focusSource || selected?.id)),
    );
    if (owner && owner.id !== projectId) setProjectId(owner.id);
    else if (projectId && !projects.some((p) => p.id === projectId) && projects.length)
      setProjectId(projects.length === 1 ? projects[0].id : undefined);
  }, [projects, focusRoot?.id, focusSource, selected?.id, projectId]);
  useEffect(() => {
    document.documentElement.classList.toggle('dark', dark);
  }, [dark]);
  useEffect(() => {
    if (body.current) body.current.scrollTop = 0;
    if (panel) document.getElementById('px-inspect-heading')?.focus({ preventScroll: true });
  }, [panel?.kind, panel?.id]);
  useEffect(() => {
    const listener = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && panel && !event.defaultPrevented) {
        event.preventDefault();
        close();
      }
    };
    window.addEventListener('keydown', listener);
    return () => window.removeEventListener('keydown', listener);
  }, [panel, projectId]);
  useEffect(() => {
    if (panel?.kind === 'blackboard' && boardHighlight)
      document.getElementById(`blackboard-${boardHighlight}`)?.scrollIntoView({ block: 'center' });
  }, [panel?.kind, boardHighlight]);
  const titles = {
    work: 'Your work',
    agents: 'Your agents',
    teams: 'Your teams',
    attention: 'Needs you',
    settings: 'Workspace settings',
    blackboard: 'Blackboard',
    context: 'Search activity',
    tools: 'Tools & MCPs',
    usage: 'Usage',
    shaping: focusRecord?.title || 'Start with an idea',
    detail: selected?.title || 'Work details',
  };
  return (
    <div className="px-app tw-live">
      <a className="skip-link" href="#main-content">
        Skip to workspace
      </a>
      <header className="px-header">
        <a
          className="px-brand"
          href="/"
          title="All work"
          onClick={(e) => {
            e.preventDefault();
            choose();
          }}
        >
          Tetonic
          <span className="brand-symbol" aria-hidden="true" />
        </a>
        <button className="tw-connection-label" onClick={() => open({ kind: 'settings' })}>
          {isConnected ? workspace?.team_name : isConnecting ? 'Connecting…' : 'Disconnected'}
        </button>
        <nav aria-label="Workspace">
          {(['work', 'teams', 'agents', 'tools'] as const).map((kind) => (
            <button
              key={kind}
              aria-pressed={panel?.kind === kind}
              onClick={() => {
                open({ kind });
              }}
            >
              {kind === 'tools' ? 'Tools & MCPs' : kind[0].toUpperCase() + kind.slice(1)}
            </button>
          ))}
          {(attention > 0 || !!readErrors.Decisions) && (
            <button onClick={() => open({ kind: 'attention' })}>
              Needs you{attention > 0 ? ` · ${attention}` : ''}
            </button>
          )}
          <button aria-label="Workspace settings" onClick={() => open({ kind: 'settings' })}>
            <Settings2 size={16} />
          </button>
        </nav>
      </header>
      <main id="main-content" tabIndex={-1}>
        <ProjectMap
          projects={projects}
          project={project}
          playing={isConnected && working > 0}
          scope={`team-work:${connectionDraftScope()}`}
          selectedStream={selected?.id || focusRoot?.id || focusSource}
          inspectionWidth={journey ? 680 : 620}
          onProject={choose}
          onStream={showWork}
          onAgent={(id) => open({ kind: 'agents', id })}
          onPlace={() => open({ kind: 'tools' })}
        />
        <div className="px-overview" data-obscured={!!panel}>
          <div className="px-breadcrumb">
            <button onClick={() => choose()} aria-label="Show all projects">
              <Layers size={14} />
              All work
            </button>
            {project && (
              <>
                <ChevronRight size={12} />
                <span>{project.team}</span>
              </>
            )}
          </div>
          <h1>{project?.title || 'Your team, at work.'}</h1>
          <p>
            {visibleRecords.length
              ? `${workJourneys(visibleRecords).length} ${workJourneys(visibleRecords).length === 1 ? 'piece of work' : 'pieces of work'} · ${project ? project.people.length : engine.uiAgents.length} agents${working ? ` · ${working} working` : ''}`
              : isConnected
                ? 'Give an agent direction, or shape an idea together.'
                : 'Connect your engine to see your team’s work.'}
          </p>
          {!panel && (
            <div className="tw-map-actions">
              <button
                onClick={() => {
                  setBoardScope(projectId);
                  setBoardHighlight(undefined);
                  open({ kind: 'blackboard' });
                }}
              >
                Blackboard
              </button>
              <button onClick={() => open({ kind: 'usage' })}>Usage</button>
              <button onClick={() => open({ kind: 'context' })}>Search activity</button>
            </div>
          )}
        </div>
        {(!isConnected || Object.keys(readErrors).length > 0) && (
          <div className="tw-connection" role="status">
            <strong>
              {!isConnected
                ? isConnecting
                  ? 'Connecting…'
                  : workspace
                    ? 'Connection lost · showing last recorded state'
                    : 'Open your engine’s connection link to begin.'
                : 'Some details could not be refreshed.'}
            </strong>
            <span>
              {workspace && !isConnected
                ? 'Work may still be running.'
                : Object.keys(readErrors).join(', ')}
            </span>
            <button onClick={engine.reconnect}>Retry connection</button>
          </div>
        )}
        {!panel && (
          <div className="px-composer">
            <WorkShelf
              records={visibleRecords}
              onWork={showWork}
              onAll={() => open({ kind: 'work' })}
            />
            <WorkComposer
              key={recipient || 'default'}
              recipient={recipient}
              onAccepted={showWork}
              onShape={shape}
              onGuideSettings={() =>
                open({ kind: 'agents', id: workspace?.shaping_agent_key, edit: true })
              }
            />
          </div>
        )}
        {panel && (
          <aside
            className="px-inspector"
            aria-label="Project details"
            data-shaping={journey}
            data-wide={[
              'detail',
              'agents',
              'teams',
              'work',
              'attention',
              'blackboard',
              'context',
              'usage',
              'tools',
            ].includes(panel.kind)}
            data-tools={panel.kind === 'tools'}
          >
            <header>
              <button
                onClick={back}
                aria-label={trail.length ? 'Back to previous view' : 'Close project details'}
              >
                <ArrowLeft size={15} />
                {trail.length
                  ? `Back to ${trail.at(-1)?.kind === 'shaping' ? 'team work' : titles[trail.at(-1)!.kind].toLowerCase()}`
                  : 'Back to the map'}
              </button>
              <button onClick={close} aria-label="Close">
                <X size={18} />
              </button>
            </header>
            <span className="px-inspector-label">
              {workspace?.team_name || 'Your workspace'}
              {!isConnected && ' · disconnected'}
            </span>
            <h2 id="px-inspect-heading" tabIndex={-1}>
              {journey ? focusRecord?.title || titles.shaping : titles[panel.kind]}
            </h2>
            <div className="px-inspector-body" ref={body}>
              {panel.kind === 'usage' && <UsagePanel onWork={showWork} />}
              {journey && (
                <LiveShaping
                  key={focusSource || 'new'}
                  workId={focusSource}
                  onAgentSettings={(key) => open({ kind: 'agents', id: key, edit: true })}
                  onWork={showWork}
                  onGuideSettings={() =>
                    open({ kind: 'agents', id: workspace?.shaping_agent_key, edit: true })
                  }
                  onSelected={(id) => {
                    setPanel({ kind: 'shaping', id });
                  }}
                />
              )}
              {panel.kind === 'detail' &&
                !journey &&
                (selected ? (
                  <WorkDetails key={selected.id} work={selected} onAccepted={showWork} />
                ) : (
                  <p>
                    {isConnecting
                      ? 'Loading work…'
                      : 'This connection has not returned that work. Open Work to choose a saved request.'}
                  </p>
                ))}
              {['work', 'agents', 'teams', 'attention', 'settings'].includes(panel.kind) && (
                <TeamPanels
                  key={`${panel.kind}:${panel.id || ''}:${!!panel.edit}`}
                  panel={panel.kind as 'work' | 'agents' | 'teams' | 'attention' | 'settings'}
                  records={panel.kind === 'attention' ? attentionRecords : records}
                  initialAgentId={panel.id}
                  editInitially={panel.edit}
                  onWork={showWork}
                  onAgent={(key) => {
                    if (key === workspace?.shaping_agent_key) shape();
                    else {
                      setRecipient(key);
                      close();
                    }
                  }}
                  dark={dark}
                  setDark={setDark}
                  onOpenAgents={() => open({ kind: 'agents' })}
                  onOpenWork={() => open({ kind: 'work' })}
                />
              )}
              {panel.kind === 'tools' && (
                <EngineTools onAgent={(id) => open({ kind: 'agents', id })} />
              )}
              {panel.kind === 'blackboard' && (
                <ProjectBlackboard
                  projects={projects}
                  entries={entries}
                  projectId={boardScope}
                  onScope={setBoardScope}
                  highlight={boardHighlight}
                />
              )}
              {panel.kind === 'context' && (
                <WorkContextPanel
                  projects={projects}
                  snapshot={context}
                  query={query}
                  onQuery={setQuery}
                  onSource={readSource}
                />
              )}
            </div>
          </aside>
        )}
      </main>
    </div>
  );
}
