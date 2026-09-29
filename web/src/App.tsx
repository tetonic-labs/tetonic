import { useEffect, useReducer, useRef, useState } from 'react';
import * as Dialog from '@radix-ui/react-dialog';
import {
  ArrowUpRight,
  ArrowUp,
  ArrowLeft,
  X,
  ChevronDown,
  Sun,
  Moon,
  Settings2,
  Plus,
  Check,
} from 'lucide-react';
import { Toaster, toast } from 'sonner';
import { TeamActivityMap } from './components/graph/TeamActivityMap';
import { FloatingChat } from './components/graph/FloatingChat';
import { LinearTrackDrawer } from './components/graph/LinearTrackDrawer';
import { TeamsView } from './components/views/TeamsView';
import { AgentsView } from './components/views/AgentsView';
import { ActionInboxView } from './components/views/ActionInboxView';
import { HuddleView } from './components/views/HuddleView';
import { CastleConsoleView } from './components/views/CastleConsoleView';
import { Portrait } from './components/ui/Portrait';
import { AgentImagePicker } from './components/ui/AgentImagePicker';
import { ViewErrorBoundary } from './components/ui/ViewErrorBoundary';
import {
  mockAgents as starterAgents,
  mockApprovals,
  mockCastleNode,
  mockStreamEvents,
  mockTeams as starterTeams,
} from './store/mockData';
import {
  mockAgentTracks as starterTracks,
  mockGraphEdges as starterEdges,
  mockGraphNodes as starterNodes,
} from './store/graphMockData';
import { Agent, ApprovalRequest, CastleNode, StreamEvent, Team } from './types';
import { teammate } from './lib/teammates';
import { activityLabel, eventSummary, latestRecordedEvent } from './lib/agentActivity';

import { useOrganizationActivity } from './components/graph/useOrganizationActivity';
import { WorkSummary } from './components/graph/WorkSummary';
import { AttentionView, workExceptions } from './components/views/AttentionView';
import { largeWorkspace, workspaceSizes } from './store/largeWorkspaces';
import { Workroom } from './components/work/Workroom';
import { workroomExamples } from './store/workroomExamples';
import { needsJudgment, workroomReducer } from './lib/workroom';
const requestedWorkspace = new URLSearchParams(window.location.search).get('workspace');
const demo =
  requestedWorkspace && Object.hasOwn(workspaceSizes, requestedWorkspace)
    ? largeWorkspace(requestedWorkspace as keyof typeof workspaceSizes)
    : null;
const mockAgents = demo?.agents || starterAgents,
  mockTeams = demo?.teams || starterTeams;
const mockGraphNodes = demo?.nodes || starterNodes,
  mockGraphEdges = demo?.edges || starterEdges,
  mockAgentTracks = demo?.tracks || starterTracks;

type Panel =
  | 'attention'
  | 'teams'
  | 'agents'
  | 'agent'
  | 'activity'
  | 'decisions'
  | 'settings'
  | null;
