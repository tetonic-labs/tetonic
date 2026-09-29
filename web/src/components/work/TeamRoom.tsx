import { useEffect, useMemo, useRef, useState } from 'react';
import type { Dispatch } from 'react';
import {
  ArrowLeft,
  ArrowUp,
  ArrowUpRight,
  Check,
  ChevronRight,
  Map as MapIcon,
  MessageCircle,
  Layers3,
  Plug,
  X,
} from 'lucide-react';
import type { Agent, ApprovalRequest, StreamEvent, Team } from '../../types';
import type { WorkItem } from '../../types/workroom';
import { teammate, teammateName } from '../../lib/teammates';
import { statusLabels, type WorkAction } from '../../lib/workroom';
import { teamWorkflows, workflowLabels, type RoomMessage } from '../../lib/teamRoom';
import { sampleTime } from '../../lib/workEvidence';
import type { OrganizationActivity } from '../graph/useOrganizationActivity';
import { Portrait } from '../ui/Portrait';

interface Props {
  team?: Team;
  visible: boolean;
  initialThread: string | null;
  teams: Team[];
  agents: Agent[];
  items: WorkItem[];
  events: StreamEvent[];
  approvals: ApprovalRequest[];
  activity: OrganizationActivity;
  dispatch: Dispatch<WorkAction>;
  onBack: () => void;
  onActivity: () => void;
  onTeam: (id: string, threadId: string | null) => void;
  onAgent: (id: string) => void;
  onRequest: (id: string) => void;
  onTools: (teamId: string) => void;
  onMessage: (teamId: string, text: string) => void;
}

