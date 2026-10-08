import { useState } from 'react';
import { Search, Users } from 'lucide-react';
import { useLocalEngine } from '../../context/LocalEngineContext';
import { engineAgentToUI } from '../../lib/engineAdapters';
import { Portrait } from '../ui/Portrait';

export function TeamsPanel({ onAgents, onWork }: { onAgents: () => void; onWork: () => void }) {
  const { teams, workspace, readErrors } = useLocalEngine();
  const [selected, setSelected] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const team = teams.find((t) => t.id === selected);
  const local = team?.id === workspace?.team_id;
  if (team)
    return (
      <section className="team-profile">
        <button className="operator-back" onClick={() => setSelected(null)}>
          All teams
        </button>
        <div className="team-profile-heading">
          <span className="team-emblem">
            <Users size={23} />
          </span>
          <div>
            <h3>{team.name}</h3>
            <p>{local ? 'Your connected workspace' : 'Team in your organization'}</p>
          </div>
        </div>
        {local && workspace ? (
          <>
            <h4>Agents in this workspace</h4>
            <div className="team-roster">
              {workspace.agents.map((a) => (
                <div key={a.key}>
                  <Portrait agent={engineAgentToUI(a)} size={34} square={false} />
                  <span>
                    <strong>{a.name}</strong>
                    <small>{a.purpose || 'No role added yet'}</small>
                  </span>
                </div>
              ))}
            </div>
            {!workspace.agents.length && (
              <p>No agents here yet. Create your first teammate in Agents.</p>
            )}
            <div className="operator-actions">
              <button className="tw-primary" onClick={onAgents}>
                Manage agents
              </button>
              <button onClick={onWork}>See work</button>
            </div>
          </>
        ) : (
          <p className="operator-notice">
            This connection reports the team’s name, but not its members or work. Open that team’s
            engine connection to inspect it.
          </p>
        )}
        <div className="team-edit-scope">
          <h4>Team settings</h4>
          <p>
            Team names and membership are read-only in this engine connection. Agent settings can be
            edited in Agents.
          </p>
        </div>
      </section>
    );
  const matching = teams.filter((t) => t.name.toLowerCase().includes(query.trim().toLowerCase()));
  return (
    <section className="teams-overview">
      <p className="operator-intro">A shared place for agents and work.</p>
      {readErrors.Teams && (
        <p className="operator-notice">
          Team information couldn’t be refreshed. Showing the last known list.
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
            <small>
              {t.id === workspace?.team_id
                ? `${workspace.agents.length} ${workspace.agents.length === 1 ? 'agent' : 'agents'} in this workspace`
                : 'Membership not returned by this connection'}
            </small>
          </span>
          {t.id === workspace?.team_id && <span className="team-current">Current</span>}
        </button>
      ))}
      {!matching.length && (
        <p className="operator-empty">
          {query ? 'No teams match your search.' : 'No team information has been returned yet.'}
        </p>
      )}
      <p className="operator-footnote">
        Creating teams and editing membership isn’t available in this connection yet.
      </p>
    </section>
  );
}