export function App({ initialView = 'work' }: { initialView?: 'work' | 'map' } = {}) {
  const [view, setView] = useState(initialView);
  const [workItems, dispatchWork] = useReducer(workroomReducer, undefined, () =>
    workroomExamples(mockAgents, mockTeams),
  );
  const [selectedWorkId, setSelectedWorkId] = useState<string | null>(null);
  const [agents, setAgents] = useState<Agent[]>(mockAgents),
    [teams, setTeams] = useState<Team[]>(mockTeams),
    [approvals, setApprovals] = useState<ApprovalRequest[]>(demo ? [] : mockApprovals),
    [events, setEvents] = useState<StreamEvent[]>(demo ? [] : mockStreamEvents),
    [castle, setCastle] = useState(mockCastleNode);
  const [teamId, setTeamId] = useState(demo ? 'all' : 'team-platform'),
    [panel, setPanel] = useState<Panel>(null),
    [selectedId, setSelectedId] = useState(mockAgents[0]?.id || ''),
    [requestId, setRequestId] = useState(''),
    [resource, setResource] = useState<string | null>(null),
    [createAgent, setCreateAgent] = useState(false);
  const [drafts, setDrafts] = useState<Record<string, string>>({}),
    [sent, setSent] = useState(''),
    [assign, setAssign] = useState(false);
  const [theme, setTheme] = useState<'light' | 'dark'>(() => {
    try {
      return localStorage.getItem('tetonic_theme') === 'dark' ? 'dark' : 'light';
    } catch {
      return 'light';
    }
  });
  const activity = useOrganizationActivity(agents, mockGraphNodes, mockGraphEdges, mockAgentTracks);
  const timelineEvents = activity.started
    ? [...activity.events, ...events.filter((e) => e.agentId === 'usr-alice')]
    : events;
  const [workOrigin, setWorkOrigin] = useState<'map' | 'agent' | 'attention'>('agent');
  const [approvalHistory, setApprovalHistory] = useState(false);
  const opener = useRef<HTMLElement | null>(null);
  const team = teams.find((t) => t.id === teamId),
    members =
      teamId === 'all' ? agents : agents.filter((a) => team?.pledgedAgentIds.includes(a.id)),
    selected = agents.find((a) => a.id === selectedId);
  const pending = approvals.filter((r) => r.status === 'pending');
  const workAttention = workItems.filter(needsJudgment);
  const attentionCount =
    pending.length + workExceptions(activity.states, approvals).length + workAttention.length;
  useEffect(() => {
    document.documentElement.classList.toggle('dark', theme === 'dark');
    try {
      localStorage.setItem('tetonic_theme', theme);
    } catch {
      /* Theme remains usable. */
    }
  }, [theme]);
  useEffect(() => {
    document.title = 'Tetonic · A place for your work';
  }, []);
  function open(next: Panel) {
    if (!panel && !resource) opener.current = document.activeElement as HTMLElement;
    setPanel(next);
    setAssign(false);
  }
  function inspect(id: string) {
    setSelectedId(id);
    open('agent');
  }
  function showWork(id: string) {
    close();
    setSelectedWorkId(id);
    setView('work');
  }
  function close() {
    setPanel(null);
    setResource(null);
  }
  function addAgent(agentId: string, target: string) {
    setTeams((prev) =>
      prev.map((t) =>
        t.id === target && !t.pledgedAgentIds.includes(agentId)
          ? { ...t, pledgedAgentIds: [...t.pledgedAgentIds, agentId] }
          : t,
      ),
    );
    toast.success('Added to team in this preview');
  }
  function newTeam(name: string) {
    const id = 'team-' + crypto.randomUUID();
    setTeams((prev) => [
      ...prev,
      {
        id,
        name,
        tagline: '',
        isPersonal: false,
        members: mockTeams[0].members,
        pledgedAgentIds: [],
        createdAt: new Date().toISOString(),
      },
    ]);
    setTeamId(id);
    toast.success('Team created in preview');
  }
  function newAgent(name: string, purpose: string, target: string) {
    const id = 'agent-' + crypto.randomUUID();
    const agent: Agent = {
      id,
      name,
      charter: purpose || 'Ready for a purpose.',
      model: 'Not connected',
      status: 'idle',
      decisionIntervalMs: 0,
      capabilities: [],
      tokensProcessed: 0,
      memoryItemsCount: 0,
      isLocalToCastle: true,
      lastActive: 'Not started',
    };
    setAgents((prev) => [...prev, agent]);
    if (target) {
      addAgent(id, target);
      setTeamId(target);
    }
    setSelectedId(id);
    setPanel('agent');
    toast.success('Preview agent created');
  }
  function decide(id: string, status: 'approved' | 'rejected') {
    const request = approvals.find((r) => r.id === id);
    if (!request || request.status !== 'pending') return;
    setApprovals((prev) => prev.map((r) => (r.id === id ? { ...r, status } : r)));
    setEvents((prev) => [
      ...prev,
      {
        id: crypto.randomUUID(),
        agentId: request.agentId,
        agentName: request.agentName,
        type: 'message',
        timestamp: new Date().toLocaleTimeString(),
        content: `Preview request ${status}: ${request.title}. No engine action was sent.`,
      },
    ]);
  }
  function send(text: string, id = selectedId) {
    if (!text.trim()) return;
    setEvents((prev) => [
      ...prev,
      {
        id: crypto.randomUUID(),
        agentId: 'usr-alice',
        agentName: 'You',
        recipientAgentId: id,
        teamId: members.some((a) => a.id === id) ? teamId : undefined,
        type: 'message',
        timestamp: new Date().toLocaleTimeString(),
        content: text.trim(),
      },
    ]);
    setDrafts((prev) => ({ ...prev, [id]: '' }));
    setSent(id);
  }
  function requests(id?: string) {
    setRequestId(id || '');
    setApprovalHistory(false);
    open(id ? 'decisions' : 'attention');
  }
  function inspectWork(id: string, origin: 'map' | 'agent' | 'attention') {
    setSelectedId(id);
    setWorkOrigin(origin);
    open('activity');
  }
  function sendMapMessage(text: string, recipient: string | null) {
    setEvents((prev) => [
      ...prev,
      {
        id: crypto.randomUUID(),
        agentId: 'usr-alice',
        agentName: 'You',
        teamId,
        recipientAgentId: recipient || undefined,
        type: 'message',
        timestamp: new Date().toLocaleTimeString(),
        content: text,
      },
    ]);
  }
  const info = selected ? teammate(selected) : null,
    update = selected ? latestRecordedEvent(selected.id, timelineEvents) : null;
  const selectedRequests = approvals.filter(
    (r) => r.agentId === selectedId && r.status === 'pending',
  );
  const graphNodes = mockGraphNodes.map((node) => {
    const agent = agents.find((a) => a.id === node.id);
    return agent ? { ...node, statusLabel: activityLabel(agent, approvals).label } : node;
  });
  const titles: Record<NonNullable<Panel>, string> = {
    attention: 'Needs attention',
    teams: 'Teams',
    agents: 'Agents',
    agent: info?.name || 'Agent',
    activity: 'Agent activity',
    decisions: 'Your decision',
    settings: 'Settings',
  };
  return (
    <div className={'canvas-app view-' + view}>
      <a className="skip-link" href="#agent-map">
        Skip to {view === 'work' ? 'work' : 'map'}
      </a>
      <Toaster position="bottom-center" theme={theme} />
      <ViewErrorBoundary>
        <main id="agent-map" tabIndex={-1}>
          <div className="workspace-layer" hidden={view !== 'work'}>
            <Workroom
              items={workItems}
              dispatch={dispatchWork}
              agents={agents}
              teams={teams}
              selectedId={selectedWorkId}
              onSelect={setSelectedWorkId}
              onAgent={inspect}
              onMap={(id) => {
                setTeamId(id);
                setView('map');
              }}
            />
          </div>
          <div className="workspace-layer" hidden={view !== 'map'}>
            <TeamActivityMap
              visible={view === 'map'}
              teams={teams}
              agents={members}
              nodes={mockGraphNodes}
              edges={mockGraphEdges}
              tracks={mockAgentTracks}
              approvals={approvals}
              activity={activity}
              onWork={(id) => inspectWork(id, 'map')}
              scope={teamId}
              onAgent={inspect}
              onRequest={requests}
              onDestination={(id) => {
                opener.current = document.activeElement as HTMLElement;
                setResource(id);
              }}
              onAddAgent={() => {
                setCreateAgent(false);
                open('agents');
              }}
            />
            <div className="floating-team">
              <button onClick={() => open('teams')}>
                <span>{teamId === 'all' ? 'All teams' : team?.name || 'Choose a team'}</span>
                <ChevronDown size={15} />
              </button>
              <p>
                {members.length} AI {members.length === 1 ? 'teammate' : 'teammates'}
              </p>
            </div>
            <FloatingChat
              teamId={teamId}
              teamName={teamId === 'all' ? 'All teams' : team?.name || 'Your team'}
              agents={members}
              events={events}
              onSend={sendMapMessage}
            />
          </div>
          <header className="floating-header">
            <div className="canvas-brand">
              <a href="#agent-map" aria-label="Tetonic workspace" onClick={() => setView('work')}>
                Tetonic
                <span className="brand-symbol" />
              </a>
              <details className="canvas-preview">
                <summary>Preview</summary>
                <div className="preview-workspaces">
                  <p>
                    Illustrative work and sample agents. No engine is connected. Changes stay in
                    this tab.
                  </p>
                  <strong>Example workspaces</strong>
                  <a href="?">Original workspace</a>
                  {Object.entries(workspaceSizes).map(([id, size]) => (
                    <a key={id} href={`?workspace=${id}`}>
                      {size.name} · {size.agents} agents / {size.teams} teams / {size.resources}{' '}
                      tools
                    </a>
                  ))}
                  <small>
                    Switching examples reloads the preview and clears unsaved session changes.
                  </small>
                </div>
              </details>
            </div>
            <nav aria-label="Workspace">
              <button
                aria-current={view === 'work' ? 'page' : undefined}
                onClick={() => setView('work')}
              >
                Work
              </button>
              <button
                aria-current={view === 'map' ? 'page' : undefined}
                onClick={() => setView('map')}
              >
                Map
              </button>
              <button onClick={() => open('teams')}>Teams</button>
              <button
                onClick={() => {
                  setCreateAgent(false);
                  open('agents');
                }}
              >
                Agents
              </button>
            </nav>
            <div className="floating-utilities">
              <button
                className={'attention-entry ' + (attentionCount ? 'has-requests' : '')}
                onClick={() => requests()}
                aria-label={`Needs you, ${attentionCount} ${attentionCount === pending.length ? (attentionCount === 1 ? 'request' : 'requests') : attentionCount === 1 ? 'item' : 'items'}`}
              >
                <span className="attention-dot" />
                Needs you{attentionCount > 0 && <b>{attentionCount}</b>}
              </button>
              <button
                className="utility-button"
                onClick={() => setTheme(theme === 'light' ? 'dark' : 'light')}
                aria-label={theme === 'light' ? 'Use dark theme' : 'Use light theme'}
              >
                {theme === 'light' ? <Moon size={17} /> : <Sun size={17} />}
              </button>
              <button
                className="utility-button"
                onClick={() => open('settings')}
                aria-label="Settings"
              >
                <Settings2 size={17} />
              </button>
            </div>
          </header>
        </main>
      </ViewErrorBoundary>
      <Dialog.Root
        open={!!panel}
        onOpenChange={(value) => {
          if (!value) close();
        }}
      >
        <Dialog.Portal>
          <Dialog.Overlay className="canvas-scrim" />
          <Dialog.Content
            className={
              'canvas-dialog ' + (panel === 'activity' || panel === 'settings' ? 'wide' : '')
            }
            onCloseAutoFocus={(e) => {
              e.preventDefault();
              (opener.current?.isConnected
                ? opener.current
                : document.getElementById('agent-map')
              )?.focus({ preventScroll: true });
            }}
          >
            <Dialog.Title className="sr-only">{panel ? titles[panel] : ''}</Dialog.Title>
            <Dialog.Description className="sr-only">
              Close to return to the same place in your workspace.
            </Dialog.Description>
            <Dialog.Close className="canvas-close" aria-label="Close window">
              <X size={20} />
            </Dialog.Close>
            {panel === 'teams' && (
              <>
                <button
                  className="quiet-back"
                  onClick={() => {
                    setTeamId('all');
                    setView('map');
                    close();
                  }}
                >
                  View all teams
                </button>
                <TeamsView
                  teams={teams}
                  agents={agents}
                  currentTeamId={teamId}
                  onSelectTeam={(id) => {
                    setTeamId(id);
                    setView('map');
                    close();
                  }}
                  onPledgeAgent={addAgent}
                  onCreate={newTeam}
                  onCreateAgent={() => {
                    setCreateAgent(true);
                    open('agents');
                  }}
                  onInspect={inspect}
                />
              </>
            )}
            {panel === 'agents' && (
              <AgentsView
                agents={agents}
                teams={teams}
                currentTeamId={teamId}
                createInitially={createAgent}
                onCreate={newAgent}
                onPledgeAgent={addAgent}
                onInspect={inspect}
              />
            )}
            {panel === 'attention' && (
              <AttentionView
                workDecisions={workAttention.map((item) => ({
                  id: item.id,
                  title: item.title,
                  context: item.context,
                  summary:
                    item.status === 'review'
                      ? 'A result is ready for review.'
                      : item.decision?.question || item.summary,
                }))}
                onDecision={showWork}
                agents={agents}
                approvals={approvals}
                states={activity.states}
                onRequest={requests}
                onWork={(id) => inspectWork(id, 'attention')}
                onHistory={() => {
                  setApprovalHistory(true);
                  setRequestId('');
                  open('decisions');
                }}
              />
            )}
            {panel === 'decisions' && (
              <ActionInboxView
                agents={agents}
                key={String(approvalHistory) + requestId}
                showHistory={approvalHistory}
                approvals={approvals}
                initialSelectedId={requestId}
                onApprove={(id) => decide(id, 'approved')}
                onReject={(id) => decide(id, 'rejected')}
              />
            )}
            {panel === 'agent' && selected && info && (
              <div className="agent-focus">
                <div className="agent-focus-person">
                  <Portrait agent={selected} size={76} />
                  <div>
                    <h2>{info.name}</h2>
                    <p>AI · {info.shortRole}</p>
                  </div>
                </div>
                <p className="agent-purpose">{info.role}</p>
                <AgentImagePicker key={selected.id} agentId={selected.id} />
                <div className="agent-focus-actions">
                  <button onClick={() => inspectWork(selected.id, 'agent')}>
                    Activity <ArrowUpRight size={15} />
                  </button>
                  <button aria-expanded={assign} onClick={() => setAssign(!assign)}>
                    Add to team <Plus size={15} />
                  </button>
                </div>
                {assign && (
                  <div className="assign-teams">
                    {teams.map((t) => {
                      const added = t.pledgedAgentIds.includes(selected.id);
                      return (
                        <button
                          key={t.id}
                          disabled={added}
                          onClick={() => addAgent(selected.id, t.id)}
                          aria-label={`Add ${info.name} to ${t.name}`}
                        >
                          <span>{t.name}</span>
                          {added ? <Check size={15} /> : <Plus size={15} />}
                        </button>
                      );
                    })}
                  </div>
                )}
                {activity.started && (
                  <WorkSummary work={activity.states.get(selected.id)} elapsed={activity.elapsed} />
                )}
                {selectedRequests.length > 0 ? (
                  <button
                    className="agent-question"
                    onClick={() => requests(selectedRequests[0].id)}
                  >
                    <span className="agent-question-mark">?</span>
                    <span>
                      <small>Needs your decision</small>
                      <strong>{selectedRequests[0].title}</strong>
                    </span>
                    <ArrowUpRight size={17} />
                  </button>
                ) : (
                  !activity.started && (
                    <p className="agent-quiet-status">{activityLabel(selected, approvals).label}</p>
                  )
                )}
                {update && (
                  <div className="agent-last-update">
                    <span>Last recorded update</span>
                    <p>{eventSummary(update)}</p>
                  </div>
                )}
                <form
                  className="agent-direction"
                  onSubmit={(e) => {
                    e.preventDefault();
                    send(drafts[selected.id] || '');
                  }}
                >
                  <label className="sr-only" htmlFor="agent-direction">
                    Message {info.name}
                  </label>
                  <textarea
                    id="agent-direction"
                    rows={2}
                    value={drafts[selected.id] || ''}
                    placeholder={`Give ${info.name} a little direction…`}
                    onChange={(e) =>
                      setDrafts((prev) => ({ ...prev, [selected.id]: e.target.value }))
                    }
                  />
                  <button aria-label="Send preview message" disabled={!drafts[selected.id]?.trim()}>
                    <ArrowUp size={18} />
                  </button>
                </form>
                <p className="preview-footnote" role="status">
                  {sent === selected.id
                    ? 'Saved in preview. No inference or tools ran.'
                    : 'Preview messages stay in this session.'}
                </p>
              </div>
            )}
            {panel === 'activity' && (
              <>
                <button
                  className="quiet-back"
                  onClick={() =>
                    workOrigin === 'map'
                      ? close()
                      : open(workOrigin === 'attention' ? 'attention' : 'agent')
                  }
                >
                  <ArrowLeft size={16} />
                  {workOrigin === 'map'
                    ? 'Back to map'
                    : workOrigin === 'attention'
                      ? 'Back to attention'
                      : `Back to ${info?.name}`}
                </button>
                <HuddleView
                  agent={selected}
                  agents={agents}
                  events={timelineEvents}
                  work={activity.states.get(selectedId)}
                  sample={
                    activity.started
                      ? { elapsed: activity.elapsed, name: activity.example?.name || 'Sample' }
                      : undefined
                  }
                  playing={activity.playing}
                  onPlayback={activity.play}
                  approvals={approvals}
                  onSelectAgent={setSelectedId}
                  onSendMessage={send}
                />
              </>
            )}
            {panel === 'settings' && (
              <CastleConsoleView
                castle={castle}
                agents={agents}
                teams={teams}
                onSelectAgent={inspect}
                onTogglePolicy={(key: keyof CastleNode['policy']) =>
                  setCastle((prev) => ({
                    ...prev,
                    policy: { ...prev.policy, [key]: !prev.policy[key] },
                  }))
                }
                onOpenCreateAgent={() => {
                  setCreateAgent(true);
                  open('agents');
                }}
              />
            )}
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
      <LinearTrackDrawer
        node={graphNodes.find((n) => n.id === resource)}
        activityRecords={
          activity.started
            ? activity.records.filter((r) => r.interaction.targetId === resource)
            : undefined
        }
        track={resource ? mockAgentTracks[resource] || null : null}
        nodes={graphNodes}
        edges={mockGraphEdges.filter((e) => e.source === resource || e.target === resource)}
        onClose={() => {
          setResource(null);
          requestAnimationFrame(() =>
            (opener.current?.isConnected
              ? opener.current
              : document.getElementById('agent-map')
            )?.focus({ preventScroll: true }),
          );
        }}
        onJumpToHuddle={(id) => {
          setResource(null);
          inspectWork(id, 'map');
        }}
      />
    </div>
  );
}