export function TeamRoom({
  team,
  visible,
  initialThread,
  teams,
  agents,
  items,
  events,
  approvals,
  activity,
  dispatch,
  onBack,
  onActivity,
  onTeam,
  onAgent,
  onRequest,
  onTools,
  onMessage,
}: Props) {
  const [messages, setMessages] = useState<RoomMessage[]>([]);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [threads, setThreads] = useState<Record<string, string | null>>({});
  const [roster, setRoster] = useState(false);
  const [allWork, setAllWork] = useState(false);
  const [recordLimit, setRecordLimit] = useState(30);
  const [workLimit, setWorkLimit] = useState(12);
  const heading = useRef<HTMLHeadingElement>(null);
  const threadHeading = useRef<HTMLHeadingElement>(null);
  const composerInput = useRef<HTMLTextAreaElement>(null);
  const [receipt, setReceipt] = useState('');
  const threadOpener = useRef<HTMLElement | null>(null);
  const rosterOpener = useRef<HTMLButtonElement>(null);
  const scroll = useRef<HTMLDivElement>(null);
  const threadScroll = useRef<HTMLDivElement>(null);
  const initialSummaries = useRef(new Map<string, string>());
  for (const work of items)
    if (!initialSummaries.current.has(work.id)) initialSummaries.current.set(work.id, work.summary);
  const scrollPositions = useRef<Record<string, number>>({});
  const pendingScroll = useRef(false);
  const [newPosts, setNewPosts] = useState(false);
  const teamId = team?.id || '';
  const selectedId = threads[teamId] || null;
  const key = selectedId || `room:${teamId}`;
  const draft = drafts[key] || '';
  const workflows = useMemo(
    () => teamWorkflows(activity.records, teams, activity.example?.id || ''),
    [activity.records, teams, activity.example?.id],
  );
  const roomWork = items.filter((item) => item.teamId === teamId);
  const roomFlows = workflows.filter((flow) => flow.teamIds.includes(teamId));
  const blockedFlows = roomFlows.filter((flow) => flow.state === 'blocked');
  const visibleFlows = [
    ...blockedFlows,
    ...roomFlows.filter((flow) => flow.state !== 'blocked').slice(-3),
  ].slice(0, 3);
  const item = roomWork.find((work) => `work:${work.id}` === selectedId);
  const flow = roomFlows.find((work) => work.id === selectedId);
  const operations = [
    ...new Map(flow?.records.map((record) => [record.interaction.id, record]) || []).values(),
  ];
  const recentOperations = operations.slice(-4);
  const members = agents.filter((agent) => team?.pledgedAgentIds.includes(agent.id));
  const requests = approvals.filter(
    (r) => r.status === 'pending' && team?.pledgedAgentIds.includes(r.agentId),
  );
  const roomMessages: RoomMessage[] = events
    .filter(
      (event) =>
        event.teamId === teamId &&
        event.type === 'message' &&
        !event.recipientAgentId &&
        event.agentId === 'usr-alice',
    )
    .map((event) => ({ id: event.id, teamId, text: event.content, at: event.timestamp }));
  const discussions = roomMessages.filter(
    (message) =>
      messages.some((reply) => reply.threadId === `message:${message.id}`) ||
      selectedId === `message:${message.id}`,
  );
  const originMessage = roomMessages.find((message) => `message:${message.id}` === selectedId);
  const replies = messages.filter((message) => message.threadId === selectedId);
  const olderMessages = events.filter(
    (event) =>
      event.teamId === teamId &&
      event.type === 'message' &&
      !event.recipientAgentId &&
      event.agentId !== 'usr-alice',
  );
  const title = item?.title || flow?.title || originMessage?.text.slice(0, 85) || 'Work record';
  const count = roomWork.length + roomFlows.length + discussions.length;
  const people = team?.members.length
    ? team.members
    : [{ id: 'usr-local', name: 'You', isCurrentUser: true, role: 'participant' }];

  useEffect(() => {
    if (!visible) return;
    if (initialThread) setThreads((old) => ({ ...old, [teamId]: initialThread }));
    setRoster(false);
    setAllWork(false);
    heading.current?.focus({ preventScroll: true });
  }, [teamId, visible, initialThread]);
  useEffect(() => {
    if (scroll.current) scroll.current.scrollTop = scrollPositions.current[teamId] || 0;
    setNewPosts(false);
  }, [teamId]);
  useEffect(() => {
    if (selectedId && visible) threadHeading.current?.focus({ preventScroll: true });
    setRecordLimit(30);
  }, [selectedId, visible]);
  useEffect(() => {
    if (!scroll.current || !visible) return;
    if (pendingScroll.current) {
      scroll.current.scrollTop = scroll.current.scrollHeight;
      pendingScroll.current = false;
      setNewPosts(false);
    } else if (roomMessages.length) setNewPosts(true);
  }, [roomMessages.length, visible]);

  function select(id: string) {
    if (scroll.current && !selectedId) scrollPositions.current[teamId] = scroll.current.scrollTop;
    threadOpener.current = document.activeElement as HTMLElement;
    setThreads((old) => ({ ...old, [teamId]: id }));
    setAllWork(false);
  }
  function closeThread() {
    setThreads((old) => ({ ...old, [teamId]: null }));
    requestAnimationFrame(() => {
      if (scroll.current) scroll.current.scrollTop = scrollPositions.current[teamId] || 0;
      (threadOpener.current?.isConnected ? threadOpener.current : heading.current)?.focus({
        preventScroll: true,
      });
    });
  }
  function send() {
    if (!draft.trim()) return;
    if (selectedId)
      setMessages((old) => [
        ...old,
        {
          id: crypto.randomUUID(),
          teamId,
          threadId: selectedId || undefined,
          text: draft.trim(),
          at: new Date().toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' }),
        },
      ]);
    else onMessage(teamId, draft.trim());
    setDrafts((old) => ({ ...old, [key]: '' }));
    setReceipt(
      selectedId
        ? 'Reply saved in this thread. No engine is connected.'
        : 'Message saved for this team. No engine is connected.',
    );
    composerInput.current?.focus();
    pendingScroll.current = !selectedId;
    if (selectedId)
      requestAnimationFrame(() => {
        if (threadScroll.current)
          threadScroll.current.scrollTop = threadScroll.current.scrollHeight;
      });
  }
  function author(id?: string) {
    const agent = agents.find((a) => a.id === id);
    return agent ? (
      <button className="tr-author" onClick={() => onAgent(agent.id)}>
        <Portrait agent={agent} size={32} square />
        <strong>{teammate(agent).name}</strong>
        <small>Agent</small>
      </button>
    ) : (
      <span className="tr-author">
        <span className="tr-human">
          {team?.members.find((m) => m.isCurrentUser)?.name[0] || 'Y'}
        </span>
        <strong>You</strong>
        <small>Person</small>
      </span>
    );
  }
  function composer(thread: boolean) {
    return (
      <form
        className="tr-composer"
        onSubmit={(event) => {
          event.preventDefault();
          send();
        }}
      >
        <label className="sr-only" htmlFor="team-message">
          {thread ? `Reply to ${title}` : `Message ${team?.name}`}
        </label>
        <textarea
          ref={composerInput}
          id="team-message"
          rows={2}
          value={draft}
          onChange={(event) => setDrafts((old) => ({ ...old, [key]: event.target.value }))}
          placeholder={
            thread
              ? 'Ask a question or give direction…'
              : 'Think out loud. Ask the team. Give direction…'
          }
          onKeyDown={(event) => {
            if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) {
              event.preventDefault();
              send();
            }
          }}
        />
        <div>
          <small>
            {thread ? 'Thread reply' : 'Team message'} · local preview · no engine connected
          </small>
          <button
            aria-label={thread ? 'Send thread reply' : 'Send team message'}
            disabled={!draft.trim()}
          >
            <ArrowUp size={18} />
          </button>
        </div>
      </form>
    );
  }
  function post(message: RoomMessage) {
    return (
      <article className="tr-post" key={message.id}>
        {author()}
        <p>{message.text}</p>
        <small>{message.at} · saved locally</small>
        {!message.threadId && (
          <button className="tr-thread-link" onClick={() => select(`message:${message.id}`)}>
            Reply in thread
            {messages.filter((reply) => reply.threadId === `message:${message.id}`).length
              ? ` · ${messages.filter((reply) => reply.threadId === `message:${message.id}`).length} replies`
              : ''}
          </button>
        )}
      </article>
    );
  }
  if (!team) return null;
  return (
    <section
      className={'team-room' + (selectedId ? ' has-thread' : '')}
      hidden={!visible}
      aria-label={`${team.name} team room`}
      onKeyDown={(event) => {
        if (event.key !== 'Escape' || event.defaultPrevented) return;
        event.stopPropagation();
        if (roster) {
          setRoster(false);
          rosterOpener.current?.focus();
        } else if (allWork) setAllWork(false);
        else if (selectedId) closeThread();
        else onBack();
      }}
    >
      <header className="tr-header">
        <p className="sr-only" role="status">
          {receipt}
        </p>
        <button
          className="tr-map-back"
          onClick={onBack}
          aria-label="Back to map"
          title="Back to map"
        >
          <ArrowLeft size={17} />
        </button>
        <h1 ref={heading} tabIndex={-1} title={team.name}>
          {team.name}
        </h1>
        <div className="tr-cast">
          <button
            ref={rosterOpener}
            className="tr-cast-button"
            aria-expanded={roster}
            onClick={() => setRoster(!roster)}
          >
            <span className="tr-faces" aria-hidden="true">
              {people.slice(0, 2).map((m) => (
                <span className="tr-human" key={m.id}>
                  {m.name[0]}
                </span>
              ))}
              {members.slice(0, 3).map((a) => (
                <Portrait key={a.id} agent={a} size={26} square />
              ))}
            </span>
            <span>
              {people.length} {people.length === 1 ? 'person' : 'people'} · {members.length} agents
            </span>
            <ChevronRight size={13} />
          </button>
          <button className="tr-map-link" onClick={onActivity} aria-label="Team activity">
            <MapIcon size={16} />
            <span>Activity</span>
          </button>
        </div>
        {roster && (
          <div className="tr-roster" aria-label="Team participants">
            <div>
              <strong>People & agents</strong>
              <button
                onClick={() => {
                  setRoster(false);
                  rosterOpener.current?.focus();
                }}
                aria-label="Close participants"
              >
                <X size={16} />
              </button>
            </div>
            {people.map((m) => (
              <div key={m.id}>
                <span className="tr-human">{m.name[0]}</span>
                <span>
                  {m.isCurrentUser && m.name !== 'You' ? `${m.name} (you)` : m.name}
                  <small>Person · {m.role}</small>
                </span>
              </div>
            ))}
            {members.map((agent) => (
              <button key={agent.id} onClick={() => onAgent(agent.id)}>
                <Portrait agent={agent} size={30} square />
                <span>
                  {teammate(agent).name}
                  <small>Agent · {teammate(agent).shortRole}</small>
                </span>
                <ArrowUpRight size={13} />
              </button>
            ))}
            {!members.length && <p>No agents have been added to this team.</p>}
          </div>
        )}
      </header>
      <div className="tr-body">
        <nav className="tr-sidebar" aria-label="Team conversations">
          <span className="tr-sidebar-label">Team rooms</span>
          <div className="tr-room-links">
            {teams.map((room) => (
              <button
                key={room.id}
                aria-label={`Open ${room.name} room`}
                aria-current={room.id === teamId ? 'page' : undefined}
                onClick={() => {
                  if (scroll.current && !selectedId)
                    scrollPositions.current[teamId] = scroll.current.scrollTop;
                  setThreads((old) => ({ ...old, [room.id]: null }));
                  onTeam(room.id, null);
                }}
              >
                <Layers3 size={16} aria-hidden="true" />
                <span>{room.name}</span>
              </button>
            ))}
          </div>
          <button className="tr-team-tools" onClick={() => onTools(teamId)}>
            <Plug size={16} /> Team tools <ArrowUpRight size={13} />
          </button>
        </nav>
        <div className="tr-conversation" hidden={!!selectedId}>
          <div className="tr-work-strip">
            <span>Conversation</span>
            <button
              onClick={() => {
                setAllWork(!allWork);
                setWorkLimit(12);
              }}
              aria-expanded={allWork}
            >
              Work threads <b>{count}</b>
              {blockedFlows.length > 0 && (
                <span className="tr-review-count">{blockedFlows.length} need review</span>
              )}
              <ChevronRight size={14} />
            </button>
          </div>
          {allWork && (
            <nav className="tr-work-list" aria-label="Team work threads">
              {discussions.map((message) => (
                <button key={message.id} onClick={() => select(`message:${message.id}`)}>
                  <span>{message.text.slice(0, 85)}</span>
                  <small>Conversation thread</small>
                </button>
              ))}
              {roomWork.map((work) => (
                <button key={work.id} onClick={() => select(`work:${work.id}`)}>
                  <span>{work.title}</span>
                  <small>{statusLabels[work.status]}</small>
                </button>
              ))}
              {[...blockedFlows, ...roomFlows.filter((flow) => flow.state !== 'blocked')]
                .slice(0, workLimit)
                .map((work) => (
                  <button key={work.id} onClick={() => select(work.id)}>
                    <span>{work.title}</span>
                    <small>{workflowLabels[work.state]}</small>
                  </button>
                ))}
              {roomFlows.length > workLimit && (
                <button onClick={() => setWorkLimit(workLimit + 12)}>
                  Show more workflows ({roomFlows.length - workLimit} remaining)
                </button>
              )}
              {!count && <p>No work recorded yet. Start with a conversation.</p>}
            </nav>
          )}
          <div
            className="tr-scroll"
            ref={scroll}
            onScroll={() => {
              if (scroll.current && !selectedId)
                scrollPositions.current[teamId] = scroll.current.scrollTop;
              if (
                scroll.current &&
                scroll.current.scrollHeight -
                  scroll.current.scrollTop -
                  scroll.current.clientHeight <
                  50
              )
                setNewPosts(false);
            }}
          >
            <div className="tr-reading">
              <div className="tr-record-label">
                Team record <span>Illustrative work</span>
              </div>
              {!roomWork.length && (
                <div className="tr-welcome">
                  <MessageCircle size={25} />
                  <h2>Start with a thought.</h2>
                  <p>
                    Questions, ideas, and direction belong here. Work can grow from the
                    conversation.
                  </p>
                </div>
              )}
              {roomWork.map((work) => (
                <article key={work.id} className="tr-work-entry">
                  <div className="tr-post">
                    {author()}
                    <p>{work.intent}</p>
                    <button className="tr-thread-link" onClick={() => select(`work:${work.id}`)}>
                      <MessageCircle size={14} />
                      {work.title}
                      <ChevronRight size={14} />
                    </button>
                  </div>
                  <div className="tr-post">
                    {author(work.leadId)}
                    <p>{initialSummaries.current.get(work.id)}</p>
                    <button
                      className="tr-checkpoint"
                      data-signal={work.status === 'needs_input' ? 'input' : 'ordinary'}
                      onClick={() => select(`work:${work.id}`)}
                    >
                      <span>
                        <small>{statusLabels[work.status]}</small>
                        <strong>
                          {work.status === 'needs_input'
                            ? work.decision?.question || work.next
                            : work.next}
                        </strong>
                      </span>
                      <ArrowUpRight size={17} />
                    </button>
                    <button className="tr-thread-link" onClick={() => select(`work:${work.id}`)}>
                      Follow the work · {work.evidence.length} evidence{' '}
                      {messages.filter((m) => m.threadId === `work:${work.id}`).length
                        ? `· ${messages.filter((m) => m.threadId === `work:${work.id}`).length} replies`
                        : ''}
                    </button>
                    {work.receipt && (
                      <div className="tr-receipt">
                        <Check size={16} />
                        <div>
                          <strong>{work.receipt.choice}</strong>
                          <p>{work.summary}</p>
                        </div>
                      </div>
                    )}
                  </div>
                </article>
              ))}
              {!!requests.length && (
                <div className="tr-requests">
                  <small>Execution requests · separate from discussion</small>
                  {requests.map((request) => (
                    <button key={request.id} onClick={() => onRequest(request.id)}>
                      <span>◆ {request.title}</span>
                      <ArrowUpRight size={15} />
                    </button>
                  ))}
                </div>
              )}
              {roomFlows.length > 0 && (
                <section className="tr-map-record" aria-label="Work from the map">
                  <div className="tr-record-label">
                    From the map <span>Sample · {sampleTime(activity.elapsed)}</span>
                  </div>
                  {visibleFlows.map((work) => (
                    <button key={work.id} onClick={() => select(work.id)}>
                      <span className="tr-state" data-state={work.state} />
                      <span>
                        <strong>{work.title}</strong>
                        <small>
                          {workflowLabels[work.state]} · {work.agentIds.length} agents ·{' '}
                          {work.operations} operations
                        </small>
                      </span>
                      <ChevronRight size={15} />
                    </button>
                  ))}
                  {roomFlows.length > 3 && (
                    <button className="tr-thread-link" onClick={() => setAllWork(true)}>
                      Browse all {roomFlows.length} recorded workflows
                    </button>
                  )}
                </section>
              )}
              {olderMessages.map((event) => (
                <article className="tr-post" key={event.id}>
                  {author(event.agentId)}
                  <p>{event.content}</p>
                  <small>{event.timestamp}</small>
                </article>
              ))}
              {!!roomMessages.length && (
                <div className="tr-record-label">
                  Your conversation <span>This session</span>
                </div>
              )}
              {roomMessages.map(post)}
            </div>
          </div>
          {newPosts && !selectedId && (
            <button
              className="tr-new-posts"
              onClick={() => {
                if (scroll.current) scroll.current.scrollTop = scroll.current.scrollHeight;
                setNewPosts(false);
              }}
            >
              Latest messages ↓
            </button>
          )}
          {!selectedId && composer(false)}
        </div>
        {selectedId && (
          <aside className="tr-thread" aria-label={`Work thread: ${title}`}>
            <div className="tr-thread-heading">
              <button aria-label="Close work thread" onClick={closeThread}>
                <ArrowLeft size={15} />
                <span>Conversation</span>
              </button>
              <h2 ref={threadHeading} tabIndex={-1}>
                {title}
              </h2>
            </div>
            <div className="tr-thread-scroll" key={selectedId} ref={threadScroll}>
              {originMessage && (
                <div className="tr-thread-origin">
                  <small>From the team conversation</small>
                  {post(originMessage)}
                </div>
              )}
              {item && (
                <>
                  <p className="tr-thread-intent">{item.intent}</p>
                  <div className="tr-thread-people">
                    {item.agentIds.map((id) => (
                      <span key={id}>{author(id)}</span>
                    ))}
                  </div>
                  <section className="tr-lineage">
                    <h3>How we got here</h3>
                    {item.milestones.map((step, i) => (
                      <div key={i} data-state={step.state}>
                        <span>{step.state === 'done' ? <Check size={13} /> : i + 1}</span>
                        <div>
                          <strong>{step.title}</strong>
                          <p>{step.detail}</p>
                          <small>
                            {step.state === 'done'
                              ? 'Recorded checkpoint'
                              : step.state === 'current'
                                ? 'Current step'
                                : 'Next step'}
                          </small>
                        </div>
                      </div>
                    ))}
                  </section>
                  {item.evidence.length > 0 && (
                    <section className="tr-evidence">
                      <h3>Evidence</h3>
                      {item.evidence.map((evidence, i) => (
                        <details key={i}>
                          <summary>{evidence.title}</summary>
                          <p>{evidence.text}</p>
                        </details>
                      ))}
                    </section>
                  )}
                  {item.decision && item.status === 'needs_input' && (
                    <section className="tr-decision">
                      <small>◆ Your judgment</small>
                      <h3>{item.decision.question}</h3>
                      <p>{item.decision.whyYou}</p>
                      <details>
                        <summary>What we know & what we don’t</summary>
                        <p>{item.decision.known}</p>
                        <p>{item.decision.unknown}</p>
                        <p>{item.decision.fallback}</p>
                      </details>
                      {item.decision.options.map((option) => (
                        <button
                          key={option.id}
                          onClick={() => {
                            dispatch({
                              type: 'decide',
                              id: item.id,
                              optionId: option.id,
                              note: '',
                            });
                            threadHeading.current?.focus({ preventScroll: true });
                          }}
                        >
                          <strong>{option.title}</strong>
                          <span>{option.consequence}</span>
                        </button>
                      ))}
                      <small>Records a preview direction. No execution is started.</small>
                    </section>
                  )}
                  {item.receipt && (
                    <section className="tr-receipt">
                      <Check size={16} />
                      <div>
                        <strong>{item.receipt.choice}</strong>
                        <p>
                          {item.receipt.phase === 'recorded'
                            ? 'Direction recorded locally. Not yet acknowledged.'
                            : item.summary}
                        </p>
                        {item.receipt.note && <p>{item.receipt.note}</p>}
                      </div>
                    </section>
                  )}
                </>
              )}
              {flow && (
                <>
                  <div className="tr-thread-people">
                    {flow.agentIds.slice(0, 4).map((id) => (
                      <span key={id}>{author(id)}</span>
                    ))}
                    {flow.agentIds.length > 4 && (
                      <details>
                        <summary>{flow.agentIds.length - 4} more contributors</summary>
                        <div>
                          {flow.agentIds.slice(4).map((id) => (
                            <span key={id}>{author(id)}</span>
                          ))}
                        </div>
                      </details>
                    )}
                  </div>
                  <p className="tr-flow-note">
                    One shared record · {flow.operations} operations ·{' '}
                    {workflowLabels[flow.state].toLowerCase()}. Recorded steps ending does not
                    establish that the overall goal is complete.
                  </p>
                  {flow.teamIds.length > 1 && (
                    <div className="tr-handoffs">
                      <small>Teams with contributors · same work</small>
                      {flow.teamIds
                        .filter((id) => id !== teamId)
                        .map((id) => (
                          <button key={id} onClick={() => onTeam(id, flow.id)}>
                            {teams.find((t) => t.id === id)?.name}
                            <ArrowUpRight size={13} />
                          </button>
                        ))}
                    </div>
                  )}
                  <section className="tr-recent-work">
                    <h3>Current & recent work</h3>
                    {recentOperations.map((record) => (
                      <article key={record.interaction.id}>
                        {author(record.interaction.agentId)}
                        <p>{record.interaction.label}</p>
                        <small>
                          {record.state} · {record.interaction.targetName} · {sampleTime(record.at)}
                        </small>
                      </article>
                    ))}
                  </section>
                  <details className="tr-operations">
                    <summary>Recorded activity · {flow.records.length} events</summary>
                    {flow.records.length > recordLimit && (
                      <button onClick={() => setRecordLimit(recordLimit + 30)}>
                        Load earlier events ({flow.records.length - recordLimit} remaining)
                      </button>
                    )}
                    <ol>
                      {flow.records.slice(-recordLimit).map((record) => (
                        <li key={record.id}>
                          <time>{sampleTime(record.at)}</time>
                          <div>
                            <strong>
                              {teammateName(
                                record.interaction.agentId,
                                agents.find((a) => a.id === record.interaction.agentId)?.name ||
                                  record.interaction.agentId,
                              )}{' '}
                              → {record.interaction.targetName}
                            </strong>
                            <p>{record.interaction.label}</p>
                            <small>
                              {record.state}
                              {record.interaction.retryOf ? ' · retry' : ''}
                            </small>
                          </div>
                        </li>
                      ))}
                    </ol>
                  </details>
                </>
              )}
              {!item && !flow && !originMessage && (
                <p className="tr-flow-note">
                  This record isn’t available in the current sample. Your replies are still saved
                  here.
                </p>
              )}
              <div className="tr-replies" aria-label="Thread replies">
                <h3>Conversation</h3>
                {replies.length ? (
                  replies.map(post)
                ) : (
                  <p>Ask for context, explore an alternative, or give the team direction.</p>
                )}
              </div>
            </div>
            {composer(true)}
          </aside>
        )}
      </div>
    </section>
  );
}
