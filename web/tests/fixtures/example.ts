import type { Agent } from '../../src/types';
import type {
  ProjectPerson,
  ProjectTask,
  ProjectView,
} from '../../src/components/workspace/ProjectMap';

const person = (
  name: string,
  role: string,
  doing: string,
  destination?: string,
  tool?: string,
): ProjectPerson => ({
  agent: {
    id: `example-${name.toLowerCase()}`,
    name,
    charter: role,
    model: 'Illustrative model',
    status: 'idle',
    decisionIntervalMs: 0,
    capabilities: [],
    tokensProcessed: 0,
    memoryItemsCount: 0,
    isLocalToCastle: true,
    lastActive: '',
  } satisfies Agent,
  doing,
  destination,
  tool,
});
const task = (
  id: string,
  title: string,
  owner: string,
  status: ProjectTask['status'],
  detail: string,
  evidence?: string,
): ProjectTask => ({
  id,
  title,
  owner: `example-${owner.toLowerCase()}`,
  status,
  detail,
  evidence,
});
const tasks = (name: string, owner: string, count: number): ProjectTask[] =>
  Array.from({ length: count }, (_, i) =>
    task(
      `${name}-${i}`,
      [
        'Gather source material',
        'Compare the findings',
        'Check the evidence',
        'Prepare a recommendation',
      ][i % 4],
      owner,
      i < 2 ? 'done' : i === 2 ? 'working' : 'waiting',
      i < 2
        ? 'The example includes a source-linked contribution.'
        : i === 2
          ? 'Checking the supporting material before handing it on.'
          : 'Starts after the evidence check.',
    ),
  );

export const handoffSteps = [
  {
    title: 'Jun is finishing the API contract',
    detail: 'Remy is building the interface in parallel. The release audience needs your choice.',
  },
  {
    title: 'Jun → Sage · review requested',
    detail:
      'The API contract and change notes travel with the handoff. Jun can pick up other work.',
  },
  {
    title: 'Sage → Ivy · contract accepted',
    detail: 'Ivy starts compatibility checks. Acknowledged handoff, with the same project context.',
  },
  {
    title: 'Ivy → Eden · checks passed',
    detail:
      'The checked build is ready for rollout planning. A launch still requires human permission.',
  },
];

