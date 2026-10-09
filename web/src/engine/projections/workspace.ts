import { type EngineWorkspace, type LocalWorkItem, type LocalApproval } from '../contracts';
import { taskIsActive, waitingAfterAnswer } from './taskState';
import { engineAgentToUI } from './agents';
import { workRecords, stateLabel, needsHelp, type WorkRecord } from './records';
import type { ProjectView, ProjectTask } from '../../lib/projectView';
import type { SharedWorkEntry } from '../../lib/workContext';
import { workSignal } from '../../lib/workSignals';

// Projection of authorized local-owner records. Replies are conversation
// lineage, not dependencies. Proposed rosters and tool grants are not activity.
export function teamWorkspace(
  workspace: EngineWorkspace | null,
  items: LocalWorkItem[],
  approvals: LocalApproval[] = [],
) {
  const records = workRecords(workspace?.tasks || [], items);
  if (!workspace)
    return { records, projects: [] as ProjectView[], entries: [] as SharedWorkEntry[] };
  const groups = new Map<string, WorkRecord[]>();
  const launched = new Map(
    records.flatMap((r) =>
      r.latest?.plan ? [[r.latest.plan.source_work_id, r.latest.plan.root_work_id] as const] : [],
    ),
  );
  for (const work of records) {
    const group = work.latest?.plan
      ? `plan:${work.latest.plan.root_work_id}`
      : launched.has(work.id)
        ? `plan:${launched.get(work.id)}`
        : work.item?.goal_id
          ? `goal:${work.item.goal_id}`
          : work.latest?.work_team
            ? `roster:${work.latest.work_team.id}`
            : `team:${workspace.team_id}`;
    groups.set(group, [...(groups.get(group) || []), work]);
  }
  // A new connected team has a real place on the map even before its first run.
  if (!groups.size) groups.set(`team:${workspace.team_id}`, []);
  const planning = workspace.planning_tasks || [];
  const agents = workspace.agents.map((agent) =>
    engineAgentToUI(agent, [...workspace.tasks, ...planning]),
  );
  const projects: ProjectView[] = [...groups].map(([id, work]) => {
    const isTeam = id.startsWith('team:') || id.startsWith('roster:');
    const roster = work.find((r) => r.latest?.work_team)?.latest?.work_team;
    const teamName = roster?.name || workspace.team_name;
    const planRoot = work.find((r) => r.id === r.latest?.plan?.root_work_id);
    const participantKeys = new Set(
      work.flatMap((record) => record.turns.map((turn) => turn.agent_key)),
    );
    const people = agents
      .filter(
        (agent) =>
          (isTeam && !roster) ||
          participantKeys.has(agent.id) ||
          (isTeam && !!roster?.agent_keys.includes(agent.id)),
      )
      .map((agent) => {
        const owned = work.filter((record) => record.latest?.agent_key === agent.id);
        const active = owned.filter((record) => record.latest && taskIsActive(record.latest));
        const shaping = planning.some(
          (task) =>
            task.agent_key === agent.id &&
            taskIsActive(task) &&
            work.some((r) => r.id === task.planning_for),
        );
        return {
          agent,
          doing: shaping
            ? 'Preparing a work plan'
            : active.some(
                  (record) =>
                    record.latest?.state === 'waiting_human' && !waitingAfterAnswer(record.latest),
                )
              ? 'Needs your input'
              : active.length && active.every((record) => waitingAfterAnswer(record.latest!))
                ? 'Waiting to continue'
                : active.length
                  ? `${active.length} active ${active.length === 1 ? 'request' : 'requests'}`
                  : 'No active request',
        };
      });
    // One avatar per actual agent. Put it at its active request first; concurrent
    // requests retain their owner label instead of cloning the agent on the map.
    const home = new Map(
      people.map(({ agent }) => {
        const owned = work.filter((record) => record.latest?.agent_key === agent.id);
        return [
          agent.id,
          planning.find(
            (task) =>
              task.agent_key === agent.id &&
              taskIsActive(task) &&
              work.some((r) => r.id === task.planning_for),
          )?.planning_for ||
            (owned.find((record) => taskIsActive(record.latest!)) || owned.at(-1))?.id,
        ];
      }),
    );
    return {
      id,
      kind: id.startsWith('plan:') ? 'plan' : isTeam ? 'workspace' : 'goal',
      title: planRoot?.title || (isTeam ? teamName : `Goal ${id.slice(5)}`),
      team: teamName,
      aim: isTeam
        ? 'Work and explorations in your connected team.'
        : 'Work grouped by its recorded goal reference.',
      area: {
        id: roster ? `roster:${roster.id}` : workspace.team_id,
        name: teamName,
        aim: roster?.purpose || '',
        tone: roster
          ? (['copper', 'forest', 'ink'] as const)[
              [...roster.id].reduce((s, c) => s + c.charCodeAt(0), 0) % 3
            ]
          : 'copper',
      },
      people,
      places: [], // This API does not report live tool destinations. Do not invent them.
      streams: work
        .filter((record) => !launched.has(record.id))
        .map((record) => {
          const planningActive = planning.some(
            (task) => task.planning_for === record.id && taskIsActive(task),
          );
          const label = planningActive ? 'Preparing a work plan' : stateLabel(record);
          const pending = approvals.some(
            (approval) =>
              record.turns.some((turn) => turn.id === approval.work_id) ||
              record.id === approval.work_id,
          );
          const status: ProjectTask['status'] = workSignal(record, approvals, planning);
          return {
            id: record.id,
            role: record.latest?.plan
              ? record.latest.plan.assignment_key
                ? 'contribution'
                : 'coordination'
              : record.latest?.purpose === 'explore'
                ? 'exploration'
                : 'request',
            name: record.title,
            summary: `${record.latest?.purpose === 'explore' ? 'Exploration' : 'Assignment'} · ${label}`,
            stateLabel: pending ? 'Needs your permission' : label,
            agents: people
              .filter(({ agent }) => home.get(agent.id) === record.id)
              .map(({ agent }) => agent.id),
            tasks: [
              {
                id: record.id,
                title: record.title,
                owner: record.latest?.agent_key || '',
                status,
                detail: record.turns[0]?.input || 'Saved work; no execution recorded.',
              },
            ],
            dependencies: (record.latest?.plan?.depends_on || []).map((id) => ({
              id,
              reason: 'Uses the recorded contribution',
            })),
          };
        }),
      update: 'Engine snapshot',
      decision: work.some(
        (record) =>
          (!launched.has(record.id) && needsHelp(record)) ||
          approvals.some((a) => a.work_id === record.id),
      )
        ? 'Review needed'
        : undefined,
    } satisfies ProjectView;
  });
  const projectFor = (id: string) =>
    launched.has(id)
      ? `plan:${launched.get(id)}`
      : projects.find((p) => p.streams.some((s) => s.id === id))!.id;
  const entries: SharedWorkEntry[] = records.flatMap((record) =>
    record.turns.flatMap((turn, index) => [
      {
        id: `${turn.id}:input`,
        projectId: projectFor(record.id),
        author: turn.plan ? 'Agreed plan' : 'You',
        kind: 'direction' as const,
        time: index ? `Follow-up ${index}` : 'Original request',
        content: turn.input,
      },
      ...turn.messages.map((message) => ({
        id: `${turn.id}:${message.id}`,
        projectId: projectFor(record.id),
        authorId: turn.agent_key,
        author: message.role === 'tool' ? 'Tool result' : turn.agent_name,
        kind: message.role === 'tool' ? ('tool' as const) : ('message' as const),
        time: `Record ${message.id}`,
        content: message.content,
      })),
    ]),
  );
  for (const task of planning) {
    const projectId = projects.find((p) => p.streams.some((s) => s.id === task.planning_for))?.id;
    if (!projectId) continue;
    entries.push(
      ...task.messages.map((message) => ({
        id: `${task.id}:${message.id}`,
        projectId,
        authorId: task.agent_key,
        author: task.agent_name,
        kind: 'message' as const,
        time: `Plan reply · record ${message.id}`,
        content: message.content,
      })),
    );
  }
  return { records, projects, entries };
}
