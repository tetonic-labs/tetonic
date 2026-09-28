import { useState } from 'react';
import { Copy, Send, Pause, Play } from 'lucide-react';
import { Agent, ApprovalRequest, StreamEvent } from '../../types';
import { Button } from '../ui/Button';
import { Badge } from '../ui/Badge';
import { EmptyState, ScreenHeading } from '../ui/Screen';
import { toast } from 'sonner';
import { teammateName } from '../../lib/teammates';
import type { WorkState } from '../../lib/workScene';
import { WorkSummary } from '../graph/WorkSummary';
import { activityLabel } from '../../lib/agentActivity';
interface Props {
  agent: Agent | undefined;
  agents: Agent[];
  events: StreamEvent[];
  approvals?: ApprovalRequest[];
  onSendMessage: (text: string) => void;
  onSelectAgent: (id: string) => void;
  work?: WorkState;
  sample?: { elapsed: number; name: string };
  playing?: boolean;
  onPlayback?: () => void;
}
const eventNames: Record<StreamEvent['type'], string> = {
  thought: 'Agent note',
  tool_call: 'Tool request',
  tool_result: 'Tool result',
  message: 'Message',
  approval_badge: 'Approval request',
};
export function HuddleView({
  agent,
  agents,
  events,
  approvals = [],
  onSendMessage,
  onSelectAgent,
  work,
  sample,
  playing,
  onPlayback,
}: Props) {
  const [input, setInput] = useState('');
  const [filter, setFilter] = useState(sample ? 'all' : 'messages');
  const [copyError, setCopyError] = useState('');
  const [announcement, setAnnouncement] = useState('');
  const selectedEvents = events.filter(
    (e) =>
      !!agent &&
      (e.agentId === agent.id ||
        e.recipientAgentId === agent.id ||
        (agent?.id === 'agt-builder' &&
          e.agentId === 'usr-alice' &&
          !e.recipientAgentId &&
          !e.teamId)),
  );
  const visible = selectedEvents.filter(
    (e) =>
      filter === 'all' ||
      (filter === 'tools'
        ? e.type === 'tool_call' || e.type === 'tool_result'
        : e.type === 'message'),
  );
  const canSend = !!agent;
  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      setCopyError('');
      toast.success('Output copied');
    } catch {
      setCopyError('Clipboard access was denied. Select the output text and copy it manually.');
    }
  }
  function send() {
    if (!input.trim() || !canSend) return;
    onSendMessage(input);
    setInput('');
    setAnnouncement('Message added to the preview. No inference or tools ran.');
    toast.success('Added to preview conversation');
  }
  return (
    <div className="screen activity-screen">
      <ScreenHeading
        title="Activity"
        description="Follow the messages, tool requests, and results for one agent."
      >
        <label className="field-label">
          Agent
          <select value={agent?.id || ''} onChange={(e) => onSelectAgent(e.target.value)}>
            {!agent && <option value="">Choose an available agent</option>}
            {agents.map((a) => (
              <option key={a.id} value={a.id}>
                {teammateName(a.id, a.name)}
              </option>
            ))}
          </select>
        </label>
      </ScreenHeading>
      {!agent ? (
        <EmptyState title="No activity record for this agent">
          Choose an available agent above. A graph node alone does not establish an agent session.
        </EmptyState>
      ) : (
        <>
          <section className="panel activity-summary">
            <div>
              <div className="row-between flex-wrap gap-2">
                <h2>{teammateName(agent.id, agent.name)}</h2>
                {!sample && (
                  <Badge variant={activityLabel(agent, approvals).pending ? 'warning' : 'default'}>
                    {activityLabel(agent, approvals).label}
                  </Badge>
                )}
              </div>
            </div>
            {sample && <WorkSummary work={work} elapsed={sample.elapsed} />}
            <details>
              <summary>Agent details & controls</summary>
              <p className="muted mt-2">{agent.charter}</p>
              <p className="muted mt-2 mb-4">
                {agent.model} · {agent.decisionIntervalMs} ms cadence ·{' '}
                {agent.tokensProcessed.toLocaleString()} sample tokens · {agent.memoryItemsCount}{' '}
                memory items
              </p>
              <div className="toolbar">
                <Button
                  onClick={() =>
                    toast.info('Showing the current sample trace', {
                      description: 'No engine ledger was queried or verified.',
                    })
                  }
                >
                  Refresh trace
                </Button>
                {onPlayback && (
                  <Button
                    onClick={onPlayback}
                    aria-label={playing ? 'Pause sample playback' : 'Play sample activity'}
                  >
                    {playing ? <Pause size={16} /> : <Play size={16} />}
                    {playing ? 'Pause sample playback' : 'Play sample activity'}
                  </Button>
                )}
              </div>
              <p className="scope-note">
                Playback controls affect this sample only. No execution process is connected.
              </p>
            </details>
          </section>
          <div className="row-between flex-wrap gap-3">
            <div className="segmented" aria-label="Activity filter">
              {[
                ['all', 'All events'],
                ['messages', 'Messages'],
                ['tools', 'Tools'],
              ].map(([id, label]) => (
                <button key={id} aria-pressed={filter === id} onClick={() => setFilter(id)}>
                  {label}
                </button>
              ))}
            </div>
            <span className="muted">
              {visible.length} {sample ? 'playback events' : 'sample events'}
            </span>
          </div>
          {copyError && (
            <p role="alert" className="inline-error">
              {copyError}
            </p>
          )}
          <ol className="event-list" aria-label="Agent activity">
            {visible.map((ev) => (
              <li key={ev.id} className={'event event-' + ev.type}>
                <div className="event-meta">
                  <span>{eventNames[ev.type]}</span>
                  <span>{teammateName(ev.agentId, ev.agentName)}</span>
                  <time>{ev.timestamp}</time>
                </div>
                {ev.type === 'thought' ? (
                  <details>
                    <summary>
                      Read agent note
                      {ev.metadata?.tokenCount !== undefined
                        ? ` · ${ev.metadata.tokenCount} tokens`
                        : ''}
                    </summary>
                    <p className="event-body">{ev.content}</p>
                  </details>
                ) : ev.type === 'tool_call' || ev.type === 'tool_result' ? (
                  <>
                    <div className="row-between flex-wrap gap-2">
                      <strong className="break-anywhere">
                        {ev.metadata?.toolName || eventNames[ev.type]}
                      </strong>
                      <div className="toolbar">
                        {ev.metadata?.durationMs !== undefined && (
                          <span className="muted">{ev.metadata.durationMs} ms</span>
                        )}
                        {ev.metadata?.exitCode !== undefined && (
                          <Badge variant={ev.metadata.exitCode === 0 ? 'success' : 'danger'}>
                            Exit {ev.metadata.exitCode}
                          </Badge>
                        )}
                        <Button
                          size="icon"
                          variant="ghost"
                          aria-label={`Copy ${eventNames[ev.type].toLowerCase()} at ${ev.timestamp}`}
                          onClick={() => copy(ev.content)}
                        >
                          <Copy size={17} />
                        </Button>
                      </div>
                    </div>
                    <pre className="code-block" tabIndex={0}>
                      <code>{ev.content}</code>
                    </pre>
                  </>
                ) : (
                  <p className="event-body">{ev.content}</p>
                )}
              </li>
            ))}
          </ol>
          {!visible.length && (
            <EmptyState title="No events in this view">
              {selectedEvents.length
                ? 'Try All events to see the rest of this agent’s activity.'
                : 'There are no sample events for this agent. Select another agent to explore a trace.'}
            </EmptyState>
          )}
          <form
            className="composer panel"
            onSubmit={(e) => {
              e.preventDefault();
              send();
            }}
          >
            <label htmlFor="agent-message" className="field-label">
              Message {teammateName(agent.id, agent.name)}
            </label>
            <p id="composer-help" className="muted">
              {canSend
                ? 'Preview only. Messages are saved for this session; no inference or tools run.'
                : 'This agent has no connected message handler in the preview.'}
            </p>
            <textarea
              id="agent-message"
              aria-describedby="composer-help"
              value={input}
              onChange={(e) => setInput(e.target.value)}
              disabled={!canSend}
              rows={3}
              placeholder="Describe the outcome you want, or ask about the work…"
              onKeyDown={(e) => {
                if ((e.metaKey || e.ctrlKey) && e.key === 'Enter' && !e.nativeEvent.isComposing) {
                  e.preventDefault();
                  send();
                }
              }}
            />
            <div className="row-between flex-wrap gap-3">
              <details>
                <summary>Try a sample message</summary>
                <div className="suggestions">
                  {[
                    'Run cargo check on tetonic-server',
                    'Audit open files for sandbox boundaries',
                    'Inspect MCP schema definitions',
                    'Draft unit tests for gatekeeper policy',
                  ].map((s) => (
                    <button
                      key={s}
                      type="button"
                      disabled={!canSend}
                      onClick={() => {
                        setInput(s);
                        document.getElementById('agent-message')?.focus();
                      }}
                    >
                      {s}
                    </button>
                  ))}
                </div>
              </details>
              <Button variant="copper" type="submit" disabled={!input.trim() || !canSend}>
                <Send size={17} />
                Send preview message
              </Button>
            </div>
            <p className="muted">Ctrl / ⌘ + Enter to send · Enter for a new line</p>
            <p role="status" className="sr-only">
              {announcement}
            </p>
          </form>
        </>
      )}
    </div>
  );
}
