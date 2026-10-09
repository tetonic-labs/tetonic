import { useState } from 'react';
import { Plus, Search } from 'lucide-react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import { agentSetup } from '../../lib/agentCapabilities';
import { engineAgentToUI } from '../../engine/projections/agents';
import { type WorkRecord } from '../../engine/projections/records';
import { Portrait } from '../ui/Portrait';
import { WorkStatus } from './WorkStatus';

export function AgentRoster({
  records,
  onSelect,
  onCreate,
}: {
  records: WorkRecord[];
  onSelect: (key: string) => void;
  onCreate: () => void;
}) {
  const engine = useLocalEngine();
  const [query, setQuery] = useState('');
  const [filter, setFilter] = useState('all');
  const agents = engine.workspace?.agents || [];
  const rows = agents.map((agent) => {
    const tasks = [
      ...records.flatMap((r) => (r.latest?.agent_key === agent.key ? [r.latest] : [])),
      ...(engine.workspace?.planning_tasks || []).filter((t) => t.agent_key === agent.key),
    ];
    const working = tasks.filter((t) =>
      ['starting', 'running', 'canceling'].includes(t.state),
    ).length;
    const waiting = tasks.some((t) => t.state === 'waiting_human');
    const setup = agentSetup(
      agent,
      engine.catalog,
      engine.isConnected && !engine.readErrors['Agent setup'],
    );
    const signal = !engine.isConnected
      ? 'unknown'
      : setup.state === 'needs_setup'
        ? 'blocked'
        : waiting
          ? 'needs_you'
          : working
            ? 'working'
            : setup.state === 'configured'
              ? 'done'
              : 'unknown';
    const label = !engine.isConnected
      ? 'Connection lost'
      : setup.state === 'needs_setup'
        ? 'Needs setup'
        : waiting
          ? 'Needs you'
          : working
            ? `${working} running`
            : setup.state === 'configured'
              ? 'Ready'
              : 'Setup unchecked';
    return {
      agent,
      signal: signal as 'working' | 'needs_you' | 'blocked' | 'done' | 'unknown',
      label,
    };
  });
  const matching = rows.filter(
    ({ agent, signal }) =>
      `${agent.name} ${agent.purpose} ${agent.model}`
        .toLowerCase()
        .includes(query.trim().toLowerCase()) &&
      (filter === 'all' ||
        (filter === 'attention'
          ? signal === 'needs_you' || signal === 'blocked'
          : signal === filter)),
  );
  return (
    <section className="agent-roster" aria-label="Agent roster">
      <div className="operator-toolbar">
        <p className="operator-intro">
          {agents.length
            ? `${agents.length} teammates. Choose who to work with.`
            : 'Your first teammate starts here.'}
        </p>
        <button
          className="tw-primary"
          disabled={!engine.workspace || !engine.isConnected}
          onClick={onCreate}
        >
          <Plus size={15} />
          Create agent
        </button>
      </div>
      {agents.length > 0 && (
        <>
          <label className="operator-search">
            <Search size={16} aria-hidden="true" />
            <input
              aria-label="Find agents"
              placeholder="Find a name, role or model…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </label>
          <div className="operator-filters" aria-label="Filter agents">
            {[
              ['all', 'Everyone'],
              ['working', 'Working'],
              ['attention', 'Needs attention'],
              ['done', 'Ready'],
            ].map(([id, title]) => (
              <button key={id} aria-pressed={filter === id} onClick={() => setFilter(id)}>
                {title}
              </button>
            ))}
          </div>
        </>
      )}
      <div className="agent-roster-list">
        {matching.map(({ agent, signal, label }) => (
          <button className="agent-roster-row" key={agent.key} onClick={() => onSelect(agent.key)}>
            <Portrait agent={engineAgentToUI(agent)} size={48} square={false} />
            <span className="agent-roster-identity">
              <strong>{agent.name}</strong>
              <span>
                {agent.key === engine.workspace?.shaping_agent_key
                  ? 'Think through ideas and coordinate work'
                  : agent.plan_coordinator
                    ? 'Coordinate the agreed team plan'
                    : agent.purpose || 'No role added yet'}
              </span>
              <small>{agent.model}</small>
            </span>
            <WorkStatus signal={signal} label={label} />
          </button>
        ))}
      </div>
      {!matching.length && agents.length > 0 && (
        <p className="operator-empty">No agents match this view.</p>
      )}
    </section>
  );
}
