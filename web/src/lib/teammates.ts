import { Agent } from '../types';

// Presentation aliases for the existing preview identities. IDs and permissions are unchanged.
const personalities: Record<
  string,
  { name: string; role: string; shortRole: string; color: string }
> = {
  'agt-builder': {
    name: 'Alex',
    role: 'Builds and improves projects',
    shortRole: 'Engineering',
    color: '#D9B69A',
  },
  'agt-doc': {
    name: 'Robin',
    role: 'Makes things easier to understand',
    shortRole: 'Documentation',
    color: '#ACB8A0',
  },
  'agt-sentinel': {
    name: 'Sam',
    role: 'Keeps an eye on the boundaries',
    shortRole: 'Safety',
    color: '#B3BBCB',
  },
  'agt-remote-guard': {
    name: 'Casey',
    role: 'Checks the details',
    shortRole: 'Review',
    color: '#D9BB82',
  },
};

export const teammate = (agent: Agent) =>
  personalities[agent.id] || {
    name: agent.name,
    role: agent.charter,
    shortRole: 'Teammate',
    color: '#D9B69A',
  };

export const teammateName = (id: string, fallback: string) => personalities[id]?.name || fallback;
