import { useEffect, useRef, useState } from 'react';
import { ArrowUp, ChevronDown, MessageCircle } from 'lucide-react';
import { Agent, StreamEvent } from '../../types';
import { teammate } from '../../lib/teammates';
interface Props {
  teamId: string;
  teamName: string;
  agents: Agent[];
  events: StreamEvent[];
  onSend: (text: string, recipient: string | null) => void;
}
export function FloatingChat({ teamId, teamName, agents, events, onSend }: Props) {
  const [recipient, setRecipient] = useState('team'),
    [drafts, setDrafts] = useState<Record<string, string>>({}),
    [expanded, setExpanded] = useState(false);
  const scroll = useRef<HTMLDivElement>(null),
    input = useRef<HTMLTextAreaElement>(null);
  const target =
    recipient === 'team' ? 'team' : agents.some((a) => a.id === recipient) ? recipient : 'team';
  const key = teamId + ':' + target,
    draft = drafts[key] || '';
  const messages = events.filter(
    (e) =>
      e.type === 'message' &&
      e.teamId === teamId &&
      (target === 'team'
        ? !e.recipientAgentId
        : e.recipientAgentId === target || e.agentId === target),
  );
  const person = agents.find((a) => a.id === target),
    name = person ? teammate(person).name : teamId === 'all' ? 'all teams' : 'the team';
  useEffect(() => {
    setRecipient('team');
    setExpanded(false);
  }, [teamId]);
  useEffect(() => {
    if (expanded && scroll.current) scroll.current.scrollTop = scroll.current.scrollHeight;
  }, [messages.length, expanded, target]);
  function send() {
    if (!draft.trim() || !agents.length) return;
    onSend(draft.trim(), target === 'team' ? null : target);
    setDrafts((prev) => ({ ...prev, [key]: '' }));
    setExpanded(true);
    input.current?.focus();
  }
  return (
    <section className="floating-chat" aria-label="Map conversation">
      {expanded && (
        <div className="floating-conversation">
          <div className="conversation-heading">
            <span>{target === 'team' ? teamName : `You & ${name}`}</span>
            <button aria-label="Collapse conversation" onClick={() => setExpanded(false)}>
              <ChevronDown size={17} />
            </button>
          </div>
          <div
            className="conversation-messages"
            ref={scroll}
            role="log"
            aria-label={`Conversation with ${name}`}
            aria-live="polite"
          >
            {messages.length ? (
              messages.map((message) => (
                <div className="map-chat-message" key={message.id}>
                  <span>{message.agentId === 'usr-alice' ? 'You' : message.agentName}</span>
                  <p>{message.content}</p>
                </div>
              ))
            ) : (
              <p className="conversation-empty">A little direction goes a long way.</p>
            )}
          </div>
          <p className="chat-preview-note" role="status">
            {messages.length
              ? 'Saved here. No engine is connected.'
              : 'Preview conversation · this tab only.'}
          </p>
          {messages.length > 0 && (
            <p className="direction-receipt">
              To: {target === 'team' ? teamName : name} · saved in this tab. Execution has not been
              accepted or started.
            </p>
          )}
        </div>
      )}
      <form
        className="floating-composer"
        onSubmit={(e) => {
          e.preventDefault();
          send();
        }}
        onKeyDown={(e) => {
          if (e.key === 'Escape') {
            setExpanded(false);
            e.stopPropagation();
          }
        }}
      >
        <div className="composer-context">
          <label>
            <span className="sr-only">Message recipient</span>
            <select
              aria-label="Message recipient"
              value={target}
              onChange={(e) => setRecipient(e.target.value)}
            >
              <option value="team">{teamId === 'all' ? 'All teams' : teamName}</option>
              {agents.map((a) => (
                <option key={a.id} value={a.id}>
                  {teammate(a).name}
                </option>
              ))}
            </select>
          </label>
          <button
            type="button"
            className="conversation-toggle"
            aria-label={expanded ? 'Hide conversation' : 'Show conversation'}
            aria-expanded={expanded}
            onClick={() => setExpanded(!expanded)}
          >
            <MessageCircle size={16} />
          </button>
        </div>
        <div className="composer-input">
          <textarea
            ref={input}
            aria-label={`Message ${name} on the map`}
            value={draft}
            onChange={(e) => setDrafts((prev) => ({ ...prev, [key]: e.target.value }))}
            placeholder={
              agents.length
                ? target === 'team'
                  ? 'What should we work on?'
                  : `Give ${name} a little direction…`
                : 'Add an agent to start a conversation'
            }
            rows={1}
            disabled={!agents.length}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                send();
              }
            }}
          />
          <button
            className="map-chat-send"
            aria-label="Send map message"
            disabled={!draft.trim() || !agents.length}
          >
            <ArrowUp size={19} />
          </button>
        </div>
      </form>
    </section>
  );
}
