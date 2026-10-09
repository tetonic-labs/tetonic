import { useEffect, useRef, useState } from 'react';
import { Plus, Search, Users, Pencil } from 'lucide-react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import { engineAgentToUI } from '../../engine/projections/agents';
import { EngineRequestError } from '../../engine/failure';
import { type WorkTeam, type SaveWorkTeam } from '../../engine/contracts';
import { Portrait } from '../ui/Portrait';

export function TeamsPanel({
  onAgents,
  onWork,
  onTeam,
}: {
  onAgents: () => void;
  onWork: () => void;
  onTeam: (id: string) => void;
}) {
  const engine = useLocalEngine();
  const teams = engine.workspace?.work_teams || [];
  const [selected, setSelected] = useState<string | null>(null);
  const [saved, setSaved] = useState<WorkTeam | null>(null);
  const [editing, setEditing] = useState<WorkTeam | 'new' | null>(null);
  const [query, setQuery] = useState('');
  const panelRef = useRef<HTMLElement>(null);
  useEffect(() => {
    panelRef.current?.scrollIntoView?.({ block: 'start' });
  }, [selected, editing]);
  const reported = teams.find((t) => t.id === selected);
  const team =
    saved?.id === selected && (!reported || saved.revision > reported.revision) ? saved : reported;
  const agents = (engine.workspace?.agents || []).filter(
    (a) => a.key !== engine.workspace?.shaping_agent_key && !a.plan_coordinator,
  );
  const available = engine.isConnected && engine.workspace?.work_teams !== undefined;
  if (editing)
    return (
      <TeamEditor
        key={editing === 'new' ? 'new' : `${editing.id}:${editing.revision}`}
        team={editing === 'new' ? undefined : editing}
        onBack={() => setEditing(null)}
        onSaved={(t) => {
          setSaved(t);
          setSelected(t.id);
          setEditing(null);
        }}
        onReload={() => setEditing(null)}
      />
    );
  if (team)
    return (
      <section ref={panelRef} className="team-profile">
        <button className="operator-back" onClick={() => setSelected(null)}>
          All teams
        </button>
        <div className="team-profile-heading">
          <span className="team-emblem">
            <Users size={23} />
          </span>
          <div>
            <h3>{team.name}</h3>
            <p>{team.purpose || 'Ready for your next idea.'}</p>
          </div>
        </div>
        <div className="operator-actions">
          <button className="tw-primary" disabled={!available} onClick={() => onTeam(team.id)}>
            Work with this team
          </button>
          <button disabled={!available} onClick={() => setEditing(team)}>
            <Pencil size={14} /> Edit team
          </button>
        </div>
        <h4>
          {team.agent_keys.length} {team.agent_keys.length === 1 ? 'agent' : 'agents'}
        </h4>
        <div className="team-roster">
          {team.agent_keys.map((key) => {
            const agent = agents.find((a) => a.key === key);
            return (
              <div key={key}>
                {agent && <Portrait agent={engineAgentToUI(agent)} size={38} square={false} />}
                <span>
                  <strong>{agent?.name || key}</strong>
                  <small>
                    {agent
                      ? agent.purpose || 'No role added yet'
                      : 'Agent unavailable in this workspace'}
                  </small>
                </span>
              </div>
            );
          })}
        </div>
        <p className="operator-footnote">
          The Guide chooses the right contributors for each request. Each agent keeps their own
          tools, access and limits.
        </p>
        <div className="operator-actions">
          <button onClick={onAgents}>Manage agents</button>
          <button onClick={onWork}>Workspace work</button>
        </div>
      </section>
    );
  const matching = teams.filter((t) =>
    `${t.name} ${t.purpose}`.toLowerCase().includes(query.trim().toLowerCase()),
  );
  return (
    <section ref={panelRef} className="teams-overview">
      <div className="team-list-intro">
        <p className="operator-intro">
          Bring the right agents together. Use the team again whenever you need it.
        </p>
        <button
          className="tw-primary"
          disabled={!available || !agents.length}
          onClick={() => setEditing('new')}
        >
          <Plus size={15} /> Create team
        </button>
      </div>
      {!available && (
        <p className="operator-notice">
          {engine.isConnected
            ? 'Update the connected engine to create and edit saved teams.'
            : 'Reconnect to manage teams.'}
        </p>
      )}
      {teams.length > 4 && (
        <label className="operator-search">
          <Search size={16} />
          <input
            aria-label="Find teams"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Find a team…"
          />
        </label>
      )}
      {matching.map((t) => (
        <button className="team-roster-row" key={t.id} onClick={() => setSelected(t.id)}>
          <span className="team-emblem">
            <Users size={21} />
          </span>
          <span>
            <strong>{t.name}</strong>
            <small>{t.purpose || `${t.agent_keys.length} agents ready to work together`}</small>
          </span>
          <span className="team-current">{t.agent_keys.length} agents</span>
        </button>
      ))}
      {!matching.length && (
        <div className="operator-empty">
          <p>
            {query
              ? 'No teams match your search.'
              : agents.length
                ? 'Your teams will live here. Start with the agents you already have.'
                : 'Create an agent first, then bring your agents together here.'}
          </p>
          {!agents.length && (
            <button className="tw-primary" onClick={onAgents}>
              Create an agent
            </button>
          )}
        </div>
      )}
    </section>
  );
}

