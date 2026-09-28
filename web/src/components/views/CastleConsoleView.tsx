import { CastleNode, Agent, Team } from '../../types';
import { Button } from '../ui/Button';
import { Badge } from '../ui/Badge';
import { ScreenHeading, EmptyState } from '../ui/Screen';
import { toast } from 'sonner';
interface Props {
  castle: CastleNode;
  agents: Agent[];
  teams: Team[];
  onTogglePolicy: (key: keyof CastleNode['policy']) => void;
  onOpenCreateAgent: () => void;
  onSelectAgent: (id: string) => void;
}
const policies = [
  {
    key: 'requireApprovalForShell',
    name: 'Require approval for shell commands',
    description: 'Review requests before an agent starts a shell command.',
  },
  {
    key: 'requireApprovalForFileWrites',
    name: 'Require approval for file changes',
    description: 'Review proposed changes before an agent writes files.',
  },
  {
    key: 'allowRemoteCastleExec',
    name: 'Allow remote execution requests',
    description: 'Permit requests originating from agents on other machines.',
  },
] as const;
export function CastleConsoleView({
  castle,
  agents,
  teams,
  onTogglePolicy,
  onOpenCreateAgent,
  onSelectAgent,
}: Props) {
  const provider = castle.inferenceProvider;
  return (
    <div className="screen">
      <ScreenHeading
        title="Workstation"
        description="Understand where work runs and which policies apply."
      >
        <Button
          onClick={() =>
            toast.info('Connection check unavailable', {
              description: 'This preview has no live engine connection.',
            })
          }
        >
          Check connection
        </Button>
        <Button variant="copper" onClick={onOpenCreateAgent}>
          Create agent
        </Button>
      </ScreenHeading>
      <section className="panel workstation-summary">
        <div>
          <span className="eyebrow">Execution host · sample</span>
          <h2>{castle.hostname}</h2>
          <p className="muted">
            {castle.castleName} · {castle.ip}
          </p>
        </div>
        <Badge>{castle.policy.sandboxingLevel} sandbox setting</Badge>
      </section>
      <div className="metrics-grid">
        <section className="panel">
          <h2 className="metric-label">Inference provider</h2>
          <p className="metric-value">{provider.model}</p>
          <p className="muted break-anywhere">
            {provider.name} · {provider.endpoint}
          </p>
          <div className="row-between mt-4">
            <span>Sample latency: {provider.latencyMs} ms</span>
            <Badge
              variant={
                provider.status === 'online'
                  ? 'success'
                  : provider.status === 'offline'
                    ? 'danger'
                    : 'warning'
              }
            >
              {provider.status}
            </Badge>
          </div>
        </section>
        <section className="panel">
          <h2 className="metric-label">Memory usage</h2>
          <p className="metric-value">
            {(castle.resources.memoryMbUsed / 1024).toFixed(1)}{' '}
            <small>of {(castle.resources.memoryMbTotal / 1024).toFixed(0)} GB</small>
          </p>
          <progress
            aria-label="Sample memory usage"
            value={castle.resources.memoryMbUsed}
            max={castle.resources.memoryMbTotal}
          />
          <p className="muted mt-4">
            CPU {castle.resources.cpuPercent}% · {castle.resources.activeProcessesCount} active
            processes
          </p>
        </section>
        <section className="panel">
          <h2 className="metric-label">Remote execution</h2>
          <p className="metric-value">
            {castle.policy.allowRemoteCastleExec ? 'Allowed' : 'Blocked'}
          </p>
          <p className="muted">
            Reflects the preview setting below. No machine permissions are changed here.
          </p>
        </section>
      </div>
      <section className="panel">
        <h2>Approval policies</h2>
        <p className="muted mt-2 mb-4">
          Explore policy settings. These controls update the preview only.
        </p>
        {policies.map((p) => (
          <div className="policy-row" key={p.key}>
            <div>
              <label id={p.key + '-label'} htmlFor={p.key}>
                {p.name}
              </label>
              <p id={p.key + '-help'} className="muted">
                {p.description}
              </p>
            </div>
            <button
              id={p.key}
              role="switch"
              aria-checked={castle.policy[p.key]}
              aria-labelledby={p.key + '-label'}
              aria-describedby={p.key + '-help'}
              className="policy-switch"
              onClick={() => {
                onTogglePolicy(p.key);
                toast.success('Preview policy updated');
              }}
            >
              <span aria-hidden="true" />
              <span className="sr-only">{castle.policy[p.key] ? 'On' : 'Off'}</span>
            </button>
          </div>
        ))}
      </section>
      <section>
        <h2 className="section-title">
          Agents <span className="count">{agents.length}</span>
          <span className="muted">{agents.filter((a) => a.isLocalToCastle).length} local</span>
        </h2>
        <div className="agent-grid">
          {agents.map((a) => (
            <article key={a.id} className="panel agent-card">
              <div className="row-between flex-wrap gap-2">
                <h3>{a.name}</h3>
                <Badge>{a.status.replaceAll('_', ' ')}</Badge>
              </div>
              <p>{a.charter}</p>
              <dl className="compact-facts">
                <div>
                  <dt>Execution</dt>
                  <dd>{a.isLocalToCastle ? 'Local' : 'Remote'}</dd>
                </div>
                <div>
                  <dt>Model</dt>
                  <dd>{a.model}</dd>
                </div>
                <div>
                  <dt>Team</dt>
                  <dd>{teams.find((t) => t.id === a.pledgedTeamId)?.name || 'Personal'}</dd>
                </div>
              </dl>
              <details>
                <summary>Capabilities ({a.capabilities.length})</summary>
                <ul className="capability-list">
                  {a.capabilities.map((c) => (
                    <li key={c}>
                      <code>{c}</code>
                    </li>
                  ))}
                </ul>
              </details>
              <Button onClick={() => onSelectAgent(a.id)}>Inspect {a.name}</Button>
            </article>
          ))}
        </div>
        {!agents.length && (
          <EmptyState title="No agents available">
            Agent creation will be available when the engine is connected.
          </EmptyState>
        )}
      </section>
    </div>
  );
}
