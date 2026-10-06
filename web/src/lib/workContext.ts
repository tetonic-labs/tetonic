import { dependenciesOf, type ProjectView } from './projectView';

export type SharedWorkEntry = {
  id: string;
  projectId: string;
  authorId?: string;
  author: string;
  kind: 'message' | 'tool' | 'direction';
  time: string;
  order?: number;
  content: string;
};
export type WorkContextSource = {
  id: string;
  projectId: string;
  project: string;
  team: string;
  areaId?: string;
  kind: 'assignment' | 'activity' | 'dependency' | 'message';
  title: string;
  content: string;
  status?: string;
  target: { kind: 'stream' | 'agent' | 'blackboard'; id: string };
};
export type WorkContextSnapshot = {
  origin: 'example' | 'engine';
  revision: string;
  observedAt: string;
  completeness: 'complete' | 'partial';
  sources: WorkContextSource[];
};
export type WorkContextQuery = {
  projectId?: string;
  areaId?: string;
  text?: string;
  filter?: 'needs_you' | 'waiting' | 'working';
  limit?: number;
};

/** Read-only projection, NOT authorization. Only pass already-authorized shared
 * records. It does not load private conversations, invoke a model or dispatch work.
 * The server tool must authorize before lookup and recheck before response.
 */
export function projectWorkContext(
  projects: ProjectView[],
  entries: SharedWorkEntry[],
  meta: Omit<WorkContextSnapshot, 'sources'>,
): WorkContextSnapshot {
  const sources: WorkContextSource[] = projects.flatMap((project) => {
    const base = {
      projectId: project.id,
      project: project.title,
      team: project.team,
      areaId: project.area?.id,
    };
    const tasks = project.streams.flatMap((stream) =>
      stream.tasks.map((task) => ({
        ...base,
        id: `task:${project.id}:${task.id}`,
        kind: 'assignment' as const,
        title: task.title,
        status: task.status,
        content: `${project.people.find((p) => p.agent.id === task.owner)?.agent.name || 'Unassigned'} · ${task.status.replaceAll('_', ' ')}\n${task.detail}${task.evidence ? `\nEvidence: ${task.evidence}` : ''}`,
        target: { kind: 'stream' as const, id: stream.id },
      })),
    );
    const activity = project.people.map((person) => ({
      ...base,
      id: `agent:${project.id}:${person.agent.id}`,
      kind: 'activity' as const,
      title: person.agent.name,
      content: `${person.doing}${person.destination ? ` · At ${project.places.find((p) => p.id === person.destination)?.name || person.destination}` : ''}`,
      target: { kind: 'agent' as const, id: person.agent.id },
    }));
    const dependencies = project.streams.flatMap((stream) =>
      dependenciesOf(stream).map((dependency) => ({
        ...base,
        id: `dependency:${project.id}:${stream.id}:${dependency.id}`,
        kind: 'dependency' as const,
        title: stream.name,
        content: `${dependency.reason} from ${project.streams.find((s) => s.id === dependency.id)?.name || 'an unavailable workstream'}. A dependency between workstreams does not block every assignment.`,
        target: { kind: 'stream' as const, id: stream.id },
      })),
    );
    const messages = entries
      .filter((entry) => entry.projectId === project.id)
      .map((entry) => ({
        ...base,
        id: `message:${entry.id}`,
        kind: 'message' as const,
        title: `${entry.author} · ${entry.time}`,
        content: entry.content,
        target: { kind: 'blackboard' as const, id: entry.id },
      }));
    return [...tasks, ...activity, ...dependencies, ...messages];
  });
  return { ...meta, sources };
}

// Literal retrieval for the product example and source inspection. This is not
// a natural-language agent or a list of prewritten answers to expected questions.
export function queryWorkContext(snapshot: WorkContextSnapshot, query: WorkContextQuery) {
  const words = (query.text?.toLowerCase().match(/[\p{L}\p{N}]+/gu) || []).filter(
    (word) =>
      !new Set([
        'what',
        'which',
        'who',
        'why',
        'is',
        'are',
        'the',
        'a',
        'an',
        'of',
        'to',
        'for',
        'on',
        'in',
        's',
        'we',
        'our',
        'me',
        'and',
        'with',
        'about',
        'how',
        'does',
        'it',
      ]).has(word),
  );
  const scoped = snapshot.sources.filter(
    (source) =>
      (!query.projectId || source.projectId === query.projectId) &&
      (!query.areaId || source.areaId === query.areaId) &&
      (!query.filter || source.status === query.filter),
  );
  const ranked = scoped
    .map((source, index) => ({
      source,
      index,
      score: words.reduce(
        (sum, word) =>
          sum +
          (source.title.toLowerCase().includes(word) ? 3 : 0) +
          (source.content.toLowerCase().includes(word) ? 1 : 0),
        0,
      ),
    }))
    .filter((item) => !words.length || item.score > 0)
    .sort((a, b) => b.score - a.score || a.index - b.index);
  const limit = Math.max(1, Math.min(50, query.limit || 8));
  return {
    ...snapshot,
    sources: ranked.slice(0, limit).map((r) => r.source),
    total: ranked.length,
    truncated: ranked.length > limit,
  };
}