function TeamEditor({
  team,
  onBack,
  onSaved,
  onReload,
}: {
  team?: WorkTeam;
  onBack: () => void;
  onSaved: (team: WorkTeam) => void;
  onReload: () => void;
}) {
  const engine = useLocalEngine();
  const [id] = useState(() => team?.id || crypto.randomUUID());
  const [name, setName] = useState(team?.name || '');
  const [purpose, setPurpose] = useState(team?.purpose || '');
  const [members, setMembers] = useState(team?.agent_keys || []);
  const [query, setQuery] = useState('');
  const [pending, setPending] = useState<SaveWorkTeam | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const agents = (engine.workspace?.agents || []).filter(
    (a) => a.key !== engine.workspace?.shaping_agent_key && !a.plan_coordinator,
  );
  const visible = agents.filter((a) =>
    `${a.name} ${a.purpose}`.toLowerCase().includes(query.toLowerCase()),
  );
  const locked = busy || !!pending;
  async function save() {
    if (busy) return;
    const request = pending || {
      id,
      request_id: crypto.randomUUID(),
      expected_revision: team?.revision || 0,
      name: name.trim(),
      purpose: purpose.trim(),
      agent_keys: members,
    };
    setPending(request);
    setBusy(true);
    setError('');
    try {
      const result = await engine.client.saveWorkTeam(request);
      if (
        result.id !== request.id ||
        result.revision !== request.expected_revision + 1 ||
        result.name !== request.name ||
        result.purpose !== request.purpose ||
        JSON.stringify(result.agent_keys) !== JSON.stringify(request.agent_keys)
      )
        throw new Error(
          'The engine did not confirm these team settings. Retry to check the same save.',
        );
      await engine.refresh();
      onSaved(result);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'We could not confirm this save.');
      if (e instanceof EngineRequestError && [400, 401, 403, 404, 409, 422].includes(e.status))
        setPending(null);
    } finally {
      setBusy(false);
    }
  }
  return (
    <form
      className="team-editor"
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <button type="button" className="operator-back" disabled={locked} onClick={onBack}>
        Back to teams
      </button>
      <div>
        <h3>{team ? 'Edit team' : 'Create a team'}</h3>
        <p className="operator-intro">A familiar group you can put to work together.</p>
      </div>
      <label>
        Team name
        <input
          autoFocus
          value={name}
          disabled={locked}
          maxLength={80}
          required
          onChange={(e) => setName(e.target.value)}
          placeholder="e.g. Research partners"
        />
      </label>
      <label>
        What is this team for? <small>Optional</small>
        <textarea
          value={purpose}
          disabled={locked}
          maxLength={1000}
          rows={2}
          onChange={(e) => setPurpose(e.target.value)}
          placeholder="The kind of work you want their help with…"
        />
      </label>
      <div className="team-member-heading">
        <h4>Choose agents</h4>
        <span>{members.length} selected</span>
      </div>
      {agents.length > 5 && (
        <label className="operator-search">
          <Search size={16} />
          <input
            aria-label="Find agents for team"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Find an agent…"
          />
        </label>
      )}
      <div className="team-member-choices">
        {visible.map((agent) => (
          <label key={agent.key} className={members.includes(agent.key) ? 'is-selected' : ''}>
            <input
              type="checkbox"
              checked={members.includes(agent.key)}
              disabled={locked || (!members.includes(agent.key) && members.length >= 24)}
              onChange={(e) => {
                const checked = e.target.checked;
                setMembers((current) =>
                  checked ? [...current, agent.key] : current.filter((k) => k !== agent.key),
                );
              }}
            />
            <Portrait agent={engineAgentToUI(agent)} size={36} square={false} />
            <span>
              <strong>{agent.name}</strong>
              <small>{agent.purpose || 'No role added yet'}</small>
            </span>
          </label>
        ))}
      </div>
      {members.some((key) => !agents.some((a) => a.key === key)) && (
        <p role="alert">
          An agent in this team is unavailable.{' '}
          <button
            type="button"
            disabled={locked}
            onClick={() => setMembers(members.filter((key) => agents.some((a) => a.key === key)))}
          >
            Remove unavailable agents
          </button>
        </p>
      )}
      <p className="operator-footnote">
        {team
          ? 'Changes apply to new discussions. Existing work keeps the roster it started with.'
          : 'Agents keep their saved models, skills, tools and permissions. Joining a team does not share private conversations.'}
      </p>
      {error && (
        <p role="alert">
          {error} {pending && 'Retry checks the same save.'}
        </p>
      )}
      {error && !pending && team && (
        <button
          type="button"
          onClick={async () => {
            await engine.refresh();
            onReload();
          }}
        >
          Reload saved team
        </button>
      )}
      <div className="operator-actions">
        <button
          className="tw-primary"
          disabled={
            busy ||
            !engine.isConnected ||
            !name.trim() ||
            !members.length ||
            members.some((k) => !agents.some((a) => a.key === k))
          }
        >
          {busy ? 'Saving…' : pending ? 'Retry save' : team ? 'Save team' : 'Create team'}
        </button>
        <button type="button" disabled={locked} onClick={onBack}>
          Cancel
        </button>
      </div>
    </form>
  );
}
