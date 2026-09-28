import type { Agent, Team } from '../types';
import type { WorkItem } from '../types/workroom';

export function workroomExamples(agents: Agent[], teams: Team[]): WorkItem[] {
  const teamFor = (index: number) => {
    const team = teams[index % Math.max(1, teams.length)];
    const members = agents.filter((a) => team?.pledgedAgentIds.includes(a.id)).slice(0, 4);
    const ids = (members.length ? members : agents.slice(0, 3)).map((a) => a.id);
    return { teamId: team?.id || '', agentIds: ids, leadId: ids[0] || '' };
  };
  const base = {
    notes: [],
    clarifications: [],
    fixture: true,
    trigger: '',
    cadence: 'At the next meaningful checkpoint',
  };
  return [
    {
      ...base,
      ...teamFor(0),
      id: 'checkout',
      title: 'Restore checkout reliability',
      context: 'Operations',
      kind: 'response',
      status: 'needs_input',
      intent:
        'Investigate the checkout errors and restore reliable purchases without risking customer data.',
      success: 'Checkout succeeds in the monitored region and the suspected cause is isolated.',
      boundary:
        'Investigate and propose freely. Ask before changing production or disabling a customer-facing feature.',
      trigger: 'Checkout errors exceed the agreed threshold.',
      summary:
        'Errors began after the latest deployment. The team has a reversible rollback ready; production changes need you.',
      next: 'Choose a recovery path. Log collection continues while you decide.',
      milestones: [
        {
          title: 'Locate the affected path',
          detail: 'Errors are isolated to checkout in one region.',
          state: 'done',
        },
        {
          title: 'Choose a recovery path',
          detail:
            'A deployment change is correlated with the errors, but causation is not confirmed.',
          state: 'current',
        },
        {
          title: 'Verify customer recovery',
          detail: 'Check real requests and synthetic purchases after intervention.',
          state: 'next',
        },
      ],
      evidence: [
        {
          title: 'Error window',
          text: 'Sample observation: checkout errors increased from 0.2% to 8.1% after deployment. Other regions remained within the usual range. This establishes timing, not causation.',
        },
        {
          title: 'Rollback check',
          text: 'Sample investigation: the previous application version is available. No schema migration was included. The team has not executed a rollback.',
        },
      ],
      decision: {
        question: 'Roll back now, or keep investigating?',
        whyYou:
          'The team can investigate independently. Changing production is outside its current authority.',
        known:
          'One region is affected. The previous version can be restored without reversing a schema change.',
        unknown: 'The deployment is a likely cause, but has not been proven to be the cause.',
        fallback:
          'The team keeps collecting logs. Checkout remains degraded; no production changes are made.',
        options: [
          {
            id: 'rollback',
            title: 'Roll back the affected region',
            consequence:
              'Prioritizes recovery. The newest checkout changes will be temporarily unavailable in that region.',
            recommended: true,
            acknowledgment:
              'The incident lead accepted the rollback scope: one region, followed by customer-path checks.',
            result:
              'Sample report: checkout errors returned to 0.3%; two synthetic purchase checks passed. Root-cause investigation remains open.',
          },
          {
            id: 'investigate',
            title: 'Investigate before changing production',
            consequence:
              'Preserves the current deployment and gathers stronger evidence. Customers may continue to encounter errors.',
            acknowledgment:
              'The incident lead accepted a further investigation checkpoint with no production changes.',
            result:
              'Sample report: a timeout path was reproduced. Errors remain elevated; a targeted change needs a separate review.',
          },
        ],
      },
    },
    {
      ...base,
      ...teamFor(1),
      id: 'release',
      title: 'Get Atlas ready for release',
      context: 'Products',
      kind: 'assignment',
      status: 'needs_input',
      intent:
        'Prepare a release candidate with a reliable core experience and clear release notes.',
      success:
        'Core acceptance checks pass, known limits are documented, and you approve the release candidate.',
      boundary:
        'Build, test, and draft documentation. Ask before changing scope or publishing a release.',
      summary:
        'Core checks pass. Export accessibility needs another day; the team needs a scope decision.',
      next: 'Decide whether export belongs in this release.',
      milestones: [
        {
          title: 'Validate the core experience',
          detail: 'Core acceptance checks pass in this example.',
          state: 'done',
        },
        {
          title: 'Resolve the export dependency',
          detail: 'Keyboard navigation prevents export from meeting the acceptance criteria.',
          state: 'current',
        },
        {
          title: 'Prepare the release candidate',
          detail: 'Publishing stays with you.',
          state: 'next',
        },
      ],
      evidence: [
        {
          title: 'Acceptance review',
          text: 'Sample review: core onboarding and editing pass. Export loses keyboard focus after choosing a format. The team does not consider export ready.',
        },
        {
          title: 'Scope dependency',
          text: 'Sample plan: export is independently gated. Deferring it does not block the core experience, but must be reflected in customer-facing release notes.',
        },
      ],
      decision: {
        question: 'Keep the date, or keep the full scope?',
        whyYou: 'Only you can trade release scope against the target date.',
        known: 'Core checks pass. Export has an unresolved accessibility issue.',
        unknown: 'The extra day is an estimate, not a confirmed completion time.',
        fallback: 'Documentation and core regression checks continue. Publishing remains on hold.',
        options: [
          {
            id: 'defer-export',
            title: 'Release the core; defer export',
            consequence:
              'Preserves the target date with a smaller scope. Export stays disabled and is named as a known limitation.',
            recommended: true,
            acknowledgment:
              'The release lead accepted the reduced scope. Export stays gated and the release notes will explain the limitation.',
            result:
              'Sample report: core regression checks passed and the revised candidate is ready for your review. Nothing has been published.',
          },
          {
            id: 'extend',
            title: 'Keep export; extend the preparation window',
            consequence:
              'Preserves the intended feature set. The team will report after another day rather than promise a new release date.',
            acknowledgment:
              'The release lead accepted another preparation checkpoint while keeping the export acceptance criteria.',
            result:
              'Sample report: keyboard navigation now passes the test case. Full regression verification is still pending; the candidate is not ready to publish.',
          },
        ],
      },
    },
    {
      ...base,
      ...teamFor(0),
      id: 'inbox',
      title: 'Keep incoming requests moving',
      context: 'Business',
      kind: 'responsibility',
      status: 'watching',
      intent: 'Sort new customer requests, gather missing context, and prepare useful replies.',
      success:
        'Every new request has an owner and a proposed next step. Uncertain or sensitive replies come to you.',
      boundary:
        'Categorize and draft independently. Ask before sending, making commitments, or handling a disputed refund.',
      trigger: 'A new customer request arrives.',
      cadence: 'A daily digest; interrupt only for an exception',
      summary:
        'The last sample cycle organized 12 requests. Two reply drafts await routine review; no urgent exception was recorded.',
      next: 'Next new request → classify → prepare context → draft or escalate.',
      milestones: [
        {
          title: 'Receive a new request',
          detail: 'Start one response per new request; avoid duplicate processing.',
          state: 'done',
        },
        {
          title: 'Classify and prepare a response',
          detail: 'Use the customer context and current operating boundaries.',
          state: 'current',
        },
        {
          title: 'Collect review or return to watching',
          detail: 'Sending a reply always requires your approval.',
          state: 'next',
        },
      ],
      evidence: [
        {
          title: 'Last cycle digest',
          text: 'Sample cycle: 12 requests categorized; 10 linked to existing answers; 2 reply drafts prepared. No reply was sent. This is a static example, not a connected inbox.',
        },
      ],
    },
    {
      ...base,
      ...teamFor(1),
      id: 'lessons',
      title: 'Plan next week’s lessons',
      context: 'Teaching',
      kind: 'assignment',
      status: 'active',
      intent:
        'Prepare a useful sequence of lessons on fractions, with room for different starting points.',
      success:
        'Five lesson outlines, a short assessment, and two alternative explanations are ready for teacher review.',
      boundary:
        'Draft with supplied curriculum material. Ask before changing learning goals. Do not access student records or assign grades.',
      summary:
        'The lesson sequence is outlined. The team is developing examples and checking them against the learning goals.',
      next: 'Review the first two lesson outlines before the rest are expanded.',
      milestones: [
        {
          title: 'Outline the sequence',
          detail: 'Connect each lesson to the supplied learning goals.',
          state: 'done',
        },
        {
          title: 'Draft examples and alternatives',
          detail: 'Make room for different starting points.',
          state: 'current',
        },
        {
          title: 'Teacher review',
          detail: 'The teacher decides what belongs in the classroom.',
          state: 'next',
        },
      ],
      evidence: [
        {
          title: 'Sequence outline',
          text: 'Sample outline: equal parts → fractions on a number line → equivalent fractions → comparison → a short formative assessment. This is illustrative curriculum content.',
        },
      ],
    },
    {
      ...base,
      ...teamFor(0),
      id: 'week',
      title: 'Make room for the week',
      context: 'Personal',
      kind: 'responsibility',
      status: 'watching',
      intent: 'Keep an achievable weekly plan, with room for rest and changing priorities.',
      success:
        'The plan reflects your actual priorities, leaves buffer time, and brings conflicts to you with options.',
      boundary:
        'Suggest changes and organize notes. Ask before moving commitments, contacting people, or booking anything.',
      trigger: 'A weekly check-in or a new commitment.',
      cadence: 'At your weekly check-in',
      summary:
        'The sample plan leaves two open afternoons. The next check-in will revisit priorities before adding more.',
      next: 'Bring a short list of tradeoffs to the next weekly check-in.',
      milestones: [
        {
          title: 'Gather commitments',
          detail: 'Use the commitments you choose to share.',
          state: 'done',
        },
        {
          title: 'Keep room in the plan',
          detail: 'Flag conflicts and suggest options without changing your calendar.',
          state: 'current',
        },
        { title: 'Check in and adjust', detail: 'Revisit priorities with you.', state: 'next' },
      ],
      evidence: [
        {
          title: 'Planning note',
          text: 'Sample preference: leave two afternoons open and avoid filling every gap. No calendar is connected, and no commitments have been moved.',
        },
      ],
    },
  ];
}
