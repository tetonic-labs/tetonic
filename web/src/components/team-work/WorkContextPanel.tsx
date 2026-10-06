import { ArrowUpRight, Search } from 'lucide-react';
import type { ProjectView } from '../../lib/projectView';
import {
  queryWorkContext,
  type WorkContextSnapshot,
  type WorkContextQuery,
  type WorkContextSource,
} from '../../lib/workContext';

export function WorkContextPanel({
  projects,
  snapshot,
  query,
  onQuery,
  onSource,
}: {
  projects: ProjectView[];
  snapshot: WorkContextSnapshot;
  query: WorkContextQuery;
  onQuery: (q: WorkContextQuery) => void;
  onSource: (source: WorkContextSource) => void;
}) {
  const result = queryWorkContext(snapshot, query);
  return (
    <>
      <p>Search the same engine records shown on the map, then open the original work.</p>
      <div className="px-board-scope">
        <label htmlFor="context-scope">Look across</label>
        <select
          id="context-scope"
          value={
            query.projectId
              ? `project:${query.projectId}`
              : query.areaId
                ? `area:${query.areaId}`
                : ''
          }
          onChange={(e) => {
            const [kind, ...parts] = e.target.value.split(':');
            const id = parts.join(':');
            onQuery({
              ...query,
              projectId: kind === 'project' ? id : undefined,
              areaId: kind === 'area' ? id : undefined,
            });
          }}
        >
          <option value="">All connected work</option>
          {[
            ...new Map(projects.filter((p) => p.area).map((p) => [p.area!.id, p.area!])).values(),
          ].map((area) => (
            <option key={area.id} value={`area:${area.id}`}>
              {area.name} · area
            </option>
          ))}
          {projects.map((p) => (
            <option key={p.id} value={`project:${p.id}`}>
              {p.title}
            </option>
          ))}
        </select>
      </div>
      <form className="px-context-search" onSubmit={(e) => e.preventDefault()}>
        <Search size={16} />
        <input
          aria-label="Search shared work context"
          value={query.text || ''}
          placeholder="Search an agent, task or question…"
          onChange={(e) => onQuery({ ...query, text: e.target.value, filter: undefined })}
        />
      </form>
      <div className="px-context-filters" aria-label="Context filters">
        {(
          [
            ['All records', undefined],
            ['Needs you', 'needs_you'],
            ['Waiting', 'waiting'],
            ['Working', 'working'],
          ] as const
        ).map(([label, filter]) => (
          <button
            key={label}
            aria-pressed={query.filter === filter}
            onClick={() => onQuery({ ...query, text: '', filter })}
          >
            {label}
          </button>
        ))}
      </div>
      <div className="px-context-boundary">
        <strong>Search recorded work</strong>
        <span>
          These are source records, not a generated answer. This search does not start work.
        </span>
      </div>
      <p className="px-context-count" role="status">
        {result.total} matching records · {snapshot.revision}
        {result.truncated ? ` · showing ${result.sources.length}` : ''}
      </p>
      {result.sources.map((source) => (
        <article className="px-context-record" key={source.id}>
          <small>
            {source.team} · {source.kind}
          </small>
          <h3>{source.title}</h3>
          <p>{source.content}</p>
          <button onClick={() => onSource(source)}>
            Open source
            <ArrowUpRight size={14} />
          </button>
        </article>
      ))}
      {!result.sources.length && (
        <p>No shared records match this search. Try a name or a task, or use the filters above.</p>
      )}
      {result.truncated && (
        <button
          className="px-text-button"
          onClick={() => onQuery({ ...query, limit: Math.min(50, (query.limit || 8) + 12) })}
          disabled={(query.limit || 8) >= 50}
        >
          Show more sources
        </button>
      )}
    </>
  );
}
