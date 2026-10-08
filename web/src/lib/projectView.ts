import type { Agent } from '../types';

// Presentation records only. An authenticated adapter must supply membership,
// dependencies and observed activity; chat parent IDs are not task dependencies.
export type ProjectTask = {
  id: string;
  title: string;
  owner: string;
  status: 'done' | 'working' | 'waiting' | 'needs_you' | 'blocked' | 'stopped' | 'unknown';
  detail: string;
  evidence?: string;
};
export type ProjectStream = {
  stateLabel?: string;
  id: string;
  name: string;
  summary: string;
  tasks: ProjectTask[];
  agents: string[];
  dependsOn?: { id: string; reason: string };
  dependencies?: { id: string; reason: string }[];
};
export type ProjectArea = {
  id: string;
  name: string;
  aim: string;
  tone: 'copper' | 'forest' | 'ink';
};
export type ProjectPlace = { id: string; name: string; kind: 'code' | 'test' | 'notes' };
export type ProjectPerson = { agent: Agent; doing: string; destination?: string; tool?: string };
export type ProjectView = {
  id: string;
  title: string;
  aim: string;
  team: string;
  area?: ProjectArea;
  streams: ProjectStream[];
  people: ProjectPerson[];
  places: ProjectPlace[];
  update: string;
  decision?: string;
};
export const dependenciesOf = (stream: ProjectStream) =>
  stream.dependencies ?? (stream.dependsOn ? [stream.dependsOn] : []);
export const projectTasks = (project: ProjectView) => project.streams.flatMap((s) => s.tasks);
export const projectCounts = (project: ProjectView) => {
  const tasks = projectTasks(project);
  return {
    total: tasks.length,
    done: tasks.filter((t) => t.status === 'done').length,
    working: tasks.filter((t) => t.status === 'working').length,
    waiting: tasks.filter((t) => t.status === 'waiting').length,
  };
};