export function exampleProjects(step: number, pilotChosen: boolean): ProjectView[] {
  const buildState: ProjectTask['status'] = step === 0 ? 'working' : step < 2 ? 'waiting' : 'done';
  const verifyState: ProjectTask['status'] = step < 2 ? 'waiting' : step === 2 ? 'working' : 'done';
  const portal: ProjectView = {
    id: 'portal',
    area: {
      id: 'customers',
      name: 'Customer experience',
      aim: 'Understand customers and improve how we serve them.',
      tone: 'copper',
    },
    title: 'Launch the customer portal',
    team: 'Product team',
    aim: 'Give customers one place to manage their account, starting with a small, reversible release.',
    update: 'A shared contract is moving from implementation to review and testing.',
    decision: pilotChosen ? undefined : 'Who should see the first release?',
    people: [
      person('Mira', 'Research lead', 'Findings ready'),
      person('Noor', 'Customer research', 'Organizing evidence', 'notes', 'Read'),
      person(
        'Jun',
        'Backend engineering',
        step === 0 ? 'Writing contract' : 'On to error handling',
        step === 0 ? 'github' : undefined,
        step === 0 ? 'Code' : undefined,
      ),
      person('Remy', 'Interface design', 'Building settings'),
      person(
        'Sage',
        'Independent review',
        step === 1 ? 'Reviewing Jun’s change' : step > 1 ? 'Review complete' : 'Ready to review',
        step === 1 ? 'github' : undefined,
        step === 1 ? 'Review' : undefined,
      ),
      person(
        'Ivy',
        'Quality and compatibility',
        step === 2 ? 'Checking compatibility' : step > 2 ? 'Checks passed' : 'Waiting for contract',
        step === 2 ? 'tests' : undefined,
        step === 2 ? 'Tests' : undefined,
      ),
      person('Leo', 'Customer communication', 'Drafting launch notes'),
      person(
        'Eden',
        'Release coordination',
        pilotChosen ? 'Preparing a pilot' : 'Needs your choice',
      ),
    ],
    places: [
      { id: 'github', name: 'GitHub', kind: 'code' },
      { id: 'tests', name: 'Test environment', kind: 'test' },
      { id: 'notes', name: 'Shared research', kind: 'notes' },
    ],
    streams: [
      {
        id: 'understand',
        name: 'Understand the customers',
        summary: 'Turn research into a clear, shared brief.',
        agents: ['example-mira', 'example-noor'],
        tasks: [
          task(
            'u1',
            'Collect customer interviews',
            'Mira',
            'done',
            'Six example interviews are grouped by the problem each person is trying to solve.',
            'Interview notes · 6 sources',
          ),
          task(
            'u2',
            'Find the recurring problems',
            'Noor',
            'done',
            'Repeated account and billing questions are separated from one-off requests.',
            'Research summary · 3 themes',
          ),
          task(
            'u3',
            'Agree on the first-release scope',
            'Mira',
            'done',
            'The team has a bounded account and settings scope.',
            'Shared brief · revision 2',
          ),
          task(
            'u4',
            'Give implementation the acceptance criteria',
            'Mira',
            'done',
            'Jun and Remy have the same criteria and source references.',
            'Acknowledged by Jun and Remy',
          ),
          task(
            'u5',
            'Organize research for later questions',
            'Noor',
            'working',
            'Consolidating source references while implementation proceeds.',
          ),
        ],
      },
      {
        id: 'build',
        name: 'Build the experience',
        summary: 'Backend and interface work proceed together.',
        agents: ['example-jun', 'example-remy'],
        dependsOn: { id: 'understand', reason: 'Shared brief' },
        tasks: [
          task(
            'b1',
            'Define the account data model',
            'Jun',
            'done',
            'The model follows the account and settings scope.',
            'Draft schema · 4 entities',
          ),
          task(
            'b2',
            'Build the API contract',
            'Jun',
            buildState,
            step === 0
              ? 'Finishing the contract before requesting independent review.'
              : step < 2
                ? 'Implementation is ready. Sage is reviewing the handoff.'
                : 'Sage accepted the contract; Ivy can test against it.',
            'Example change #42 · API contract',
          ),
          task(
            'b3',
            'Build account settings',
            'Remy',
            'working',
            'The interface uses the agreed contract; no need to wait for the whole backend.',
          ),
          task(
            'b4',
            'Handle expired sessions',
            'Jun',
            step > 0 ? 'working' : 'waiting',
            'Independent error handling can proceed while review happens.',
          ),
          task(
            'b5',
            'Combine the interface and backend',
            'Remy',
            'waiting',
            'Needs the settings interface and reviewed API contract.',
          ),
        ],
      },
      {
        id: 'verify',
        name: 'Make sure it holds up',
        summary: 'Independent review before a wider release.',
        agents: ['example-sage', 'example-ivy'],
        dependsOn: { id: 'build', reason: 'Reviewable change' },
        tasks: [
          task(
            'v1',
            'Write the acceptance checks',
            'Ivy',
            'done',
            'The checks are grounded in Mira’s criteria.',
            'Acceptance checklist · 8 cases',
          ),
          task(
            'v2',
            'Review the API contract',
            'Sage',
            step === 0 ? 'waiting' : step === 1 ? 'working' : 'done',
            step < 1
              ? 'Jun will send a bounded review request with the change attached.'
              : 'Review only the submitted contract and return findings to Jun.',
            step > 1 ? 'Review note · contract v2' : undefined,
          ),
          task(
            'v3',
            'Run compatibility checks',
            'Ivy',
            verifyState,
            step < 2
              ? 'Waiting for Sage’s accepted review; other test preparation continues.'
              : step === 2
                ? 'Checking the contract against the expected account flows.'
                : 'The example checks passed. This does not complete the whole release.',
            step > 2 ? 'Example test report · 8 checks' : undefined,
          ),
          task(
            'v4',
            'Check accessibility of the final interface',
            'Ivy',
            'waiting',
            'Needs Remy’s assembled interface.',
          ),
          task(
            'v5',
            'Verify the full customer journey',
            'Sage',
            'waiting',
            'Needs the assembled build and compatibility results.',
          ),
        ],
      },
      {
        id: 'launch',
        name: 'Prepare the release',
        summary: pilotChosen
          ? 'Prepare invitations for an invited pilot.'
          : 'Only the audience choice is waiting on you.',
        agents: ['example-leo', 'example-eden'],
        dependsOn: { id: 'verify', reason: 'Checked build' },
        tasks: [
          task(
            'l1',
            'Draft the release explanation',
            'Leo',
            'working',
            'Drafting customer-facing notes while the technical work proceeds.',
          ),
          task(
            'l2',
            'Write a rollback plan',
            'Eden',
            'done',
            'The team has a reversible plan before any rollout.',
            'Rollback checklist · draft',
          ),
          task(
            'l3',
            'Choose the initial audience',
            'Eden',
            pilotChosen ? 'done' : 'needs_you',
            pilotChosen
              ? 'The operator chose 25 invited customers. This is scope guidance, not permission to send invitations.'
              : 'The team needs a human to choose an invited pilot or a broad release.',
          ),
          task(
            'l4',
            'Prepare the invitation draft',
            'Eden',
            pilotChosen ? 'working' : 'waiting',
            'Depends on the audience choice; it does not stop research, implementation or testing.',
          ),
          task(
            'l5',
            'Request permission to send and release',
            'Eden',
            'waiting',
            'Sending invitations and deploying require a separate review of the exact effects.',
          ),
        ],
      },
    ],
  };
  const other = (
    id: string,
    title: string,
    team: string,
    aim: string,
    names: string[],
    streamNames: string[],
  ): ProjectView => ({
    id,
    area:
      id === 'research'
        ? {
            id: 'customers',
            name: 'Customer experience',
            aim: 'Understand customers and improve how we serve them.',
            tone: 'copper',
          }
        : {
            id: 'reliability',
            name: 'Reliable operations',
            aim: 'Keep the services people depend on healthy.',
            tone: 'forest',
          },
    title,
    team,
    aim,
    update: 'Parallel work with an inspectable owner and contribution for each assignment.',
    places: [],
    people: names.map((name, i) =>
      person(
        name,
        i % 2 ? 'Contributor' : 'Workstream lead',
        i % 2 ? 'Checking evidence' : 'Preparing findings',
      ),
    ),
    streams: streamNames.map((name, i) => ({
      id: `${id}-${i}`,
      name,
      summary: [
        'A bounded question with its own owner.',
        'Contributions stay attached to the shared goal.',
        'Review the evidence before deciding.',
      ][i],
      agents: names.slice(i * 2, i * 2 + 2).map((name) => `example-${name.toLowerCase()}`),
      tasks: tasks(`${id}-${i}`, names[i * 2], 4),
    })),
  });
  return [
    portal,
    other(
      'research',
      'Explore a new market',
      'Research team',
      'Find where our product could help most, with evidence we can challenge.',
      ['Ari', 'Bea', 'Sol', 'Rae', 'Kit', 'Lou'],
      ['Understand the landscape', 'Talk to potential customers', 'Compare the opportunities'],
    ),
    other(
      'operations',
      'Keep the service healthy',
      'Operations team',
      'Investigate recurring failures and prepare bounded fixes for review.',
      ['Ash', 'Kai', 'Em', 'Bo'],
      ['Investigate recurring incidents', 'Prepare durable fixes'],
    ),
  ];
}
