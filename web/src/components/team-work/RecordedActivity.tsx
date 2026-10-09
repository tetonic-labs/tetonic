import type { ProjectView } from '../../lib/projectView';
import type { SharedWorkEntry } from '../../lib/workContext';
import { FormattedMarkdown } from '../ui/FormattedMarkdown';
import { Portrait } from '../ui/Portrait';
import { Terminal, UserRound } from 'lucide-react';

export function RecordedActivity({
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
  const shown = entries.filter((entry) => !projectId || entry.projectId === projectId);
  return (
    <>
      <div className="px-board-scope">
        <label htmlFor="blackboard-scope">Work scope</label>
        <select
          id="blackboard-scope"
          value={projectId || ''}
          onChange={(e) => onScope(e.target.value || undefined)}
        >
          <option value="">All connected work</option>
          {projects.map((p) => (
            <option key={p.id} value={p.id}>
              {p.team} · {p.title}
            </option>
          ))}
        </select>
      </div>
      <p className="px-board-intro">
        Recorded requests, responses and tool results from your local owner workspace, grouped by
        discussion in turn order. The engine returns up to 100 transcript records per run; this is
        not a token stream or a shared human room.
      </p>
      <div className="px-blackboard" role="log" aria-label="Recorded team output" aria-live="off">
        {shown.map((entry) => {
          const project = projects.find((p) => p.id === entry.projectId);
          const agent = project?.people.find((p) => p.agent.id === entry.authorId)?.agent;
          return (
            <article
              key={entry.id}
              id={`blackboard-${entry.id}`}
              data-highlighted={highlight === entry.id}
              className="px-board-entry"
            >
              <div className="px-board-avatar">
                {agent ? (
                  <Portrait agent={agent} size={36} square={false} />
                ) : entry.kind === 'tool' ? (
                  <Terminal size={20} />
                ) : (
                  <UserRound size={20} />
                )}
              </div>
              <div>
                <header>
                  <strong>{entry.author}</strong>
                  <span>{entry.time}</span>
                  <small>{entry.kind === 'tool' ? 'Tool result' : project?.team}</small>
                </header>
                <FormattedMarkdown content={entry.content} />
                <small className="px-source-id">{entry.id} · Engine record</small>
              </div>
            </article>
          );
        })}
        {!shown.length && <p>No shared output in this scope yet.</p>}
      </div>
    </>
  );
}
