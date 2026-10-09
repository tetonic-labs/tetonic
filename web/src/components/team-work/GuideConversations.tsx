import { useState } from 'react';
import * as Popover from '@radix-ui/react-popover';
import { History, Plus, Search } from 'lucide-react';
import { type WorkRecord } from '../../engine/projections/records';
import { taskIsActive } from '../../engine/projections/taskState';
import './guide-conversations.css';

export function GuideConversations({
  conversations,
  selectedId,
  onSelect,
}: {
  conversations: WorkRecord[];
  selectedId?: string;
  onSelect: (id?: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [limit, setLimit] = useState(20);
  const matching = conversations
    .slice()
    .reverse()
    .filter((conversation) =>
      conversation.turns.some((turn) =>
        turn.input.toLowerCase().includes(query.trim().toLowerCase()),
      ),
    );
  function choose(id?: string) {
    setOpen(false);
    onSelect(id);
  }
  return (
    <Popover.Root open={open} onOpenChange={setOpen}>
      <Popover.Trigger asChild>
        <button type="button" className="guide-history-trigger">
          <History size={15} aria-hidden="true" /> Conversations
        </button>
      </Popover.Trigger>
      <Popover.Content
        className="guide-history"
        side="top"
        align="end"
        sideOffset={10}
        collisionPadding={16}
        aria-label="Guide conversations"
      >
        <header>
          <strong>With the Guide</strong>
          <button type="button" onClick={() => choose()}>
            <Plus size={14} aria-hidden="true" /> New conversation
          </button>
        </header>
        {conversations.length > 5 && (
          <label className="guide-history-search">
            <Search size={14} aria-hidden="true" />
            <input
              aria-label="Find a conversation"
              placeholder="Find a conversation…"
              value={query}
              onChange={(event) => {
                setQuery(event.target.value);
                setLimit(20);
              }}
            />
          </label>
        )}
        <div className="guide-history-list">
          {matching.slice(0, limit).map((conversation) => (
            <button
              key={conversation.id}
              type="button"
              aria-current={selectedId === conversation.id ? 'true' : undefined}
              onClick={() => choose(conversation.id)}
            >
              <strong>{conversation.title}</strong>
              <span>
                {conversation.latest && taskIsActive(conversation.latest)
                  ? 'Replying…'
                  : conversation.latest?.error ||
                      ['not_started', 'failed', 'recovery_required'].includes(
                        conversation.latest?.state || '',
                      )
                    ? 'Reply needs attention · open to review'
                    : `${conversation.turns.length} ${conversation.turns.length === 1 ? 'exchange' : 'exchanges'}`}
              </span>
            </button>
          ))}
          {!matching.length && (
            <p>
              {query
                ? 'No matching conversations.'
                : 'A place to ask, explore, and think together.'}
            </p>
          )}
          {matching.length > limit && (
            <button type="button" onClick={() => setLimit(limit + 20)}>
              Earlier conversations
            </button>
          )}
        </div>
      </Popover.Content>
    </Popover.Root>
  );
}
