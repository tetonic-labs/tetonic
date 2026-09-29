import { ArrowUpRight, Files, Plug, Terminal } from 'lucide-react';
import type { WorkspaceResource } from '../../lib/toolLibrary';
import type { OrganizationActivity } from './useOrganizationActivity';

export function TeamMapResources({
  name,
  resources,
  memberIds,
  activity,
  onInspect,
  onManage,
}: {
  name: string;
  resources: WorkspaceResource[];
  memberIds: Set<string>;
  activity: OrganizationActivity;
  onInspect: (resource: WorkspaceResource) => void;
  onManage: () => void;
}) {
  return (
    <aside className="team-map-resources" aria-label={`${name} tools and MCPs`}>
      <header>
        <span>
          Team resources <small>{resources.length}</small>
        </span>
        <button
          onClick={onManage}
          aria-label={`Manage ${name} resources`}
          title="Manage team resources"
        >
          <ArrowUpRight size={16} />
        </button>
      </header>
      <div className="team-map-resource-list">
        {(['mcp', 'tools'] as const).map((kind) => {
          const group = resources.filter((r) =>
            kind === 'mcp' ? r.kind === 'mcp' : r.kind !== 'mcp',
          );
          if (!group.length) return null;
          return (
            <section
              key={kind}
              aria-label={kind === 'mcp' ? 'MCP connections' : 'Tools and storage'}
            >
              <h3>{kind === 'mcp' ? 'MCPs' : 'Tools'}</h3>
              {group.map((resource) => {
                const work = [...activity.states.values()].filter(
                  (w) => memberIds.has(w.id) && w.interaction?.targetId === resource.id,
                );
                const blocked = [...activity.states.values()].filter(
                  (w) =>
                    memberIds.has(w.id) &&
                    w.failures.some((f) => f.interaction.targetId === resource.id),
                ).length;
                const waiting = work.filter((w) => w.waiting).length;
                const Icon =
                  resource.kind === 'mcp' ? Plug : resource.kind === 'storage' ? Files : Terminal;
                return (
                  <button
                    key={resource.id}
                    className="team-map-resource"
                    onClick={() => onInspect(resource)}
                    data-resource={resource.id}
                    data-signal={blocked ? 'blocked' : work.length ? 'working' : 'idle'}
                  >
                    <Icon size={17} strokeWidth={1.6} />
                    <span>
                      <strong>{resource.name}</strong>
                      <small>
                        {resource.source === 'draft'
                          ? 'Setup draft'
                          : blocked
                            ? `${blocked} blocked`
                            : work.length
                              ? `${work.length - waiting} working${waiting ? ` · ${waiting} waiting` : ''}`
                              : 'Available'}
                      </small>
                    </span>
                    {work.length > 0 && (
                      <i title={`${work.length} active interactions`}>{work.length}</i>
                    )}
                  </button>
                );
              })}
            </section>
          );
        })}
        {!resources.length && (
          <p>
            No tools assigned yet.{' '}
            <button onClick={onManage}>
              Choose team resources <ArrowUpRight size={12} />
            </button>
          </p>
        )}
      </div>
    </aside>
  );
}
