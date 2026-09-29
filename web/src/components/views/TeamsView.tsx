import { useState } from 'react';
import { Plus, ArrowUpRight, Check, ArrowLeft, Layers3 } from 'lucide-react';
import { Agent, Team } from '../../types';
import { Portrait } from '../ui/Portrait';
import { teammate } from '../../lib/teammates';
import { homeTeamId } from '../../lib/mapLayout';
interface Props {
  teams: Team[];
  agents: Agent[];
  currentTeamId: string;
  onSelectTeam: (id: string) => void;
  onViewMap?: (id: string) => void;
  onPledgeAgent: (agentId: string, teamId: string) => void;
  onCreate: (name: string) => void;
  onCreateAgent: () => void;
  onInspect: (id: string) => void;
}
export function TeamsView({
  teams,
  agents,
  currentTeamId,
  onSelectTeam,
  onViewMap,
  onPledgeAgent,
  onCreate,
  onCreateAgent,
  onInspect,
}: Props) {
  const [creating, setCreating] = useState(false),
    [name, setName] = useState(''),
    [adding, setAdding] = useState<string | null>(null);
  if (creating)
    return (
      <form
        className="simple-form"
        onSubmit={(e) => {
          e.preventDefault();
          if (name.trim()) {
            onCreate(name.trim());
            setName('');
            setCreating(false);
          }
        }}
      >
        <button type="button" className="quiet-back" onClick={() => setCreating(false)}>
          <ArrowLeft size={16} />
          Teams
        </button>
        <h2>A team of your own.</h2>
        <label>
          Team name
          <input
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="A name that feels right"
            maxLength={80}
            required
          />
        </label>
        <button className="canvas-primary" type="submit" disabled={!name.trim()}>
          Create team <Plus size={16} />
        </button>
        <p className="preview-footnote">Creates a team in this preview. No engine is connected.</p>
      </form>
    );
  const addingTeam = teams.find((t) => t.id === adding);
  if (addingTeam)
    return (
      <div>
        <button className="quiet-back" onClick={() => setAdding(null)}>
          <ArrowLeft size={16} />
          Teams
        </button>
        <h2>Add to {addingTeam.name}</h2>
        <p className="dialog-intro">Choose who joins the team.</p>
        <div className="directory-list">
          {agents.map((agent) => {
            const added = addingTeam.pledgedAgentIds.includes(agent.id);
            return (
              <div className="directory-row" key={agent.id}>
                <Portrait agent={agent} size={42} />
                <div>
                  <strong>{teammate(agent).name}</strong>
                  <span>{teammate(agent).shortRole}</span>
                </div>
                <button
                  className="quiet-action"
                  disabled={added}
                  onClick={() => onPledgeAgent(agent.id, addingTeam.id)}
                  aria-label={`Add ${teammate(agent).name} to ${addingTeam.name}`}
                >
                  {added ? (
                    <>
                      <Check size={15} />
                      Added
                    </>
                  ) : (
                    <>
                      <Plus size={15} />
                      Add
                    </>
                  )}
                </button>
              </div>
            );
          })}
        </div>
        <button className="text-action" onClick={onCreateAgent}>
          Create a new agent <ArrowUpRight size={15} />
        </button>
      </div>
    );
  return (
    <div>
      <div className="dialog-title-row">
        <div>
          <h2>Your teams</h2>
          <p className="dialog-intro">People to put things in motion.</p>
        </div>
        <button
          className="circle-action"
          aria-label="Create a team"
          onClick={() => setCreating(true)}
        >
          <Plus size={21} />
        </button>
      </div>
      <div className="team-directory">
        {teams.map((team) => {
          const members = agents.filter((a) => team.pledgedAgentIds.includes(a.id));
          const shared = members.filter((a) => homeTeamId(a, teams) !== team.id).length;
          return (
            <article className="simple-team" key={team.id} data-current={team.id === currentTeamId}>
              <div className="team-name-row">
                <button onClick={() => onSelectTeam(team.id)}>
                  <Layers3 className="team-directory-symbol" size={25} aria-hidden="true" />
                  <strong>{team.name}</strong>
                  <span>
                    {team.id === currentTeamId
                      ? 'On your map'
                      : `${members.length} contributors${shared ? ` · ${shared} shared` : ''}`}
                  </span>
                </button>
                <button
                  className="quiet-action"
                  aria-label={`Open ${team.name} map`}
                  onClick={() => (onViewMap || onSelectTeam)(team.id)}
                >
                  <ArrowUpRight size={19} />
                </button>
              </div>
              <div className="team-portraits">
                {members.map((a) => (
                  <button
                    key={a.id}
                    aria-label={`Inspect ${teammate(a).name}`}
                    onClick={() => onInspect(a.id)}
                  >
                    <Portrait agent={a} size={36} />
                  </button>
                ))}
                {!members.length && <span>No agents yet</span>}
                <button
                  className="add-to-team"
                  onClick={() => setAdding(team.id)}
                  aria-label={`Add agent to ${team.name}`}
                >
                  <Plus size={14} />
                  Add agent
                </button>
              </div>
            </article>
          );
        })}
      </div>
      <button className="text-action" onClick={() => setCreating(true)}>
        Create a team <Plus size={16} />
      </button>
      <p className="preview-footnote">Preview teams · changes stay in this tab.</p>
    </div>
  );
}
