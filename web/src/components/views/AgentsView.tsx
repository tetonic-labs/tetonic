import { useState } from 'react';
import { Plus, ArrowLeft, Check, ArrowUpRight } from 'lucide-react';
import { Agent, Team } from '../../types';
import { teammate } from '../../lib/teammates';
import { Portrait } from '../ui/Portrait';
interface Props {
  agents: Agent[];
  teams: Team[];
  currentTeamId: string;
  createInitially?: boolean;
  onCreate: (name: string, purpose: string, teamId: string) => void;
  onPledgeAgent: (agentId: string, teamId: string) => void;
  onInspect: (id: string) => void;
}
export function AgentsView({
  agents,
  teams,
  currentTeamId,
  createInitially,
  onCreate,
  onPledgeAgent,
  onInspect,
}: Props) {
  const [creating, setCreating] = useState(!!createInitially || !agents.length),
    [name, setName] = useState(''),
    [purpose, setPurpose] = useState(''),
    [teamId, setTeamId] = useState(currentTeamId),
    [adding, setAdding] = useState<string | null>(null);
  if (creating)
    return (
      <form
        className="simple-form"
        onSubmit={(e) => {
          e.preventDefault();
          if (name.trim()) {
            onCreate(name.trim(), purpose.trim(), teamId);
            setCreating(false);
            setName('');
            setPurpose('');
          }
        }}
      >
        {agents.length > 0 && (
          <button type="button" className="quiet-back" onClick={() => setCreating(false)}>
            <ArrowLeft size={16} />
            Agents
          </button>
        )}
        <h2>Meet your next teammate.</h2>
        <p className="dialog-intro">A name. A purpose. Somewhere to begin.</p>
        <label>
          Name
          <input
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            required
            maxLength={60}
            placeholder="What should we call them?"
          />
        </label>
        <label>
          What will they help with?
          <textarea
            value={purpose}
            onChange={(e) => setPurpose(e.target.value)}
            rows={2}
            placeholder="Research ideas, improve our project…"
            maxLength={500}
          />
        </label>
        <label>
          Team
          <select value={teamId} onChange={(e) => setTeamId(e.target.value)}>
            <option value="">No team yet</option>
            {teams.map((t) => (
              <option key={t.id} value={t.id}>
                {t.name}
              </option>
            ))}
          </select>
        </label>
        <button type="submit" className="canvas-primary" disabled={!name.trim()}>
          Create agent <Plus size={16} />
        </button>
        <p className="preview-footnote">
          Creates a preview identity. Runtime and permissions are not connected.
        </p>
      </form>
    );
  const addingAgent = agents.find((a) => a.id === adding);
  if (addingAgent)
    return (
      <div>
        <button className="quiet-back" onClick={() => setAdding(null)}>
          <ArrowLeft size={16} />
          Agents
        </button>
        <h2>A team for {teammate(addingAgent).name}</h2>
        <p className="dialog-intro">Choose where they belong.</p>
        <div className="directory-list">
          {teams.map((t) => {
            const added = t.pledgedAgentIds.includes(addingAgent.id);
            return (
              <div key={t.id} className="directory-row">
                <div>
                  <strong>{t.name}</strong>
                  <span>{t.pledgedAgentIds.length} agents</span>
                </div>
                <button
                  className="quiet-action"
                  disabled={added}
                  aria-label={`Add ${teammate(addingAgent).name} to ${t.name}`}
                  onClick={() => onPledgeAgent(addingAgent.id, t.id)}
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
        {!teams.length && <p>Create a team first using Teams on the map.</p>}
      </div>
    );
  return (
    <div>
      <div className="dialog-title-row">
        <div>
          <h2>Your agents</h2>
          <p className="dialog-intro">Familiar faces. Different strengths.</p>
        </div>
        <button
          className="circle-action"
          aria-label="Create an agent"
          onClick={() => setCreating(true)}
        >
          <Plus size={21} />
        </button>
      </div>
      <div className="directory-list agent-directory">
        {agents.map((a) => (
          <div className="directory-agent" key={a.id}>
            <button
              className="directory-person"
              onClick={() => onInspect(a.id)}
              aria-label={`Inspect ${teammate(a).name}`}
            >
              <Portrait agent={a} size={48} />
              <span>
                <strong>{teammate(a).name}</strong>
                <small>AI · {teammate(a).shortRole}</small>
              </span>
              <ArrowUpRight size={16} />
            </button>
            <button className="add-team-link" onClick={() => setAdding(a.id)}>
              Add to team <Plus size={13} />
            </button>
          </div>
        ))}
      </div>
      <button className="text-action" onClick={() => setCreating(true)}>
        Create an agent <Plus size={16} />
      </button>
      <p className="preview-footnote">Preview identities · changes stay in this tab.</p>
    </div>
  );
}
