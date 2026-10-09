import { useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import { Check, MessageCircle, Users } from 'lucide-react';
import type { ProjectView } from '../../lib/projectView';
import type { SharedWorkEntry } from '../../lib/workContext';
import type {
  BlackboardMessage,
  BlackboardReaction,
  BlackboardThread,
} from '../../engine/contracts';
import { useLocalEngine } from '../../context/LocalEngineContext';
import { FormattedMarkdown } from '../ui/FormattedMarkdown';
import { RecordedActivity } from './RecordedActivity';
import './blackboard.css';

const reactionLabels: Record<BlackboardReaction['emoji'], string> = {
  '👍': 'Like',
  '❤️': 'Love',
  '👀': 'Looking',
  '🎉': 'Celebrate',
  '💡': 'Idea',
  '🙏': 'Thanks',
  '🤔': 'Thinking',
  '✅': 'Acknowledged',
};

function Reactions({ reactions }: { reactions: BlackboardReaction[] }) {
  const [selected, setSelected] = useState<string>();
  const id = useId();
  const active = reactions.find((r) => r.emoji === selected && r.agents.length);
  return (
    <div className="bb-reactions">
      <div className="bb-reaction-pills" aria-label="Agent reactions">
        {reactions
          .filter((r) => r.agents.length)
          .map((reaction) => {
            const names = reaction.agents.map((a) => a.name).join(', ');
            return (
              <button
                key={reaction.emoji}
                className="bb-reaction"
                aria-label={`${reactionLabels[reaction.emoji]} · ${reaction.agents.length}: ${names}`}
                title={names}
                aria-expanded={selected === reaction.emoji}
                aria-controls={id}
                onClick={() =>
                  setSelected(selected === reaction.emoji ? undefined : reaction.emoji)
                }
              >
                <span aria-hidden="true">{reaction.emoji}</span>
                <span>{reaction.agents.length}</span>
              </button>
            );
          })}
      </div>
      {active && (
        <div id={id} className="bb-reaction-people">
          <span>{reactionLabels[active.emoji]}</span>
          <ul aria-label={`Agents who reacted with ${reactionLabels[active.emoji]}`}>
            {active.agents.map((agent) => (
              <li key={agent.agent_id}>{agent.name}</li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

function Message({
  message,
  parent,
  title,
}: {
  message: BlackboardMessage;
  parent?: BlackboardMessage;
  title?: string;
}) {
  const [expanded, setExpanded] = useState(false);
  const [long, setLong] = useState(
    message.body.length > 600 || message.body.split('\n').length > 6,
  );
  const content = useRef<HTMLDivElement>(null);
  const bodyId = useId();
  useLayoutEffect(() => {
    const element = content.current;
    if (!element) return;
    const measure = () => {
      const height = element.getBoundingClientRect().height;
      // Measure rendered Markdown, including lists, tables and code. The inner content
      // stays unconstrained so expansion and resizing cannot change this measurement.
      if (height > 0)
        setLong(height > (parseFloat(getComputedStyle(element).fontSize) || 13) * 9.9 + 1);
    };
    measure();
    const observer =
      typeof ResizeObserver === 'undefined' ? undefined : new ResizeObserver(measure);
    observer?.observe(element);
    return () => observer?.disconnect();
  }, [message.body]);
  return (
    <article className="bb-message">
      <span className="bb-avatar" aria-hidden="true">
        {message.author.name.slice(0, 1)}
      </span>
      <div className="bb-message-content">
        <header>
          <strong>{message.author.name}</strong>
          <time dateTime={message.created_at} title={new Date(message.created_at).toLocaleString()}>
            {new Date(message.created_at).toLocaleTimeString([], {
              hour: 'numeric',
              minute: '2-digit',
            })}
          </time>
        </header>
        {title && <h3>{title}</h3>}
        {parent && <small className="bb-reply-context">Replying to {parent.author.name}</small>}
        <div className="bb-body" id={bodyId} data-collapsed={long && !expanded}>
          <div className="bb-body-inner" ref={content}>
            <FormattedMarkdown content={message.body} />
          </div>
        </div>
        {long && (
          <button
            className="bb-text-button"
            aria-expanded={expanded}
            aria-controls={bodyId}
            onClick={() => setExpanded(!expanded)}
          >
            {expanded ? 'Show less' : 'Read full message'}
          </button>
        )}
        {!!message.reactions?.length && <Reactions reactions={message.reactions} />}
      </div>
    </article>
  );
}

export function ProjectBlackboard({
  projects,
  entries,
  projectId,
  onScope,
  highlight,
}: {
  projects: ProjectView[];
  entries: SharedWorkEntry[];
  projectId?: string;
  onScope: (id?: string) => void;
  highlight?: string;
}) {
  const { client, isConnected } = useLocalEngine();
  const [view, setView] = useState<'conversations' | 'records'>(
    highlight ? 'records' : 'conversations',
  );
  const [threads, setThreads] = useState<BlackboardThread[]>([]);
  const project = projects.find((p) => p.id === projectId);
  const scopeKey =
    !projectId || project?.kind === 'workspace' || projectId.startsWith('team:')
      ? ''
      : JSON.stringify(
          [
            ...new Set([
              ...(project?.streams.map((s) => s.id) || []),
              ...(projectId.startsWith('plan:') ? [projectId.slice(5)] : []),
            ]),
          ].sort(),
        );
  const [pagination, setPagination] = useState({ scope: scopeKey, offset: 0 });
  const page = pagination.scope === scopeKey ? pagination.offset : 0;
  const setPage = (offset: number) => setPagination({ scope: scopeKey, offset });
  const [hasMore, setHasMore] = useState(false);
  const [selected, setSelected] = useState<string>();
  const [detail, setDetail] = useState<BlackboardThread>();
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [detailError, setDetailError] = useState('');
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    if (highlight) setView('records');
  }, [highlight]);
  useEffect(() => {
    setSelected(undefined);
  }, [scopeKey]);
  useEffect(() => {
    if (view !== 'conversations' || !isConnected) return;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout>;
    setLoading(true);
    setThreads([]);
    setHasMore(false);
    setError('');
    async function read() {
      try {
        const result = scopeKey
          ? await client.blackboard(
              undefined,
              page,
              controller.signal,
              JSON.parse(scopeKey) as string[],
            )
          : await client.blackboard(undefined, page, controller.signal);
        if (controller.signal.aborted) return;
        setThreads(result.threads);
        setHasMore(result.has_more);
        setError('');
      } catch (e) {
        if (!controller.signal.aborted)
          setError(e instanceof Error ? e.message : 'Conversations could not be loaded.');
      } finally {
        if (!controller.signal.aborted) {
          setLoading(false);
          timer = setTimeout(read, 5000);
        }
      }
    }
    void read();
    return () => {
      controller.abort();
      clearTimeout(timer);
    };
  }, [client, isConnected, page, scopeKey, retry, view]);
  useEffect(() => {
    setDetail(undefined);
    setDetailError('');
    if (!selected || !isConnected || view !== 'conversations') return;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout>;
    async function read() {
      try {
        const result = scopeKey
          ? await client.blackboard(
              selected,
              0,
              controller.signal,
              JSON.parse(scopeKey) as string[],
            )
          : await client.blackboard(selected, 0, controller.signal);
        if (!controller.signal.aborted) {
          setDetail(result.threads[0]);
          setDetailError(result.threads.length ? '' : 'This conversation is no longer available.');
        }
      } catch (e) {
        if (!controller.signal.aborted)
          setDetailError(e instanceof Error ? e.message : 'Replies could not be loaded.');
      } finally {
        if (!controller.signal.aborted) timer = setTimeout(read, 5000);
      }
    }
    void read();
    return () => {
      controller.abort();
      clearTimeout(timer);
    };
  }, [client, isConnected, selected, scopeKey, retry, view]);
  const shown = threads;
  return (
    <>
      <nav className="bb-views" aria-label="Blackboard view">
        <button aria-pressed={view === 'conversations'} onClick={() => setView('conversations')}>
          Conversations
        </button>
        <button aria-pressed={view === 'records'} onClick={() => setView('records')}>
          Execution records
        </button>
      </nav>
      {view === 'records' ? (
        <RecordedActivity
          projects={projects}
          entries={entries}
          projectId={projectId}
          onScope={onScope}
          highlight={highlight}
        />
      ) : (
        <>
          <p className="px-board-intro">
            Your agents’ shared findings, questions and handoffs. Open a thread to follow the
            exchange.
          </p>
          <div className="px-board-scope">
            <label htmlFor="board-work-scope">Work scope</label>
            <select
              id="board-work-scope"
              value={projectId || ''}
              onChange={(e) => {
                onScope(e.target.value || undefined);
                setSelected(undefined);
                setPage(0);
              }}
            >
              <option value="">All connected work</option>
              {projects.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.title}
                </option>
              ))}
            </select>
          </div>
          {!isConnected ? (
            <p role="status">Connect to the engine to see agent conversations.</p>
          ) : (
            <>
              {error && (
                <p role="alert">
                  {error} <button onClick={() => setRetry((n) => n + 1)}>Retry</button>
                </p>
              )}
              {loading && <p role="status">Loading conversations…</p>}
              <div className="bb-topics" aria-label="Agent conversations">
                {shown.map((thread) => {
                  const expanded = selected === thread.id;
                  const live = expanded && detail?.id === thread.id ? detail : thread;
                  const project = projects.find((p) => p.id === `plan:${thread.root_work_id}`);
                  return (
                    <section className="bb-topic" key={thread.id} aria-label={thread.title}>
                      {live.messages[0] && (
                        <Message
                          key={live.messages[0].id}
                          message={live.messages[0]}
                          title={thread.title}
                        />
                      )}
                      <div className="bb-topic-heading">
                        <span className="bb-kind" data-kind={thread.kind}>
                          {live.resolved ? (
                            <>
                              <Check size={12} />
                              Resolved
                            </>
                          ) : (
                            thread.kind
                          )}
                        </span>
                        {project && (
                          <a href={`#project=${encodeURIComponent(project.id)}`}>{project.title}</a>
                        )}
                        <span
                          className="bb-audience"
                          title={`Shared with ${thread.audience.map((a) => a.name).join(', ')}`}
                        >
                          <Users size={12} aria-hidden="true" />
                          <span>Shared with {thread.audience.map((a) => a.name).join(', ')}</span>
                        </span>
                      </div>
                      <button
                        className="bb-thread-button"
                        aria-expanded={expanded}
                        aria-controls={`replies-${thread.id}`}
                        onClick={() => setSelected(expanded ? undefined : thread.id)}
                      >
                        <MessageCircle size={14} />
                        {expanded
                          ? 'Close thread'
                          : live.reply_count
                            ? `${live.reply_count} ${live.reply_count === 1 ? 'reply' : 'replies'}`
                            : 'Open thread'}
                      </button>
                      {expanded && (
                        <div className="bb-replies" id={`replies-${thread.id}`}>
                          {detailError ? (
                            <p role="alert">
                              {detailError}{' '}
                              <button onClick={() => setRetry((n) => n + 1)}>Retry</button>
                            </p>
                          ) : !detail ? (
                            <p role="status">Loading replies…</p>
                          ) : (
                            <>
                              {detail.messages.slice(1).map((message) => (
                                <Message
                                  key={message.id}
                                  message={message}
                                  parent={detail.messages.find((m) => m.id === message.reply_to)}
                                />
                              ))}
                              {!detail.reply_count && (
                                <p className="bb-empty-replies">
                                  No replies yet. This conversation does not hold up other work.
                                </p>
                              )}
                            </>
                          )}
                        </div>
                      )}
                    </section>
                  );
                })}
              </div>
              {!loading && !error && !shown.length && (
                <div className="bb-empty">
                  <MessageCircle size={24} />
                  <h3>No conversations here yet</h3>
                  <p>
                    Give agents Blackboard access in their tool settings. Agents assigned to the
                    same effort can then share messages within their communication permissions.
                  </p>
                </div>
              )}
              {(page > 0 || hasMore) && (
                <div className="bb-pagination">
                  <button
                    disabled={!page}
                    onClick={() => {
                      setPage(Math.max(0, page - 20));
                      setSelected(undefined);
                    }}
                  >
                    Newer topics
                  </button>
                  <button
                    disabled={!hasMore}
                    onClick={() => {
                      setPage(page + 20);
                      setSelected(undefined);
                    }}
                  >
                    Older topics
                  </button>
                </div>
              )}
            </>
          )}
        </>
      )}
    </>
  );
}
